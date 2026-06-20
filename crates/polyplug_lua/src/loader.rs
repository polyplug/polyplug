//! LuaJIT VM initialization and plugin loader implementation.
//!
//! Loads Lua plugin bundles via the embedded LuaJIT VM (mlua).
//! Each bundle gets its own Lua VM for complete isolation between bundles
//! and between polyplug Runtime instances.

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::thread::ThreadId;

use mlua::Function;
use mlua::Lua;
use mlua::RegistryKey;
use mlua::Table;
use mlua::Value;

use crate::config::LuaConfig;
use polyplug::Runtime;
use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::ManifestData;
use polyplug::logger::LoggerHandle;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::SupportedLanguage;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::LogLevel;
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
/// `polyplug_init` only RETURNS the registrations table; the actual
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

/// Loader-specific data for Lua plugin dispatch and per-instance lifecycle.
///
/// Each bundle gets its own Lua VM, ensuring complete isolation between
/// bundles and between polyplug Runtime instances.
///
/// The loader — not the guest module — owns per-instance state. `create_instance`
/// calls the contract's author `factory(host_ptr)` to build a fresh impl object,
/// mints a non-zero instance id, and keys the impl (held as an mlua
/// [`RegistryKey`]) under that id in the per-contract registry; dispatch resolves
/// the impl from the instance handle and passes it as the Lua handler's first
/// argument; `destroy_instance` drops it. A null instance handle (id 0) resolves
/// to a per-contract `default_impl` built once at load — this serves stateless
/// contracts and the low-level dispatch paths that call with a null instance. Two
/// live instances of the same contract therefore never share state.
///
/// Impl objects are held as [`RegistryKey`] rather than raw [`Value`]: a
/// `RegistryKey`'s `Drop` is deferred-unref-safe (it does not need the VM lock),
/// whereas dropping a `Value` requires the VM lock and would deadlock if dropped
/// from a context already holding it.
pub struct LuaLoaderData {
    pub _vm: Lua,
    pub functions: Vec<Function>,
    /// Per-VM arena allocator `arena_alloc(size, arena) -> integer`, threaded as
    /// the FINAL argument of every dispatch call (NOT injected as a VM global —
    /// Rule 12). It bumps exactly the per-call [`CallArena`] whose pointer the
    /// dispatch passes, or falls back to `host->alloc` when that pointer is 0, so a
    /// concurrent or same-VM reentrant dispatch can never perturb this call's arena.
    pub arena_alloc: Function,
    /// Author factory `factory(host_ptr_int) -> impl`, called once per
    /// `create_instance` to build a fresh implementation object bound to its
    /// owning runtime's host pointer.
    pub factory: Function,
    /// Stateless default implementation, built once at load via the factory.
    /// Dispatch resolves to this when the instance handle is null (id 0). Held as
    /// a [`RegistryKey`] so its drop is deferred-unref-safe.
    pub default_impl: RegistryKey,
    /// Live instances keyed by their non-zero instance id (the value stored in
    /// `GuestContractInstance::data`). Each impl is held as a [`RegistryKey`].
    pub instances: Mutex<HashMap<u64, RegistryKey>>,
    /// Monotonic source of non-zero instance ids. Starts at 1 so a real instance
    /// handle is never null (a null `data` denotes the default impl, since
    /// `GuestContractInstance::is_null` keys on `data`).
    pub next_id: AtomicU64,
    /// Contract id stamped into every instance handle this contract mints.
    pub contract_id: GuestContractId,
    /// Thread-aware same-VM reentrancy guard for [`lua_dispatch`].
    ///
    /// mlua is built with the `send` feature, so a single `Lua` VM is reachable
    /// from any thread and is internally lock-guarded. Two cases must be told
    /// apart, and a plain `AtomicBool` cannot distinguish them:
    ///
    /// 1. SAME-thread nested dispatch — a plugin→plugin cross-call that resolves
    ///    back to a contract in THIS
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
    /// Per-VM serialization of the guest dispatch call in [`lua_dispatch`].
    ///
    /// The per-call arena pointer and its allocator are now threaded as explicit
    /// dispatch ARGUMENTS (no `_polyplug_arena` VM global), so each call carries its
    /// own arena on its own call frame and a concurrent dispatch can no longer
    /// perturb it. This lock remains as explicit per-VM serialization of the call —
    /// belt-and-suspenders with mlua's own internal `send` lock — so cross-thread
    /// callers queue rather than interleave VM work. Same-thread nested reentrancy is
    /// refused (`ReentrantCall`) BEFORE this lock is taken, so it can never deadlock
    /// against its own thread. It lives on the per-VM `LuaLoaderData`, never
    /// globally, so it is Rule-12 compliant.
    pub dispatch_lock: Mutex<()>,
    /// Instance-owned copy of the runtime's logger, taken at load time.
    ///
    /// Dispatch-time diagnostics have no `&Runtime` back-reference, so the
    /// per-VM data carries its own `Copy` of the handle. Same callback
    /// contract as `RuntimeConfig::log` — never invoked under a lock guard.
    pub logger: LoggerHandle,
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
    ///
    /// Test-only since unload became uniform epoch-deferred reclaim: production code no
    /// longer inspects per-VM state at unload time (the epoch governs liveness), so the
    /// only remaining caller is the in-flight-marking unit test.
    #[cfg(test)]
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

// ─── Instance Lifecycle ────────────────────────────────────────────────────────

/// Create a fresh instance of a Lua contract.
///
/// Calls the contract's `factory(host_ptr)` to build a new implementation object,
/// stores it as an mlua [`RegistryKey`], mints a non-zero instance id, keys the
/// impl under that id in the per-contract registry, and writes a
/// `GuestContractInstance` whose `data` carries the id and whose `contract_id` is
/// the contract's stamped id. A factory failure (or a poisoned registry lock)
/// writes a null instance handle (there is no error out-param on create_instance).
///
/// # Reentrancy
/// `create_instance` runs Lua (the factory call), and mlua's VM lock is
/// non-reentrant on the same thread. If this thread is already inside a dispatch
/// on this VM (a plugin→plugin cross-call that resolves back here while mid-call),
/// re-entering mlua to run the factory would deadlock. In that case this writes a
/// null instance and returns WITHOUT calling the factory — the host must quiesce
/// before creating instances mid-dispatch.
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`LuaLoaderData`] created by the
///   loader (its box address is stable for as long as the bundle is loaded).
/// - `host` is the owning runtime's `HostApi` pointer, forwarded to the factory.
/// - `out_instance`, when non-null, must be writable per the ABI contract.
unsafe extern "C" fn lua_create_instance(
    loader_data: VmLoaderData,
    host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if out_instance.is_null() {
        return;
    }
    // A null loader_data carries no contract to build an instance for (e.g. a peer
    // caller that constructs a zeroed VmLoaderData and routes by stamping the
    // contract id afterward). Write a null instance and return rather than deref a
    // null pointer.
    if loader_data.data.is_null() {
        // SAFETY: out_instance is non-null (checked above) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
        return;
    }
    // SAFETY: loader_data wraps a valid LuaLoaderData pointer created by the
    // loader (non-null checked above); its box address stays valid for as long as
    // the bundle is loaded.
    let data: &LuaLoaderData = unsafe { &*(loader_data.data as *const LuaLoaderData) };

    // Refuse a same-thread nested create: running the factory would re-enter mlua's
    // non-reentrant VM lock already held by the in-flight dispatch on this thread
    // and deadlock. There is no error out-param here, so write a null instance.
    let this_thread: ThreadId = std::thread::current().id();
    {
        // Poison recovery: the Vec<ThreadId> cannot be left logically corrupt
        // between lock/unlock, so reusing it is sound. Production code, no unwrap.
        let threads: std::sync::MutexGuard<'_, Vec<ThreadId>> = data
            .in_dispatch_threads
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if threads.contains(&this_thread) {
            drop(threads);
            // SAFETY: out_instance is non-null (checked above) and writable per the ABI contract.
            unsafe { out_instance.write(GuestContractInstance::null()) };
            return;
        }
    }

    let host_i64: i64 = host as usize as i64;
    let instance: GuestContractInstance = match data.factory.call::<Value>(host_i64) {
        Ok(value) => match data._vm.create_registry_value(value) {
            Ok(key) => {
                let id: u64 = data.next_id.fetch_add(1, Ordering::Relaxed);
                // Poison recovery: a poisoned registry between lock/unlock leaves the
                // HashMap usable. Production code, no unwrap.
                let mut map: std::sync::MutexGuard<'_, HashMap<u64, RegistryKey>> = data
                    .instances
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                map.insert(id, key);
                GuestContractInstance {
                    data: id as usize as *mut core::ffi::c_void,
                    contract_id: data.contract_id,
                }
            }
            Err(e) => {
                data.logger.log(LogLevel::Error, "loader.lua", || {
                    format!("Lua create_instance: registry_value failed: {e}")
                });
                GuestContractInstance::null()
            }
        },
        Err(e) => {
            data.logger.log(LogLevel::Error, "loader.lua", || {
                format!("Lua create_instance: factory call failed: {e}")
            });
            GuestContractInstance::null()
        }
    };

    // SAFETY: out_instance is non-null (checked above) and writable per the ABI contract.
    unsafe { out_instance.write(instance) };
}

