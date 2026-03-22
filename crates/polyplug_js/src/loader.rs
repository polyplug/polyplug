//! QuickJS in-process plugin loader implementation.
//!
//! Loads JS plugin bundles via the embedded QuickJS VM (rquickjs).
//! Each bundle gets its own QuickJS Runtime and Context for complete isolation
//! between bundles and between polyplug Runtime instances.
//! Uses VM dispatch to call JS functions through the QuickJS API.

use std::path::Path;
use std::path::PathBuf;

use rquickjs::Array;
use rquickjs::Context;
use rquickjs::Ctx;
use rquickjs::Function;
use rquickjs::Object;
use rquickjs::Persistent;
use rquickjs::Runtime;
use rquickjs::Value;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::BundleLoader;
use polyplug::runtime::HostContext;
use polyplug::runtime::Runtime as PolyplugRuntime;
use polyplug_abi::AbiError;
use polyplug_abi::DispatchType;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginDispatch;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_abi::VmDispatch;
use polyplug_abi::ABI_OK;

use crate::config::JsConfig;

// ─── JS Loader Data for VM Dispatch ───────────────────────────────────────────

/// Type alias for a persistent JS function stored across scope boundaries.
type PersistentFunction = Persistent<Function<'static>>;

/// Vtable data extracted from a JS bundle: (contract_id, version, fn_count, contract_name, functions).
type VtableData = (u64, u32, usize, String, Vec<PersistentFunction>);

/// Loader-specific data for JS plugin dispatch.
///
/// Each bundle gets its own QuickJS Runtime and Context, ensuring complete
/// isolation between bundles and between polyplug Runtime instances.
/// The Context is cached for fast dispatch without per-call creation overhead.
pub struct JsLoaderData {
    pub _runtime: Runtime,
    pub ctx: Context,
    pub functions: Vec<PersistentFunction>,
}

// ─── JS Dispatch Function ─────────────────────────────────────────────────────

/// Dispatch function for JS plugins using VM dispatch pattern.
///
/// # Safety
/// - `loader_data` must be a valid pointer to `JsLoaderData`
/// - `args` and `out` must be valid pointers for the ABI call
unsafe extern "C" fn js_dispatch(
    loader_data: *mut core::ffi::c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: loader_data is a valid pointer to JsLoaderData created by the loader.
    let data: &JsLoaderData = unsafe { &*(loader_data as *const JsLoaderData) };

    let func_persistent: &PersistentFunction = match data.functions.get(fn_id as usize) {
        Some(f) => f,
        None => {
            return AbiError {
                code: polyplug_abi::ABI_FUNCTION_NOT_AVAIL,
                message: StringView::null(),
            };
        }
    };

    let args_usize: usize = args as usize;
    let out_usize: usize = out as usize;
    let args_lo: u32 = args_usize as u32;
    let args_hi: u32 = (args_usize >> 32) as u32;
    let out_lo: u32 = out_usize as u32;
    let out_hi: u32 = (out_usize >> 32) as u32;

    let call_result: Result<(), rquickjs::Error> = data.ctx.with(|ctx| {
        let js_fn: Function<'_> = func_persistent.clone().restore(&ctx)?;
        js_fn.call::<(u32, u32, u32, u32), ()>((args_lo, args_hi, out_lo, out_hi))
    });

    match call_result {
        Ok(()) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(e) => {
            eprintln!("[polyplug_js] JS function call failed: {}", e);
            AbiError {
                code: polyplug_abi::ABI_ERROR_GENERIC,
                message: StringView::null(),
            }
        }
    }
}

// ─── Host function registration ───────────────────────────────────────────────

fn pack_handle(h: PluginHandle) -> Option<u64> {
    if h.is_null() {
        None
    } else {
        Some((h.generation as u64) << 32 | h.index as u64)
    }
}

