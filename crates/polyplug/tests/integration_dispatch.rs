#![allow(clippy::expect_used)]

//! Integration test: call through interface, verify function executes and returns Ok.
//!
//! This test crate is the crate root for the `integration_dispatch` test binary.

use core::cell::Ref;
use core::cell::RefCell;
use core::ffi::c_void;
use core::mem;
use core::ptr;
use core::slice;
use core::str;

use libloading::{Library, Symbol};
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_abi::{
    AbiError, AbiErrorCode, Array, BundleInitContext, DependencyInfo, GuestContractHandle,
    GuestContractInstance, GuestContractInterface, HostApi, HostContractInstance,
    HostContractInterface, PluginDescriptor, StringView,
};
use polyplug_utils::{BundleId, GuestContractId};

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Host functions that store interface into a Registry ─────────────────────────

/// A register_guest_contract callback that stores interface entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `this`, `descriptor`, `interface`, and `out_err` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if descriptor.is_null() || interface.is_null() {
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe {
                out_err.write(AbiError {
                    code: AbiErrorCode::InvalidPointer as u32,
                    message: StringView::null(),
                })
            };
        }
        return;
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] = slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        str::from_utf8_unchecked(bytes)
    };

    // Register with thread-local Registry.
    // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<GuestContractHandle, _> = DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: Ref<'_, RuntimeStore> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe {
            registry.register_guest_contract(
                *desc,
                interface,
                contract_name.to_owned(),
                BundleId::from_u64(iface.contract_id.id()),
            )
        }
    });

    let err: AbiError = match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok as u32,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(err) };
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    ptr::null()
}

/// No-op load_bundle callback.
unsafe extern "C" fn noop_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op get_last_error callback.
unsafe extern "C" fn noop_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// No-op get_error_len callback.
unsafe extern "C" fn noop_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

thread_local! {
    static DISPATCH_REGISTRY: RefCell<RuntimeStore> = RefCell::new(RuntimeStore::new());
}

/// AddArgs — mirrors the struct in test_plugin (must be `#[repr(C)]`).
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_dispatch_add_function() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: Library =
        unsafe { Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library") };

    // Resolve init function (2-arg signature).
    // SAFETY: polyplug_init matches the expected ABI.
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset the thread-local registry before the test.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    let host_interface: HostApi = HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    };

    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must succeed"
    );

    // Look up the test.add plugin.
    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: GuestContractHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });

    // Resolve the interface.
    let interface_ptr: *const GuestContractInterface = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("handle must be valid")
    });

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: dispatch is a union, accessing .native requires unsafe since dispatch_type is Native.
    let function_count: u32 = unsafe { interface.dispatch.native.function_count };
    assert_eq!(function_count, 1, "test.add interface must have 1 function");

    // Call function_id 0 (the `add` function).
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface. args and out are correctly typed.
    // The function has the frozen native ABI signature:
    //   extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError)
    // SAFETY: dispatch is a union, accessing .native requires unsafe since dispatch_type is Native.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: function 0 has the exact generated native dispatch ABI declared above.
        unsafe { mem::transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid AddArgs, out is a valid u32 location. The instance
    // is a null stateless handle, which test.add's stateless add ignores.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "add function must return Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    // Leak the library.
    mem::forget(library);
}

#[test]
fn test_dispatch_add_with_zero() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: Library =
        unsafe { Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library") };

    // SAFETY: polyplug_init matches the expected ABI (2-arg signature).
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset registry.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    let host_interface: HostApi = HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    };

    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok as u32);

    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: GuestContractHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let interface_ptr: *const GuestContractInterface = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("handle must be valid")
    });

    // SAFETY: interface_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*interface_ptr).dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: function 0 has the exact generated native dispatch ABI declared above.
        unsafe { mem::transmute(fn_ptr) }
    };

    let args: AddArgs = AddArgs { a: 0, b: 0 };
    let mut out: u32 = 99_u32;

    let mut result: AbiError = AbiError::ok();
    // SAFETY: args and out are valid and correctly typed; null stateless instance.
    unsafe {
        dispatch_fn(
            (*interface_ptr).adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut result,
        )
    };

    assert_eq!(result.code, AbiErrorCode::Ok as u32);
    assert_eq!(out, 0_u32, "add(0, 0) must equal 0");

    mem::forget(library);
}

#[test]
fn test_dispatch_add_wrapping_overflow() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: Library =
        unsafe { Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library") };

    // SAFETY: polyplug_init matches the expected ABI (2-arg signature).
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    let host_interface: HostApi = HostApi {
        runtime: ptr::null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: ptr::null(),
    };

    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok as u32);

    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: GuestContractHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let interface_ptr: *const GuestContractInterface = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("handle must be valid")
    });

    // SAFETY: interface_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*interface_ptr).dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: function 0 has the exact generated native dispatch ABI declared above.
        unsafe { mem::transmute(fn_ptr) }
    };

    // u32::MAX + 1 wraps to 0 (wrapping_add).
    let args: AddArgs = AddArgs { a: u32::MAX, b: 1 };
    let mut out: u32 = 42_u32;

    let mut result: AbiError = AbiError::ok();
    // SAFETY: args and out are valid and correctly typed; null stateless instance.
    unsafe {
        dispatch_fn(
            (*interface_ptr).adapter_context,
            GuestContractInstance::null(),
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut result,
        )
    };

    assert_eq!(result.code, AbiErrorCode::Ok as u32);
    assert_eq!(out, 0_u32, "u32::MAX + 1 wraps to 0");

    mem::forget(library);
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const HostApi,
    _level: u32,
    _scope: StringView,
    _message: StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _args: *const c_void,
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _instance: GuestContractInstance,
) {
}

unsafe extern "C" fn stub_registry_revision(_this: *const HostApi) -> u64 {
    0
}
