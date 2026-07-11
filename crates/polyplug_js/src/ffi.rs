//! FFI exports for polyplug_js — `polyplug_js_loader_create` and `polyplug_js_loader_free`.

use core::ffi::c_void;

use core::mem;
use core::ptr;

use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::CallArena;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::StringView;
use polyplug_abi::VmLoaderData;
use polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms;
use polyplug_abi::dispatch::vm_dispatch::VmDispatch;
use polyplug_abi::types::Version;
use polyplug_utils::GuestContractId;

use polyplug::loader::BundleLoader;

use crate::JsLoader;

type JsDispatchCallback = unsafe extern "C" fn(*mut c_void, u32, *const c_void, *mut c_void) -> u32;
type JsDestroyCallback = unsafe extern "C" fn(*mut c_void);
type JsCreateCallback = unsafe extern "C" fn(*const HostApi, *const c_void) -> u64;

/// Scalar callbacks supplied by a JavaScript host adapter.
///
/// The Rust trampoline owns every by-value ABI structure. JavaScript callbacks
/// therefore receive only pointers and scalar values, a shape supported by Deno,
/// Node, and Bun FFI backends.
#[repr(C)]
pub struct PolyplugJsInProcessBridge {
    dispatch: JsDispatchCallback,
    destroy: JsDestroyCallback,
    create: JsCreateCallback,
    contract_id: u64,
}

struct JsInProcessResident {
    bridge: Box<PolyplugJsInProcessBridge>,
    interface: Box<GuestContractInterface>,
}

/// Translate JavaScript's scalar dispatch callback into the canonical VM ABI.
///
/// # Safety
/// Every non-null pointer must satisfy the canonical VM dispatch ABI. `adapter_context`
/// must identify a live bridge resident for the duration of the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_vm_dispatch(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    instance: GuestContractInstance,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    _arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    let result: AbiError = if adapter_context.is_null() {
        AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::from_static(b"javascript in-process bridge is null"),
        }
    } else {
        // SAFETY: adapter_context comes from the resident-owned interface created below.
        let bridge: &PolyplugJsInProcessBridge =
            unsafe { &*(adapter_context as *const PolyplugJsInProcessBridge) };
        // SAFETY: the JavaScript resident retains the callback handle until logical unload.
        let code: u32 = unsafe { (bridge.dispatch)(instance.data, fn_id, args.cast(), out.cast()) };
        if code == AbiErrorCode::Ok as u32 {
            AbiError::ok()
        } else {
            AbiError {
                code,
                message: StringView::from_static(
                    b"javascript in-process implementation returned an error",
                ),
            }
        }
    };
    if !out_err.is_null() {
        // SAFETY: ABI callers provide a writable error out-param when non-null.
        unsafe { out_err.write(result) };
    }
}

/// Translate the canonical create-instance ABI into JavaScript's scalar factory.
///
/// # Safety
/// `adapter_context` must identify a live bridge resident, and non-null host, argument,
/// and output pointers must satisfy the canonical create-instance ABI.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_create_instance(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    host: *const HostApi,
    args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if out_instance.is_null() {
        return;
    }
    // SAFETY: checked non-null above; always initialize before a callback can fail.
    unsafe { out_instance.write(GuestContractInstance::null()) };
    if adapter_context.is_null() {
        return;
    }
    // SAFETY: adapter_context comes from the resident-owned interface created below.
    let bridge: &PolyplugJsInProcessBridge =
        unsafe { &*(adapter_context as *const PolyplugJsInProcessBridge) };
    // SAFETY: the JavaScript resident retains the callback handle until logical unload.
    let instance_id: u64 = unsafe { (bridge.create)(host, args.cast()) };
    let Ok(instance_id) = usize::try_from(instance_id) else {
        return;
    };
    if instance_id == 0 {
        return;
    }
    // SAFETY: out_instance remains writable for this synchronous callback.
    unsafe {
        (*out_instance).data = instance_id as *mut c_void;
        (*out_instance).contract_id = GuestContractId::from_u64(bridge.contract_id);
    }
}

/// Translate the canonical destroy-instance ABI into JavaScript's scalar teardown.
///
/// # Safety
/// `adapter_context` must identify a live bridge resident. A non-null instance must
/// have been created by that resident and must not have been destroyed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_destroy_instance(
    adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    instance: GuestContractInstance,
) {
    if adapter_context.is_null() || instance.data.is_null() {
        return;
    }
    // SAFETY: adapter_context comes from the resident-owned interface created below.
    let bridge: &PolyplugJsInProcessBridge =
        unsafe { &*(adapter_context as *const PolyplugJsInProcessBridge) };
    // SAFETY: instance data is the opaque numeric identifier issued by this bridge.
    unsafe { (bridge.destroy)(instance.data) };
}

