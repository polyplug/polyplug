#![allow(clippy::expect_used)]

//! Integration test: multi-contract registration and lookup.
//!
//! This test crate is the crate root for the `integration_graph` test binary.
//!
//! Tests that:
//! - Multiple registrations in a registry work correctly
//! - contract_id lookup returns correct handles
//! - Stale handles are detected after replacement

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::{
    AbiError, AbiErrorCode, BundleInitContext, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostInterface, PluginDescriptor, StringView, Version,
};
use polyplug_utils::{BundleId, GuestContractId};

/// Path to the compiled test_plugin shared library -- set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// --- Host functions for integration tests ------------------------------------

/// A register_contract callbacks that stores interface entries into a Registry
/// via thread-local state (avoids threading through the opaque host pointer).
///
/// # Safety
/// `this`, `descriptor`, and `interface` must be valid non-null pointers for the call duration.
unsafe extern "C" fn graph_register_callback(
    _this: *const HostInterface,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer as u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call.
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call.
    let iface: &GuestContractInterface = unsafe { &*interface };

    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name -- guaranteed valid UTF-8 by construction.
    let contract_name_str: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
    let result: Result<GuestContractHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow().register_guest_contract(
            *desc,
            interface,
            contract_name_str.to_owned(),
            BundleId::from_u64(iface.contract_id.id()),
        )
    });

    match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok as u32,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(_this: *const HostInterface, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostInterface,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
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

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

/// No-op create_instance for fake interface.
unsafe extern "C" fn fake_create_instance(
    _host: *const HostInterface,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// No-op destroy_instance for fake interface.
unsafe extern "C" fn fake_destroy_instance(
    _host: *const HostInterface,
    _instance: GuestContractInstance,
) {
}

// ─── Stub functions for new HostInterface fields (18-01 placeholders) ────────

unsafe extern "C" fn noop_load_bundle(
    _this: *const HostInterface,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostInterface,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostInterface,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_register_loader(
    _this: *const HostInterface,
    _runtime_name: StringView,
    _loader_ptr: *mut core::ffi::c_void,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Generic as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_get_last_error(
    _this: *const HostInterface,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

unsafe extern "C" fn noop_get_error_len(_this: *const HostInterface) -> usize {
    0
}

unsafe extern "C" fn noop_get_extension(
    _this: *const HostInterface,
    _extension_id: u32,
) -> *const () {
    core::ptr::null()
}

std::thread_local! {
    static GRAPH_REGISTRY: core::cell::RefCell<RuntimeStore> =
        core::cell::RefCell::new(RuntimeStore::new());
}

/// Load the test_plugin and call polyplug_init, storing results in GRAPH_REGISTRY.
/// Returns the loaded Library (caller must `std::mem::forget` it to prevent unload).
fn load_and_init_plugin() -> libloading::Library {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib with correct ABI.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init signature is `extern "C" fn(*const HostInterface, *const BundleInitContext) -> AbiError`.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostInterface, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: graph_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        // Stub fields for new operations (implemented in 18-02)
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        get_extension: noop_get_extension,
    };

    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must succeed"
    );

    library
}

// --- Tests -------------------------------------------------------------------

#[test]
fn test_single_contract_registration_and_lookup() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = RuntimeStore::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Find the test.add contract.
    let handle: GuestContractHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("test.add must be found")
    });

    assert!(!handle.is_null(), "handle must not be null");

    // Resolve the interface.
    let interface_ptr: *const GuestContractInterface = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("handle must resolve to interface")
    });

    // SAFETY: interface_ptr is valid -- library is alive (not yet forgotten).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.contract_id, test_add_id,
        "interface contract_id must match"
    );
    // SAFETY: dispatch.native is valid because dispatch_type is Native
    let function_count: u32 = unsafe { interface.dispatch.native.function_count };
    assert_eq!(function_count, 1, "test.add must have 1 function");

    core::mem::forget(lib);
}

#[test]
fn test_unknown_contract_returns_not_found() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = RuntimeStore::new());

    let lib: libloading::Library = load_and_init_plugin();

    let unknown_id: GuestContractId = GuestContractId::new("unknown.contract", 1);
    let result: Result<GuestContractHandle, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().find(unknown_id, 0));

    assert!(
        result.is_err(),
        "lookup of unregistered contract must return Err"
    );

    core::mem::forget(lib);
}

#[test]
fn test_duplicate_registration_allowed() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = RuntimeStore::new());

    let lib: libloading::Library = load_and_init_plugin();

    // Try to manually register the same contract again -- should succeed (multi-impl).
    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Build a fake interface for the second registration.
    // function_count=0, so the functions pointer is never dereferenced.
    let fake_interface: GuestContractInterface = GuestContractInterface {
        contract_id: test_add_id,
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: polyplug_abi::DispatchType::Native,
        create_instance: fake_create_instance,
        destroy_instance: fake_destroy_instance,
        dispatch: polyplug_abi::DispatchMechanisms {
            native: polyplug_abi::NativeDispatch {
                function_count: 0,
                functions: core::ptr::null(),
            },
        },
    };
    let fake_descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"duplicate_adder"),
        contract_name: StringView::from_static(b"test.add"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };

    // SAFETY: fake_interface is a local static with 'static lifetime.
    let result: Result<GuestContractHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow().register_guest_contract(
            fake_descriptor,
            &fake_interface as *const GuestContractInterface,
            "test.add".to_owned(),
            BundleId::from_u64(test_add_id.id()),
        )
    });

    assert!(
        result.is_ok(),
        "second registration of same contract should succeed (multi-impl allowed)"
    );

    core::mem::forget(lib);
}

#[test]
fn test_invalid_handle_detected() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = RuntimeStore::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);
    let _handle: GuestContractHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("must find test.add")
    });

    // Construct an invalid handle with an out-of-bounds index.
    let invalid: GuestContractHandle = GuestContractHandle { index: 999 };

    let result: Result<*const GuestContractInterface, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().resolve_guest_contract(invalid));

    assert!(result.is_err(), "invalid handle must return Err");

    core::mem::forget(lib);
}

#[test]
fn test_multi_lookup_consistent() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = RuntimeStore::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Repeated lookups must return consistent results.
    let handle_a: GuestContractHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("first find must succeed")
    });
    let handle_b: GuestContractHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("second find must succeed")
    });

    assert_eq!(
        handle_a.index, handle_b.index,
        "repeated lookups must return same slot index"
    );

    core::mem::forget(lib);
}
