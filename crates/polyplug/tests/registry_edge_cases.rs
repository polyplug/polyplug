#![allow(clippy::expect_used)]

//! Edge case tests for Registry.
//!
//! Tests for:
//! - resolve with valid/stale handles
//! - concurrent access thread safety
//! - find_by_contract with multiple implementations
//! - swap_interface during active resolve

use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::RegistryError;
use polyplug::plugin_registry::PluginRegistry;
use polyplug_abi::{
    DispatchType, GuestContractInterface, NativeDispatch, PluginDescriptor, PluginDispatch,
    PluginHandle, StringView,
};

const MOCK_FUNCTIONS: [*const (); 0] = [];

macro_rules! make_interface {
    ($contract_id:expr, $version:expr) => {
        GuestContractInterface {
            rt_ctx: core::ptr::null(),
            contract_id: $contract_id,
            contract_version: $version,
            function_count: 0_u32,
            dispatch_type: DispatchType::Native,
            dispatch: PluginDispatch {
                native: NativeDispatch {
                    functions: MOCK_FUNCTIONS.as_ptr(),
                },
            },
        }
    };
}

// =============================================================================
// Test 1: resolve with valid handle after multiple registrations
// =============================================================================

#[test]
fn resolve_valid_handle_after_multiple_registrations() {
    static VTABLE_A: GuestContractInterface = make_interface!(0xEEEE_0000_0000_0001_u64, 1_u32 << 16);
    static VTABLE_B: GuestContractInterface = make_interface!(0xEEEE_0000_0000_0002_u64, 2_u32 << 16);
    static VTABLE_C: GuestContractInterface = make_interface!(0xEEEE_0000_0000_0003_u64, 3_u32 << 16);

    let registry: PluginRegistry = PluginRegistry::new();

    let descriptor_a: PluginDescriptor = make_descriptor("plugin_a", "contract.a");
    let descriptor_b: PluginDescriptor = make_descriptor("plugin_b", "contract.b");
    let descriptor_c: PluginDescriptor = make_descriptor("plugin_c", "contract.c");

    // SAFETY: VTABLE_A, VTABLE_B, VTABLE_C are 'static, pointers are valid for Registry lifetime.
    let handle_a: PluginHandle = unsafe {
        registry
            .register(descriptor_a, &VTABLE_A, "contract.a".to_owned(), 1_u64)
            .expect("registration A should succeed")
    };

    // SAFETY: VTABLE_B is 'static, pointer is valid for Registry lifetime.
    let handle_b: PluginHandle = unsafe {
        registry
            .register(descriptor_b, &VTABLE_B, "contract.b".to_owned(), 2_u64)
            .expect("registration B should succeed")
    };

    // SAFETY: VTABLE_C is 'static, pointer is valid for Registry lifetime.
    let handle_c: PluginHandle = unsafe {
        registry
            .register(descriptor_c, &VTABLE_C, "contract.c".to_owned(), 3_u64)
            .expect("registration C should succeed")
    };

    let vtable_ptr_a: *const GuestContractInterface =
        registry.resolve(handle_a).expect("resolve for handle_a should succeed");
    // SAFETY: vtable_ptr_a points to VTABLE_A which is 'static.
    let contract_id_a: u64 = unsafe { (*vtable_ptr_a).contract_id };
    assert_eq!(
        contract_id_a, VTABLE_A.contract_id,
        "handle_a should return VTABLE_A"
    );

    let vtable_ptr_b: *const GuestContractInterface =
        registry.resolve(handle_b).expect("resolve for handle_b should succeed");
    // SAFETY: vtable_ptr_b points to VTABLE_B which is 'static.
    let contract_id_b: u64 = unsafe { (*vtable_ptr_b).contract_id };
    assert_eq!(
        contract_id_b, VTABLE_B.contract_id,
        "handle_b should return VTABLE_B"
    );

    let vtable_ptr_c: *const GuestContractInterface =
        registry.resolve(handle_c).expect("resolve for handle_c should succeed");
    // SAFETY: vtable_ptr_c points to VTABLE_C which is 'static.
    let contract_id_c: u64 = unsafe { (*vtable_ptr_c).contract_id };
    assert_eq!(
        contract_id_c, VTABLE_C.contract_id,
        "handle_c should return VTABLE_C"
    );
}