/// Allocate a runtime-local JavaScript bridge and its canonical interface.
///
/// Returns an opaque resident which must be released by
/// [`polyplug_js_in_process_bridge_free`] after logical unload.
///
/// # Safety
/// The callback pointers must be non-null functions with the scalar callback signatures
/// represented by [`PolyplugJsInProcessBridge`] and must remain callable until unload.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_bridge_create(
    dispatch: *const c_void,
    destroy: *const c_void,
    create: *const c_void,
    contract_id: u64,
    major: u32,
    minor: u32,
    patch: u32,
) -> *mut c_void {
    if dispatch.is_null() || destroy.is_null() || create.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: null has been rejected. The constructor requires this exact callback signature.
    let dispatch: JsDispatchCallback =
        unsafe { mem::transmute::<*const c_void, JsDispatchCallback>(dispatch) };
    // SAFETY: null has been rejected. The constructor requires this exact callback signature.
    let destroy: JsDestroyCallback =
        unsafe { mem::transmute::<*const c_void, JsDestroyCallback>(destroy) };
    // SAFETY: null has been rejected. The constructor requires this exact callback signature.
    let create: JsCreateCallback =
        unsafe { mem::transmute::<*const c_void, JsCreateCallback>(create) };
    let bridge: Box<PolyplugJsInProcessBridge> = Box::new(PolyplugJsInProcessBridge {
        dispatch,
        destroy,
        create,
        contract_id,
    });
    let adapter_context: *mut c_void = (&*bridge as *const PolyplugJsInProcessBridge)
        .cast_mut()
        .cast();
    let interface: Box<GuestContractInterface> = Box::new(GuestContractInterface {
        contract_id: GuestContractId::from_u64(contract_id),
        contract_version: Version {
            major,
            minor,
            patch,
        },
        dispatch_type: DispatchType::VirtualMachine,
        adapter_context,
        create_instance: polyplug_js_in_process_create_instance,
        destroy_instance: polyplug_js_in_process_destroy_instance,
        dispatch: DispatchMechanisms {
            vm: VmDispatch {
                call: polyplug_js_in_process_vm_dispatch,
                loader_data: VmLoaderData::null(),
            },
        },
    });
    Box::into_raw(Box::new(JsInProcessResident { bridge, interface })).cast()
}

/// Return the canonical interface owned by a JavaScript bridge resident.
///
/// # Safety
/// `resident` must be a non-freed result of
/// [`polyplug_js_in_process_bridge_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_bridge_interface(
    resident: *const c_void,
) -> *const GuestContractInterface {
    if resident.is_null() {
        return ptr::null();
    }
    // SAFETY: caller guarantees the opaque resident was allocated by this module.
    unsafe { &*(resident as *const JsInProcessResident) }
        .interface
        .as_ref()
}

/// Return the opaque callback context owned by a JavaScript bridge resident.
///
/// # Safety
/// `resident` must be a non-freed result of
/// [`polyplug_js_in_process_bridge_create`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_bridge_context(
    resident: *const c_void,
) -> *mut c_void {
    if resident.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: caller guarantees the opaque resident was allocated by this module.
    let resident: &JsInProcessResident = unsafe { &*(resident as *const JsInProcessResident) };
    (&*resident.bridge as *const PolyplugJsInProcessBridge)
        .cast_mut()
        .cast()
}

/// Release a JavaScript bridge resident after logical unload.
///
/// # Safety
/// `resident` must be a non-freed result of
/// [`polyplug_js_in_process_bridge_create`], or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_in_process_bridge_free(resident: *mut c_void) {
    if resident.is_null() {
        return;
    }
    // SAFETY: caller guarantees this opaque allocation is freed at most once.
    unsafe { drop(Box::from_raw(resident as *mut JsInProcessResident)) };
}

/// Returns a pointer that must be freed with `polyplug_js_loader_free`.
#[unsafe(no_mangle)]
pub extern "C" fn polyplug_js_loader_create() -> *mut c_void {
    let loader: JsLoader = JsLoader::new();
    // Double-box: inner Box<dyn BundleLoader> preserves the fat pointer (data + vtable),
    // outer Box stores it on the heap so we can pass a thin *mut c_void across FFI.
    let trait_obj: Box<dyn BundleLoader> = Box::new(loader);
    Box::into_raw(Box::new(trait_obj)) as *mut c_void
}