/// Destroy a Lua contract instance.
///
/// Removes the impl keyed under the instance handle's id from the per-contract
/// registry. A null handle (id 0) refers to the stateless default impl, which the
/// loader owns for the bundle lifetime, so it is a no-op. Dropping the removed
/// [`RegistryKey`] is deferred-unref-safe (it does not take the VM lock).
///
/// # Safety
/// - `loader_data` must wrap a valid pointer to a [`LuaLoaderData`] created by the
///   loader (its box address is stable for as long as the bundle is loaded).
/// - `instance` must be a handle previously produced by [`lua_create_instance`]
///   for this contract (or a null handle).
unsafe extern "C" fn lua_destroy_instance(
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    instance: GuestContractInstance,
) {
    let id: u64 = instance.data as usize as u64;
    if id == 0 {
        return;
    }
    // A null loader_data carries no per-contract registry to remove from (e.g. a
    // peer caller that passes a zeroed VmLoaderData). Nothing to drop.
    if _loader_data.data.is_null() {
        return;
    }
    // SAFETY: loader_data wraps a valid LuaLoaderData pointer created by the
    // loader (non-null checked above); its box address stays valid for as long as
    // the bundle is loaded.
    let data: &LuaLoaderData = unsafe { &*(_loader_data.data as *const LuaLoaderData) };
    // Poison recovery: a poisoned registry between lock/unlock leaves the HashMap
    // usable. Production code, no unwrap. RegistryKey drop is deferred-unref-safe.
    let mut map: std::sync::MutexGuard<'_, HashMap<u64, RegistryKey>> = data
        .instances
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    map.remove(&id);
}

