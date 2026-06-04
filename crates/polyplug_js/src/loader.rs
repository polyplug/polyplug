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

use polyplug::Runtime as PolyplugRuntime;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::ManifestData;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::GuestContractId;

use crate::config::JsConfig;

// ─── Registration data stored in QuickJS runtime userdata ──────────────────────

use core::cell::RefCell;
use std::rc::Rc;

use rquickjs::JsLifetime;
use rquickjs::runtime::UserDataError;
use rquickjs::runtime::UserDataGuard;

/// Registration data collected from the JS plugin during polyplug_init.
///
/// This struct is stored in the QuickJS runtime's userdata to avoid thread-local
/// storage, ensuring multiple polyplug runtimes can coexist in the same process.
struct JsRegistrationData {
    contract_id: u64,
    contract_version: u32,
    _fn_count: usize,
    contract_name: String,
    functions: Vec<PersistentFunction>,
}

// SAFETY: JsRegistrationData has no lifetime parameters and contains only 'static
// data (Persistent<Function<'static>> is 'static). This implementation allows the
// type to be stored in rquickjs's userdata storage.
unsafe impl<'js> JsLifetime<'js> for JsRegistrationData {
    type Changed<'to> = JsRegistrationData;
}

/// Type alias for the registration slot stored in userdata.
/// Uses Rc<RefCell<>> to allow shared mutable access from both the loader and
/// the registerVtable callback.
type RegistrationSlot = Rc<RefCell<Option<JsRegistrationData>>>;

// ─── JS Loader Data for VM Dispatch ───────────────────────────────────────────

/// Type alias for a persistent JS function stored across scope boundaries.
type PersistentFunction = Persistent<Function<'static>>;

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

// ─── Instance Lifecycle Stubs ──────────────────────────────────────────────────

/// Stub create_instance for JS plugins - returns null instance.
///
/// # Safety
/// JS plugins use VM dispatch with global state; instances are not used.
unsafe extern "C" fn js_create_instance(
    _host: *const HostInterface,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// Stub destroy_instance for JS plugins - no cleanup needed.
///
/// # Safety
/// JS plugins don't own instance data.
unsafe extern "C" fn js_destroy_instance(
    _host: *const HostInterface,
    _instance: GuestContractInstance,
) {
}

// ─── JS Dispatch Function ─────────────────────────────────────────────────────

/// Dispatch function for JS plugins using VM dispatch pattern.
///
/// # Safety
/// - `loader_data` must be a valid VmLoaderData wrapping JsLoaderData
/// - `args` and `out` must be valid pointers for the ABI call
unsafe extern "C" fn js_dispatch(
    loader_data: VmLoaderData,
    _instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: loader_data wraps a valid pointer to JsLoaderData created by the loader.
    let data: &JsLoaderData = unsafe { &*(loader_data.data as *const JsLoaderData) };

    let func_persistent: &PersistentFunction = match data.functions.get(fn_id as usize) {
        Some(f) => f,
        None => {
            return AbiError {
                code: AbiErrorCode::FunctionNotAvailable,
                message: StringView::null(),
            };
        }
    };

    let args_usize: usize = args as usize;
    let out_usize: usize = out as usize;

    // Pass pointers as f64. User-space addresses fit within 2^48 < 2^53 (float64 mantissa),
    // so the conversion from usize → f64 → usize is exact on all supported platforms.
    // The generated JS wrapper receives plain Number arguments and passes them directly
    // to readU32/writeU32, which also accept f64 — no BigInt conversion needed.
    let args_f64: f64 = args_usize as f64;
    let out_f64: f64 = out_usize as f64;

    let call_result: Result<i32, rquickjs::Error> = data.ctx.with(|ctx| {
        let js_fn: Function<'_> = func_persistent.clone().restore(&ctx)?;

        let result: i32 = js_fn.call::<(f64, f64), i32>((args_f64, out_f64))?;
        Ok(result)
    });

    match call_result {
        Ok(0) => AbiError::ok(),
        Ok(code) => AbiError {
            // Safe: from_u32 does an exhaustive match; unknown codes map to Generic.
            // Previous transmute::<u32, AbiErrorCode> caused SIGABRT on invalid discriminants.
            code: AbiErrorCode::from_u32(code as u32),
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic,
            message: StringView::null(),
        },
    }
}

// ─── Host function registration ───────────────────────────────────────────────

fn pack_handle(h: GuestContractHandle) -> Option<u64> {
    if h.is_null() {
        None
    } else {
        Some(h.index as u64)
    }
}

/// Helper to get HostInterface pointer from JS globals.
///
/// Lo/hi are stored as f64 to avoid rquickjs sign-extending u32 > INT32_MAX to negative
/// tagged ints, which would cause u32::from_js to fail or return a wrong value.
fn get_host_interface_from_globals<'js>(ctx: &Ctx<'js>) -> Option<*const HostInterface> {
    let polyplug_obj: Object<'js> = ctx.globals().get::<&str, Object<'js>>("polyplug").ok()?;

    let vtable_lo: f64 = polyplug_obj.get::<&str, f64>("_hostVtableLo").ok()?;
    let vtable_hi: f64 = polyplug_obj.get::<&str, f64>("_hostVtableHi").ok()?;

    let host_interface_ptr: *const HostInterface =
        ((vtable_hi as u64) << 32 | vtable_lo as u64) as usize as *const HostInterface;

    if host_interface_ptr.is_null() {
        None
    } else {
        Some(host_interface_ptr)
    }
}

