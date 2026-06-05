//! LuaJIT VM initialization and plugin loader implementation.
//!
//! Loads Lua plugin bundles via the embedded LuaJIT VM (mlua).
//! Each bundle gets its own Lua VM for complete isolation between bundles
//! and between polyplug Runtime instances.

use std::ffi::OsStr;
use std::path::Path;

use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;

use crate::config::LuaConfig;
use polyplug::Runtime;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
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
use polyplug_utils::GuestContractId;

/// The path to the sdks/lua/guest/ directory, set at compile time by build.rs.
const GUEST_LUA_DIR: &str = env!("POLYPLUG_GUEST_LUA_DIR");

/// The path to the abi/lua/ directory, set at compile time by build.rs.
const ABI_LUA_DIR: &str = env!("POLYPLUG_ABI_LUA_DIR");

// ─── Lua Loader Data for VM Dispatch ───────────────────────────────────────────

/// Loader-specific data for Lua plugin dispatch.
///
/// Each bundle gets its own Lua VM, ensuring complete isolation between
/// bundles and between polyplug Runtime instances.
pub struct LuaLoaderData {
    pub _vm: Lua,
    pub functions: Vec<Function>,
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
    // serve allocations from it. The mlua call below runs single-threaded on this
    // VM, so the global cannot be observed concurrently. The pointer is cleared
    // after the call so a stale arena is never reachable.
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
}

impl LuaLoader {
    /// Create a new `LuaLoader` with the given configuration.
    pub fn new(config: LuaConfig) -> Self {
        Self { config }
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

    /// Shared load/reload implementation.
    ///
    /// Both `load` and `reload` produce identical behaviour; `reload` only adds a
    /// hot-reload-enabled guard before delegating here.
    fn load_inner(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
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

        let bundle_id: u64 = manifest.id;
        let bundle_dir: &Path = &manifest.path;
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();

        // Create a new Lua VM for this bundle (per-bundle isolation).
        // mlua 0.10: Lua::unsafe_new() enables the FFI module required by LuaJIT plugins.
        // SAFETY: We trust the Lua scripts loaded through this loader. The LuaJIT FFI is
        // required for the polyplug_guest.lua ABI bridge (struct layout, pointer casts).
        let lua: Lua = unsafe { Lua::unsafe_new() };

        // Configure package.path so require("polyplug_guest") and
        // require("polyplug_abi") resolve, plus the bundle's own directory; and
        // package.cpath for native modules. All entries are built in Rust and
        // pushed through the mlua API — never interpolated into Lua source — so a
        // path containing quotes or newlines cannot inject code.
        let guest_dir_fwd: String = GUEST_LUA_DIR.replace('\\', "/");
        let abi_dir_fwd: String = ABI_LUA_DIR.replace('\\', "/");
        let bundle_dir_fwd: String = bundle_dir_str.replace('\\', "/");
        let cpath_ext: &str = if cfg!(windows) { "dll" } else { "so" };

        let path_entries: String = format!(
            "{bundle_dir_fwd}/?.lua;{bundle_dir_fwd}/?.init.lua;{guest_dir_fwd}/?.lua;{abi_dir_fwd}/?.lua"
        );
        Self::prepend_package_field(&lua, &manifest.name, "path", &path_entries)?;

        let cpath_entries: String = format!("{bundle_dir_fwd}/?.{cpath_ext}");
        Self::prepend_package_field(&lua, &manifest.name, "cpath", &cpath_entries)?;

        // Read the plugin script source.
        let source: String =
            std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("Lua script load failed at {}: {}", bundle_path.display(), e),
                })
            })?;

        // Execute the script. This defines polyplug_init in the global environment.
        lua.load(&source).exec().map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("Lua script load failed at {}: {}", bundle_path.display(), e),
            })
        })?;

        // Derive bundle name for error messages.
        let bundle_name: String = bundle_path
            .file_name()
            .map(|n: &OsStr| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| bundle_path.display().to_string());

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

        // Push bundle_id onto the runtime's per-thread init stack for dependency
        // enforcement during init. The matching pop MUST run on every exit path
        // (success and error) so the stack never leaks an entry.
        runtime.push_init_bundle_id(bundle_id);

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

        // Pop bundle_id from the init stack after init completes (always, including
        // the error path) so the stack does not leak an entry.
        runtime.pop_init_bundle_id();

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
            let loader_data: Box<LuaLoaderData> = Box::new(LuaLoaderData {
                _vm: lua.clone(),
                functions: lua_functions,
            });

            let loader_data_ptr: *mut LuaLoaderData = Box::into_raw(loader_data);

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
                            data: loader_data_ptr as *mut core::ffi::c_void,
                        },
                    },
                },
            };

            // Leak the interface so it has 'static lifetime.
            // SAFETY: GuestContractInterface is leaked intentionally — the loader data must be 'static.
            // The interface is valid for the process lifetime (Lua plugins are never unloaded).
            let static_interface: *const GuestContractInterface =
                Box::into_raw(Box::new(plugin_interface));

            // Build static string slices for PluginDescriptor.
            // We leak String → &'static str so StringView ptrs remain valid indefinitely.
            //
            // The descriptor's human-readable `contract_name` must be the canonical
            // `"<name>@<major>"` form so it matches what every other language registers
            // (rust/cpp/python/js generated code emit this full form directly). The bare
            // `contract_name_str` + `contract_version` are the hash inputs already consumed
            // by `GuestContractId::new` above; reusing the bare name in the descriptor would
            // diverge from the other loaders and trip the registry's collision check.
            let contract_display_name: String =
                format!("{}@{}", contract_name_str, contract_version);
            let plugin_name_leaked: &'static str = Box::leak(plugin_name_str.into_boxed_str());
            let contract_name_leaked: &'static str =
                Box::leak(contract_display_name.into_boxed_str());

            let descriptor: PluginDescriptor = PluginDescriptor {
                name: StringView {
                    ptr: plugin_name_leaked.as_ptr(),
                    len: plugin_name_leaked.len(),
                },
                contract_name: StringView {
                    ptr: contract_name_leaked.as_ptr(),
                    len: contract_name_leaked.len(),
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
            // `static_interface` is a leaked Box — valid for 'static lifetime.
            let reg_result: AbiError = unsafe {
                ((*host_interface).register_guest_contract)(
                    host_interface,
                    &descriptor as *const PluginDescriptor,
                    static_interface,
                )
            };

            if !reg_result.is_ok() {
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

        Ok(())
    }
}

impl BundleLoader for LuaLoader {
    fn runtime_name(&self) -> &'static str {
        "lua"
    }

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.load_inner(manifest, runtime)
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }
        self.load_inner(manifest, runtime)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use super::*;

    #[test]
    fn lua_runtime_name() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        assert_eq!(loader.runtime_name(), "lua");
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
