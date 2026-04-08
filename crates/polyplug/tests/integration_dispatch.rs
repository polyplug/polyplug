#![allow(clippy::expect_used)]

//! Integration test: call through vtable, verify function executes and returns ABI_OK.
//!
//! This test crate is the crate root for the `integration_dispatch` test binary.

use polyplug::registry::plugin_registry::PluginRegistry;
use polyplug_abi::{
    AbiErrorCode, AbiError, HostInterface, GuestContractInterface, GuestContractInstance,
    PluginContext, PluginDescriptor, PluginHandle, StringView,
};
use polyplug_utils::{GuestContractId, BundleId};

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Host functions that store vtable into a Registry ─────────────────────────

/// A register_contract callback that stores vtable entries into the thread-local
/// Registry for dispatch testing.
///
/// # Safety
/// `this`, `descriptor`, and `interface` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _this: *const HostInterface,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    // Register with thread-local Registry.
    // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<PluginHandle, _> = DISPATCH_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static — extracted from a loaded library that outlives registry.
        unsafe { registry.register(*desc, interface, contract_name.to_owned(), BundleId::from_u64(iface.contract_id.id())) }
    });

    match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic,
            message: StringView::null(),
        },
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _this: *const HostInterface,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<PluginHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_contract(
    _this: *const HostInterface,
    _handle: PluginHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostInterface,
    _instance: GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    }
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

std::thread_local! {
    static DISPATCH_REGISTRY: core::cell::RefCell<PluginRegistry> =
        core::cell::RefCell::new(PluginRegistry::new());
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
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve init function (2-arg signature).
    // SAFETY: polyplug_init matches the expected ABI.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset the thread-local registry before the test.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok, "polyplug_init must succeed");

    // Look up the test.add plugin.
    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });

    // Resolve the vtable.
    let interface_ptr: *const GuestContractInterface =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    assert_eq!(
        interface.dispatch.native.function_count, 1,
        "test.add interface must have 1 function"
    );

    // Call function_id 0 (the `add` function).
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the vtable. args and out are correctly typed.
    // The function has signature: extern "C" fn(*const (), *mut ()) -> AbiError
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (AddArgs matches what test_plugin expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid AddArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(call_result.code, AbiErrorCode::Ok, "add function must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");

    // Leak the library.
    core::mem::forget(library);
}

#[test]
fn test_dispatch_add_with_zero() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init matches the expected ABI (2-arg signature).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Reset registry.
    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok);

    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let interface_ptr: *const GuestContractInterface =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: interface_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*interface_ptr).dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is the add function with compatible signature.
        unsafe { core::mem::transmute(fn_ptr) };

    let args: AddArgs = AddArgs { a: 0, b: 0 };
    let mut out: u32 = 99_u32;

    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(result.code, AbiErrorCode::Ok);
    assert_eq!(out, 0_u32, "add(0, 0) must equal 0");

    core::mem::forget(library);
}

#[test]
fn test_dispatch_add_wrapping_overflow() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init matches the expected ABI (2-arg signature).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    DISPATCH_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok);

    let contract_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: PluginHandle = DISPATCH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered")
    });
    let interface_ptr: *const GuestContractInterface =
        DISPATCH_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"));

    // SAFETY: interface_ptr is valid.
    let fn_ptr: *const () = unsafe { *(*interface_ptr).dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is the add function with compatible signature.
        unsafe { core::mem::transmute(fn_ptr) };

    // u32::MAX + 1 wraps to 0 (wrapping_add).
    let args: AddArgs = AddArgs { a: u32::MAX, b: 1 };
    let mut out: u32 = 42_u32;

    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(result.code, AbiErrorCode::Ok);
    assert_eq!(out, 0_u32, "u32::MAX + 1 wraps to 0");

    core::mem::forget(library);
}