/// Helper to get host context pointers from JS globals.
fn get_host_ctx_from_globals<'js>(
    ctx: &Ctx<'js>,
) -> Option<(*const HostVTable, *mut core::ffi::c_void)> {
    let polyplug_obj: Object<'js> = ctx.globals().get::<&str, Object<'js>>("polyplug").ok()?;

    let vtable_lo: u32 = polyplug_obj.get::<&str, u32>("_hostVtableLo").ok()?;
    let vtable_hi: u32 = polyplug_obj.get::<&str, u32>("_hostVtableHi").ok()?;
    let rt_ctx_lo: u32 = polyplug_obj.get::<&str, u32>("_rtCtxLo").ok()?;
    let rt_ctx_hi: u32 = polyplug_obj.get::<&str, u32>("_rtCtxHi").ok()?;

    let vtable_ptr: *const HostVTable =
        ((vtable_hi as u64) << 32 | vtable_lo as u64) as usize as *const HostVTable;
    let rt_ctx: *mut core::ffi::c_void =
        ((rt_ctx_hi as u64) << 32 | rt_ctx_lo as u64) as usize as *mut core::ffi::c_void;

    if vtable_ptr.is_null() || rt_ctx.is_null() {
        None
    } else {
        Some((vtable_ptr, rt_ctx))
    }
}