// =============================================================================
// Test 2: resolve with handle pointing to vacant slot
// =============================================================================

#[test]
fn resolve_vacant_slot_returns_stale_handle() {
    static VTABLE: GuestContractInterface = make_interface!(0xEEEE_0000_0000_0010_u64, 1_u32 << 16);

    let registry: PluginRegistry = PluginRegistry::new();

    let descriptor: PluginDescriptor = make_descriptor("plugin_single", "contract.single");
    // SAFETY: VTABLE is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(descriptor, &VTABLE, "contract.single".to_owned(), 100_u64)
            .expect("registration should succeed")
    };

    // Test 1: Handle with index beyond slots length
    let out_of_bounds_handle: PluginHandle = PluginHandle {
        index: 9999_u32,
        generation: 0_u32,
    };
    let result: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve(out_of_bounds_handle);
    assert!(
        matches!(result, Err(RegistryError::StaleHandle { .. })),
        "out of bounds handle should return StaleHandle error"
    );

    // Test 2: Handle with wrong generation (simulates vacant/reused slot)
    let stale_handle: PluginHandle = PluginHandle {
        index: handle.index,
        generation: handle.generation.wrapping_add(1_u32),
    };
    let result_stale: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve(stale_handle);
    assert!(
        matches!(result_stale, Err(RegistryError::StaleHandle { .. })),
        "wrong generation handle should return StaleHandle error"
    );

    // Test 3: Handle pointing to slot that was never used (index 1 when only slot 0 exists)
    let unused_slot_handle: PluginHandle = PluginHandle {
        index: 1_u32,
        generation: 0_u32,
    };
    let result_unused: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve(unused_slot_handle);
    assert!(
        matches!(result_unused, Err(RegistryError::StaleHandle { .. })),
        "unused slot handle should return StaleHandle error"
    );
}

// =============================================================================
// Test 3: resolve concurrent access (thread safety)
// =============================================================================

const CONCURRENT_THREADS: usize = 8_usize;
const CONCURRENT_ROUNDS: usize = 100_usize;

const CONCURRENT_CONTRACT_IDS: [u64; CONCURRENT_THREADS] = [
    0xEEEE_1000_0000_0001_u64,
    0xEEEE_1000_0000_0002_u64,
    0xEEEE_1000_0000_0003_u64,
    0xEEEE_1000_0000_0004_u64,
    0xEEEE_1000_0000_0005_u64,
    0xEEEE_1000_0000_0006_u64,
    0xEEEE_1000_0000_0007_u64,
    0xEEEE_1000_0000_0008_u64,
];

const CONCURRENT_CONTRACT_NAMES: [&str; CONCURRENT_THREADS] = [
    "concurrent.contract.1",
    "concurrent.contract.2",
    "concurrent.contract.3",
    "concurrent.contract.4",
    "concurrent.contract.5",
    "concurrent.contract.6",
    "concurrent.contract.7",
    "concurrent.contract.8",
];

const CONCURRENT_PLUGIN_NAMES: [&str; CONCURRENT_THREADS] = [
    "concurrent_plugin_0",
    "concurrent_plugin_1",
    "concurrent_plugin_2",
    "concurrent_plugin_3",
    "concurrent_plugin_4",
    "concurrent_plugin_5",
    "concurrent_plugin_6",
    "concurrent_plugin_7",
];

static CONCURRENT_VTABLES: [GuestContractInterface; CONCURRENT_THREADS] = [
    make_interface!(CONCURRENT_CONTRACT_IDS[0], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[1], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[2], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[3], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[4], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[5], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[6], 1_u32 << 16),
    make_interface!(CONCURRENT_CONTRACT_IDS[7], 1_u32 << 16),
];