// ─── Lua Dispatch Function ─────────────────────────────────────────────────────

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
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    // SAFETY: loader_data wraps a valid pointer to LuaLoaderData created by the
    // loader; args/out/arena satisfy the ABI dispatch contract for this call.
    let result: AbiError =
        unsafe { lua_dispatch_impl(loader_data, instance, fn_id, args, out, arena) };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(result) };
    }
}

unsafe fn lua_dispatch_impl(
    loader_data: VmLoaderData,
    instance: GuestContractInstance,
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

    // Resolve the impl object for this instance: a null handle (id 0) uses the
    // stateless default impl; otherwise look the live instance up by id. The Value
    // is read OUT of the registry and is owned independently of the map, so the
    // map guard drops at the end of this block — a nested dispatch cannot deadlock
    // on the registry mutex.
    let instance_id: u64 = instance.data as usize as u64;
    let instance_value: Value = if instance_id == 0 {
        match data._vm.registry_value::<Value>(&data.default_impl) {
            Ok(v) => v,
            Err(_) => {
                return AbiError {
                    code: AbiErrorCode::Generic as u32,
                    message: StringView::null(),
                };
            }
        }
    } else {
        // Poison recovery: the HashMap cannot be left logically corrupt between
        // lock/unlock, so reusing it is sound. Production code, no unwrap.
        let map: std::sync::MutexGuard<'_, HashMap<u64, RegistryKey>> = data
            .instances
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match map.get(&instance_id) {
            Some(key) => match data._vm.registry_value::<Value>(key) {
                Ok(v) => v,
                Err(_) => {
                    return AbiError {
                        code: AbiErrorCode::Generic as u32,
                        message: StringView::null(),
                    };
                }
            },
            None => {
                return AbiError {
                    code: AbiErrorCode::FunctionNotAvailable as u32,
                    message: StringView::null(),
                };
            }
        }
    };

    // Pass pointers as i64 to preserve full 64-bit precision on LuaJIT.
    // LuaJIT lua_Integer is int64_t — safe for pointer-width integers.
    let args_i64: i64 = args as usize as i64;
    let out_i64: i64 = out as usize as i64;

    // Call the guest under the per-VM `dispatch_lock`, passing the per-call arena
    // pointer and its allocator as explicit arguments (no VM global is touched —
    // Rule 12). The lock provides explicit per-VM serialization of the call (a
    // cross-thread caller queues here); same-thread reentrancy was already refused
    // above, so it cannot deadlock against its own thread. Poison recovery: the unit
    // value cannot be left logically corrupt, so reusing it is sound. Production
    // code, so no unwrap. The lock scope ends with the call: the logger callback
    // below must run with no guard held (RuntimeConfig::log lock rule).
    let call_result: Result<(), mlua::Error> = {
        let _dispatch_lock: std::sync::MutexGuard<'_, ()> = data
            .dispatch_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        // The per-call arena pointer and its allocator are threaded as explicit
        // dispatch arguments — nothing is published into or cleared from any VM
        // global (Rule 12). Each call carries its own arena on its own frame, so a
        // concurrent or same-VM reentrant dispatch cannot perturb it.
        let arena_i64: i64 = arena as usize as i64;
        lua_fn.call::<()>((
            instance_value,
            args_i64,
            out_i64,
            arena_i64,
            data.arena_alloc.clone(),
        ))
    };

    match call_result {
        Ok(()) => AbiError::ok(),
        Err(e) => {
            data.logger.log(LogLevel::Error, "loader.lua", || {
                format!("Lua function call failed: {e}")
            });
            AbiError {
                code: AbiErrorCode::Generic as u32,
                message: StringView::null(),
            }
        }
    }
}

/// Lua plugin loader — loads Lua plugin bundles via the embedded LuaJIT VM.
///
/// The Lua script must define a global function `polyplug_init(host_ptr, ctx_ptr)`
/// which RETURNS `(registrations, abi_error)`: the per-contract handler table
/// (plugin metadata + function tables) plus the canonical `AbiError` `{ code,
/// message }` table. Nothing is deposited into any global namespace (Rule 12).
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
    /// Count of VM-state boxes scheduled for epoch-deferred reclamation.
    ///
    /// Test/diagnostic only: epoch collection timing is non-deterministic, but this
    /// counter is incremented the instant a box is handed to
    /// `crossbeam_epoch::pin().defer(...)`, so it deterministically proves the VM was
    /// scheduled for reclaim — NOT parked alive forever. Instance state (Rule 12).
    scheduled_reclaims: AtomicU64,
}

