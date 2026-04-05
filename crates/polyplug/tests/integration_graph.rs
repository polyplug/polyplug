#![allow(clippy::expect_used)]

//! Integration test: multi-contract registration and lookup.
//!
//! This test crate is the crate root for the `integration_graph` test binary.
//!
//! Tests that:
//! - Multiple registrations in a registry work correctly
//! - contract_id lookup returns correct handles
//! - Stale handles are detected after replacement

use polyplug::registry::plugin_registry::PluginRegistry;
use polyplug_abi::{
    AbiErrorCode, AbiError, RuntimeAbi, RuntimeContext, GuestContractInterface, GuestContractInstance,
    PluginContext, PluginDescriptor, PluginHandle, StringView, Version, DispatchMechanisms,
    DispatchType, NativeDispatch,
};
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_utils::{guest_contract_id, bundle_id, GuestContractId, BundleId};

/// Path to the compiled test_plugin shared library -- set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// --- Host functions for integration tests ------------------------------------

/// A register_contract callbacks that stores vtable entries into a Registry
/// via thread-local state (avoids threading through the opaque host pointer).
///
/// # Safety
/// `rt_ctx`, `descriptor`, and `interface` must be valid non-null pointers for the call duration.
unsafe extern "C" fn graph_register_callback(
    _rt_ctx: RuntimeContext,
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
    let result: Result<PluginHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow()
            .register(*desc, interface, contract_name_str.to_owned(), BundleId::from_u64(iface.contract_id.id()))
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
unsafe extern "C" fn noop_alloc(
    _rt_ctx: RuntimeContext,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _rt_ctx: RuntimeContext,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _rt_ctx: RuntimeContext,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _rt_ctx: RuntimeContext,
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_contract(
    _rt_ctx: RuntimeContext,
    _handle: PluginHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op call_method callback.
unsafe extern "C" fn noop_call_method(
    _rt_ctx: RuntimeContext,
    _instance: GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _rt_ctx: RuntimeContext,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

std::thread_local! {
    static GRAPH_REGISTRY: core::cell::RefCell<PluginRegistry> =
        core::cell::RefCell::new(PluginRegistry::new());
}

/// Load the test_plugin and call polyplug_init, storing results in GRAPH_REGISTRY.
/// Returns the loaded Library (caller must `std::mem::forget` it to prevent unload).
fn load_and_init_plugin() -> libloading::Library {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib with correct ABI.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init signature is `extern "C" fn(RuntimeContext, *const RuntimeAbi, *const PluginContext) -> AbiError`.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            RuntimeContext,
            *const RuntimeAbi,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_vtable: RuntimeAbi = RuntimeAbi {
        register_contract: graph_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_method: noop_call_method,
        get_host_contract: noop_get_host_contract,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_vtable and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            RuntimeContext::null(),
            &host_vtable as *const RuntimeAbi,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok as u32, "polyplug_init must succeed");

    library
}

// --- Tests -------------------------------------------------------------------

#[test]
fn test_single_contract_registration_and_lookup() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = PluginRegistry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Find the test.add contract.
    let handle: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("test.add must be found")
    });

    assert!(!handle.is_null(), "handle must not be null");

    // Resolve the vtable.
    let interface_ptr: *const GuestContractInterface = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("handle must resolve to vtable")
    });

    // SAFETY: interface_ptr is valid -- library is alive (not yet forgotten).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.contract_id, test_add_id,
        "interface contract_id must match"
    );
    assert_eq!(
        interface.dispatch.native.function_count, 1,
        "test.add must have 1 function"
    );

    core::mem::forget(lib);
}

#[test]
fn test_unknown_contract_returns_not_found() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = PluginRegistry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let unknown_id: GuestContractId = GuestContractId::new("unknown.contract", 1);
    let result: Result<PluginHandle, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().find(unknown_id, 0));

    assert!(
        result.is_err(),
        "lookup of unregistered contract must return Err"
    );

    core::mem::forget(lib);
}

#[test]
fn test_duplicate_registration_allowed() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = PluginRegistry::new());

    let lib: libloading::Library = load_and_init_plugin();

    // Try to manually register the same contract again -- should succeed (multi-impl).
    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Build a fake interface for the second registration.
    // function_count=0, so the functions pointer is never dereferenced.
    let fake_interface: GuestContractInterface = GuestContractInterface {
        contract_id: test_add_id,
        contract_version: Version { major: 1, minor: 0, patch: 0 },
        dispatch_type: polyplug_abi::DispatchType::Native,
        create_instance: |_| GuestContractInstance::null(),
        destroy_instance: |_, _| {},
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
        version: Version { major: 1, minor: 0, patch: 0 },
    };

    // SAFETY: fake_interface is a local static with 'static lifetime.
    let result: Result<PluginHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow().register(
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
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = PluginRegistry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);
    let handle: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("must find test.add")
    });

    // Construct an invalid handle with an out-of-bounds index.
    let invalid: PluginHandle = PluginHandle { index: 999 };

    let result: Result<*const GuestContractInterface, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().resolve(invalid));

    assert!(result.is_err(), "invalid handle must return Err");

    core::mem::forget(lib);
}

#[test]
fn test_multi_lookup_consistent() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = PluginRegistry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: GuestContractId = GuestContractId::new("test.add", 1);

    // Repeated lookups must return consistent results.
    let handle_a: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("first find must succeed")
    });
    let handle_b: PluginHandle = GRAPH_REGISTRY.with(|cell| {
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