fn register_host_functions<'js>(
    ctx: &Ctx<'js>,
    polyplug_obj: &Object<'js>,
    host_interface: *const HostInterface,
    bundle_name: &str,
) -> Result<(), RuntimeError> {
    // Store host interface pointer as JS globals on the polyplug object
    let host_interface_usize: usize = host_interface as usize;

    // Store as f64: u32 > INT32_MAX would be sign-extended by rquickjs to a negative
    // tagged int, causing f64::from_js or u32::from_js to fail on read-back.
    polyplug_obj
        .set("_hostVtableLo", (host_interface_usize as u32) as f64)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: _hostVtableLo set failed: {e}"),
            })
        })?;
    polyplug_obj
        .set(
            "_hostVtableHi",
            ((host_interface_usize >> 32) as u32) as f64,
        )
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: _hostVtableHi set failed: {e}"),
            })
        })?;

    let find_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> Option<u64> {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt: *const HostInterface = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostInterface data.
            let handle: GuestContractHandle =
                unsafe { ((*hvt).find_guest_contract)(hvt, contract_id, min_ver) };
            pack_handle(handle)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findByContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findByContract", find_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findByContract set failed: {e}"),
            })
        })?;

    let find_by_bundle_fn: Function<'js> = Function::new(
        ctx.clone(),
        |_ctx: Ctx<'js>,
         _blo: u32,
         _bhi: u32,
         _clo: u32,
         _chi: u32,
         _min_ver: u32|
         -> Option<u64> {
            // Note: find_by_bundle was removed from HostInterface in the instance-based model.
            // Use find_by_contract instead.
            None
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findByBundle function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findByBundle", find_by_bundle_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findByBundle set failed: {e}"),
            })
        })?;

    let find_all_by_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, lo: u32, hi: u32, min_ver: u32| -> u32 {
            let contract_id: u64 = (hi as u64) << 32 | lo as u64;
            let hvt: *const HostInterface = match get_host_interface_from_globals(&ctx) {
                Some(ptr) => ptr,
                None => return 0_u32,
            };
            // SAFETY: hvt points to 'static HostInterface data.
            // find_all_guest_contracts returns Array<GuestContractHandle>.
            let handles: polyplug_abi::types::Array<GuestContractHandle> =
                unsafe { ((*hvt).find_all_guest_contracts)(hvt, contract_id, min_ver) };
            handles.len as u32
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: findAllByContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("findAllByContract", find_all_by_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: findAllByContract set failed: {e}"),
            })
        })?;

    let resolve_guest_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, packed: u64| -> Option<u64> {
            let index: u32 = packed as u32;
            let handle: GuestContractHandle = GuestContractHandle { index };
            let hvt: *const HostInterface = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostInterface data.
            let vtable_ptr: *const GuestContractInterface =
                unsafe { ((*hvt).resolve_guest_contract)(hvt, handle) };
            if vtable_ptr.is_null() {
                None
            } else {
                Some(vtable_ptr as usize as u64)
            }
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: resolveGuestContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("resolveGuestContract", resolve_guest_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: resolveGuestContract set failed: {e}"),
            })
        })?;

    let get_host_contract_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, contract_id: u64, min_version: u32| -> Option<u64> {
            let hvt: *const HostInterface = get_host_interface_from_globals(&ctx)?;
            // SAFETY: hvt points to 'static HostInterface data.
            let instance: polyplug_abi::HostContractInstance =
                unsafe { ((*hvt).get_host_contract)(hvt, contract_id, min_version) };
            if instance.data.is_null() {
                None
            } else {
                Some(instance.data as usize as u64)
            }
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: getHostContract function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("getHostContract", get_host_contract_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: getHostContract set failed: {e}"),
            })
        })?;

    let register_vtable_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>,
         contract_lo: u32,
         contract_hi: u32,
         vtable_obj: Object<'js>,
         fn_count: u32,
         contract_name: String| {
            let contract_id: u64 = (contract_hi as u64) << 32 | contract_lo as u64;
            let fn_count_usize: usize = fn_count as usize;

            let mut functions: Vec<PersistentFunction> = Vec::with_capacity(fn_count_usize);
            let functions_array: Object<'js> =
                match vtable_obj.get::<&str, Object<'js>>("functions") {
                    Ok(arr) => arr,
                    Err(_) => return,
                };

            for i in 0..fn_count_usize {
                let func: Function<'js> = match functions_array.get::<u32, Function<'js>>(i as u32)
                {
                    Ok(f) => f,
                    Err(_) => return,
                };
                let func_persistent: PersistentFunction = Persistent::save(&ctx, func);
                functions.push(func_persistent);
            }

            let data: JsRegistrationData = JsRegistrationData {
                contract_id,
                contract_version: 0,
                _fn_count: fn_count_usize,
                contract_name,
                functions,
            };

            let slot_guard: UserDataGuard<RegistrationSlot> =
                match ctx.userdata::<RegistrationSlot>() {
                    Some(guard) => guard,
                    None => return,
                };
            let mut cell: core::cell::RefMut<Option<JsRegistrationData>> = slot_guard.borrow_mut();
            *cell = Some(data);
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!(
                "JS runtime js-quickjs error: registerVtable function creation failed: {e}"
            ),
        })
    })?;

    polyplug_obj
        .set("registerVtable", register_vtable_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: registerVtable set failed: {e}"),
            })
        })?;

    let alloc_fn: Function<'js> = Function::new(
        ctx.clone(),
        |ctx: Ctx<'js>, size: u32| -> Result<Array<'js>, rquickjs::Error> {
            let hvt: *const HostInterface = match get_host_interface_from_globals(&ctx) {
                Some(ptr) => ptr,
                None => {
                    let arr: Array<'js> = Array::new(ctx.clone()).map_err(|_| {
                        rquickjs::Exception::throw_message(&ctx, "array creation failed")
                    })?;
                    let _ = arr.set(0, 0.0_f64);
                    let _ = arr.set(1, 0.0_f64);
                    return Ok(arr);
                }
            };
            // SAFETY: hvt points to 'static HostInterface data.
            let ptr: *mut u8 = unsafe { ((*hvt).alloc)(hvt, size as usize, 1) };
            let ptr_usize: usize = ptr as usize;
            let arr: Array<'js> = Array::new(ctx.clone())
                .map_err(|_| rquickjs::Exception::throw_message(&ctx, "array creation failed"))?;
            // Store as f64: rquickjs sign-extends u32 > INT32_MAX to negative JS ints,
            // which breaks BigInt reconstruction. f64 preserves the unsigned value exactly.
            let _ = arr.set(0, (ptr_usize as u32) as f64);
            let _ = arr.set(1, ((ptr_usize >> 32) as u32) as f64);
            Ok(arr)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: alloc function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("alloc", alloc_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: alloc set failed: {e}"),
            })
        })?;

    // lo/hi are f64 for the same reason as alloc's return values: u32 > INT32_MAX would be
    // sign-extended by rquickjs to a negative JS int, corrupting the pointer reconstruction.
    let free_fn: Function<'js> = Function::new(ctx.clone(), |ctx: Ctx<'js>, lo: f64, hi: f64| {
        let hvt: *const HostInterface = match get_host_interface_from_globals(&ctx) {
            Some(ptr) => ptr,
            None => return,
        };
        let ptr: *mut u8 = ((hi as u64) << 32 | lo as u64) as usize as *mut u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: hvt points to 'static HostInterface data.
        unsafe { ((*hvt).free)(hvt, ptr, 0, 1) };
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: free function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("free", free_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: free set failed: {e}"),
            })
        })?;

    let read_i32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> i32 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const i32 = ptr_u64 as usize as *const i32;
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readI32", read_i32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readI32 set failed: {e}"),
            })
        })?;

    let write_i32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: i32| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut i32 = ptr_u64 as usize as *mut i32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeI32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeI32", write_i32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeI32 set failed: {e}"),
            })
        })?;

    let read_byte_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> u32 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const u8 = ptr_u64 as usize as *const u8;
        if ptr.is_null() {
            return 0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr as u32 }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readByte function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readByte", read_byte_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readByte set failed: {e}"),
            })
        })?;

    let write_byte_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: u32| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut u8 = ptr_u64 as usize as *mut u8;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value as u8;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeByte function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeByte", write_byte_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeByte set failed: {e}"),
            })
        })?;

    let read_memory_fn: Function<'js> = Function::new(
        ctx.clone(),
        // Returns Array<'js> of u8 values instead of ArrayBuffer: a plain Array of integers
        // is unambiguous and Uint8Array(array_of_ints) works correctly in QuickJS.
        |ctx: Ctx<'js>, ptr_num: f64, len: u32| -> Result<Array<'js>, rquickjs::Error> {
            let ptr_u64: u64 = ptr_num as u64;
            let ptr: *const u8 = ptr_u64 as usize as *const u8;
            let len_usize: usize = len as usize;

            let arr: Array<'js> = Array::new(ctx.clone())
                .map_err(|_| rquickjs::Exception::throw_message(&ctx, "Array creation failed"))?;

            if ptr.is_null() || len_usize == 0 {
                return Ok(arr);
            }

            // SAFETY: ptr is a valid pointer provided by the host for reading.
            // The caller guarantees the memory region [ptr, ptr+len) is valid.
            let bytes: &[u8] = unsafe { core::slice::from_raw_parts(ptr, len_usize) };

            for (i, &byte) in bytes.iter().enumerate() {
                // Byte values are 0-255, always fit in 31-bit signed int — no sign-extension issue.
                let _ = arr.set(i, u32::from(byte) as f64);
            }

            Ok(arr)
        },
    )
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readMemory function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readMemory", read_memory_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readMemory set failed: {e}"),
            })
        })?;

    // Return f64 instead of u32: rquickjs sign-extends u32 > INT32_MAX when converting to
    // JavaScript tagged ints. Returning f64 ensures the full unsigned range [0, 2^32) is
    // represented exactly as a JS Number (all u32 values are < 2^53 = float64 mantissa cap).
    let read_u32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64| -> f64 {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *const u32 = ptr_u64 as usize as *const u32;
        if ptr.is_null() {
            return 0.0;
        }
        // SAFETY: ptr is a valid pointer provided by the host for reading.
        unsafe { *ptr as f64 }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: readU32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("readU32", read_u32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: readU32 set failed: {e}"),
            })
        })?;

    // Both arguments are f64: same reasoning as readU32 — u32 values > INT32_MAX would be
    // sign-extended by rquickjs if typed as u32, corrupting large pointer halves or values.
    let write_u32_fn: Function<'js> = Function::new(ctx.clone(), |ptr_num: f64, value: f64| {
        let ptr_u64: u64 = ptr_num as u64;
        let ptr: *mut u32 = ptr_u64 as usize as *mut u32;
        if ptr.is_null() {
            return;
        }
        // SAFETY: ptr is a valid pointer provided by the host for writing.
        unsafe {
            *ptr = value as u64 as u32;
        }
    })
    .map_err(|e: rquickjs::Error| {
        RuntimeError::Loader(LoaderError::InitFailed {
            bundle: bundle_name.to_owned(),
            error: format!("JS runtime js-quickjs error: writeU32 function creation failed: {e}"),
        })
    })?;

    polyplug_obj
        .set("writeU32", write_u32_fn)
        .map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: bundle_name.to_owned(),
                error: format!("JS runtime js-quickjs error: writeU32 set failed: {e}"),
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

    fn load(&self, manifest: &ManifestData, runtime: &PolyplugRuntime) -> Result<(), RuntimeError> {
        let bundle_id: u64 = manifest.id;

        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            manifest.path.join("bundle.js")
        };
        let bundle_js: String =
            std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                RuntimeError::Loader(LoaderError::ManifestParse {
                    path: bundle_path.display().to_string(),
                    reason: e.to_string(),
                })
            })?;

        let bundle_dir: &Path = &manifest.path;

        let qjs_runtime: Runtime = Runtime::new().map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime init failed: QuickJS runtime init failed: {e}"),
            })
        })?;

        let ctx: Context = Context::full(&qjs_runtime).map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime js-quickjs error: context creation failed: {e}"),
            })
        })?;

        // Get the HostInterface pointer from the runtime.
        // This interface already has the runtime pointer set internally.
        let host_interface: *const HostInterface = runtime.as_context_ptr();

        // Set bundle_id in TLS for dependency enforcement during init.
        polyplug::runtime::set_init_bundle_id(bundle_id);

        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();

        let registration_slot: RegistrationSlot = Rc::new(RefCell::new(None));

        ctx.with(|ctx_ref: Ctx<'_>| {
            ctx_ref
                .store_userdata(Rc::clone(&registration_slot))
                .map_err(
                    |_: UserDataError<Rc<RefCell<Option<JsRegistrationData>>>>| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: "JS runtime js-quickjs error: failed to store registration slot in userdata".to_owned(),
                        })
                    },
                )?;

            let globals: Object<'_> = ctx_ref.globals();
            let polyplug_obj: Object<'_> =
                Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: object creation failed: {e}"),
                    })
                })?;
            register_host_functions(
                &ctx_ref,
                &polyplug_obj,
                host_interface,
                &manifest.name,
            )?;
            globals
                .set("polyplug", polyplug_obj)
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: global set failed: {e}"),
                    })
                })?;

            let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
            ctx_ref
                .eval::<Value<'_>, _>(set_bundle.as_str())
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundlePath injection failed: {e}"),
                    })
                })?;

            ctx_ref
                .eval::<Value<'_>, _>(bundle_js.as_str())
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundle eval failed: {e}"),
                    })
                })?;

            let init_fn: Function<'_> = ctx_ref
                .globals()
                .get::<&str, Function<'_>>("polyplug_init")
                .map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_dir_str.clone(),
                    })
                })?;

            // SAFETY: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str =
                Box::leak(bundle_dir_str.clone().into_boxed_str());
            let plugin_ctx: BundleInitContext = BundleInitContext {
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
                bundle_id,
            };

            // Pass HostInterface and BundleInitContext pointers as 4 f64 arguments:
            //   (host_lo, host_hi, ctx_lo, ctx_hi)
            // The generated polyplug_init expects this 4-arg lo/hi split convention.
            // f64 is used instead of u32 because rquickjs sign-extends u32 > INT32_MAX to
            // negative tagged ints. f64 represents the full unsigned 32-bit range exactly.
            let host_usize: usize = host_interface as usize;
            let ctx_usize: usize = &plugin_ctx as *const BundleInitContext as usize;
            let host_lo: f64 = (host_usize as u32) as f64;
            let host_hi: f64 = ((host_usize >> 32) as u32) as f64;
            let ctx_lo: f64 = (ctx_usize as u32) as f64;
            let ctx_hi: f64 = ((ctx_usize >> 32) as u32) as f64;

            init_fn
                .call::<(f64, f64, f64, f64), ()>((host_lo, host_hi, ctx_lo, ctx_hi))
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: polyplug_init call failed: {e}"),
                    })
                })?;

            Ok::<(), RuntimeError>(())
        })?;

        // Clear bundle_id TLS after init completes.
        polyplug::runtime::clear_init_bundle_id();

        let registration_data: JsRegistrationData =
            registration_slot.borrow_mut().take().ok_or_else(|| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: "JS runtime js-quickjs error: polyplug_init did not call registerVtable"
                        .to_owned(),
                })
            })?;

        let loader_data: Box<JsLoaderData> = Box::new(JsLoaderData {
            _runtime: qjs_runtime,
            ctx,
            functions: registration_data.functions,
        });

        let loader_data_ptr: *mut JsLoaderData = Box::into_raw(loader_data);

        let contract_id = GuestContractId::from_u64(registration_data.contract_id);
        let major_version = (registration_data.contract_version >> 16) as u32;

        let plugin_interface: GuestContractInterface = GuestContractInterface {
            contract_id,
            contract_version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::VirtualMachine,
            create_instance: js_create_instance,
            destroy_instance: js_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: js_dispatch,
                    loader_data: VmLoaderData {
                        data: loader_data_ptr as *mut core::ffi::c_void,
                    },
                },
            },
        };

        let static_interface: *const GuestContractInterface =
            Box::into_raw(Box::new(plugin_interface));

        let contract_name_leaked: &'static str =
            Box::leak(registration_data.contract_name.into_boxed_str());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-quickjs-plugin"),
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: host_interface, descriptor, and static_interface are valid for this call.
        // The register_contract function uses self-passing pattern.
        let abi_result: AbiError = unsafe {
            ((*host_interface).register_contract)(host_interface, &descriptor, static_interface)
        };

        if !abi_result.is_ok() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "JS runtime js-quickjs error: register_contract returned error code {:?}",
                    abi_result.code
                ),
            }));
        }

        Ok(())
    }

    fn reload(
        &self,
        manifest: &ManifestData,
        runtime: &PolyplugRuntime,
    ) -> Result<(), RuntimeError> {
        if !runtime.config().hot_reload_enabled {
            return Err(RuntimeError::HotReloadDisabled);
        }

        let bundle_id: u64 = manifest.id;

        let bundle_path: PathBuf = if !manifest.file.is_empty() {
            manifest.path.join(&manifest.file)
        } else {
            manifest.path.join("bundle.js")
        };
        let bundle_js: String =
            std::fs::read_to_string(&bundle_path).map_err(|e: std::io::Error| {
                RuntimeError::Loader(LoaderError::ManifestParse {
                    path: bundle_path.display().to_string(),
                    reason: e.to_string(),
                })
            })?;

        let bundle_dir: &Path = &manifest.path;

        let qjs_runtime: Runtime = Runtime::new().map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime init failed: QuickJS runtime init failed: {e}"),
            })
        })?;

        let ctx: Context = Context::full(&qjs_runtime).map_err(|e: rquickjs::Error| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("JS runtime js-quickjs error: context creation failed: {e}"),
            })
        })?;

        // Get the HostInterface pointer from the runtime.
        // This interface already has the runtime pointer set internally.
        let host_interface: *const HostInterface = runtime.as_context_ptr();

        // Set bundle_id in TLS for dependency enforcement during init.
        polyplug::runtime::set_init_bundle_id(bundle_id);

        let bundle_dir_str: String = bundle_dir.to_string_lossy().into_owned();

        let registration_slot: RegistrationSlot = Rc::new(RefCell::new(None));

        ctx.with(|ctx_ref: Ctx<'_>| {
            ctx_ref
                .store_userdata(Rc::clone(&registration_slot))
                .map_err(
                    |_: UserDataError<Rc<RefCell<Option<JsRegistrationData>>>>| {
                        RuntimeError::Loader(LoaderError::InitFailed {
                            bundle: manifest.name.clone(),
                            error: "JS runtime js-quickjs error: failed to store registration slot in userdata".to_owned(),
                        })
                    },
                )?;

            let globals: Object<'_> = ctx_ref.globals();
            let polyplug_obj: Object<'_> =
                Object::new(ctx_ref.clone()).map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: object creation failed: {e}"),
                    })
                })?;
            register_host_functions(
                &ctx_ref,
                &polyplug_obj,
                host_interface,
                &manifest.name,
            )?;
            globals
                .set("polyplug", polyplug_obj)
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: global set failed: {e}"),
                    })
                })?;

            let set_bundle: String = format!("globalThis.bundlePath = {:?};", bundle_dir_str);
            ctx_ref
                .eval::<Value<'_>, _>(set_bundle.as_str())
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundlePath injection failed: {e}"),
                    })
                })?;

            ctx_ref
                .eval::<Value<'_>, _>(bundle_js.as_str())
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: bundle eval failed: {e}"),
                    })
                })?;

            let init_fn: Function<'_> = ctx_ref
                .globals()
                .get::<&str, Function<'_>>("polyplug_init")
                .map_err(|_| {
                    RuntimeError::Loader(LoaderError::InitSymbolMissing {
                        bundle: bundle_dir_str.clone(),
                    })
                })?;

            // SAFETY: Intentionally leaked; bundle_path_static outlives this call.
            let bundle_path_static: &'static str =
                Box::leak(bundle_dir_str.clone().into_boxed_str());
            let plugin_ctx: BundleInitContext = BundleInitContext {
                bundle_path: StringView {
                    ptr: bundle_path_static.as_ptr(),
                    len: bundle_path_static.len(),
                },
                bundle_id,
            };

            // Pass HostInterface and BundleInitContext pointers as 4 f64 arguments:
            //   (host_lo, host_hi, ctx_lo, ctx_hi)
            // The generated polyplug_init expects this 4-arg lo/hi split convention.
            // f64 is used instead of u32 because rquickjs sign-extends u32 > INT32_MAX to
            // negative tagged ints. f64 represents the full unsigned 32-bit range exactly.
            let host_usize: usize = host_interface as usize;
            let ctx_usize: usize = &plugin_ctx as *const BundleInitContext as usize;
            let host_lo: f64 = (host_usize as u32) as f64;
            let host_hi: f64 = ((host_usize >> 32) as u32) as f64;
            let ctx_lo: f64 = (ctx_usize as u32) as f64;
            let ctx_hi: f64 = ((ctx_usize >> 32) as u32) as f64;

            init_fn
                .call::<(f64, f64, f64, f64), ()>((host_lo, host_hi, ctx_lo, ctx_hi))
                .map_err(|e: rquickjs::Error| {
                    RuntimeError::Loader(LoaderError::InitFailed {
                        bundle: manifest.name.clone(),
                        error: format!("JS runtime js-quickjs error: polyplug_init call failed: {e}"),
                    })
                })?;

            Ok::<(), RuntimeError>(())
        })?;

        // Clear bundle_id TLS after init completes.
        polyplug::runtime::clear_init_bundle_id();

        let registration_data: JsRegistrationData =
            registration_slot.borrow_mut().take().ok_or_else(|| {
                RuntimeError::Loader(LoaderError::InitFailed {
                    bundle: manifest.name.clone(),
                    error: "JS runtime js-quickjs error: polyplug_init did not call registerVtable"
                        .to_owned(),
                })
            })?;

        let loader_data: Box<JsLoaderData> = Box::new(JsLoaderData {
            _runtime: qjs_runtime,
            ctx,
            functions: registration_data.functions,
        });

        let loader_data_ptr: *mut JsLoaderData = Box::into_raw(loader_data);

        let contract_id = GuestContractId::from_u64(registration_data.contract_id);
        let major_version = (registration_data.contract_version >> 16) as u32;

        let plugin_interface: GuestContractInterface = GuestContractInterface {
            contract_id,
            contract_version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
            dispatch_type: DispatchType::VirtualMachine,
            create_instance: js_create_instance,
            destroy_instance: js_destroy_instance,
            dispatch: DispatchMechanisms {
                vm: VmDispatch {
                    call: js_dispatch,
                    loader_data: VmLoaderData {
                        data: loader_data_ptr as *mut core::ffi::c_void,
                    },
                },
            },
        };

        let static_interface: *const GuestContractInterface =
            Box::into_raw(Box::new(plugin_interface));

        let contract_name_leaked: &'static str =
            Box::leak(registration_data.contract_name.into_boxed_str());
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"js-quickjs-plugin"),
            contract_name: StringView {
                ptr: contract_name_leaked.as_ptr(),
                len: contract_name_leaked.len(),
            },
            version: Version {
                major: major_version,
                minor: 0,
                patch: 0,
            },
        };

        // SAFETY: host_interface, descriptor, and static_interface are valid for this call.
        // The register_contract function uses self-passing pattern.
        let abi_result: AbiError = unsafe {
            ((*host_interface).register_contract)(host_interface, &descriptor, static_interface)
        };

        if !abi_result.is_ok() {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "JS runtime js-quickjs error: register_contract returned error code {:?}",
                    abi_result.code
                ),
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