impl LuaLoader {
    /// Create a new `LuaLoader` with the given configuration.
    pub fn new(config: LuaConfig) -> Self {
        Self {
            config,
            live: Mutex::new(HashMap::new()),
            scheduled_reclaims: AtomicU64::new(0),
        }
    }

    /// Schedule one bundle's VM-state boxes for epoch-deferred drop and record the
    /// scheduling.
    ///
    /// SAFETY/why: each `LuaVm` box is already unreachable by any *new* dispatch (the
    /// bundle has been removed from `live` / the registry before this is called). Any
    /// in-flight runtime-mediated call holds a crossbeam-epoch pin, so `defer` runs the
    /// drop — freeing the cloned `Lua` VM handle — only once no such reader remains;
    /// the global epoch coordinates that with the runtime's reader pins. FFI host→VM
    /// direct callers must quiesce before unload per the documented host contract
    /// (docs/TRUST_MODEL.md). `LuaVm` owns its box (`Send + 'static`), so moving it
    /// into the deferred closure is sound.
    fn schedule_reclaim(&self, state: Vec<LuaVm>) {
        for vm in state {
            self.scheduled_reclaims.fetch_add(1, Ordering::Relaxed);
            crossbeam_epoch::pin().defer(move || drop(vm));
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
    ) -> Result<(), LoaderError> {
        let package: Table = lua
            .globals()
            .get::<Table>("package")
            .map_err(|e: mlua::Error| LoaderError::InitFailed {
                bundle: bundle.to_owned(),
                error: format!("Lua VM init failed: missing package table: {e}"),
            })?;

        let current: String = package
            .get::<String>(field)
            .unwrap_or_else(|_: mlua::Error| String::new());

        let combined: String = if current.is_empty() {
            entries.to_owned()
        } else {
            format!("{entries};{current}")
        };

        package
            .set(field, combined)
            .map_err(|e: mlua::Error| LoaderError::InitFailed {
                bundle: bundle.to_owned(),
                error: format!("Lua VM init failed: package.{field} update failed: {e}"),
            })
    }

    /// Build the per-VM arena allocator `arena_alloc(size, arena) -> integer`.
    ///
    /// The returned [`Function`] is stored on each contract's [`LuaLoaderData`] and
    /// threaded as the FINAL argument of every dispatch call — it is NEVER injected
    /// into the VM global namespace (Rule 12). It serves the guest's per-call return
    /// buffers from the [`CallArena`] whose pointer the dispatch passes as the
    /// `arena` argument; when that pointer is 0 it falls back to `host->alloc`,
    /// preserving the per-value allocation behaviour. Returns the allocated address
    /// as an integer (0 on failure), matching the Lua pointer convention.
    fn build_arena_alloc(
        lua: &Lua,
        bundle: &str,
        host_interface: *const HostApi,
    ) -> Result<Function, LoaderError> {
        // Capture the host pointer as a usize: raw pointers are not Send, but the
        // pointee is 'static HostApi for the runtime lifetime, so reconstructing it
        // inside the (Send) closure is sound.
        let host_addr: usize = host_interface as usize;

        lua.create_function(
            move |_lua_ctx: &Lua, (size, arena_addr): (u32, i64)| -> mlua::Result<i64> {
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
                    // SAFETY: `arena` is the valid per-call CallArena whose pointer the
                    // dispatch threaded in; alloc bumps within it or chains a
                    // host-allocated overflow block.
                    unsafe { (*arena).alloc(size as usize, 1) }
                };
                Ok(ptr as usize as i64)
            },
        )
        .map_err(|e: mlua::Error| LoaderError::InitFailed {
            bundle: bundle.to_owned(),
            error: format!("Lua VM init failed: arena allocator creation failed: {e}"),
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
    ) -> Result<(String, String, Option<String>), LoaderError> {
        match source {
            BundleSource::Path(_) => {
                let bundle_path: std::path::PathBuf = if !manifest.file.is_empty() {
                    manifest.path.join(&manifest.file)
                } else {
                    return Err(LoaderError::ManifestMissingFile {
                        bundle: manifest.name.clone(),
                    });
                };

                if !bundle_path.exists() {
                    return Err(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!(
                            "Lua script load failed at {}: file does not exist",
                            bundle_path.display()
                        ),
                    });
                }

                let source_text: String =
                    std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                        LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: format!(
                                "Lua script load failed at {}: {}",
                                bundle_path.display(),
                                e
                            ),
                        }
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
                        LoaderError::InvalidSourceEncoding {
                            loader: "lua",
                            source_kind: source.kind(),
                            bundle: manifest.name.clone(),
                        }
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
    ) -> Result<(), LoaderError> {
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
            .map_err(|e: mlua::Error| LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("Lua script load failed for {}: {}", chunk_name, e),
            })?;

        // Derive bundle name for error messages.
        let bundle_name: String = chunk_name;

        // Retrieve polyplug_init global function.
        let init_fn: Function =
            lua.globals()
                .get::<Function>("polyplug_init")
                .map_err(|_: mlua::Error| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!(
                        "Lua plugin missing polyplug_init function: bundle={}",
                        bundle_name
                    ),
                })?;

        // Get HostApi pointer from runtime.
        // The interface already has the runtime pointer set.
        let host_interface: *const HostApi = runtime.as_context_ptr();

        // Build the per-call arena allocator so the guest can route its return-value
        // buffers through the host's CallArena (zero host allocations after warmup).
        // It is threaded as a dispatch ARGUMENT (stored on each contract's
        // LuaLoaderData, cloned below) rather than injected as a VM global — Rule 12.
        // The guest log helper calls `HostApi.log` directly through the threaded host
        // pointer, so there is no `_polyplug_log` bridge to register.
        let arena_alloc: Function = Self::build_arena_alloc(&lua, &bundle_name, host_interface)?;

        // Open the init-bundle window for BOTH the init call and the registration
        // loop below: `host_register_guest_contract` attributes each registration to
        // the bundle id on top of this stack, and the `register_guest_contract` calls
        // happen later in this function (after init returns the registrations table).
        // The guard's Drop pops once on every exit path — including the `?`
        // early-returns between here and the end of the registration loop — so the
        // stack never leaks an entry and registrations carry the real bundle id.
        let _init_window: InitBundleGuard<'_> = InitBundleGuard::enter(runtime, bundle_id);

        // Call polyplug_init — it RETURNS (registrations, abi_error).
        // Signature: polyplug_init(host, ctx) - self-passing pattern.
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

        // polyplug_init RETURNS (registrations, abi_error): the per-contract handler
        // table plus the canonical AbiError `{ code, message }`. Nothing is deposited
        // into any global/module namespace (Rule 12) — the loader consumes both
        // return values directly. The registrations shape is per-contract:
        // `registrations[contract_name] = { contract_version, plugin_name, factory,
        // functions }`; the loop below registers EVERY entry.
        let (handlers, abi_error): (Table, Table) = init_fn
            .call::<(Table, Table)>((host_interface_i64, ctx_ptr))
            .map_err(|e: mlua::Error| LoaderError::InitFailed {
                bundle: bundle_name.clone(),
                error: format!("Lua polyplug_init failed: {}", e),
            })?;

        // Honor the AbiError. A non-Ok code means the guest refused to initialize —
        // fail the load, surfacing the guest's own `message` when present.
        let init_code: u32 = abi_error.get::<u32>("code").unwrap_or(0_u32);
        if init_code != AbiErrorCode::Ok as u32 {
            let init_message: Option<String> = abi_error
                .get::<String>("message")
                .ok()
                .filter(|s: &String| !s.is_empty());
            let error: String = match init_message {
                Some(msg) => msg,
                None => format!("Lua polyplug_init returned error code {}", init_code),
            };
            return Err(LoaderError::InitFailed {
                bundle: bundle_name.clone(),
                error,
            });
        }

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
            let (contract_name_str, entry): (String, Table) =
                pair.map_err(|e: mlua::Error| LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("Lua handlers iteration error: {}", e),
                })?;