#[test]
fn resolve_concurrent_access_thread_safety() {
    let registry: Arc<PluginRegistry> = Arc::new(PluginRegistry::new());
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(CONCURRENT_THREADS));
    let mut thread_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(CONCURRENT_THREADS);

    let mut handles: Vec<PluginHandle> = Vec::with_capacity(CONCURRENT_THREADS);
    for idx in 0_usize..CONCURRENT_THREADS {
        let descriptor: PluginDescriptor =
            make_descriptor(CONCURRENT_PLUGIN_NAMES[idx], CONCURRENT_CONTRACT_NAMES[idx]);
        // SAFETY: CONCURRENT_VTABLES[idx] is 'static, pointer is valid for Registry lifetime.
        let handle: PluginHandle = unsafe {
            registry
                .register(
                    descriptor,
                    &CONCURRENT_VTABLES[idx],
                    CONCURRENT_CONTRACT_NAMES[idx].to_owned(),
                    idx as u64,
                )
                .expect("registration should succeed")
        };
        handles.push(handle);
    }

    for idx in 0_usize..CONCURRENT_THREADS {
        let registry_clone: Arc<PluginRegistry> = Arc::clone(&registry);
        let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
        let handle: PluginHandle = handles[idx];
        let expected_contract_id: u64 = CONCURRENT_CONTRACT_IDS[idx];

        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            barrier_clone.wait();

            for _round in 0_usize..CONCURRENT_ROUNDS {
                let vtable_ptr: *const GuestContractInterface = registry_clone
                    .resolve(handle)
                    .expect("resolve should succeed in concurrent context");
                // SAFETY: vtable_ptr points to a 'static GuestContractInterface.
                let contract_id: u64 = unsafe { (*vtable_ptr).contract_id };
                assert_eq!(
                    contract_id, expected_contract_id,
                    "thread {} got wrong contract_id",
                    idx
                );
            }
        });
        thread_handles.push(thread_handle);
    }

    for handle in thread_handles {
        handle.join().expect("thread should not panic");
    }
}

// =============================================================================
// Test 4: find_by_contract with multiple implementations
// =============================================================================

#[test]
fn find_by_contract_multiple_implementations_returns_first() {
    const MULTI_CONTRACT_ID: u64 = 0xEEEE_2000_0000_0001_u64;

    static VTABLE_IMPL_A: GuestContractInterface = make_interface!(MULTI_CONTRACT_ID, 1_u32 << 16);
    static VTABLE_IMPL_B: GuestContractInterface = make_interface!(MULTI_CONTRACT_ID, 2_u32 << 16);
    static VTABLE_IMPL_C: GuestContractInterface = make_interface!(MULTI_CONTRACT_ID, 3_u32 << 16);

    let registry: PluginRegistry = PluginRegistry::new();

    let descriptor_a: PluginDescriptor = make_descriptor("impl_a", "multi.contract");
    let descriptor_b: PluginDescriptor = make_descriptor("impl_b", "multi.contract");
    let descriptor_c: PluginDescriptor = make_descriptor("impl_c", "multi.contract");

    // SAFETY: VTABLE_IMPL_A is 'static, pointer is valid for Registry lifetime.
    let handle_a: PluginHandle = unsafe {
        registry
            .register(
                descriptor_a,
                &VTABLE_IMPL_A,
                "multi.contract".to_owned(),
                1000_u64,
            )
            .expect("registration A should succeed")
    };

    // SAFETY: VTABLE_IMPL_B is 'static, pointer is valid for Registry lifetime.
    let handle_b: PluginHandle = unsafe {
        registry
            .register(
                descriptor_b,
                &VTABLE_IMPL_B,
                "multi.contract".to_owned(),
                2000_u64,
            )
            .expect("registration B should succeed")
    };

    // SAFETY: VTABLE_IMPL_C is 'static, pointer is valid for Registry lifetime.
    let handle_c: PluginHandle = unsafe {
        registry
            .register(
                descriptor_c,
                &VTABLE_IMPL_C,
                "multi.contract".to_owned(),
                3000_u64,
            )
            .expect("registration C should succeed")
    };

    assert_ne!(
        handle_a.index, handle_b.index,
        "each impl should have its own slot"
    );
    assert_ne!(
        handle_b.index, handle_c.index,
        "each impl should have its own slot"
    );
    assert_ne!(
        handle_a.index, handle_c.index,
        "each impl should have its own slot"
    );

    let found: PluginHandle = registry
        .find_by_contract(MULTI_CONTRACT_ID, 0_u32)
        .expect("find_by_contract should find an implementation");

    assert_eq!(
        found.index, handle_a.index,
        "find_by_contract should return first registered implementation"
    );
    assert_eq!(
        found.generation, handle_a.generation,
        "generation should match first implementation"
    );

    let vtable_ptr: *const GuestContractInterface =
        registry.resolve(found).expect("resolve should succeed");
    // SAFETY: vtable_ptr points to VTABLE_IMPL_A which is 'static.
    let version: u32 = unsafe { (*vtable_ptr).contract_version };
    assert_eq!(
        version, VTABLE_IMPL_A.contract_version,
        "should resolve to first implementation's vtable"
    );

    let mut all_handles: [PluginHandle; 4] = [PluginHandle {
        index: 0_u32,
        generation: 0_u32,
    }; 4];
    let count: usize = registry.find_all_by_contract(MULTI_CONTRACT_ID, 0_u32, &mut all_handles);
    assert_eq!(
        count, 3,
        "find_all_by_contract should return all 3 implementations"
    );
}

