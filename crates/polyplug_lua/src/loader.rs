//! LuaJIT VM initialization and plugin loader implementation.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::OnceLock;

use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Value;

use crate::config::LuaConfig;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::BundleLoader;
use polyplug::runtime::HostContext;
use polyplug::runtime::Runtime;
use polyplug_abi::contract_id;
use polyplug_abi::AbiError;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug_abi::ABI_OK;

/// The path to the guest-libs/lua/ directory, set at compile time by build.rs.
const GUEST_LUA_DIR: &str = env!("POLYPLUG_GUEST_LUA_DIR");

/// Process-global LuaJIT VM. Created on first use.
/// mlua::Lua with the `send` feature is Send+Sync, so OnceLock<Lua> is valid.
/// A separate Mutex serializes the initialization race.
static LUA_VM: OnceLock<Lua> = OnceLock::new();

/// Guards concurrent initialization of LUA_VM.
static LUA_VM_INIT: Mutex<()> = Mutex::new(());

/// Global registry of Lua functions stored for trampoline dispatch.
/// Indexed by slot index assigned during plugin registration.
/// Protected by a Mutex because trampolines may be called from any thread.
static FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<Function>>>> = OnceLock::new();

/// Get or initialize the function registry.
fn function_registry() -> &'static Mutex<Vec<Option<Function>>> {
    FUNCTION_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Dispatch a Lua function call by slot index.