            let contract_version: u32 = entry.get::<u32>("contract_version").unwrap_or(1_u32);

            let plugin_name_str: String = entry
                .get::<String>("plugin_name")
                .unwrap_or_else(|_: mlua::Error| bundle_name.clone());

            let functions_table: Table =
                entry.get::<Table>("functions").map_err(|e: mlua::Error| {
                    LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua handlers error: missing functions table for contract '{}': {}",
                            contract_name_str, e
                        ),
                    }
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
                    |e: mlua::Error| LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua function slot {} error for contract '{}': {}",
                            slot_idx, contract_name_str, e
                        ),
                    },
                )?;
                lua_functions.push(lua_fn);
            }

            // Read the contract's author factory. The loader owns per-instance
            // state: it calls the factory once per create_instance (and once here
            // to build the stateless default impl). A contract entry without a
            // `factory` cannot construct instances, so reject the load.
            let factory: Function =
                entry.get::<Function>("factory").map_err(|e: mlua::Error| {
                    LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "Lua handlers error: contract '{}' needs a `factory` function: {}",
                            contract_name_str, e
                        ),
                    }
                })?;

            // Build the stateless default impl once at load via the factory, bound
            // to the runtime's host pointer. Dispatch uses it for null instance
            // handles (stateless contracts and the low-level null-instance paths).
            let host_i64: i64 = host_interface as usize as i64;
            let default_value: Value =
                factory.call::<Value>(host_i64).map_err(|e: mlua::Error| {
                    LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "factory for contract '{}' failed building the default instance: {}",
                            contract_name_str, e
                        ),
                    }
                })?;
            let default_impl: RegistryKey =
                lua.create_registry_value(default_value)
                    .map_err(|e: mlua::Error| LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!(
                            "failed to register default impl for contract '{}': {}",
                            contract_name_str, e
                        ),
                    })?;

            // Build contract_id from contract_name and version.
            let cid: GuestContractId = GuestContractId::new(&contract_name_str, contract_version);

            // Create LuaLoaderData with a clone of the Lua VM handle and the
            // contract's functions. Cloning `Lua` shares the same underlying VM.
            let loader_data: LuaVm = LuaVm(Box::new(LuaLoaderData {
                _vm: lua.clone(),
                functions: lua_functions,
                arena_alloc: arena_alloc.clone(),
                factory,
                default_impl,
                instances: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
                contract_id: cid,
                in_dispatch_threads: Mutex::new(Vec::new()),
                dispatch_lock: Mutex::new(()),
                logger: runtime.logger(),
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
            let mut reg_result: AbiError = AbiError::ok();
            // SAFETY: `host_interface` is a valid HostApi pointer; `reg_result` is a
            // valid, writable out-param for the duration of the call.
            unsafe {
                ((*host_interface).register_guest_contract)(
                    host_interface,
                    &descriptor as *const PluginDescriptor,
                    static_interface,
                    &mut reg_result,
                )
            };

            if !reg_result.is_ok() {
                // A contract earlier in this loop may already be registered with the
                // registry pointing at its box in `bundle_vm_state`. Schedule those
                // boxes for epoch-deferred drop rather than dropping them inline here,
                // which would dangle the registry's bridge_data while a reader is pinned.
                self.schedule_reclaim(bundle_vm_state);
                return Err(LoaderError::InitFailed {
                    bundle: bundle_name,
                    error: format!(
                        "register_guest_contract error for contract '{}': code={:?}",
                        contract_name_str, reg_result.code
                    ),
                });
            }

            registered += 1;
        }

        if registered == 0 {
            return Err(LoaderError::InitFailed {
                bundle: bundle_name,
                error: "Lua plugin registered no contracts: polyplug_init returned an empty registrations table".to_owned(),
            });
        }

        // Take ownership of this bundle's VM state. A reload of the same bundle id
        // REPLACES the prior VM set and schedules the superseded VMs for epoch-deferred
        // reclaim (mirroring the native loader): the global epoch keeps the old VMs
        // alive for any in-flight dispatch and frees them once no reader is pinned,
        // rather than parking them until unload.
        let superseded: Option<Vec<LuaVm>> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            live.insert(BundleId::from_u64(bundle_id), bundle_vm_state)
        };
        if let Some(old_state) = superseded {
            self.schedule_reclaim(old_state);
        }

        Ok(())
    }

    /// Number of live VM-state entries currently owned for `bundle_id`.
    #[cfg(test)]
    fn live_vm_count(&self, bundle_id: BundleId) -> usize {
        let live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
            self.live.lock().unwrap_or_else(PoisonError::into_inner);
        live.get(&bundle_id).map(Vec::len).unwrap_or(0)
    }

    /// Number of VM-state boxes scheduled for epoch-deferred reclaim. Deterministic
    /// (incremented at scheduling time), so tests assert the resource was handed to
    /// the epoch collector without depending on its non-deterministic timing.
    #[cfg(test)]
    fn scheduled_reclaim_count(&self) -> u64 {
        self.scheduled_reclaims.load(Ordering::Relaxed)
    }
}

