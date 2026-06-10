//! LuaJIT VM initialization and plugin loader implementation.
//!
//! Loads Lua plugin bundles via the embedded LuaJIT VM (mlua).
//! Each bundle gets its own Lua VM for complete isolation between bundles
//! and between polyplug Runtime instances.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::thread::ThreadId;

use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;

use crate::config::LuaConfig;
use polyplug::Runtime;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

/// The path to the sdks/lua/guest/ directory, set at compile time by build.rs.
const GUEST_LUA_DIR: &str = env!("POLYPLUG_GUEST_LUA_DIR");

/// The path to the abi/lua/ directory, set at compile time by build.rs.
const ABI_LUA_DIR: &str = env!("POLYPLUG_ABI_LUA_DIR");

/// RAII guard that keeps the runtime's per-thread init-bundle window open for the
/// duration of `load_inner`'s init **and** registration phases.
///
/// `host_register_guest_contract` attributes each registration to the bundle id at
/// the top of the runtime's init-bundle stack (`current_init_bundle_id`). The Lua
/// `polyplug_init` only populates `_G._polyplug_handlers`; the actual
/// `register_guest_contract` calls happen later in `load_inner`. The window must
/// therefore stay open across BOTH phases, and `pop` must run on EVERY exit path —
/// including the many `?` early-returns between init and the end of the registration
/// loop. Dropping this guard pops exactly once, whether the function returns Ok,
/// returns Err via `?`, or unwinds, so the stack never leaks an entry.
struct InitBundleGuard<'r> {
    runtime: &'r Runtime,
}

impl<'r> InitBundleGuard<'r> {
    /// Push `bundle_id` onto the runtime's init-bundle stack and return a guard that
    /// pops it on drop.
    fn enter(runtime: &'r Runtime, bundle_id: u64) -> Self {
        runtime.push_init_bundle_id(bundle_id);
        Self { runtime }
    }
}

impl Drop for InitBundleGuard<'_> {
    fn drop(&mut self) {
        self.runtime.pop_init_bundle_id();
    }
}

// ─── Lua Loader Data for VM Dispatch ───────────────────────────────────────────

/// Loader-specific data for Lua plugin dispatch.
///
/// Each bundle gets its own Lua VM, ensuring complete isolation between
/// bundles and between polyplug Runtime instances.
pub struct LuaLoaderData {
    pub _vm: Lua,
    pub functions: Vec<Function>,
    /// Thread-aware same-VM reentrancy guard for [`lua_dispatch`].
    ///
    /// mlua is built with the `send` feature, so a single `Lua` VM is reachable
    /// from any thread and is internally lock-guarded. Two cases must be told
    /// apart, and a plain `AtomicBool` cannot distinguish them:
    ///
    /// 1. SAME-thread nested dispatch — a plugin→plugin cross-call
    ///    (`host->call_guest_method`) that resolves back to a contract in THIS
    ///    same VM while this thread is already mid-dispatch. Re-entering mlua from
    ///    a nested host frame on the same thread would deadlock mlua's internal
    ///    `send`-feature mutex (already held by this thread). This MUST be refused
    ///    with `ReentrantCall`.
    /// 2. CROSS-thread concurrent dispatch — a different thread dispatches into
    ///    this VM while a dispatch is in flight on another thread. mlua serializes
    ///    this safely by blocking on its internal lock, matching the HostApi
    ///    contract ("safe to call from any thread; the runtime handles internal
    ///    synchronization"). This MUST proceed and be allowed to block.
    ///
    /// The set of thread ids currently inside a dispatch on this VM captures
    /// exactly that distinction: presence of the current thread's id means a
    /// same-thread nested call (refuse); absence means a fresh caller — possibly
    /// from another thread concurrently — which proceeds. It lives on the per-VM
    /// `LuaLoaderData`, never globally, so it is Rule-12 compliant. Contention is
    /// trivial: the vec holds 0..N concurrent caller threads and never duplicates.
    pub in_dispatch_threads: Mutex<Vec<ThreadId>>,
    /// Per-VM serialization of the arena publish→call→clear span in [`lua_dispatch`].
    ///
    /// The arena pointer is published as the `_polyplug_arena` VM global, the guest
    /// is called, and the global is cleared — three SEPARATE mlua operations. mlua's
    /// internal `send` lock serializes each individual operation but NOT the
    /// sequence, so without this lock a CROSS-thread concurrent dispatch could
    /// overwrite the global between this thread's publish and its guest call (the
    /// guest then allocates from the wrong arena), or clear the global to 0
    /// mid-call (the guest then falls back to `host->alloc` and leaks). Holding this
    /// lock across the whole span makes both impossible: a cross-thread caller
    /// blocks here until the in-flight call finishes, exactly the serialization mlua
    /// would impose on the VM lock anyway. Same-thread nested reentrancy is refused
    /// (`ReentrantCall`) BEFORE this lock is taken, so the lock can never deadlock
    /// against its own thread. It lives on the per-VM `LuaLoaderData`, never
    /// globally, so it is Rule-12 compliant.
    pub dispatch_lock: Mutex<()>,
}

/// Owning handle to a bundle's [`LuaLoaderData`] with a stable heap address.
///
/// The dispatch `bridge_data` stores `self.as_ptr()`; that address must stay valid
/// for as long as the bundle is loaded, so the `LuaLoaderData` lives behind a `Box`
/// (a bare `Vec<LuaLoaderData>` would move its elements on reallocation and dangle
/// every `bridge_data`). This newtype makes the required indirection explicit and
/// keeps the owned collections as `Vec<LuaVm>` rather than `Vec<Box<..>>`.
struct LuaVm(Box<LuaLoaderData>);

impl LuaVm {
    /// The stable heap address of the wrapped [`LuaLoaderData`], used as the dispatch
    /// `bridge_data`. Stable across moves of the `LuaVm`/`Box` while owned.
    fn as_ptr(&self) -> *const LuaLoaderData {
        &*self.0 as *const LuaLoaderData
    }

    /// Borrow the wrapped [`LuaLoaderData`] (e.g. to inspect `in_dispatch_threads`).
    fn data(&self) -> &LuaLoaderData {
        &self.0
    }
}

/// RAII guard that removes the current thread's id from
/// [`LuaLoaderData::in_dispatch_threads`] on every exit path, including panics
/// that unwind through `lua_dispatch`.
struct LuaDispatchGuard<'a> {
    threads: &'a Mutex<Vec<ThreadId>>,
}

impl Drop for LuaDispatchGuard<'_> {
    fn drop(&mut self) {
        let this: ThreadId = std::thread::current().id();
        // Recover from poisoning: a panic in another dispatch may have poisoned
        // the lock, but the data is a plain Vec<ThreadId> that cannot be left
        // logically corrupt between lock/unlock, so reusing the inner value is
        // sound. This is production code, so we never unwrap.
        let mut guard: std::sync::MutexGuard<'_, Vec<ThreadId>> =
            self.threads.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(pos) = guard.iter().position(|&id| id == this) {
            guard.swap_remove(pos);
        }
    }
}

// ─── Instance Lifecycle Stubs ──────────────────────────────────────────────────

/// Stub create_instance for Lua plugins - returns null instance.
///
/// # Safety
/// Lua plugins use VM dispatch with global state; instances are not used.
unsafe extern "C" fn lua_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Stub destroy_instance for Lua plugins - no cleanup needed.
///
/// # Safety
/// Lua plugins don't own instance data.
unsafe extern "C" fn lua_destroy_instance(_host: *const HostApi, _instance: GuestContractInstance) {
}

// ─── Lua Dispatch Function ─────────────────────────────────────────────────────

