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
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::BundleLoader;
use polyplug::runtime::HostContext;
use polyplug::runtime::Runtime;
use polyplug_abi::contract_id;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_abi::VmDispatch;
use polyplug_abi::ABI_OK;

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

// ─── Lua Dispatch Function ─────────────────────────────────────────────────────

/// Dispatch function for Lua plugins using VM dispatch pattern.
///
/// # Safety
/// - `loader_data` must be a valid pointer to `LuaLoaderData`
/// - `args` and `out` must be valid pointers for the ABI call
unsafe extern "C" fn lua_dispatch(
    loader_data: *mut core::ffi::c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: loader_data is a valid pointer to LuaLoaderData created by the loader.
    let data: &LuaLoaderData = unsafe { &*(loader_data as *const LuaLoaderData) };

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

    let call_result: Result<(), mlua::Error> = lua_fn.call::<()>((args_i64, out_i64));

    match call_result {
        Ok(()) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(e) => {
            eprintln!("[polyplug_lua] Lua function call failed: {}", e);
            AbiError {
                code: polyplug_abi::ABI_ERROR_GENERIC,
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
}

impl BundleLoader for LuaLoader {
    fn runtime_name(&self) -> &'static str {
        "lua"
    }

    fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
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

        // Set package.path so that require("polyplug_guest") resolves correctly.
        let guest_path_code: String = format!(
            "package.path = package.path .. ';' .. '{}/?.lua'",
            GUEST_LUA_DIR.replace('\\', "/")
        );
        lua.load(&guest_path_code)
            .exec()
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: format!("Lua VM init failed: failed to set guest package.path: {}", e),
                })
            })?;

        // Set package.path so that require("polyplug_abi") resolves correctly.
        let abi_path_code: String = format!(
            "package.path = package.path .. ';' .. '{}/?.lua'",
            ABI_LUA_DIR.replace('\\', "/")
        );
        lua.load(&abi_path_code).exec().map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("Lua VM init failed: failed to set abi package.path: {}", e),
            })
        })?;

        // Read the plugin script source.
        let source: String =
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

        let bundle_dir_fwd: String = bundle_dir_str.replace('\\', "/");
        let path_code: String = format!(
            "package.path = \"{}/?.lua;{}/?.init.lua;\" .. package.path",
            bundle_dir_fwd, bundle_dir_fwd
        );
        lua.load(&path_code).exec().map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("Lua VM init failed: package.path injection failed: {e}"),
            })
        })?;
        let cpath_ext: &str = if cfg!(windows) { "dll" } else { "so" };
        let cpath_code: String = format!(
            "package.cpath = \"{}/?.{};\" .. package.cpath",
            bundle_dir_fwd, cpath_ext
        );
        lua.load(&cpath_code).exec().map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("Lua VM init failed: package.cpath injection failed: {e}"),
            })
        })?;
        // Execute the script. This defines polyplug_init in the global environment.
        lua.load(&source).exec().map_err(|e: mlua::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "Lua script load failed at {}: {}",
                    bundle_path.display(),
                    e
                ),
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

        // Create HostContext for rt_ctx parameter.
        let host_ctx: HostContext = HostContext {
            runtime: runtime as *const Runtime as *mut Runtime,
            bundle_id,
        };
        let rt_ctx: *mut core::ffi::c_void =
            &host_ctx as *const HostContext as *mut core::ffi::c_void;

        // Get host_vtable from runtime.
        let host_vtable: &'static polyplug_abi::RuntimeAbi = runtime.host_vtable();

        // Call polyplug_init — it populates _G._polyplug_handlers.
        // Pass rt_ctx, host_vtable pointer, and PluginContext pointer.
        // SAFETY: bundle_path_static outlives this call; leaked intentionally.
        let bundle_path_static: &'static str = Box::leak(bundle_dir_str.clone().into_boxed_str());
        let ctx: polyplug_abi::PluginContext = polyplug_abi::PluginContext {
            bundle_path: polyplug_abi::StringView {
                ptr: bundle_path_static.as_ptr(),
                len: bundle_path_static.len(),
            },
            host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
            bundle_id,
        };
        let rt_ctx_i64: i64 = rt_ctx as usize as i64;
        let host_vtable_i64: i64 = host_vtable as *const polyplug_abi::RuntimeAbi as usize as i64;
        let ctx_ptr: i64 = &ctx as *const polyplug_abi::PluginContext as i64;
        init_fn
            .call::<()>((rt_ctx_i64, host_vtable_i64, ctx_ptr))
            .map_err(|e: mlua::Error| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: bundle_name.clone(),
                    error: format!("Lua polyplug_init raised error: {}", e),
                })
            })?;

        // Read the handler table that polyplug_init populated.
        let handlers: Table =
            lua.globals()
                .get::<Table>("_polyplug_handlers")
                .map_err(|_: mlua::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("Lua plugin missing _polyplug_handlers: bundle={}", bundle_name),
                    })
                })?;

        // Extract metadata from handlers table.
        let contract_name_str: String = handlers
            .get::<String>("contract_name")
            .unwrap_or_else(|_: mlua::Error| "unknown".to_owned());

        let contract_version: u32 = handlers.get::<u32>("contract_version").unwrap_or(1_u32);

        let plugin_name_str: String = handlers
            .get::<String>("plugin_name")
            .unwrap_or_else(|_: mlua::Error| bundle_name.clone());

        let functions_table: Table =
            handlers
                .get::<Table>("functions")
                .map_err(|e: mlua::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: bundle_name.clone(),
                        error: format!("Lua handlers error: missing functions table: {}", e),
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
            let lua_fn: Function =
                functions_table
                    .get::<Function>(slot_idx as i64)
                    .map_err(|e: mlua::Error| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: bundle_name.clone(),
                            error: format!("Lua function slot {} error: {}", slot_idx, e),
                        })
                    })?;
            lua_functions.push(lua_fn);
        }

        // Build contract_id from contract_name and version.
        let cid: u64 = contract_id(&contract_name_str, contract_version);

        // Create LuaLoaderData with the Lua VM and functions.
        let loader_data: Box<LuaLoaderData> = Box::new(LuaLoaderData {
            _vm: lua,
            functions: lua_functions,
        });

        let loader_data_ptr: *mut LuaLoaderData = Box::into_raw(loader_data);

        // Build GuestContractInterface with VM dispatch.
        let plugin_interface: GuestContractInterface = GuestContractInterface {
            rt_ctx: core::ptr::null(),
            contract_id: cid,
            contract_version,
            function_count,
            dispatch_type: DispatchType::VirtualMachine,
            dispatch: polyplug_abi::PluginDispatch {
                vm: VmDispatch {
                    call: lua_dispatch,
                    loader_data: loader_data_ptr as *mut core::ffi::c_void,
                },
            },
        };

        // Leak the interface so it has 'static lifetime.
        // SAFETY: GuestContractInterface is leaked intentionally — the loader data must be 'static.
        // The interface is valid for the process lifetime (Lua plugins are never unloaded).
        let static_interface: *const GuestContractInterface = Box::into_raw(Box::new(plugin_interface));

        // Build static string slices for PluginDescriptor.
        // We leak String → &'static str so StringView ptrs remain valid indefinitely.
        let plugin_name_leaked: &'static str = Box::leak(plugin_name_str.into_boxed_str());
        let contract_name_leaked: &'static str = Box::leak(contract_name_str.into_boxed_str());

        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView {
                ptr: plugin_name_leaked.as_ptr(),
                len: plugin_name_leaked.len(),
            },
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version_major: contract_version,
            version_minor: 0_u32,
            version_patch: 0_u32,
        };

        // Call register_plugin via the HostVTable.
        // SAFETY: `rt_ctx` is a valid HostContext pointer for this call.
        // `descriptor` is stack-allocated and valid for this call (register_plugin must copy
        // any data it needs to retain — the contract is that descriptor is borrowed for the call only).
        // `static_interface` is a leaked Box — valid for 'static lifetime.
        let reg_result: AbiError = unsafe {
            (host_vtable.register_plugin)(
                rt_ctx,
                &descriptor as *const PluginDescriptor,
                static_interface,
            )
        };

        if reg_result.code != ABI_OK {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name,
                error: format!("register_plugin error: code={}", reg_result.code),
            }));
        }

        Ok(())
    }

    fn reload(
        &self,
        _manifest: &ManifestData,
        _runtime: &Runtime,
    ) -> Result<(), polyplug::error::RuntimeError> {
        Err(polyplug::error::RuntimeError::HotReloadDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_runtime_name() {
        let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
        assert_eq!(loader.runtime_name(), "lua");
    }
}