impl BundleLoader for LuaLoader {
    fn loader_name(&self) -> &'static str {
        "lua"
    }

    fn loader_language(&self) -> SupportedLanguage {
        SupportedLanguage::Lua
    }

    fn supports_hot_reload(&self) -> bool {
        true
    }

    fn load(
        &self,
        manifest: &ManifestData,
        source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        // The Lua loader serves every BundleSource: Path reads the on-disk entry
        // file, Code evaluates in-memory source text, and Bytes is UTF-8 source
        // text. All three converge on the same compile/init/register path.
        self.load_inner(manifest, source, runtime)
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), LoaderError> {
        // reload re-reads the on-disk entry file (only path-backed bundles can be
        // hot-reloaded — there is no on-disk artifact to re-read for in-memory
        // sources; the runtime gates hot-reload before calling this).
        self.load_inner(
            manifest,
            &BundleSource::Path(manifest.path.clone()),
            runtime,
        )
    }

    /// Reclaim the bundle's Lua VM via epoch-deferred drop.
    ///
    /// Called by the runtime AFTER `invalidate_bundle` has removed the bundle from
    /// the registry, so no dispatch can *resolve* this contract anew.
    ///
    /// # Host-coordination contract
    /// The bundle's VM-state boxes are removed from `live` and scheduled for
    /// epoch-deferred drop (see [`LuaLoader::schedule_reclaim`]): each box's cloned
    /// `Lua` VM handle is freed only once no crossbeam-epoch reader is pinned, so any
    /// in-flight *runtime-mediated* call (which holds an epoch pin across
    /// dispatch) keeps the VM alive until it completes. Direct
    /// FFI host→VM callers the runtime does not mediate are covered by the documented
    /// trusted-same-process contract: exactly like hot-reload, the host MUST NOT call
    /// a bundle's contracts concurrently with unloading it (see `Runtime::unload_bundle`
    /// and docs/TRUST_MODEL.md).
    ///
    /// The VM is always epoch-reclaimed (never parked alive forever).
    fn unload(&self, bundle_id: BundleId, _runtime: &Runtime) -> Result<(), LoaderError> {
        let state: Vec<LuaVm> = {
            let mut live: std::sync::MutexGuard<'_, HashMap<BundleId, Vec<LuaVm>>> =
                self.live.lock().unwrap_or_else(PoisonError::into_inner);
            match live.remove(&bundle_id) {
                Some(v) => v,
                None => return Ok(()),
            }
        };

        self.schedule_reclaim(state);

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
    fn lua_loader_name() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        assert_eq!(loader.loader_name(), "lua");
    }

    /// A minimal valid Lua plugin registering the `test.unload@1` contract.
    ///
    /// Handlers take the resolved instance as their first argument (the loader
    /// passes it); the entry carries a `factory` the loader calls to build the
    /// default impl and any per-instance impls.
    fn unload_plugin_script() -> &'static [u8] {
        br#"