fn register_host_functions<'js>(
    ctx: &Ctx<'js>,
    polyplug_obj: &Object<'js>,
    host_vtable: *const HostVTable,
    rt_ctx: *mut core::ffi::c_void,
) -> Result<(), PolyplugError> {
    // Store host context pointers as JS globals on the polyplug object
    let vtable_usize: usize = host_vtable as usize;
    let rt_ctx_usize: usize = rt_ctx as usize;

    polyplug_obj
        .set("_hostVtableLo", vtable_usize as u32)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("_hostVtableLo set failed: {e}"),
            })
        })?;
    polyplug_obj
        .set("_hostVtableHi", (vtable_usize >> 32) as u32)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("_hostVtableHi set failed: {e}"),
            })
        })?;
    polyplug_obj
        .set("_rtCtxLo", rt_ctx_usize as u32)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("_rtCtxLo set failed: {e}"),
            })
        })?;
    polyplug_obj
        .set("_rtCtxHi", (rt_ctx_usize >> 32) as u32)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("_rtCtxHi set failed: {e}"),
            })
        })?;

    let find_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> Option<u64> {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let (hvt, rt_ctx) = get_host_ctx_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let handle: PluginHandle =
                unsafe { ((*hvt).find_by_contract)(rt_ctx, contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("findByContract function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("findByContract", find_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findByContract set failed: {e}"),
            })
        })?;

    let find_by_bundle_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, blo: u32, bhi: u32, clo: u32, chi: u32, min_ver: u32| -> Option<u64> {
            let bundle_id: u64 = (bhi as u64) << 32 | blo as u64;
            let contract_id: u64 = (chi as u64) << 32 | clo as u64;
            let (hvt, rt_ctx) = get_host_ctx_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let handle: PluginHandle =
                unsafe { ((*hvt).find_by_bundle)(rt_ctx, bundle_id, contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("findByBundle function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("findByBundle", find_by_bundle_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findByBundle set failed: {e}"),
            })
        })?;

    let find_all_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> u32 {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let (hvt, rt_ctx) = match get_host_ctx_from_globals(&ctx) {
                Some(pair) => pair,
                None => return 0_u32,
            };
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let count: usize = unsafe {
                ((*hvt).find_all_by_contract)(
                    rt_ctx,
                    contract_id,
                    min_ver,
                    core::ptr::null_mut(),
                    0,
                )
            };
            count as u32
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("findAllByContract function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("findAllByContract", find_all_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("findAllByContract set failed: {e}"),
            })
        })?;

    let resolve_plugin_fn: Function<'js> =
        Function::new(ctx.clone(), |ctx: Ctx<'js>, packed: u64| -> Option<u64> {
            let index: u32 = packed as u32;
            let generation: u32 = (packed >> 32) as u32;
            let handle: PluginHandle = PluginHandle { index, generation };
            let (hvt, rt_ctx) = get_host_ctx_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let vtable_ptr: *const PluginInterface =
                unsafe { ((*hvt).resolve_plugin)(rt_ctx, handle) };
            if vtable_ptr.is_null() {
                None
            } else {
                Some(vtable_ptr as usize as u64)
            }
        })
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("resolvePlugin function creation failed: {e}"),
            })
        })?;

    polyplug_obj
        .set("resolvePlugin", resolve_plugin_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("resolvePlugin set failed: {e}"),
            })
        })?;

    let get_extension_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, extension_id: u32| -> Option<u64> {
            let (hvt, rt_ctx) = get_host_ctx_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let ext_ptr: *const () = unsafe { ((*hvt).get_extension)(rt_ctx, extension_id) };
            if ext_ptr.is_null() {
                None
            } else {
                Some(ext_ptr as usize as u64)
            }
        },
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("getExtension function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("getExtension", get_extension_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("getExtension set failed: {e}"),
            })
        })?;

    let register_vtable_fn: Function<'js> = Function::new(
        ctx.clone(),
        |_contract_lo: u32,
         _contract_hi: u32,
         _vtable_lo: u32,
         _vtable_hi: u32,
         _fn_count: u32,
         _contract_name: String| {},
    )
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("registerVtable function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("registerVtable", register_vtable_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("registerVtable set failed: {e}"),
            })
        })?;

    let alloc_fn: Function<'js> =
        Function::new(ctx.clone(), |ctx: Ctx<'js>, size: u32| -> Array<'js> {
            let (hvt, rt_ctx) = match get_host_ctx_from_globals(&ctx) {
                Some(pair) => pair,
                None => {
                    let arr: Array<'js> = Array::new(ctx.clone()).unwrap_or_else(|_| {
                        Array::new(ctx.clone()).unwrap_or_else(|_| panic!("array creation failed"))
                    });
                    let _ = arr.set(0, 0_u32);
                    let _ = arr.set(1, 0_u32);
                    return arr;
                }
            };
            // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
            let ptr: *mut u8 = unsafe { ((*hvt).alloc)(rt_ctx, size as usize, 1) };
            let ptr_usize: usize = ptr as usize;
            let arr: Array<'js> = Array::new(ctx.clone()).unwrap_or_else(|_| {
                Array::new(ctx.clone()).unwrap_or_else(|_| panic!("array creation failed"))
            });
            let _ = arr.set(0, ptr_usize as u32);
            let _ = arr.set(1, (ptr_usize >> 32) as u32);
            arr
        })
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("alloc function creation failed: {e}"),
            })
        })?;

    polyplug_obj
        .set("alloc", alloc_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("alloc set failed: {e}"),
            })
        })?;

    let free_fn: Function<'js> = Function::new(ctx.clone(), |ctx: Ctx<'js>, lo: u32, hi: u32| {
        let (hvt, rt_ctx) = match get_host_ctx_from_globals(&ctx) {
            Some(pair) => pair,
            None => return,
        };
        let ptr: *mut u8 = ((hi as u64) << 32 | lo as u64) as usize as *mut u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: hvt points to 'static data; rt_ctx is valid during bundle eval.
        unsafe { ((*hvt).free)(rt_ctx, ptr, 0, 1) };
    })
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("free function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("free", free_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("free set failed: {e}"),
            })
        })?;

    // readI32(ptr_lo: u32, ptr_hi: u32) -> i32
    // Reads an i32 from host memory at the given pointer.
    let read_i32_fn: Function<'js> = Function::new(ctx.clone(), |lo: u32, hi: u32| -> i32 {
        let ptr: *const i32 = ((hi as u64) << 32 | lo as u64) as usize as *const i32;
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr }
    })
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("readI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readI32", read_i32_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("readI32 set failed: {e}"),
            })
        })?;

    // writeI32(ptr_lo: u32, ptr_hi: u32, value: i32) -> void
    // Writes an i32 to host memory at the given pointer.
    let write_i32_fn: Function<'js> = Function::new(ctx.clone(), |lo: u32, hi: u32, value: i32| {
        let ptr: *mut i32 = ((hi as u64) << 32 | lo as u64) as usize as *mut i32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value;
        }
    })
    .map_err(|e: rquickjs::Error| {
        PolyplugError::Loader(LoaderError::JsRuntimePanic {
            runtime: "js-quickjs".to_owned(),
            message: format!("writeI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeI32", write_i32_fn)
        .map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("writeI32 set failed: {e}"),
            })
        })?;

    Ok(())
}