/// # Safety
/// `ptr` must be a non-freed pointer returned by `polyplug_js_loader_create`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn polyplug_js_loader_free(ptr: *mut c_void) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: ptr was returned by polyplug_js_loader_create via
    // Box::into_raw(Box::new(trait_obj)) where trait_obj: Box<dyn BundleLoader>.
    // Caller guarantees ptr is not used after this call.
    unsafe {
        drop(Box::<Box<dyn BundleLoader>>::from_raw(
            ptr as *mut Box<dyn BundleLoader>,
        ))
    };
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use core::ptr;
    use core::sync::atomic::AtomicU64;
    use core::sync::atomic::Ordering;

    use polyplug_abi::AbiError;
    use polyplug_abi::AbiErrorCode;
    use polyplug_abi::GuestContractInstance;
    use polyplug_abi::HostApi;
    use polyplug_abi::VmLoaderData;

    use super::polyplug_js_in_process_bridge_context;
    use super::polyplug_js_in_process_bridge_create;
    use super::polyplug_js_in_process_bridge_free;
    use super::polyplug_js_in_process_bridge_interface;
    use super::polyplug_js_loader_create;
    use super::polyplug_js_loader_free;

    #[test]
    fn create_without_configuration_returns_a_freeable_loader() {
        let loader = polyplug_js_loader_create();
        assert!(!loader.is_null());
        // SAFETY: `loader` was just returned by the matching constructor and
        // has not been freed.
        unsafe { polyplug_js_loader_free(loader) };
    }

    static DESTROYED_INSTANCE: AtomicU64 = AtomicU64::new(0);

    unsafe extern "C" fn bridge_create(_host: *const HostApi, _args: *const c_void) -> u64 {
        41
    }

    unsafe extern "C" fn bridge_dispatch(
        instance: *mut c_void,
        function_id: u32,
        _args: *const c_void,
        out: *mut c_void,
    ) -> u32 {
        if instance as usize != 41 || function_id != 3 || out.is_null() {
            return AbiErrorCode::Generic as u32;
        }
        // SAFETY: this test supplies a valid writable u32 output slot.
        unsafe { (out as *mut u32).write(99) };
        AbiErrorCode::Ok as u32
    }

    unsafe extern "C" fn bridge_destroy(instance: *mut c_void) {
        DESTROYED_INSTANCE.store(instance as usize as u64, Ordering::SeqCst);
    }

    #[test]
    fn scalar_bridge_expands_the_canonical_lifecycle_and_vm_abis() {
        DESTROYED_INSTANCE.store(0, Ordering::SeqCst);
        // SAFETY: all callback pointers use the constructor's fixed scalar ABI and
        // the returned resident remains live through each callback invocation.
        let resident: *mut c_void = unsafe {
            polyplug_js_in_process_bridge_create(
                bridge_dispatch as *const () as *const c_void,
                bridge_destroy as *const () as *const c_void,
                bridge_create as *const () as *const c_void,
                0xCAFE_BABE,
                1,
                0,
                0,
            )
        };
        assert!(!resident.is_null());
        // SAFETY: resident was created above and has not been freed.
        let interface = unsafe { polyplug_js_in_process_bridge_interface(resident) };
        // SAFETY: resident was created above and has not been freed.
        let context = unsafe { polyplug_js_in_process_bridge_context(resident) };
        assert!(!interface.is_null());
        assert!(!context.is_null());

        let mut instance = GuestContractInstance::null();
        // SAFETY: the bridge, interface, and out instance stay valid for this call.
        unsafe {
            ((*interface).create_instance)(
                context,
                VmLoaderData::null(),
                ptr::null(),
                ptr::null(),
                &mut instance,
            );
        }
        assert_eq!(instance.data as usize, 41);
        assert_eq!(instance.contract_id.id(), 0xCAFE_BABE);

        let mut output: u32 = 0;
        let mut error = AbiError::ok();
        // SAFETY: interface is VM-dispatch and the bridge validates all scalar values.
        unsafe {
            ((*interface).dispatch.vm.call)(
                context,
                (*interface).dispatch.vm.loader_data,
                instance,
                3,
                ptr::null(),
                (&mut output as *mut u32).cast(),
                ptr::null_mut(),
                &mut error,
            );
        }
        assert_eq!(error.code, AbiErrorCode::Ok as u32);
        assert_eq!(output, 99);

        // SAFETY: instance was created by this bridge and remains live.
        unsafe {
            ((*interface).destroy_instance)(context, VmLoaderData::null(), ptr::null(), instance);
            polyplug_js_in_process_bridge_free(resident);
        }
        assert_eq!(DESTROYED_INSTANCE.load(Ordering::SeqCst), 41);
    }
}