/// The Lua global holding the active per-call [`CallArena`] pointer as an integer.
///
/// `lua_dispatch` publishes the arena pointer here under the VM lock for the
/// duration of one call and clears it (sets it to 0) afterwards, so the
/// `_polyplug_arena_alloc` bridge can serve the guest's return buffers from the
/// arena. A value of 0 means "no arena" — the bridge falls back to `host->alloc`.
const ARENA_GLOBAL: &str = "_polyplug_arena";

/// Dispatch function for Lua plugins using VM dispatch pattern.
///
/// # Safety
/// - `loader_data` must be a valid VmLoaderData wrapping LuaLoaderData
/// - `args` and `out` must be valid pointers for the ABI call
/// - `arena`, when non-null, must point to a valid [`CallArena`] reset by the
///   caller for this call. Values written by the guest into the arena (via
///   `polyplug_guest.alloc_string_arena`) are valid until the caller's next reset.
unsafe extern "C" fn lua_dispatch(
    loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
) -> AbiError {
    // SAFETY: loader_data wraps a valid pointer to LuaLoaderData created by the loader.
    let data: &LuaLoaderData = unsafe { &*(loader_data.data as *const LuaLoaderData) };

    // Reject ONLY same-thread nested reentrancy BEFORE touching the VM. If this
    // thread is already inside a dispatch on this VM (a plugin→plugin cross-call
    // resolving back here), re-entering mlua would deadlock its internal
    // `send`-feature mutex, so refuse with ReentrantCall. A different thread
    // dispatching concurrently is NOT reentrancy: it is allowed to proceed and
    // mlua's internal lock serializes it safely. The tracking Mutex is held only
    // around the membership check/insert below, never across the VM call.
    let this_thread: ThreadId = std::thread::current().id();
    {
        // Recover from poisoning (a prior dispatch panic): the Vec<ThreadId>
        // cannot be left logically corrupt between lock/unlock, so the inner
        // value is reusable. Production code, so no unwrap.
        let mut threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = data
            .in_dispatch_threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if threads.contains(&this_thread) {
            // Drop the guard (unlock) before returning.
            drop(threads);
            return AbiError {
                code: AbiErrorCode::ReentrantCall as u32,
                message: StringView::null(),
            };
        }
        threads.push(this_thread);
    }
    // From here on this thread's id is registered; the guard removes it on every
    // exit path (early return, normal return, or panic unwind).
    let _dispatch_guard: LuaDispatchGuard<'_> = LuaDispatchGuard {
        threads: &data.in_dispatch_threads,
    };

    let lua_fn: &Function = match data.functions.get(fn_id as usize) {
        Some(f) => f,
        None => {
            return AbiError {
                code: AbiErrorCode::FunctionNotAvailable as u32,
                message: StringView::null(),
            };
        }
    };

    // Pass pointers as i64 to preserve full 64-bit precision on LuaJIT.
    // LuaJIT lua_Integer is int64_t — safe for pointer-width integers.
    let args_i64: i64 = args as usize as i64;
    let out_i64: i64 = out as usize as i64;

    // Publish the per-call arena pointer so the _polyplug_arena_alloc bridge can
    // serve allocations from it. The publish→call→clear span is THREE separate mlua
    // operations; mlua's internal `send` lock serializes each one but not the
    // sequence, so the per-VM `dispatch_lock` is held across the whole span. Without
    // it, a cross-thread concurrent dispatch could overwrite the global between this
    // publish and the guest call (wrong arena), or clear it to 0 mid-call (the guest
    // would fall back to host->alloc and leak). Same-thread reentrancy was already
    // refused above, so this lock cannot deadlock against its own thread; a
    // cross-thread caller blocks here, exactly the serialization mlua imposes on the
    // VM lock anyway. Poison recovery: the unit value cannot be left logically
    // corrupt, so reusing it is sound. Production code, so no unwrap.
    let _dispatch_lock: std::sync::MutexGuard<'_, ()> = data
        .dispatch_lock
        .lock()
        .unwrap_or_else(PoisonError::into_inner);

    let arena_i64: i64 = arena as usize as i64;
    let _ = data._vm.globals().set(ARENA_GLOBAL, arena_i64);

    let call_result: Result<(), mlua::Error> = lua_fn.call::<()>((args_i64, out_i64));

    let _ = data._vm.globals().set(ARENA_GLOBAL, 0_i64);

    match call_result {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            eprintln!("[polyplug_lua] Lua function call failed: {}", e);
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// Lua plugin loader — loads Lua plugin bundles via the embedded LuaJIT VM.
///
/// The Lua script must define a global function `polyplug_init(registrar_ptr: integer)`
/// which populates `_G._polyplug_handlers` with plugin metadata and function tables.
pub struct LuaLoader {
    /// Configuration for this loader instance.
    pub config: LuaConfig,
    /// Per-bundle VM state owned by the loader, keyed by [`BundleId`].
    ///
    /// Each registered contract contributes one [`LuaLoaderData`] (which holds a
    /// clone of the bundle's `Lua` VM handle). The boxes are owned here instead of
    /// leaked via `Box::into_raw`, so [`LuaLoader::unload`] can drop them and truly
    /// reclaim the VM. The VM dispatch `bridge_data` points at the boxed
    /// `LuaLoaderData`'s stable heap address; the box is never moved out of the map
    /// while owned, so the pointer stays valid for as long as the bundle is loaded —
    /// exactly the guarantee the old leak provided. Reload appends rather than
    /// replaces so a superseded VM stays alive for any in-flight dispatch.
    live: Mutex<HashMap<BundleId, Vec<LuaVm>>>,
    /// VM state that could not be dropped at unload because a dispatch was still in
    /// flight on the VM (non-quiescent). Held for the loader's lifetime so the raw
    /// `bridge_data` pointer the in-flight dispatch still dereferences stays valid —
    /// dropping it would be a use-after-free. This is the deferred-reclaim fallback.
    retired: Mutex<Vec<LuaVm>>,
}

impl LuaLoader {
    /// Create a new `LuaLoader` with the given configuration.
    pub fn new(config: LuaConfig) -> Self {
        Self {
            config,
            live: Mutex::new(HashMap::new()),
            retired: Mutex::new(Vec::new()),
        }
    }

    /// Prepend `entries` to a Lua `package` field (`path` or `cpath`) through the
    /// mlua API.
    ///
    /// This deliberately avoids building Lua source code with string
    /// interpolation: a bundle/guest/abi directory path containing a `"` or a
    /// newline would otherwise break out of the string literal and execute
    /// arbitrary Lua. We read the current value off the `package` table, prepend
    /// the Rust-built entries, and set it back — no path bytes are ever
    /// interpreted as code.
    fn prepend_package_field(
        lua: &Lua,
        bundle: &str,
        field: &str,
        entries: &str,
    ) -> Result<(), RuntimeError> {
        let package: Table = lua
            .globals()
            .get::<Table>("package")
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle.to_owned(),
                    error: format!("Lua VM init failed: missing package table: {e}"),
                })
            })?;

        let current: String = package
            .get::<String>(field)
            .unwrap_or_else(|_: mlua::Error| String::new());

        let combined: String = if current.is_empty() {
            entries.to_owned()
        } else {
            format!("{entries};{current}")
        };

        package.set(field, combined).map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle.to_owned(),
                error: format!("Lua VM init failed: package.{field} update failed: {e}"),
            })
        })
    }

    /// Register `_polyplug_arena_alloc(size) -> integer` on the plugin VM.
    ///
    /// The bridge serves the guest's per-call return buffers from the active
    /// [`CallArena`] published by `lua_dispatch` in the `_polyplug_arena` global.
    /// When no arena is active (the global is 0), it falls back to `host->alloc`,
    /// preserving today's per-value allocation behaviour. Returns the allocated
    /// address as an integer (0 on failure), matching the Lua pointer convention.
    fn register_arena_alloc(
        lua: &Lua,
        bundle: &str,
        host_interface: *const HostApi,
    ) -> Result<(), RuntimeError> {
        // Capture the host pointer as a usize: raw pointers are not Send, but the
        // pointee is 'static HostApi for the runtime lifetime, so reconstructing it
        // inside the (Send) closure is sound.
        let host_addr: usize = host_interface as usize;

        let arena_alloc_fn: Function = lua
            .create_function(move |lua_ctx: &Lua, size: u32| -> mlua::Result<i64> {
                let arena_addr: i64 = lua_ctx.globals().get::<i64>(ARENA_GLOBAL).unwrap_or(0);
                let arena: *mut CallArena = arena_addr as usize as *mut CallArena;
                let ptr: *mut u8 = if arena.is_null() {
                    let host: *const HostApi = host_addr as *const HostApi;
                    if host.is_null() {
                        core::ptr::null_mut()
                    } else {
                        // SAFETY: host points to 'static HostApi data for the runtime
                        // lifetime; align 1 is valid for raw byte buffers.
                        unsafe { ((*host).alloc)(host, size as usize, 1) }
                    }
                } else {
                    // SAFETY: `arena` is the valid per-call CallArena published by
                    // lua_dispatch under the VM lock; alloc bumps within it or chains
                    // a host-allocated overflow block.
                    unsafe { (*arena).alloc(size as usize, 1) }
                };
                Ok(ptr as usize as i64)
            })
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle.to_owned(),
                    error: format!(
                        "Lua VM init failed: _polyplug_arena_alloc creation failed: {e}"
                    ),
                })
            })?;

        lua.globals()
            .set("_polyplug_arena_alloc", arena_alloc_fn)
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle.to_owned(),
                    error: format!("Lua VM init failed: _polyplug_arena_alloc set failed: {e}"),
                })
            })
    }

    /// Resolve a [`BundleSource`] into the Lua source text plus the contextual
    /// information the shared load path needs.
    ///
    /// Returns `(source_text, chunk_name, bundle_dir)`:
    /// - `source_text` — the Lua source to execute in the fresh VM.
    /// - `chunk_name` — the name Lua reports in tracebacks / error messages; for
    ///   on-disk bundles this is the entry file name, for in-memory sources it is
    ///   derived from the manifest bundle name.
    /// - `bundle_dir` — `Some(dir)` for [`BundleSource::Path`] (used to prepend the
    ///   bundle directory to `package.path`/`cpath`), or `None` for in-memory
    ///   sources which have no bundle directory.
    ///
    /// # Single-file limitation for in-memory sources
    ///
    /// [`BundleSource::Code`] and [`BundleSource::Bytes`] carry no bundle directory,
    /// so a bundle-relative `require` of a sibling file vendored next to the entry
    /// (e.g. a bundle-local module) cannot be satisfied. The loader-owned SDK
    /// modules (`polyplug_guest`, `polyplug_abi`) still resolve, because they come
    /// from the compile-time `GUEST_LUA_DIR` / `ABI_LUA_DIR`, not the bundle dir.
    fn resolve_source(
        manifest: &ManifestData,
        source: &BundleSource,
    ) -> Result<(String, String, Option<String>), RuntimeError> {
        match source {
            BundleSource::Path(_) => {
                let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
                    manifest.path.join(&manifest.file)
                } else {
                    return Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
                        bundle: manifest.name.clone(),
                    }));
                };

                if !bundle_path.exists() {
                    return Err(RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "Lua script load failed at {}: file does not exist",
                            bundle_path.display()
                        ),
                    }));
                }

                let source_text: String =
                    std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: format!(
                                "Lua script load failed at {}: {}",
                                bundle_path.display(),
                                e
                            ),
                        })
                    })?;

                let chunk_name: String = bundle_path
                    .file_name()
                    .map(|n: &OsStr| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| bundle_path.display().to_string());

                let bundle_dir_str: String = manifest.path.to_string_lossy().into_owned();
                Ok((source_text, chunk_name, Some(bundle_dir_str)))
            }
            BundleSource::Code(code) => Ok((code.clone(), manifest.name.clone(), None)),
            BundleSource::Bytes(bytes) => {
                let source_text: String =
                    String::from_utf8(bytes.clone()).map_err(|_: std::string::FromUtf8Error| {
                        RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
                            loader: "lua",
                            source_kind: source.kind(),
                            bundle: manifest.name.clone(),
                        })
                    })?;
                Ok((source_text, manifest.name.clone(), None))
            }
        }
    }

    /// Shared load/reload implementation.
    ///
    /// Both `load` and `reload` produce identical behaviour; `reload` only adds a
    /// hot-reload-enabled guard before delegating here. The [`BundleSource`]
    /// selects where the Lua source text comes from — an on-disk entry file
    /// ([`BundleSource::Path`]) or in-memory source text
    /// ([`BundleSource::Code`] / [`BundleSource::Bytes`]).
    fn load_inner(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        let (source_text, chunk_name, bundle_dir): (String, String, Option<String>) =
            Self::resolve_source(manifest, source)?;

        let bundle_id: u64 = manifest.id;

        // For in-memory sources there is no bundle directory; the loader passes the
        // manifest path through to the guest as the bundle path so init still gets a
        // stable identifier, but it is NOT prepended to package.path.
        let bundle_dir_str: String = bundle_dir
            .clone()
            .unwrap_or_else(|| manifest.path.to_string_lossy().into_owned());

        // Create a new Lua VM for this bundle (per-bundle isolation).
        // mlua 0.10: Lua::unsafe_new() enables the FFI module required by LuaJIT plugins.
        // SAFETY: We trust the Lua scripts loaded through this loader. The LuaJIT FFI is
        // required for the polyplug_guest.lua ABI bridge (struct layout, pointer casts).
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // Configure package.path so require("polyplug_guest") and
        // require("polyplug_abi") resolve; for on-disk bundles also add the bundle's
        // own directory, and package.cpath for native modules. All entries are built
        // in Rust and pushed through the mlua API — never interpolated into Lua
        // source — so a path containing quotes or newlines cannot inject code.
        //
        // In-memory sources (Code/Bytes) skip the bundle-dir entries: they carry no
        // bundle directory, so only the loader-owned SDK module dirs are provisioned.
        let guest_dir_fwd: String = GUEST_LUA_DIR.replace('\\', "/");
        let abi_dir_fwd: String = ABI_LUA_DIR.replace('\\', "/");
        let cpath_ext: &str = if cfg!(windows) { "dll" } else { "so" };

        let path_entries: String = match &bundle_dir {
            Some(dir) => {
                let bundle_dir_fwd: String = dir.replace('\\', "/");
                format!(
                    "{bundle_dir_fwd}/?.lua;{bundle_dir_fwd}/?.init.lua;{guest_dir_fwd}/?.lua;{abi_dir_fwd}/?.lua"
                )
            }
            None => format!("{guest_dir_fwd}/?.lua;{abi_dir_fwd}/?.lua"),
        };
        Self::prepend_package_field(&lua, &manifest.name, "path", &path_entries)?;

        if let Some(dir) = &bundle_dir {
            let bundle_dir_fwd: String = dir.replace('\\', "/");
            let cpath_entries: String = format!("{bundle_dir_fwd}/?.{cpath_ext}");
            Self::prepend_package_field(&lua, &manifest.name, "cpath", &cpath_entries)?;
        }

        // Execute the script. This defines polyplug_init in the global environment.
        // The chunk name is shown in Lua tracebacks/error messages.
        lua.load(&source_text)
            .set_name(&chunk_name)
            .exec()
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("Lua script load failed for {}: {}", chunk_name, e),
                })
            })?;

        // Derive bundle name for error messages.
        let bundle_name: String = chunk_name;

        // Retrieve polyplug_init global function.
        let init_fn: Function =
            lua.globals()
                .get::<Function>("polyplug_init")
                .map_err(|_: mlua::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua plugin missing polyplug_init function: bundle={}",
                            bundle_name
                        ),
                    })
                })?;

        // Get HostApi pointer from runtime.
        // The interface already has the runtime pointer set.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Register the per-call arena allocator bridge so the guest can route its
        // return-value buffers through the host's CallArena (zero host allocations
        // after warmup). Must be registered before dispatch; init runs first but
        // dispatch happens later, so registering here is sufficient.
        Self::register_arena_alloc(&lua, &bundle_name, host_interface)?;

        // Open the init-bundle window for BOTH the init call and the registration
        // loop below: `host_register_guest_contract` attributes each registration to
        // the bundle id on top of this stack, and the `register_guest_contract` calls
        // happen later in this function (after init only populates _G._polyplug_handlers).
        // The guard's Drop pops once on every exit path — including the `?`
        // early-returns between here and the end of the registration loop — so the
        // stack never leaks an entry and registrations carry the real bundle id.
        let _init_window: InitBundleGuard<'_> = InitBundleGuard::enter(runtime, bundle_id);

        // Call polyplug_init — it populates _G._polyplug_handlers.
        // New signature: polyplug_init(host, ctx) - self-passing pattern.
        // SAFETY: bundle_path_static outlives this call; leaked intentionally.
        let bundle_path_static: &'static str = Box::leak(bundle_dir_str.clone().into_boxed_str());
        let ctx: polyplug_abi::BundleInitContext = polyplug_abi::BundleInitContext {
            bundle_path: polyplug_abi::StringView {
                ptr: bundle_path_static.as_ptr(),
                len: bundle_path_static.len(),
            },
            bundle_id,
        };
        // Pass HostApi pointer and BundleInitContext pointer to Lua.
        // The HostApi uses self-passing pattern - Lua guest code will pass it back
        // as the first parameter to each HostApi function call.
        let host_interface_i64: i64 = host_interface as usize as i64;
        let ctx_ptr: i64 = &ctx as *const polyplug_abi::BundleInitContext as i64;
        let init_result: Result<(), mlua::Error> =
            init_fn.call::<()>((host_interface_i64, ctx_ptr));

        init_result.map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.clone(),
                error: format!("Lua polyplug_init raised error: {}", e),
            })
        })?;

        // Read the handler table that polyplug_init populated. Its shape is
        // per-contract: `_polyplug_handlers[contract_name] = { contract_version,
        // plugin_name, functions }`. Multi-contract bundles add one entry per
        // contract; the loop below registers EVERY entry.
        let handlers: Table =
            lua.globals()
                .get::<Table>("_polyplug_handlers")
                .map_err(|_: mlua::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua plugin missing _polyplug_handlers: bundle={}",
                            bundle_name
                        ),
                    })
                })?;

        // Iterate every contract entry and register each one. The Lua VM is shared
        // across all contracts in this bundle: each per-contract LuaLoaderData holds
        // its own clone of the `Lua` handle, which ref-counts the underlying VM so it
        // stays alive for the process lifetime (Lua plugins are never unloaded).
        // Per-bundle VM state collected during this load. Each contract's box is
        // owned here (not leaked); ownership is moved into the loader's `live` map
        // after all contracts register successfully. The dispatch `bridge_data`
        // points at each box's stable heap address, captured below.
        let mut bundle_vm_state: Vec<LuaVm> = Vec::new();

        let mut registered: u32 = 0_u32;
        for pair in handlers.pairs::<String, Table>() {
            let (contract_name_str, entry): (String, Table) = pair.map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("Lua handlers iteration error: {}", e),
                })
            })?;

            let contract_version: u32 = entry.get::<u32>("contract_version").unwrap_or(1_u32);

            let plugin_name_str: String = entry
                .get::<String>("plugin_name")
                .unwrap_or_else(|_: mlua::Error| bundle_name.clone());

            let functions_table: Table =
                entry.get::<Table>("functions").map_err(|e: mlua::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua handlers error: missing functions table for contract '{}': {}",
                            contract_name_str, e
                        ),
                    })
                })?;

            // Count functions in the table (0-indexed integers).
            let function_count: u32 = {
                let mut count: u32 = 0_u32;
                let mut idx: i64 = 0_i64;
                loop {
                    let v: Value = functions_table.get::<Value>(idx).unwrap_or(Value::Nil);
                    if v == Value::Nil {
                        break;
                    }
                    count += 1;
                    idx += 1;
                }
                count
            };

            // Collect Lua functions into a Vec for VM dispatch.
            let mut lua_functions: Vec<Function> = Vec::with_capacity(function_count as usize);
            for slot_idx in 0..function_count {
                let lua_fn: Function = functions_table.get::<Function>(slot_idx as i64).map_err(
                    |e: mlua::Error| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: format!(
                                "Lua function slot {} error for contract '{}': {}",
                                slot_idx, contract_name_str, e
                            ),
                        })
                    },
                )?;
                lua_functions.push(lua_fn);
            }

            // Build contract_id from contract_name and version.
            let cid: GuestContractId = GuestContractId::new(&contract_name_str, contract_version);

            // Create LuaLoaderData with a clone of the Lua VM handle and the
            // contract's functions. Cloning `Lua` shares the same underlying VM.
            let loader_data: LuaVm = LuaVm(Box::new(LuaLoaderData {
                _vm: lua.clone(),
                functions: lua_functions,
                in_dispatch_threads: Mutex::new(Vec::new()),
                dispatch_lock: Mutex::new(()),
            }));

            // The box's heap address is stable across later moves of the `LuaVm`/`Box`
            // (moving them moves the pointer, not the allocation), so it stays valid
            // once the box is owned by the loader's `live` map below.
            let loader_data_ptr: *const LuaLoaderData = loader_data.as_ptr();
            bundle_vm_state.push(loader_data);

            // Build GuestContractInterface with VM dispatch.
            let plugin_interface: GuestContractInterface = GuestContractInterface {
                contract_id: cid,
                contract_version: Version {
                    major: contract_version,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::VirtualMachine,
                create_instance: lua_create_instance,
                destroy_instance: lua_destroy_instance,
                dispatch: DispatchMechanisms {
                    vm: VmDispatch {
                        call: lua_dispatch,
                        loader_data: VmLoaderData {
                            data: loader_data_ptr as *mut LuaLoaderData as *mut core::ffi::c_void,
                        },
                    },
                },
            };

            // The interface is passed to register_guest_contract, which COPIES every
            // field into the registry's own `Arc<GuestContractInterface>` during the
            // synchronous call (the copy's `dispatch.vm.bridge_data` still points at
            // our owned `LuaLoaderData` box). The registry never retains this pointer,
            // so a stack value valid for the call is sufficient — no leak, which keeps
            // a load→unload→load loop bounded.
            let interface_for_reg: GuestContractInterface = plugin_interface;
            let static_interface: *const GuestContractInterface =
                &interface_for_reg as *const GuestContractInterface;

            // Build the PluginDescriptor strings. register_guest_contract copies the
            // borrowed StringViews into owned Strings during the call, so stack-owned
            // strings valid for the call suffice — no leak (keeps the loop bounded).
            //
            // The descriptor's human-readable `contract_name` must be the canonical
            // `"<name>@<major>"` form so it matches what every other language registers
            // (rust/cpp/python/js generated code emit this full form directly). The bare
            // `contract_name_str` + `contract_version` are the hash inputs already consumed
            // by `GuestContractId::new` above; reusing the bare name in the descriptor would
            // diverge from the other loaders and trip the registry's collision check.
            let contract_display_name: String =
                format!("{}@{}", contract_name_str, contract_version);

            let descriptor: PluginDescriptor = PluginDescriptor {
                name: StringView {
                    ptr: plugin_name_str.as_ptr(),
                    len: plugin_name_str.len(),
                },
                contract_name: StringView {
                    ptr: contract_display_name.as_ptr(),
                    len: contract_display_name.len(),
                },
                version: Version {
                    major: contract_version,
                    minor: 0,
                    patch: 0,
                },
            };

            // Call register_guest_contract via the HostApi self-passing pattern.
            // SAFETY: `host_interface` is a valid HostApi pointer for this call.
            // `descriptor` is stack-allocated and valid for this call (register_guest_contract must copy
            // any data it needs to retain — the contract is that descriptor is borrowed for the call only).
            // `static_interface` is a stack value; the registry copies it during the call.
            let reg_result: AbiError = unsafe {
                ((*host_interface).register_guest_contract)(
                    host_interface,
                    &descriptor as *const PluginDescriptor,
                    static_interface,
                )
            };

            if !reg_result.is_ok() {
                // A contract earlier in this loop may already be registered with the
                // registry pointing at its box in `bundle_vm_state`. Retire those
                // boxes (keep them alive for the loader's lifetime) rather than
                // dropping them here, which would dangle the registry's bridge_data.
                self.retire_vm_state(bundle_vm_state);
                return Err(RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name,
                    error: format!(
                        "register_guest_contract error for contract '{}': code={:?}",
                        contract_name_str, reg_result.code
                    ),
                }));
            }

            registered += 1;
        }

        if registered == 0 {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name,
                error: "Lua plugin registered no contracts: _polyplug_handlers is empty".to_owned(),
            }));
        }

        // Take ownership of this bundle's VM state. Reload appends to any existing
        // entry instead of replacing it, so a superseded VM stays alive for an
        // in-flight dispatch (retire-not-drop across reload).
        let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.entry(BundleId::from_u64(bundle_id))
            .or_default()
            .append(&mut bundle_vm_state);

        Ok(())
    }

    /// Move per-bundle VM state into the loader's `retired` list, keeping it alive
    /// for the loader's lifetime. Used when a box must not be dropped because the
    /// registry (or an in-flight dispatch) still references its heap address.
    fn retire_vm_state(&self, mut state: Vec<LuaVm>) {
        if state.is_empty() {
            return;
        }
        let mut retired: std::sync::MutexGuard<'_, Vec<LuaVm>> =
            self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        retired.append(&mut state);
    }

    /// Number of live VM-state entries currently owned for `bundle_id`.
    #[cfg(test)]
    fn live_vm_count(&self, bundle_id: BundleId) -> usize {
        let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.get(&bundle_id).map(Vec::len).unwrap_or(0)
    }

    /// Number of VM-state entries retired (deferred reclaim) by this loader.
    #[cfg(test)]
    fn retired_vm_count(&self) -> usize {
        let retired: std::sync::MutexGuard<'_, Vec<LuaVm>> =
            self.retired.lock().unwrap_or_else(PoisonError::into_inner);
        retired.len()
    }
}