// ─── JsLoader ────────────────────────────────────────────────────────────────

/// QuickJS in-process JS plugin loader.
pub struct JsLoader {
    _config: JsConfig,
}

impl JsLoader {
    pub fn new(config: JsConfig) -> JsLoader {
        JsLoader { _config: config }
    }
}

impl BundleLoader for JsLoader {
    fn runtime_name(&self) -> &'static str {
        "js-quickjs"
    }

    fn load(&self, path: &Path, runtime: &PolyplugRuntime) -> Result<(), PolyplugError> {
        let host_vtable: &'static HostVTable = runtime.host_vtable();

        let bundle_dir: &Path = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        let manifest: ManifestData = polyplug::loader::parse_manifest(bundle_dir)
            .map_err(|e: polyplug::error::LoaderError| PolyplugError::Loader(e))?;
        let bundle_id: u64 = manifest.id;

        let bundle_path: PathBuf = if path.is_dir() {
            path.join("bundle.js")
        } else {
            path.to_path_buf()
        };
        let bundle_js: String =
            std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                PolyplugError::Loader(LoaderError::ManifestParse {
                    path: bundle_path.display().to_string(),
                    reason: e.to_string(),
                })
            })?;

        let qjs_runtime: Runtime = Runtime::new().map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimeInitFailed {
                reason: format!("QuickJS runtime init failed: {e}"),
            })
        })?;

        let ctx: Context = Context::full(&qjs_runtime).map_err(|e: rquickjs::Error| {
            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("context creation failed: {e}"),
            })
        })?;

        let mut host_ctx: HostContext = HostContext {
            runtime: runtime as *const PolyplugRuntime as *mut PolyplugRuntime,
            bundle_id,
        };
        let rt_ctx: *mut core::ffi::c_void =
            &mut host_ctx as *mut HostContext as *mut core::ffi::c_void;

        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();

        let eval_result: Result<VtableData, PolyplugError> = ctx.with(|ctx_ref: Ctx<'_>| {
            let globals: Object<'_> = ctx_ref.globals();
            let polyplug_obj: Object<'_> =
                Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-quickjs".to_owned(),
                        message: format!("object creation failed: {e}"),
                    })
                })?;
            register_host_functions(
                &ctx_ref,
                &polyplug_obj,
                host_vtable as *const HostVTable,
                rt_ctx,
            )?;
            globals
                .set("polyplug", polyplug_obj)
                .map_err(|e: rquickjs::Error| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-quickjs".to_owned(),
                        message: format!("global set failed: {e}"),
                    })
                })?;

            let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
            ctx_ref
                .eval::<Value<'_>, _>(set_bundle.as_str())
                .map_err(|e: rquickjs::Error| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-quickjs".to_owned(),
                        message: format!("bundlePath injection failed: {e}"),
                    })
                })?;

            ctx_ref
                .eval::<Value<'_>, _>(bundle_js.as_str())
                .map_err(|e: rquickjs::Error| {
                    PolyplugError::Loader(LoaderError::JsRuntimePanic {
                        runtime: "js-quickjs".to_owned(),
                        message: format!("bundle eval failed: {e}"),
                    })
                })?;

            // Scan global variables for vtable objects (ending with _VTABLE).
            let global_obj: Object<'_> = ctx_ref.globals();
            let mut found_vtable: Option<VtableData> = None;

            for key_result in global_obj.keys::<String>() {
                let key: String = match key_result {
                    Ok(k) => k,
                    Err(_) => continue,
                };

                if !key.ends_with("_VTABLE") {
                    continue;
                }

                let vtable_obj: Object<'_> = match global_obj.get::<String, Object<'_>>(key) {
                    Ok(obj) => obj,
                    Err(_) => continue,
                };

                let contract_lo: u32 = vtable_obj.get::<&str, u32>("contractLo").unwrap_or(0);
                let contract_hi: u32 = vtable_obj.get::<&str, u32>("contractHi").unwrap_or(0);
                let contract_id: u64 = (contract_hi as u64) << 32 | contract_lo as u64;
                let fn_count: usize = vtable_obj.get::<&str, u32>("fnCount").unwrap_or(0) as usize;
                let contract_name: String = vtable_obj
                    .get::<&str, String>("contractName")
                    .unwrap_or_else(|_| "unknown".to_owned());

                let functions_array: Object<'_> =
                    match vtable_obj.get::<&str, Object<'_>>("functions") {
                        Ok(arr) => arr,
                        Err(_) => continue,
                    };

                let mut functions: Vec<PersistentFunction> = Vec::with_capacity(fn_count);
                for i in 0..fn_count {
                    let func: Function<'_> = functions_array
                        .get::<u32, Function<'_>>(i as u32)
                        .map_err(|e| {
                            PolyplugError::Loader(LoaderError::JsRuntimePanic {
                                runtime: "js-quickjs".to_owned(),
                                message: format!("failed to get function at index {}: {}", i, e),
                            })
                        })?;
                    // Use Persistent to safely store the function across scope boundaries.
                    let func_persistent: PersistentFunction = Persistent::save(&ctx_ref, func);
                    functions.push(func_persistent);
                }

                found_vtable = Some((contract_id, 0_u32, fn_count, contract_name, functions));
                break;
            }

            found_vtable.ok_or_else(|| {
                PolyplugError::Loader(LoaderError::JsRuntimePanic {
                    runtime: "js-quickjs".to_owned(),
                    message: "no vtable found in bundle (expected global ending with _VTABLE)"
                        .to_owned(),
                })
            })
        });

        let (contract_id_val, contract_version, fn_count, contract_name_str, js_functions) =
            eval_result?;

        let loader_data: Box<JsLoaderData> = Box::new(JsLoaderData {
            _runtime: qjs_runtime,
            ctx,
            functions: js_functions,
        });

        let loader_data_ptr: *mut JsLoaderData = Box::into_raw(loader_data);

        let plugin_interface: PluginInterface = PluginInterface {
            rt_ctx: core::ptr::null(),
            contract_id: contract_id_val,
            contract_version,
            function_count: fn_count as u32,
            dispatch_type: DispatchType::VirtualMachine,
            dispatch: PluginDispatch {
                vm: VmDispatch {
                    call: js_dispatch,
                    loader_data: loader_data_ptr as *mut core::ffi::c_void,
                },
            },
        };

        let static_interface: *const PluginInterface = Box::into_raw(Box::new(plugin_interface));

        let contract_name_leaked: &'static str = Box::leak(contract_name_str.into_boxed_str());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-quickjs-plugin"),
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version_major: contract_version >> 16,
            version_minor: contract_version & 0xFFFF,
            version_patch: 0_u32,
        };

        // SAFETY: rt_ctx, descriptor, and static_interface are valid for this call.
        let abi_result: AbiError =
            unsafe { (host_vtable.register_plugin)(rt_ctx, &descriptor, static_interface) };

        if abi_result.code != ABI_OK {
            return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic {
                runtime: "js-quickjs".to_owned(),
                message: format!("register_plugin returned error code {}", abi_result.code),
            }));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_quickjs_runtime_name() {
        let loader: JsLoader = JsLoader::new(JsConfig {});
        assert_eq!(loader.runtime_name(), "js-quickjs");
    }
}
