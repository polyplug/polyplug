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

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInterface, HostApi,
    NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

// ─── Static interfaces for testing ─────────────────────────────────────────────

const MOCK_FNS: [*const (); 0] = [];

/// No-op create_instance callback.
unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> polyplug_abi::GuestContractInstance {
    polyplug_abi::GuestContractInstance::null()
}

/// No-op destroy_instance callback.
unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

static INTERFACE_V1: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

static INTERFACE_V2: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    contract_version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

// ─── Helper functions ────────────────────────────────────────────────────────

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

// ─── Test 1: Direct swap changes the interface ─────────────────────────────────────

/// Verifies that swap_interface directly swaps the interface and the new interface
/// is returned by resolve after the swap.
#[test]
fn test_swap_interface_changes_interface_pointer() {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_test_plugin", "swap.test.contract");

    // SAFETY: INTERFACE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "swap.test.contract".to_owned(),
            BundleId::from_u64(2_u64),
        )
    }
    .expect("registration should succeed");

    // Pin the epoch for the whole resolve→swap→deref sequence. The registry copies
    // each registered interface into an Arc, so a pointer resolved before the swap
    // points into the Arc the swap supersedes; holding this guard keeps that Arc alive
    // (epoch reclamation cannot run while a reader is pinned), so the pre-swap pointer
    // stays valid across the swap and its deref below.
    let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();

    // The handle should be valid before the swap.
    let resolve_result_before: Result<*const GuestContractInterface, _> =
        registry.resolve_guest_contract(handle);
    assert!(
        resolve_result_before.is_ok(),
        "handle should be valid before swap"
    );

    let interface_ptr_before: *const GuestContractInterface =
        resolve_result_before.expect("resolve before swap should succeed");
    // SAFETY: the epoch guard pinned above keeps the pre-swap interface Arc alive across
    // this deref even though the swap below supersedes it.
    let version_before: &Version = unsafe { &(*interface_ptr_before).contract_version };
    assert_eq!(
        version_before.major, 1,
        "before swap: should have version 1"
    );

    // Perform the swap - direct swap_interface
    let new_arc: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
    registry
        .swap_guest_contract_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // The same handle should now resolve to INTERFACE_V2.
    let resolve_result_after: Result<
        *const GuestContractInterface,
        polyplug::error::RegistryError,
    > = registry.resolve_guest_contract(handle);

    // With the new model (no generation), the handle should still be valid after swap
    assert!(
        resolve_result_after.is_ok(),
        "handle should still be valid after swap (no generation tracking)"
    );

    let interface_ptr_after: *const GuestContractInterface =
        resolve_result_after.expect("resolve after swap should succeed");
    // SAFETY: interface_ptr_after points at the live post-swap interface Arc, kept alive
    // by the epoch guard pinned above.
    let version_after: &Version = unsafe { &(*interface_ptr_after).contract_version };
    assert_eq!(version_after.major, 2, "after swap: should have version 2");
}

// ─── Test 2: Direct swap verification ──────────────────────────────────────────

/// Verifies that swap_interface directly swaps the interface under RwLock write guard.
#[test]
fn test_direct_swap_interface() {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "swap.direct.contract");

    // SAFETY: INTERFACE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "swap.direct.contract".to_owned(),
            BundleId::from_u64(3_u64),
        )
    }
    .expect("registration should succeed");

    // Pin the epoch across resolve→swap→deref so the pre-swap interface Arc the
    // registry copied stays alive across the swap that supersedes it.
    let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();

    // Resolve before swap
    let interface_ptr_before: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve should succeed before swap");

    // SAFETY: the epoch guard pinned above keeps the pre-swap interface Arc alive
    // across this deref.
    let version_before: &Version = unsafe { &(*interface_ptr_before).contract_version };
    assert_eq!(version_before.major, 1, "before swap: V1");

    // Perform direct swap
    let new_arc: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
    registry
        .swap_guest_contract_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // Resolve after swap - the handle should still be valid
    let interface_ptr_after: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve should succeed after swap");

    // SAFETY: interface_ptr_after points at the live post-swap interface Arc, kept
    // alive by the epoch guard pinned above.
    let version_after: &Version = unsafe { &(*interface_ptr_after).contract_version };
    assert_eq!(version_after.major, 2, "after swap: V2");
}