impl BundleLoader for LuaLoader {
    fn runtime_name(&self) -> &'static str {
        "lua"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        // The Lua loader serves every BundleSource: Path reads the on-disk entry
        // file, Code evaluates in-memory source text, and Bytes is UTF-8 source
        // text. All three converge on the same compile/init/register path.
        self.load_inner(manifest, source, runtime)
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }
        // reload re-reads the on-disk entry file (only path-backed bundles can be
        // hot-reloaded — there is no on-disk artifact to re-read for in-memory
        // sources, and reload is gated on hot_reload_enabled above).
        self.load_inner(
            manifest,
            &BundleSource::Path(manifest.path.clone()),
            runtime,
        )
    }

    /// Reclaim the bundle's Lua VM at a quiescence point.
    ///
    /// Called by the runtime AFTER `invalidate_bundle` has removed the bundle from
    /// the registry, so no dispatch can *resolve* this contract anew.
    ///
    /// # Host-coordination contract (best-effort quiescence)
    /// `call_guest_method` deliberately releases the registry lock between resolving
    /// an interface and the moment this VM registers the call in
    /// `in_dispatch_threads` (runtime.rs — no lock is held across guest dispatch, for
    /// concurrency/reentrancy). A call that resolved *just before* `invalidate_bundle`
    /// is therefore not guaranteed to be visible in `in_dispatch_threads` here. So,
    /// exactly like hot-reload, the host MUST NOT call a bundle's contracts
    /// concurrently with unloading it (see [`crate`]'s `Runtime::unload_bundle` doc and
    /// the trusted-same-process model in TRUST_MODEL.md). `in_dispatch_threads` is a
    /// best-effort defense-in-depth, not a complete guarantee.
    ///
    /// For each `LuaLoaderData` owned by the bundle:
    /// - if its `in_dispatch_threads` is EMPTY (the expected case when the host has
    ///   honored the contract), the box is dropped here, dropping its `Lua` VM handle
    ///   — true reclaim;
    /// - if it is NON-EMPTY (a dispatch is visibly in flight on another thread),
    ///   dropping the box would free the VM out from under that dispatch (a UAF), so
    ///   the box is moved into the loader-owned `retired` list instead and a single
    ///   line is logged. Reclaim is deferred; the VM stays alive for the loader's
    ///   lifetime.
    ///
    /// Spin-waiting is deliberately NOT used: a same-thread re-entrant unload would
    /// deadlock against its own in-flight dispatch.
    // `_reclaim_safe` is ignored: VM dispatch is mediated and quiescence-tracked via
    // `in_dispatch_threads`, so this loader makes its own reclaim-vs-retire decision
    // independent of the runtime's Arc-based hint (unlike zero-overhead native dispatch).
    fn unload(
        &self,
        bundle_id: BundleId,
        _runtime: &Runtime,
        _reclaim_safe: bool,
    ) -> Result<(), RuntimeError> {
        let state: Vec<LuaVm> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            match live.remove(&bundle_id) {
                Some(v) => v,
                None => return Ok(()),
            }
        };

        for data in state {
            let in_flight: bool = {
                let threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = data
                    .data()
                    .in_dispatch_threads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                !threads.is_empty()
            };

            if in_flight {
                eprintln!(
                    "[polyplug_lua] unload of bundle {:#x} deferred: a dispatch is in flight on this VM; retiring its state to avoid a use-after-free",
                    bundle_id.id()
                );
                self.retire_vm_state(vec![data]);
            } else {
                // Quiescent: dropping `data` drops the cloned `Lua` handle. The VM is
                // freed once the last clone for this bundle is dropped — true reclaim.
                drop(data);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use core::sync::atomic::AtomicUsize;
    use core::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::sync::Barrier;

    use super::*;

    #[test]
    fn lua_runtime_name() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        assert_eq!(loader.runtime_name(), "lua");
    }

    /// A minimal valid Lua plugin registering the `test.unload@1` contract.
    fn unload_plugin_script() -> &'static [u8] {
        br#"
local function impl_noop(_a, _o) end
function polyplug_init(_host, _ctx)
    _G._polyplug_handlers = {
        ["test.unload"] = {
            contract_version = 1,
            plugin_name      = "test-unload",
            functions        = { [0] = impl_noop },
        },
    }
end
"#
    }

    /// Write a temp bundle directory with manifest.toml + bundle.lua and return the
    /// dir (kept alive) plus a ManifestData for it.
    fn write_unload_bundle(name: &str) -> (tempfile::TempDir, ManifestData) {
        let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("bundle.lua"), unload_plugin_script())
            .expect("write bundle.lua");
        let manifest: ManifestData = ManifestData {
            id: polyplug_utils::bundle_id(name),
            name: name.to_owned(),
            runtime: "lua".to_owned(),
            file: "bundle.lua".to_owned(),
            path: dir.path().to_path_buf(),
            version: String::new(),
            provides: Vec::new(),
            function_count: std::collections::HashMap::new(),
            dependencies: Vec::new(),
            needs_reinit_on_dep_reload: false,
            bundle_dependencies: Vec::new(),
        };
        (dir, manifest)
    }

    /// A quiescent unload (no in-flight dispatch) drops the bundle's VM state from
    /// the loader's owned map — true reclaim — and does not retire anything.
    #[test]
    fn unload_quiescent_reclaims_vm_state() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        let runtime: std::sync::Arc<polyplug::Runtime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(LuaLoader::new(LuaConfig::default()))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("lua_unload_quiescent");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("load must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "one contract's VM state must be owned after load"
        );

        loader
            .unload(bundle_id, &runtime, true)
            .expect("unload must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "quiescent unload must drop the bundle's VM state (true reclaim)"
        );
        assert_eq!(
            loader.retired_vm_count(),
            0,
            "quiescent unload must NOT retire any state"
        );
    }

    /// A load→unload→load loop on the same bundle must not grow the loader's live
    /// map unboundedly: reclaim keeps memory bounded at one entry.
    #[test]
    fn unload_load_loop_is_bounded() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        let runtime: std::sync::Arc<polyplug::Runtime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(LuaLoader::new(LuaConfig::default()))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("lua_unload_loop");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        for _ in 0..5 {
            loader
                .load(
                    &manifest,
                    &BundleSource::Path(manifest.path.clone()),
                    &runtime,
                )
                .expect("load must succeed");
            assert_eq!(
                loader.live_vm_count(bundle_id),
                1,
                "live map must hold exactly one entry per load"
            );
            loader
                .unload(bundle_id, &runtime, true)
                .expect("unload must succeed");
            // Driving the loader directly bypasses `Runtime::unload_bundle`'s registry
            // invalidation, so the test mirrors it explicitly.
            runtime
                .registry()
                .invalidate_bundle(bundle_id)
                .expect("invalidate must succeed");
            assert_eq!(
                loader.live_vm_count(bundle_id),
                0,
                "unload must reclaim the entry each iteration"
            );
        }
        assert_eq!(
            loader.retired_vm_count(),
            0,
            "no iteration should have deferred reclaim"
        );
    }

    /// When a dispatch is in flight (the bundle's `in_dispatch_threads` is marked
    /// non-empty), unload must DEFER reclaim: the state is moved to the retired list
    /// (kept alive, no UAF) rather than dropped.
    #[test]
    fn unload_non_quiescent_defers_reclaim() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        let runtime: std::sync::Arc<polyplug::Runtime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(LuaLoader::new(LuaConfig::default()))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("lua_unload_deferred");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("load must succeed");

        // Simulate an in-flight dispatch by registering a fake thread id in the
        // bundle's tracking vec. This is exactly the state the dispatch guard would
        // leave while a call is mid-flight on another thread.
        {
            let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
                loader.live.lock().unwrap_or_else(PoisonError::into_inner);
            let state: &Vec<LuaVm> = live.get(&bundle_id).expect("bundle must be live");
            let mut threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = state[0]
                .data()
                .in_dispatch_threads
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            threads.push(std::thread::current().id());
        }

        loader
            .unload(bundle_id, &runtime, true)
            .expect("unload must succeed even when non-quiescent");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "unload must remove the bundle from the live map"
        );
        assert_eq!(
            loader.retired_vm_count(),
            1,
            "non-quiescent unload must RETIRE the state (deferred reclaim), not drop it"
        );
    }

    /// Build a leaked LuaLoaderData with the given Lua functions and return a
    /// VmLoaderData pointing at it plus a borrow for direct flag inspection.
    ///
    /// The data is intentionally leaked so the raw pointer inside VmLoaderData
    /// stays valid for the whole test, mirroring the loader's `Box::into_raw`.
    fn make_loader_data(
        vm: Lua,
        functions: Vec<Function>,
    ) -> (VmLoaderData, &'static LuaLoaderData) {
        let boxed: Box<LuaLoaderData> = Box::new(LuaLoaderData {
            _vm: vm,
            functions,
            in_dispatch_threads: Mutex::new(Vec::new()),
            dispatch_lock: Mutex::new(()),
        });
        let ptr: *mut LuaLoaderData = Box::into_raw(boxed);
        // SAFETY: ptr was just produced by Box::into_raw and is never freed in the
        // test, so the &'static borrow is valid for the test's lifetime.
        let data_ref: &'static LuaLoaderData = unsafe { &*ptr };
        let vm_loader_data: VmLoaderData = VmLoaderData {
            data: ptr as *mut core::ffi::c_void,
        };
        (vm_loader_data, data_ref)
    }

    /// A normal (non-reentrant) Lua dispatch succeeds and clears the flag.
    #[test]
    fn lua_dispatch_normal_call_succeeds() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };
        // A trivial guest function that ignores its (args, out) integer pointers.
        let noop: Function = lua
            .create_function(|_, (_a, _o): (i64, i64)| Ok(()))
            .expect("create_function should succeed");
        let (vm_loader_data, data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![noop]);

        let mut out_buf: i32 = 0;
        // SAFETY: vm_loader_data wraps a live LuaLoaderData; the out pointer is a
        // valid local i32; the guest function ignores both pointers.
        let err: AbiError = unsafe {
            lua_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                &mut out_buf as *mut i32 as *mut (),
                core::ptr::null_mut(),
            )
        };
        assert!(err.is_ok(), "normal dispatch should return Ok");
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after a normal dispatch"
        );
    }

    /// A genuine same-VM reentrant dispatch — triggered from inside a guest call —
    /// returns ReentrantCall, and the VM stays usable for a later normal dispatch.
    #[test]
    fn lua_dispatch_reentrant_call_is_rejected_and_vm_recovers() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // The reentrant guest function re-invokes lua_dispatch on the SAME
        // loader_data while it is itself executing (the flag is set), simulating a
        // plugin→plugin cross-call that resolves back into this VM. It records the
        // nested call's returned code in a global so the test can assert on it.
        // The loader_data pointer is shared via an Arc<AtomicUsize> (Send + Sync,
        // as mlua's `send` closures require Send); it is reconstructed inside.
        let loader_data_cell: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        let cell_for_fn: Arc<AtomicUsize> = Arc::clone(&loader_data_cell);

        let reentrant_fn: Function = lua
            .create_function(move |lua_ctx: &Lua, (_a, _o): (i64, i64)| {
                let ptr_usize: usize = cell_for_fn.load(Ordering::Acquire);
                let vm_loader_data: VmLoaderData = VmLoaderData {
                    data: ptr_usize as *mut core::ffi::c_void,
                };
                // SAFETY: the cell holds the live leaked LuaLoaderData pointer set
                // up by the test before dispatch; the guest function ignores the
                // forwarded args/out pointers.
                let nested: AbiError = unsafe {
                    lua_dispatch(
                        vm_loader_data,
                        GuestContractInstance::null(),
                        0,
                        core::ptr::null(),
                        core::ptr::null_mut(),
                        core::ptr::null_mut(),
                    )
                };
                lua_ctx.globals().set("_nested_code", nested.code as i64)?;
                Ok(())
            })
            .expect("create_function should succeed");

        let (vm_loader_data, data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![reentrant_fn]);
        loader_data_cell.store(vm_loader_data.data as usize, Ordering::Release);

        // Outer dispatch: sets the flag, runs the guest fn, which re-enters.
        // SAFETY: vm_loader_data wraps the live leaked LuaLoaderData.
        let outer: AbiError = unsafe {
            lua_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert!(outer.is_ok(), "outer dispatch should complete Ok");

        // The nested dispatch must have been rejected with ReentrantCall.
        let nested_code: i64 = data_ref
            ._vm
            .globals()
            .get::<i64>("_nested_code")
            .expect("nested code global must be set by the guest fn");
        assert_eq!(
            nested_code,
            AbiErrorCode::ReentrantCall as i64,
            "nested same-VM dispatch must return ReentrantCall"
        );

        // The tracking is cleared and the VM is still usable for a fresh dispatch.
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after the outer dispatch returns"
        );
        // SAFETY: vm_loader_data still wraps the live leaked LuaLoaderData.
        let recovered: AbiError = unsafe {
            lua_dispatch(
                vm_loader_data,
                GuestContractInstance::null(),
                0,
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            )
        };
        assert!(
            recovered.is_ok(),
            "VM must remain usable after a rejected reentrant call"
        );
    }

    /// A concurrent dispatch from ANOTHER thread into the same VM must SUCCEED,
    /// not be rejected as reentrancy. The thread-aware guard only refuses a
    /// same-thread nested call; a cross-thread caller proceeds and mlua's internal
    /// `send` lock serializes the two calls.
    ///
    /// Choreography proves a true in-flight overlap: thread A's guest fn registers
    /// thread A in the tracking vec, then blocks on a barrier. While A is parked
    /// mid-dispatch, the main thread (a different thread) dispatches into the SAME
    /// VM. The main call passes the reentrancy check (different thread id) and then
    /// blocks on mlua's internal VM lock held by A. Releasing the barrier lets A
    /// finish, freeing the lock so the main call completes with Ok.
    #[test]
    fn lua_dispatch_cross_thread_concurrent_call_succeeds() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // Two barriers shared with the guest fn: `entered` lets the main thread
        // know A is mid-dispatch (inside the VM lock); `release` lets A finish
        // only after the main thread has launched its concurrent dispatch.
        let entered: Arc<Barrier> = Arc::new(Barrier::new(2));
        let release: Arc<Barrier> = Arc::new(Barrier::new(2));
        let entered_for_fn: Arc<Barrier> = Arc::clone(&entered);
        let release_for_fn: Arc<Barrier> = Arc::clone(&release);

        let blocking_fn: Function = lua
            .create_function(move |_lua_ctx: &Lua, (_a, _o): (i64, i64)| {
                // Signal that thread A is now inside the dispatch (VM lock held).
                entered_for_fn.wait();
                // Hold the dispatch (and the VM lock) until the main thread has
                // begun its concurrent dispatch.
                release_for_fn.wait();
                Ok(())
            })
            .expect("create_function should succeed");
        // The concurrent caller dispatches THIS function (fn_id 1) — it must not
        // touch the barriers, or it would park with no partner and deadlock.
        let noop_fn: Function = lua
            .create_function(|_lua_ctx: &Lua, (_a, _o): (i64, i64)| Ok(()))
            .expect("create_function should succeed");

        let (vm_loader_data, data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![blocking_fn, noop_fn]);

        // VmLoaderData is a thin pointer wrapper; move its address across the
        // thread boundary as a usize to satisfy Send, then rebuild it inside.
        let data_addr: usize = vm_loader_data.data as usize;

        let handle: std::thread::JoinHandle<AbiError> = std::thread::spawn(move || {
            let vm_loader_data_a: VmLoaderData = VmLoaderData {
                data: data_addr as *mut core::ffi::c_void,
            };
            // SAFETY: data_addr is the live leaked LuaLoaderData pointer; it
            // outlives all threads in this test. The guest fn ignores its args.
            unsafe {
                lua_dispatch(
                    vm_loader_data_a,
                    GuestContractInstance::null(),
                    0,
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            }
        });

        // Wait until thread A is confirmed inside its dispatch.
        entered.wait();

        // Now, from THIS (different) thread, dispatch into the SAME VM. This must
        // not be rejected; it blocks on mlua's lock until A releases it.
        let main_handle: std::thread::JoinHandle<AbiError> = std::thread::spawn(move || {
            let vm_loader_data_b: VmLoaderData = VmLoaderData {
                data: data_addr as *mut core::ffi::c_void,
            };
            // SAFETY: same live leaked pointer as above. fn_id 1 is the no-op
            // function — dispatching fn_id 0 here would re-enter the barrier
            // choreography with no partner and deadlock.
            unsafe {
                lua_dispatch(
                    vm_loader_data_b,
                    GuestContractInstance::null(),
                    1,
                    core::ptr::null(),
                    core::ptr::null_mut(),
                    core::ptr::null_mut(),
                )
            }
        });

        // Unblock thread A so it finishes and frees the VM lock, allowing the
        // concurrent dispatch to complete.
        release.wait();

        let a_result: AbiError = handle.join().expect("thread A must not panic");
        let b_result: AbiError = main_handle
            .join()
            .expect("concurrent thread must not panic");

        assert!(a_result.is_ok(), "the initial dispatch must succeed");
        assert!(
            b_result.is_ok(),
            "a concurrent cross-thread dispatch must succeed, not return ReentrantCall (got code {})",
            b_result.code
        );
        assert!(
            data_ref
                .in_dispatch_threads
                .lock()
                .expect("tracking mutex must not be poisoned")
                .is_empty(),
            "thread tracking must be empty after both dispatches return"
        );
    }

    /// Concurrency regression for the racy arena publish→call→clear span in
    /// `lua_dispatch`.
    ///
    /// `lua_dispatch` publishes the per-call arena as the `_polyplug_arena` VM
    /// global, calls the guest (which allocates its return buffer through the
    /// `_polyplug_arena_alloc` bridge reading that global), then clears the global
    /// to 0 — THREE separate mlua operations. mlua's internal `send` lock serializes
    /// each individual operation but NOT the sequence, so the lock is briefly free
    /// between A's publish and A's call, and between A's call and A's clear. In
    /// those gaps a concurrent dispatch B can publish its OWN arena (so A's guest
    /// allocates from B's arena) or clear the global to 0 mid-span (so A's guest
    /// falls back to host->alloc). Both corrupt A's arena-backed return.
    ///
    /// This test hammers two threads dispatching into the SAME VM with DISTINCT
    /// per-thread arenas for many iterations. Each guest fn allocates from the
    /// bridge and reports the address; the Rust side verifies the address lands in
    /// the buffer of the arena THAT THREAD dispatched with. A single misattributed
    /// allocation (the bug) lands in the other thread's buffer or is null, failing
    /// the per-iteration assertion. With the per-VM `dispatch_lock` holding the
    /// whole publish→call→clear span atomic, every allocation is correctly
    /// attributed. The two buffers are deliberately disjoint so a cross-arena
    /// allocation is unambiguously detectable.
    #[test]
    fn lua_dispatch_concurrent_arena_returns_stay_isolated() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // Register the production arena bridge with a null host: with a real arena
        // active the bridge bumps the arena; if the global is wrongly cleared to 0
        // the bridge falls back to host->alloc, which with a null host yields 0.
        LuaLoader::register_arena_alloc(&lua, "concurrent-arena-test", core::ptr::null())
            .expect("register_arena_alloc must succeed");

        // Both worker functions are identical: allocate 64 bytes via the bridge and
        // write the returned address into the out slot. fn_id selects which thread.
        let make_alloc_fn = |lua: &Lua| -> Function {
            lua.create_function(
                |lua_ctx: &Lua, (_a, out_ptr): (i64, i64)| -> mlua::Result<()> {
                    let alloc: Function =
                        lua_ctx.globals().get::<Function>("_polyplug_arena_alloc")?;
                    let addr: i64 = alloc.call::<i64>(64_u32)?;
                    let out: *mut i64 = out_ptr as usize as *mut i64;
                    // SAFETY: out_ptr is a valid local i64 supplied by the test.
                    unsafe { *out = addr };
                    Ok(())
                },
            )
            .expect("create_function should succeed")
        };
        let fn0: Function = make_alloc_fn(&lua);
        let fn1: Function = make_alloc_fn(&lua);

        let (vm_loader_data, _data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![fn0, fn1]);
        let data_addr: usize = vm_loader_data.data as usize;

        // Per-thread disjoint 4 KiB buffers + arenas, leaked so their addresses stay
        // valid across the worker threads. Null host: 64-byte allocs fit the primary
        // region (no overflow), and each iteration resets its arena.
        let buf_a: &'static mut [u8] = Box::leak(vec![0_u8; 4096].into_boxed_slice());
        let buf_b: &'static mut [u8] = Box::leak(vec![0_u8; 4096].into_boxed_slice());
        let a_lo: usize = buf_a.as_ptr() as usize;
        let a_hi: usize = a_lo + buf_a.len();
        let b_lo: usize = buf_b.as_ptr() as usize;
        let b_hi: usize = b_lo + buf_b.len();
        let arena_a: &'static mut CallArena =
            Box::leak(Box::new(CallArena::new(buf_a, core::ptr::null())));
        let arena_b: &'static mut CallArena =
            Box::leak(Box::new(CallArena::new(buf_b, core::ptr::null())));
        let arena_a_addr: usize = arena_a as *mut CallArena as usize;
        let arena_b_addr: usize = arena_b as *mut CallArena as usize;

        const ITERS: usize = 2_000;
        let start: Arc<Barrier> = Arc::new(Barrier::new(2));
        let start_a: Arc<Barrier> = Arc::clone(&start);
        let start_b: Arc<Barrier> = Arc::clone(&start);

        // Worker A: fn_id 0, arena_a, buffer [a_lo, a_hi). Each iteration resets its
        // arena, dispatches, and verifies the allocation landed in its own buffer.
        let handle_a: std::thread::JoinHandle<Result<(), String>> = std::thread::spawn(move || {
            start_a.wait();
            for i in 0..ITERS {
                // SAFETY: arena_a_addr is the live leaked CallArena for this
                // worker; only this worker resets/uses it (fn_id 0).
                let arena: &mut CallArena = unsafe { &mut *(arena_a_addr as *mut CallArena) };
                arena.reset();
                let mut out: i64 = 0;
                let vm: VmLoaderData = VmLoaderData {
                    data: data_addr as *mut core::ffi::c_void,
                };
                // SAFETY: data_addr is the live leaked LuaLoaderData; out is a
                // valid local; arena_a_addr is a valid CallArena.
                let err: AbiError = unsafe {
                    lua_dispatch(
                        vm,
                        GuestContractInstance::null(),
                        0,
                        core::ptr::null(),
                        &mut out as *mut i64 as *mut (),
                        arena_a_addr as *mut CallArena,
                    )
                };
                if !err.is_ok() {
                    return Err(format!("A iter {i}: dispatch failed code={}", err.code));
                }
                let p: usize = out as usize;
                if !(p >= a_lo && p < a_hi) {
                    return Err(format!(
                        "A iter {i}: allocation {p:#x} escaped arena A buffer [{a_lo:#x}, {a_hi:#x}) — racy publish/clear misattributed it"
                    ));
                }
            }
            Ok(())
        });

        // Worker B: fn_id 1, arena_b, buffer [b_lo, b_hi).
        let handle_b: std::thread::JoinHandle<Result<(), String>> = std::thread::spawn(move || {
            start_b.wait();
            for i in 0..ITERS {
                // SAFETY: arena_b_addr is the live leaked CallArena for this
                // worker; only this worker resets/uses it (fn_id 1).
                let arena: &mut CallArena = unsafe { &mut *(arena_b_addr as *mut CallArena) };
                arena.reset();
                let mut out: i64 = 0;
                let vm: VmLoaderData = VmLoaderData {
                    data: data_addr as *mut core::ffi::c_void,
                };
                // SAFETY: data_addr is the live leaked LuaLoaderData; out is a
                // valid local; arena_b_addr is a valid CallArena.
                let err: AbiError = unsafe {
                    lua_dispatch(
                        vm,
                        GuestContractInstance::null(),
                        1,
                        core::ptr::null(),
                        &mut out as *mut i64 as *mut (),
                        arena_b_addr as *mut CallArena,
                    )
                };
                if !err.is_ok() {
                    return Err(format!("B iter {i}: dispatch failed code={}", err.code));
                }
                let p: usize = out as usize;
                if !(p >= b_lo && p < b_hi) {
                    return Err(format!(
                        "B iter {i}: allocation {p:#x} escaped arena B buffer [{b_lo:#x}, {b_hi:#x}) — racy publish/clear misattributed it"
                    ));
                }
            }
            Ok(())
        });

        let a_outcome: Result<(), String> = handle_a.join().expect("thread A must not panic");
        let b_outcome: Result<(), String> = handle_b.join().expect("thread B must not panic");
        if let Err(e) = a_outcome {
            panic!("{e}");
        }
        if let Err(e) = b_outcome {
            panic!("{e}");
        }
    }

    /// Regression test for the package.path code-injection vulnerability.
    ///
    /// A directory path containing a `"`, a newline, and Lua source that would
    /// set a global must be stored verbatim into `package.path` and MUST NOT be
    /// interpreted as Lua code. If the old `format!`-into-source approach were
    /// still used, the embedded `_INJECTED = true` statement would execute and
    /// set the global; with the mlua-API approach it stays inert text.
    #[test]
    fn prepend_package_field_does_not_execute_injected_code() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        let malicious: &str = "/tmp/evil\";_G._INJECTED=true;package.path=\"x/?.lua";
        let entries: String = format!("{malicious}/?.lua");

        LuaLoader::prepend_package_field(&lua, "test-bundle", "path", &entries)
            .expect("prepend_package_field should succeed for any path bytes");

        // The injected statement must NOT have run.
        let injected: Value = lua
            .globals()
            .get::<Value>("_INJECTED")
            .expect("globals lookup should not fail");
        assert_eq!(
            injected,
            Value::Nil,
            "injected Lua code executed — package.path was interpreted as source"
        );

        // The malicious bytes must be present verbatim in package.path.
        let package: Table = lua
            .globals()
            .get::<Table>("package")
            .expect("package table must exist");
        let path: String = package
            .get::<String>("path")
            .expect("package.path must be a string");
        assert!(
            path.contains(malicious),
            "package.path should contain the raw entry verbatim: {path}"
        );
    }
}