///
/// Reads from `FUNCTION_REGISTRY[slot]`, calls the `mlua::Function` with
/// `(args_ptr_as_i64, out_ptr_as_i64)`, and returns an `AbiError`.
fn dispatch_lua_call(slot: usize, args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
    let reg: &Mutex<Vec<Option<Function>>> = function_registry();
    let guard: MutexGuard<'_, Vec<Option<Function>>> =
        reg.lock().unwrap_or_else(|e| e.into_inner());
    let lua_fn: &Function = match guard.get(slot).and_then(|f: &Option<Function>| f.as_ref()) {
        Some(f) => f,
        None => {
            return AbiError {
                code: 1,
                message: StringView::null(),
            };
        }
    };
    // Pass pointers as i64 to preserve full 64-bit precision on LuaJIT.
    // LuaJIT lua_Integer is int64_t — safe for pointer-width integers.
    let args_i64: i64 = args_ptr as usize as i64;
    let out_i64: i64 = out_ptr as usize as i64;
    let call_result: Result<(), mlua::Error> = lua_fn.call::<()>((args_i64, out_i64));
    // The mlua call borrows `lua_fn` from `guard`. We must drop `guard` before returning
    // so the Mutex is released. The Result is extracted first.
    drop(guard);
    match call_result {
        Ok(()) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

// Pre-generated static extern "C" trampolines (slots 0..63).
// Each trampoline has a hardcoded slot index and calls `dispatch_lua_call`.
// We cannot use closures for extern "C" fn pointers — static trampolines
// with a hardcoded slot are the correct Rust solution.
macro_rules! make_trampoline {
    ($name:ident, $slot:expr) => {
        // SAFETY: trampolines are `extern "C"` functions with the ABI signature
        // expected by PluginVTable.functions: fn(*const (), *mut ()) -> AbiError.
        // `dispatch_lua_call` is safe to call from any thread (uses Mutex-protected registry).
        unsafe extern "C" fn $name(args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
            dispatch_lua_call($slot, args_ptr, out_ptr)
        }
    };
}

make_trampoline!(trampoline_0, 0);
make_trampoline!(trampoline_1, 1);
make_trampoline!(trampoline_2, 2);
make_trampoline!(trampoline_3, 3);
make_trampoline!(trampoline_4, 4);
make_trampoline!(trampoline_5, 5);
make_trampoline!(trampoline_6, 6);
make_trampoline!(trampoline_7, 7);
make_trampoline!(trampoline_8, 8);
make_trampoline!(trampoline_9, 9);
make_trampoline!(trampoline_10, 10);
make_trampoline!(trampoline_11, 11);
make_trampoline!(trampoline_12, 12);
make_trampoline!(trampoline_13, 13);
make_trampoline!(trampoline_14, 14);
make_trampoline!(trampoline_15, 15);
make_trampoline!(trampoline_16, 16);
make_trampoline!(trampoline_17, 17);
make_trampoline!(trampoline_18, 18);
make_trampoline!(trampoline_19, 19);
make_trampoline!(trampoline_20, 20);
make_trampoline!(trampoline_21, 21);
make_trampoline!(trampoline_22, 22);
make_trampoline!(trampoline_23, 23);
make_trampoline!(trampoline_24, 24);
make_trampoline!(trampoline_25, 25);
make_trampoline!(trampoline_26, 26);
make_trampoline!(trampoline_27, 27);
make_trampoline!(trampoline_28, 28);
make_trampoline!(trampoline_29, 29);
make_trampoline!(trampoline_30, 30);
make_trampoline!(trampoline_31, 31);
make_trampoline!(trampoline_32, 32);
make_trampoline!(trampoline_33, 33);
make_trampoline!(trampoline_34, 34);
make_trampoline!(trampoline_35, 35);
make_trampoline!(trampoline_36, 36);
make_trampoline!(trampoline_37, 37);
make_trampoline!(trampoline_38, 38);
make_trampoline!(trampoline_39, 39);
make_trampoline!(trampoline_40, 40);
make_trampoline!(trampoline_41, 41);
make_trampoline!(trampoline_42, 42);
make_trampoline!(trampoline_43, 43);
make_trampoline!(trampoline_44, 44);
make_trampoline!(trampoline_45, 45);
make_trampoline!(trampoline_46, 46);
make_trampoline!(trampoline_47, 47);
make_trampoline!(trampoline_48, 48);
make_trampoline!(trampoline_49, 49);
make_trampoline!(trampoline_50, 50);
make_trampoline!(trampoline_51, 51);
make_trampoline!(trampoline_52, 52);
make_trampoline!(trampoline_53, 53);
make_trampoline!(trampoline_54, 54);
make_trampoline!(trampoline_55, 55);
make_trampoline!(trampoline_56, 56);
make_trampoline!(trampoline_57, 57);
make_trampoline!(trampoline_58, 58);
make_trampoline!(trampoline_59, 59);
make_trampoline!(trampoline_60, 60);
make_trampoline!(trampoline_61, 61);
make_trampoline!(trampoline_62, 62);
make_trampoline!(trampoline_63, 63);

/// Maximum number of function slots supported (one per pre-generated trampoline).
const MAX_TRAMPOLINES: usize = 64;

/// Static array of pre-generated trampolines indexed by slot.
static TRAMPOLINES: [unsafe extern "C" fn(*const (), *mut ()) -> AbiError; MAX_TRAMPOLINES] = [
    trampoline_0,
    trampoline_1,
    trampoline_2,
    trampoline_3,
    trampoline_4,
    trampoline_5,
    trampoline_6,
    trampoline_7,
    trampoline_8,
    trampoline_9,
    trampoline_10,
    trampoline_11,
    trampoline_12,
    trampoline_13,
    trampoline_14,
    trampoline_15,
    trampoline_16,
    trampoline_17,
    trampoline_18,
    trampoline_19,
    trampoline_20,
    trampoline_21,
    trampoline_22,
    trampoline_23,
    trampoline_24,
    trampoline_25,
    trampoline_26,
    trampoline_27,
    trampoline_28,
    trampoline_29,
    trampoline_30,
    trampoline_31,
    trampoline_32,
    trampoline_33,
    trampoline_34,
    trampoline_35,
    trampoline_36,
    trampoline_37,
    trampoline_38,
    trampoline_39,
    trampoline_40,
    trampoline_41,
    trampoline_42,
    trampoline_43,
    trampoline_44,
    trampoline_45,
    trampoline_46,
    trampoline_47,
    trampoline_48,
    trampoline_49,
    trampoline_50,
    trampoline_51,
    trampoline_52,
    trampoline_53,
    trampoline_54,
    trampoline_55,
    trampoline_56,
    trampoline_57,
    trampoline_58,
    trampoline_59,
    trampoline_60,
    trampoline_61,
    trampoline_62,
    trampoline_63,
];

/// Ensures the global Lua VM is initialized with the correct package.path.
/// Idempotent: subsequent calls return the already-initialized VM.
///
/// # Errors
/// Returns `PolyplugError::Loader(LoaderError::LuaVmInitFailed)` if VM creation fails.
pub(crate) fn ensure_lua_initialized(_config: &LuaConfig) -> Result<&'static Lua, PolyplugError> {
    if let Some(vm) = LUA_VM.get() {
        return Ok(vm);
    }
    let _guard: MutexGuard<'_, ()> = LUA_VM_INIT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(vm) = LUA_VM.get() {
        return Ok(vm);
    }
    // mlua 0.10: Lua::new() returns Lua directly (not Result).
    // mlua 0.10: Lua::unsafe_new() enables the FFI module required by LuaJIT plugins.
    // SAFETY: We trust the Lua scripts loaded through this loader. The LuaJIT FFI is
    // required for the polyplug_guest.lua ABI bridge (struct layout, pointer casts).
    // All plugins are vetted before being passed to the loader.
    let lua: Lua = unsafe { Lua::unsafe_new() };
    // Set package.path so that require("polyplug_guest") resolves correctly.
    let package_path_code: String = format!(
        "package.path = package.path .. ';' .. '{}/?.lua'",
        GUEST_LUA_DIR.replace('\\', "/")
    );
    lua.load(&package_path_code)
        .exec()
        .map_err(|e: mlua::Error| {
            PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                reason: format!("failed to set package.path: {}", e),
            })
        })?;
    // SAFETY: We hold `_guard` (LUA_VM_INIT) and already checked above that
    let _ = LUA_VM.set(lua);
    LUA_VM.get().ok_or_else(|| {
        PolyplugError::Loader(LoaderError::LuaVmInitFailed {
            reason: "LUA_VM unavailable after initialization".to_owned(),
        })
    })
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

    fn load(&self, path: &Path, runtime: &Runtime) -> Result<(), PolyplugError> {
        let lua: &Lua = ensure_lua_initialized(&self.config)?;

        // Clear globals from any previous load to ensure isolation.
        // If the script does not define polyplug_init, we must return LuaInitFunctionMissing.
        lua.globals()
            .set("polyplug_init", mlua::Value::Nil)
            .map_err(|e: mlua::Error| {
                PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                    reason: format!("failed to clear polyplug_init global: {}", e),
                })
            })?;
        lua.globals()
            .set("_polyplug_handlers", mlua::Value::Nil)
            .map_err(|e: mlua::Error| {
                PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                    reason: format!("failed to clear _polyplug_handlers global: {}", e),
                })
            })?;

        // Read the plugin script source.
        let source: String = std::fs::read_to_string(path).map_err(|e: std::io::Error| {
            PolyplugError::Loader(LoaderError::LuaScriptLoadFailed {
                path: path.display().to_string(),
                reason: e.to_string(),
            })
        })?;

        // Extract bundle directory for package.path / package.cpath injection.
        let bundle_dir: std::path::PathBuf = path.parent().unwrap_or(path).to_path_buf();
        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();

        // Parse manifest to get bundle_id.
        let manifest: ManifestData =
            polyplug::loader::parse_manifest(&bundle_dir).map_err(PolyplugError::Loader)?;
        if manifest.id == 0 {
            return Err(PolyplugError::Loader(LoaderError::InitFailed {
                bundle: path.display().to_string(),
                error: "manifest.id is required but was 0 or missing".to_owned(),
            }));
        }
        let bundle_id: u64 = manifest.id;

        let bundle_dir_fwd: String = bundle_dir_str.replace('\\', "/");
        let path_code: String = format!(
            "package.path = \"{}/?.lua;{}/?.init.lua;\" .. package.path",
            bundle_dir_fwd, bundle_dir_fwd
        );
        lua.load(&path_code).exec().map_err(|e: mlua::Error| {
            PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                reason: format!("package.path injection failed: {e}"),
            })
        })?;
        let cpath_ext: &str = if cfg!(windows) { "dll" } else { "so" };
        let cpath_code: String = format!(
            "package.cpath = \"{}/?.{};\" .. package.cpath",
            bundle_dir_fwd, cpath_ext
        );
        lua.load(&cpath_code).exec().map_err(|e: mlua::Error| {
            PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                reason: format!("package.cpath injection failed: {e}"),
            })
        })?;
        // Execute the script. This defines polyplug_init in the global environment.
        lua.load(&source).exec().map_err(|e: mlua::Error| {
            PolyplugError::Loader(LoaderError::LuaScriptLoadFailed {
                path: path.display().to_string(),
                reason: e.to_string(),
            })
        })?;

        // Derive bundle name for error messages.
        let bundle_name: String = path
            .file_name()
            .map(|n: &OsStr| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());

        // Retrieve polyplug_init global function.
        let init_fn: Function =
            lua.globals()
                .get::<Function>("polyplug_init")
                .map_err(|_: mlua::Error| {
                    PolyplugError::Loader(LoaderError::LuaInitFunctionMissing {
                        bundle: bundle_name.clone(),
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
        let host_vtable: &'static polyplug_abi::HostVTable = runtime.host_vtable();

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
        let host_vtable_i64: i64 = host_vtable as *const polyplug_abi::HostVTable as usize as i64;
        let ctx_ptr: i64 = &ctx as *const polyplug_abi::PluginContext as i64;
        init_fn
            .call::<()>((rt_ctx_i64, host_vtable_i64, ctx_ptr))
            .map_err(|e: mlua::Error| {
                PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                    bundle: bundle_name.clone(),
                    message: e.to_string(),
                })
            })?;

        // Read the handler table that polyplug_init populated.
        let handlers: Table =
            lua.globals()
                .get::<Table>("_polyplug_handlers")
                .map_err(|_: mlua::Error| {
                    PolyplugError::Loader(LoaderError::LuaInitFunctionMissing {
                        bundle: bundle_name.clone(),
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
                    PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                        bundle: bundle_name.clone(),
                        message: format!("missing functions table: {}", e),
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

        // Validate we have enough pre-generated trampolines.
        let base_slot: usize;
        {
            let reg: &Mutex<Vec<Option<Function>>> = function_registry();
            let mut guard: MutexGuard<'_, Vec<Option<Function>>> =
                reg.lock().unwrap_or_else(|e| e.into_inner());
            base_slot = guard.len();

            // Check we won't exceed the trampoline array bounds.
            if base_slot + function_count as usize > MAX_TRAMPOLINES {
                return Err(PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                    bundle: bundle_name.clone(),
                    message: format!(
                        "too many total function slots: {} + {} > {}",
                        base_slot, function_count, MAX_TRAMPOLINES
                    ),
                }));
            }

            // Store Lua functions in the global registry.
            for slot_idx in 0..function_count {
                let lua_fn: Function = functions_table.get::<Function>(slot_idx as i64).map_err(
                    |e: mlua::Error| {
                        PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                            bundle: bundle_name.clone(),
                            message: format!("function slot {} error: {}", slot_idx, e),
                        })
                    },
                )?;
                guard.push(Some(lua_fn));
            }
        }

        // Build the function pointer array using pre-generated trampolines.
        // Each element points to the static trampoline for its slot index.
        let mut fn_ptr_vec: Vec<*const ()> = Vec::with_capacity(function_count as usize);
        for slot_offset in 0..function_count as usize {
            let slot: usize = base_slot + slot_offset;
            // SAFETY: TRAMPOLINES[slot] is a valid static extern "C" function.
            // We cast the fn pointer to *const () for storage in PluginVTable.functions.
            // The trampoline is 'static — it lives for the entire process lifetime.
            let fn_ptr: *const () = TRAMPOLINES[slot] as *const ();
            fn_ptr_vec.push(fn_ptr);
        }

        // Leak the fn_ptr_vec as a boxed slice so it has 'static lifetime.
        // SAFETY: PluginVTable.functions must point to 'static data.
        // Box::into_raw produces a valid, non-null, properly-aligned pointer.
        let fn_pointers_box: Box<[*const ()]> = fn_ptr_vec.into_boxed_slice();
        let functions_ptr: *const *const () = Box::into_raw(fn_pointers_box) as *const *const ();

        // Build contract_id from contract_name and version.
        let cid: u64 = contract_id(&contract_name_str, contract_version);

        // Build PluginVTable.
        let vtable: PluginVTable = PluginVTable {
            contract_id: cid,
            contract_version,
            function_count,
            functions: functions_ptr,
        };

        // Leak vtable so it has 'static lifetime.
        // SAFETY: PluginVTable is leaked intentionally — fn pointer arrays must be 'static.
        // The vtable is valid for the process lifetime (Lua plugins are never unloaded).
        let vtable_ptr: *const PluginVTable = Box::into_raw(Box::new(vtable));

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
        // `vtable_ptr` is a leaked Box — valid for 'static lifetime.
        let reg_result: AbiError = unsafe {
            (host_vtable.register_plugin)(
                rt_ctx,
                &descriptor as *const PluginDescriptor,
                vtable_ptr,
            )
        };

        if reg_result.code != ABI_OK {
            return Err(PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                bundle: bundle_name,
                message: format!("register_plugin returned error code {}", reg_result.code),
            }));
        }

        Ok(())
    }
}
