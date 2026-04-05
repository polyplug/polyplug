#![allow(clippy::expect_used)]

//! Hot-reload safety tests.
//!
//! Verifies the safety guarantees of the hot-reload mechanism:
//! 1. Direct interface swap via RwLock write guard
//! 2. Callback-based model: host destroys instances in Preparing callback
//!
//! Safety contract: Host MUST destroy all instances before interface swap.
//! Runtime emits warning if Arc refs remain after Preparing callback.

use std::sync::Arc;

use polyplug::registry::plugin_registry::PluginRegistry;
use polyplug_abi::{
    DispatchType, GuestContractInterface, NativeDispatch, PluginDescriptor,
    PluginHandle, StringView, Version, DispatchMechanisms,
};

// ─── Static vtables for testing ──────────────────────────────────────────────

const MOCK_FNS: [*const (); 0] = [];

/// No-op create_instance callback.
unsafe extern "C" fn noop_create_instance(
    _rt_ctx: *mut core::ffi::c_void,
    _args: *const (),
) -> polyplug_abi::GuestContractInstance {
    polyplug_abi::GuestContractInstance::null()
}

/// No-op destroy_instance callback.
unsafe extern "C" fn noop_destroy_instance(
    _rt_ctx: *mut core::ffi::c_void,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

static VTABLE_V1: GuestContractInterface = GuestContractInterface {
    contract_id: 0xDEAD_BEEF_0000_0001_u64.into(),
    contract_version: Version { major: 1, minor: 0, patch: 0 },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

static VTABLE_V2: GuestContractInterface = GuestContractInterface {
    contract_id: 0xDEAD_BEEF_0000_0001_u64.into(),
    contract_version: Version { major: 2, minor: 0, patch: 0 },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

// ─── Helper functions ────────────────────────────────────────────────────────

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version { major: 1, minor: 0, patch: 0 },
    }
}

// ─── Test 1: Direct swap changes the interface ─────────────────────────────────────

/// Verifies that swap_interface directly swaps the interface and the new interface
/// is returned by resolve after the swap.
#[test]
fn test_swap_interface_changes_vtable() {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_test_plugin", "swap.test.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "swap.test.contract".to_owned(),
            2_u64,
        )
    }
    .expect("registration should succeed");

    // The handle should be valid before the swap.
    let resolve_result_before: Result<*const GuestContractInterface, _> =
        registry.resolve(handle);
    assert!(
        resolve_result_before.is_ok(),
        "handle should be valid before swap"
    );

    // SAFETY: vtable pointer is valid.
    let vtable_ptr_before: *const GuestContractInterface =
        resolve_result_before.expect("resolve before swap should succeed");
    let version_before: &Version = unsafe { &(*vtable_ptr_before).contract_version };
    assert_eq!(
        version_before.major, 1,
        "before swap: should have version 1"
    );

    // Perform the swap - direct swap_interface
    let new_arc: Arc<GuestContractInterface> = Arc::new(&VTABLE_V2);
    registry
        .swap_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // The same handle should now resolve to VTABLE_V2.
    let resolve_result_after: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        registry.resolve(handle);

    // With the new model (no generation), the handle should still be valid after swap
    assert!(
        resolve_result_after.is_ok(),
        "handle should still be valid after swap (no generation tracking)"
    );

    // SAFETY: vtable pointer is valid.
    let vtable_ptr_after: *const GuestContractInterface =
        resolve_result_after.expect("resolve after swap should succeed");
    let version_after: &Version = unsafe { &(*vtable_ptr_after).contract_version };
    assert_eq!(
        version_after.major, 2,
        "after swap: should have version 2"
    );
}

// ─── Test 2: Direct swap verification ──────────────────────────────────────────

/// Verifies that swap_interface directly swaps the interface under RwLock write guard.
#[test]
fn test_direct_swap_interface() {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "swap.direct.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "swap.direct.contract".to_owned(),
            3_u64,
        )
    }
    .expect("registration should succeed");

    // Resolve before swap
    let vtable_ptr_before: *const GuestContractInterface =
        registry.resolve(handle).expect("resolve should succeed before swap");

    // SAFETY: vtable_ptr_before points to VTABLE_V1 which is 'static.
    let version_before: &Version = unsafe { &(*vtable_ptr_before).contract_version };
    assert_eq!(version_before.major, 1, "before swap: V1");

    // Perform direct swap
    let new_arc: Arc<GuestContractInterface> = Arc::new(&VTABLE_V2);
    registry
        .swap_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // Resolve after swap - the handle should still be valid
    let vtable_ptr_after: *const GuestContractInterface =
        registry.resolve(handle).expect("resolve should succeed after swap");

    // SAFETY: vtable_ptr_after points to VTABLE_V2 which is 'static.
    let version_after: &Version = unsafe { &(*vtable_ptr_after).contract_version };
    assert_eq!(version_after.major, 2, "after swap: V2");
}