// =============================================================================
// Test 5: swap_interface during active resolve
// =============================================================================

#[test]
fn swap_interface_during_active_resolve() {
    const SWAP_TEST_CONTRACT_ID: u64 = 0xEEEE_3000_0000_0001_u64;
    const VERSION_V1: u32 = 1_u32 << 16;
    const VERSION_V2: u32 = 2_u32 << 16;

    static VTABLE_V1: GuestContractInterface = make_interface!(SWAP_TEST_CONTRACT_ID, VERSION_V1);
    static VTABLE_V2: GuestContractInterface = make_interface!(SWAP_TEST_CONTRACT_ID, VERSION_V2);

    let registry: PluginRegistry = PluginRegistry::new();

    let descriptor: PluginDescriptor = make_descriptor("swap_test_plugin", "swap.test.contract");
    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle_v1: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_V1,
                "swap.test.contract".to_owned(),
                5000_u64,
            )
            .expect("initial registration should succeed")
    };

    let vtable_ptr_before: *const GuestContractInterface =
        registry.resolve(handle_v1).expect("resolve before swap should succeed");
    // SAFETY: vtable_ptr_before points to VTABLE_V1 which is 'static.
    let version_before: u32 = unsafe { (*vtable_ptr_before).contract_version };
    assert_eq!(
        version_before, VERSION_V1,
        "vtable before swap should have V1"
    );

    // Perform the swap - direct swap_interface takes Arc<GuestContractInterface>
    let new_arc: Arc<GuestContractInterface> = Arc::new(&VTABLE_V2);
    registry
        .swap_interface(handle_v1.index, new_arc)
        .expect("swap_interface should succeed");

    // The old handle should now be stale (generation mismatch).
    let result_after_swap: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve(handle_v1);

    assert!(
        matches!(result_after_swap, Err(RegistryError::StaleHandle { .. })),
        "old handle should be stale after swap_interface bumps generation"
    );

    let new_handle: PluginHandle = registry
        .find_by_contract(SWAP_TEST_CONTRACT_ID, 0_u32)
        .expect("find_by_contract should find the swapped implementation");

    assert_eq!(
        new_handle.generation,
        handle_v1.generation.wrapping_add(1_u32),
        "new handle should have incremented generation"
    );

    let vtable_ptr_after: *const GuestContractInterface =
        registry.resolve(new_handle).expect("resolve with new handle should succeed");
    // SAFETY: vtable_ptr_after points to VTABLE_V2 which is 'static.
    let version_after: u32 = unsafe { (*vtable_ptr_after).contract_version };
    assert_eq!(version_after, VERSION_V2, "new resolve should point to V2");
}

// =============================================================================
// Helper functions
// =============================================================================

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1,
        version_minor: 0,
        version_patch: 0,
    }
}