local function impl_noop(_instance, _a, _o, _arena_ptr, _arena_alloc) end
function polyplug_init(_host, _ctx)
    local registrations = {
        ["test.unload"] = {
            contract_version = 1,
            plugin_name      = "test-unload",
            factory          = function(_host) return {} end,
            functions        = { [0] = impl_noop },
        },
    }
    return registrations, { code = 0 }
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
            loader: "lua".to_owned(),
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

    /// Unload removes the bundle from the loader's live map and SCHEDULES its VM state
    /// for epoch-deferred reclaim. Unload is uniform regardless of `in_dispatch_threads`:
    /// every unload epoch-reclaims, so even an in-flight call schedules reclaim (the
    /// epoch keeps the VM alive until that reader's pin clears).
    #[test]
    fn unload_removes_live_and_schedules_reclaim() {
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
            .unload(bundle_id, &runtime)
            .expect("unload must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "unload must remove the bundle's VM state from the live map"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "unload must schedule the VM state for epoch-deferred reclaim"
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
                .unload(bundle_id, &runtime)
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
            loader.scheduled_reclaim_count(),
            5,
            "each of the 5 unloads must schedule its VM state for epoch-deferred reclaim"
        );
    }

    /// A reload of the same bundle id must REPLACE the live VM set and schedule the
    /// superseded VMs for epoch-deferred reclaim — not park them in the live map until
    /// unload. This mirrors the native loader's reload-reclaim and keeps a reload loop
    /// from leaking the bundle's VM set per reload.
    ///
    /// Driving the loader directly bypasses `Runtime`'s reload orchestration, so the
    /// test invalidates the prior registry registration between loads (so the second
    /// `register_guest_contract` is not rejected as a duplicate provider) — exactly the
    /// supersede path a real reload takes.
    #[test]
    fn reload_replaces_live_and_reclaims_superseded_vm() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        let runtime: std::sync::Arc<polyplug::Runtime> = polyplug::runtime::RuntimeBuilder::new()
            .loader(LuaLoader::new(LuaConfig::default()))
            .build()
            .expect("runtime build must succeed");
        let (_dir, manifest): (tempfile::TempDir, ManifestData) =
            write_unload_bundle("lua_reload_reclaim");
        let bundle_id: BundleId = BundleId::from_u64(manifest.id);

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("first load must succeed");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "first load installs exactly one live VM entry"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            0,
            "nothing is superseded by the first load"
        );

        // Drop the registry-side registration so the second load's
        // register_guest_contract is not a duplicate, exercising the supersede path.
        runtime
            .registry()
            .invalidate_bundle(bundle_id)
            .expect("invalidate must succeed");

        loader
            .load(
                &manifest,
                &BundleSource::Path(manifest.path.clone()),
                &runtime,
            )
            .expect("second load (reload) must succeed");

        assert_eq!(
            loader.live_vm_count(bundle_id),
            1,
            "reload replaces the live VM set — the live map must not grow"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "reload must schedule the superseded VM set for epoch-deferred reclaim"
        );
    }

    /// Even when a dispatch is marked in flight (the bundle's `in_dispatch_threads`
    /// is non-empty), unload behaves uniformly: it removes the bundle from `live` and
    /// SCHEDULES its VM state for epoch-deferred reclaim. The in-flight reader's epoch
    /// pin — not an `in_dispatch_threads`-gated retire branch — is what keeps the VM
    /// alive until the call completes, so unload never parks the state forever.
    #[test]
    fn unload_schedules_reclaim_even_when_in_flight() {
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

        // Mark a fake in-flight dispatch in the bundle's tracking vec — exactly the
        // state the dispatch guard would leave while a call is mid-flight on another
        // thread. An in-flight mark must not change the uniform epoch-reclaim outcome.
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
            .unload(bundle_id, &runtime)
            .expect("unload must succeed even when marked in-flight");
        assert_eq!(
            loader.live_vm_count(bundle_id),
            0,
            "unload must remove the bundle from the live map"
        );
        assert_eq!(
            loader.scheduled_reclaim_count(),
            1,
            "unload must schedule epoch-deferred reclaim even when marked in-flight"
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
        // A trivial factory + default impl so the data is well-formed. These tests
        // exercise stateless dispatch (null instance → default impl), so the
        // default impl is a plain empty table and the factory returns the same.
        let factory: Function = vm
            .load("return function(_host) return {} end")
            .eval::<Function>()
            .expect("create trivial factory");
        let default_impl: RegistryKey = vm
            .create_registry_value(Value::Table(
                vm.create_table().expect("create default impl table"),
            ))
            .expect("register default impl");
        let arena_alloc: Function =
            LuaLoader::build_arena_alloc(&vm, "make_loader_data", core::ptr::null())
                .expect("build arena allocator");
        let boxed: Box<LuaLoaderData> = Box::new(LuaLoaderData {
            _vm: vm,
            functions,
            arena_alloc,
            factory,
            default_impl,
            instances: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            contract_id: GuestContractId::new("test.make_loader_data", 1),
            in_dispatch_threads: Mutex::new(Vec::new()),
            dispatch_lock: Mutex::new(()),
            logger: LoggerHandle::default_stderr(),
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
        // A trivial guest function that ignores its (instance, args, out) args.
        let noop: Function = lua
            .create_function(|_, (_instance, _a, _o): (Value, i64, i64)| Ok(()))
            .expect("create_function should succeed");
        let (vm_loader_data, data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![noop]);

        let mut out_buf: i32 = 0;
        // SAFETY: vm_loader_data wraps a live LuaLoaderData; the out pointer is a
        // valid local i32; the guest function ignores its args. A null instance
        // resolves to the default impl built by make_loader_data.
        let err: AbiError = unsafe {
            lua_dispatch_impl(
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
            .create_function(
                move |lua_ctx: &Lua, (_instance, _a, _o): (Value, i64, i64)| {
                    let ptr_usize: usize = cell_for_fn.load(Ordering::Acquire);
                    let vm_loader_data: VmLoaderData = VmLoaderData {
                        data: ptr_usize as *mut core::ffi::c_void,
                    };
                    // SAFETY: the cell holds the live leaked LuaLoaderData pointer set
                    // up by the test before dispatch; the guest function ignores the
                    // forwarded args/out pointers.
                    let nested: AbiError = unsafe {
                        lua_dispatch_impl(
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
                },
            )
            .expect("create_function should succeed");

        let (vm_loader_data, data_ref): (VmLoaderData, &'static LuaLoaderData) =
            make_loader_data(lua, vec![reentrant_fn]);
        loader_data_cell.store(vm_loader_data.data as usize, Ordering::Release);

        // Outer dispatch: sets the flag, runs the guest fn, which re-enters.
        // SAFETY: vm_loader_data wraps the live leaked LuaLoaderData.
        let outer: AbiError = unsafe {
            lua_dispatch_impl(
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
            lua_dispatch_impl(
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
            .create_function(
                move |_lua_ctx: &Lua, (_instance, _a, _o): (Value, i64, i64)| {
                    // Signal that thread A is now inside the dispatch (VM lock held).
                    entered_for_fn.wait();
                    // Hold the dispatch (and the VM lock) until the main thread has
                    // begun its concurrent dispatch.
                    release_for_fn.wait();
                    Ok(())
                },
            )
            .expect("create_function should succeed");
        // The concurrent caller dispatches THIS function (fn_id 1) — it must not
        // touch the barriers, or it would park with no partner and deadlock.
        let noop_fn: Function = lua
            .create_function(|_lua_ctx: &Lua, (_instance, _a, _o): (Value, i64, i64)| Ok(()))
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
                lua_dispatch_impl(
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
                lua_dispatch_impl(
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

    /// Concurrency property of the threaded-arena dispatch: two threads dispatching
    /// into the SAME VM with DISTINCT per-call arenas keep their arena-backed returns
    /// isolated.
    ///
    /// The per-call arena pointer and its allocator are threaded as explicit
    /// arguments of `lua_dispatch` (no `_polyplug_arena` VM global), so each call
    /// carries its own arena on its own frame and a concurrent dispatch can never
    /// perturb it. This test hammers two threads dispatching into the same VM with
    /// distinct per-thread arenas for many iterations; each guest fn allocates from
    /// the threaded `arena_alloc` (using the threaded arena pointer) and reports the
    /// address, and the Rust side verifies the address lands in the buffer of the
    /// arena THAT THREAD dispatched with. A single misattributed allocation lands in
    /// the other thread's buffer or is null, failing the per-iteration assertion. The
    /// two buffers are deliberately disjoint so a cross-arena allocation is
    /// unambiguously detectable.
    #[test]
    fn lua_dispatch_concurrent_arena_returns_stay_isolated() {
        // SAFETY: test-only VM; no untrusted scripts are executed here.
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // Both worker functions are identical: allocate 64 bytes from the threaded
        // arena allocator (the dispatch passes `arena_alloc` and `arena_ptr` as the
        // 5th and 4th arguments) and write the returned address into the out slot.
        // fn_id selects which thread.
        let make_alloc_fn = |lua: &Lua| -> Function {
            lua.create_function(
                |_lua_ctx: &Lua,
                 (_instance, _a, out_ptr, arena_ptr, arena_alloc): (
                    Value,
                    i64,
                    i64,
                    i64,
                    Function,
                )|
                 -> mlua::Result<()> {
                    let addr: i64 = arena_alloc.call::<i64>((64_u32, arena_ptr))?;
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
                    lua_dispatch_impl(
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
                        "A iter {i}: allocation {p:#x} escaped arena A buffer [{a_lo:#x}, {a_hi:#x}) — threaded arena was misattributed"
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
                    lua_dispatch_impl(
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
                        "B iter {i}: allocation {p:#x} escaped arena B buffer [{b_lo:#x}, {b_hi:#x}) — threaded arena was misattributed"
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
