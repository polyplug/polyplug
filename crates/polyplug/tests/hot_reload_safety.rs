#![allow(clippy::expect_used)]

//! Hot-reload safety tests.
//!
//! Verifies the safety guarantees of the hot-reload mechanism:
//! 1. Generation increment makes old handles stale after swap
//! 2. Direct interface swap via RwLock write guard
//! 3. Callback-based model: host destroys instances in Preparing callback
//!
//! Safety contract: Host MUST destroy all instances before interface swap.
//! Runtime emits warning if Arc refs remain after Preparing callback.

use std::sync::Arc;

use polyplug::plugin_registry::PluginRegistry;
use polyplug_abi::{
    DispatchType, GuestContractInterface, NativeDispatch, PluginDescriptor, PluginDispatch,
    PluginHandle, StringView,
};

// ─── Static vtables for testing ──────────────────────────────────────────────

const MOCK_FNS: [*const (); 0] = [];

static VTABLE_V1: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xDEAD_BEEF_0000_0001_u64,
    contract_version: (1_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

static VTABLE_V2: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xDEAD_BEEF_0000_0001_u64,
    contract_version: (2_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
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
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    }
}

// ─── Test 1: Generation increment on swap ─────────────────────────────────────

/// Verifies that the generation counter is incremented on swap and that
/// old handles become stale (return StaleHandle error) after the swap.
#[test]
fn test_generation_increment_on_swap() {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("gen_test_plugin", "gen.test.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle_before: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "gen.test.contract".to_owned(),
            2_u64,
        )
    }
    .expect("registration should succeed");

    let generation_before: u32 = handle_before.generation;

    // The handle should be valid before the swap.
    let resolve_result_before: Result<*const GuestContractInterface, _> =
        registry.resolve(handle_before);
    assert!(
        resolve_result_before.is_ok(),
        "handle should be valid before swap"
    );

    // Perform the swap - direct swap_interface
    let new_arc: Arc<GuestContractInterface> = Arc::new(&VTABLE_V2);
    registry
        .swap_interface(handle_before.index, new_arc)
        .expect("swap_interface should succeed");

    // The old handle should now be stale (generation mismatch).
    let resolve_result_after: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        registry.resolve(handle_before);

    match resolve_result_after {
        Err(polyplug::error::RegistryError::StaleHandle {
            index,
            expected,
            found,
        }) => {
            assert_eq!(index, handle_before.index, "stale handle index should match");
            assert_eq!(
                expected, generation_before,
                "expected generation should be the old generation"
            );
            assert_eq!(
                found,
                generation_before.wrapping_add(1_u32),
                "found generation should be incremented"
            );
        }
        Ok(_) => panic!("expected StaleHandle error, but resolve succeeded"),
        Err(e) => panic!("expected StaleHandle error, got: {:?}", e),
    }

    // Find the plugin again — should get a new handle with the new generation.
    let handle_after: PluginHandle = registry
        .find_by_contract(0xDEAD_BEEF_0000_0001_u64, 0_u32)
        .expect("find_by_contract should succeed after swap");

    assert_eq!(
        handle_after.index, handle_before.index,
        "slot index should be the same"
    );
    assert_eq!(
        handle_after.generation,
        generation_before.wrapping_add(1_u32),
        "generation should be incremented"
    );

    // The new handle should be valid.
    let vtable_ptr_after: *const GuestContractInterface =
        registry.resolve(handle_after).expect("new handle should be valid");

    // SAFETY: vtable_ptr_after points to VTABLE_V2 which is 'static.
    let version: u32 = unsafe { (*vtable_ptr_after).contract_version };
    assert_eq!(
        version,
        (2_u32 << 16),
        "new handle should reference v2 vtable"
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
    let version_before: u32 = unsafe { (*vtable_ptr_before).contract_version };
    assert_eq!(version_before, (1_u32 << 16), "before swap: V1");

    // Perform direct swap
    let new_arc: Arc<GuestContractInterface> = Arc::new(&VTABLE_V2);
    registry
        .swap_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // Old handle is stale due to generation bump
    let result_old: Result<*const GuestContractInterface, polyplug::error::RegistryError> =
        registry.resolve(handle);
    assert!(
        matches!(result_old, Err(polyplug::error::RegistryError::StaleHandle { .. })),
        "old handle should be stale after swap"
    );

    // Find new handle
    let new_handle: PluginHandle = registry
        .find_by_contract(0xDEAD_BEEF_0000_0001_u64, 0_u32)
        .expect("find_by_contract should succeed");

    let vtable_ptr_after: *const GuestContractInterface =
        registry.resolve(new_handle).expect("resolve should succeed for new handle");

    // SAFETY: vtable_ptr_after points to VTABLE_V2 which is 'static.
    let version_after: u32 = unsafe { (*vtable_ptr_after).contract_version };
    assert_eq!(version_after, (2_u32 << 16), "after swap: V2");
}