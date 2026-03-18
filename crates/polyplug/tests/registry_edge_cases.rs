//! Edge case tests for Registry.
//!
//! Tests for:
//! - resolve_guard with valid/stale handles
//! - concurrent access thread safety
//! - find_by_contract with multiple implementations
//! - swap_vtable during active resolve_guard

use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::RegistryError;
use polyplug::registry::PluginVTableGuard;
use polyplug::registry::Registry;
use polyplug::registry::VTableSlot;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;

const MOCK_FUNCTIONS: [*const (); 0] = [];

// =============================================================================
// Test 1: resolve_guard with valid handle after multiple registrations
// =============================================================================

/// Verifies that resolve_guard returns the correct vtable after multiple
/// registrations have occurred in the registry.
#[test]
fn resolve_guard_valid_handle_after_multiple_registrations() {
    static VTABLE_A: PluginVTable = PluginVTable {
        contract_id: 0xEEEE_0000_0000_0001_u64,
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    static VTABLE_B: PluginVTable = PluginVTable {
        contract_id: 0xEEEE_0000_0000_0002_u64,
        contract_version: 2_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    static VTABLE_C: PluginVTable = PluginVTable {
        contract_id: 0xEEEE_0000_0000_0003_u64,
        contract_version: 3_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    let registry: Registry = Registry::new();

    // Register multiple plugins
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

    // Resolve each handle and verify correct vtable is returned
    let guard_a: PluginVTableGuard = registry
        .resolve_guard(handle_a)
        .expect("resolve_guard for handle_a should succeed");
    let vtable_ptr_a: *const PluginVTable = guard_a.vtable();
    // SAFETY: vtable_ptr_a points to VTABLE_A which is 'static.
    let contract_id_a: u64 = unsafe { (*vtable_ptr_a).contract_id };
    assert_eq!(
        contract_id_a, VTABLE_A.contract_id,
        "handle_a should return VTABLE_A"
    );

    let guard_b: PluginVTableGuard = registry
        .resolve_guard(handle_b)
        .expect("resolve_guard for handle_b should succeed");
    let vtable_ptr_b: *const PluginVTable = guard_b.vtable();
    // SAFETY: vtable_ptr_b points to VTABLE_B which is 'static.
    let contract_id_b: u64 = unsafe { (*vtable_ptr_b).contract_id };
    assert_eq!(
        contract_id_b, VTABLE_B.contract_id,
        "handle_b should return VTABLE_B"
    );

    let guard_c: PluginVTableGuard = registry
        .resolve_guard(handle_c)
        .expect("resolve_guard for handle_c should succeed");
    let vtable_ptr_c: *const PluginVTable = guard_c.vtable();
    // SAFETY: vtable_ptr_c points to VTABLE_C which is 'static.
    let contract_id_c: u64 = unsafe { (*vtable_ptr_c).contract_id };
    assert_eq!(
        contract_id_c, VTABLE_C.contract_id,
        "handle_c should return VTABLE_C"
    );
}

// =============================================================================
// Test 2: resolve_guard with handle pointing to vacant slot
// =============================================================================

/// Verifies that resolve_guard returns StaleHandle error when the handle
/// points to a slot that has been vacated (e.g., after unload).
///
/// Note: The current Registry implementation does not have an explicit unload
/// method that vacates slots. We simulate a vacant slot by using a handle
/// with an index that was never populated (out of bounds or wrong generation).
#[test]
fn resolve_guard_vacant_slot_returns_stale_handle() {
    static VTABLE: PluginVTable = PluginVTable {
        contract_id: 0xEEEE_0000_0000_0010_u64,
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    let registry: Registry = Registry::new();

    // Register one plugin to populate slot 0
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
    let result: Result<PluginVTableGuard, RegistryError> =
        registry.resolve_guard(out_of_bounds_handle);
    assert!(
        matches!(result, Err(RegistryError::StaleHandle { .. })),
        "out of bounds handle should return StaleHandle error"
    );

    // Test 2: Handle with wrong generation (simulates vacant/reused slot)
    let stale_handle: PluginHandle = PluginHandle {
        index: handle.index,
        generation: handle.generation.wrapping_add(1_u32),
    };
    let result_stale: Result<PluginVTableGuard, RegistryError> =
        registry.resolve_guard(stale_handle);
    assert!(
        matches!(result_stale, Err(RegistryError::StaleHandle { .. })),
        "wrong generation handle should return StaleHandle error"
    );

    // Test 3: Handle pointing to slot that was never used (index 1 when only slot 0 exists)
    let unused_slot_handle: PluginHandle = PluginHandle {
        index: 1_u32,
        generation: 0_u32,
    };
    let result_unused: Result<PluginVTableGuard, RegistryError> =
        registry.resolve_guard(unused_slot_handle);
    assert!(
        matches!(result_unused, Err(RegistryError::StaleHandle { .. })),
        "unused slot handle should return StaleHandle error"
    );
}

// =============================================================================
// Test 3: resolve_guard concurrent access (thread safety)
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

static CONCURRENT_VTABLES: [PluginVTable; CONCURRENT_THREADS] = [
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[0],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[1],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[2],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[3],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[4],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[5],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[6],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONCURRENT_CONTRACT_IDS[7],
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
];

/// Verifies thread safety of resolve_guard under concurrent access.
/// Multiple threads resolve handles simultaneously without data races.
#[test]
fn resolve_guard_concurrent_access_thread_safety() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(CONCURRENT_THREADS));
    let mut thread_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(CONCURRENT_THREADS);

    // First, register all plugins (sequentially to avoid registration races)
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

    // Now spawn threads that concurrently resolve their assigned handle
    for idx in 0_usize..CONCURRENT_THREADS {
        let registry_clone: Arc<Registry> = Arc::clone(&registry);
        let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
        let handle: PluginHandle = handles[idx];
        let expected_contract_id: u64 = CONCURRENT_CONTRACT_IDS[idx];

        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            barrier_clone.wait();

            for _round in 0_usize..CONCURRENT_ROUNDS {
                let guard: PluginVTableGuard = registry_clone
                    .resolve_guard(handle)
                    .expect("resolve_guard should succeed in concurrent context");
                let vtable_ptr: *const PluginVTable = guard.vtable();
                // SAFETY: vtable_ptr points to a 'static PluginVTable.
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

    // Wait for all threads to complete
    for handle in thread_handles {
        handle.join().expect("thread should not panic");
    }
}

// =============================================================================
// Test 4: find_by_contract with multiple implementations
// =============================================================================

/// Verifies that find_by_contract returns the first matching implementation
/// when multiple bundles register the same contract.
#[test]
fn find_by_contract_multiple_implementations_returns_first() {
    const MULTI_CONTRACT_ID: u64 = 0xEEEE_2000_0000_0001_u64;

    static VTABLE_IMPL_A: PluginVTable = PluginVTable {
        contract_id: MULTI_CONTRACT_ID,
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    static VTABLE_IMPL_B: PluginVTable = PluginVTable {
        contract_id: MULTI_CONTRACT_ID,
        contract_version: 2_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    static VTABLE_IMPL_C: PluginVTable = PluginVTable {
        contract_id: MULTI_CONTRACT_ID,
        contract_version: 3_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    let registry: Registry = Registry::new();

    // Register three different implementations of the same contract
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

    // Verify all three are registered at different slots
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

    // find_by_contract should return the first registered implementation
    let found: PluginHandle = registry
        .find_by_contract(MULTI_CONTRACT_ID, 0_u32)
        .expect("find_by_contract should find an implementation");

    // The first implementation (handle_a) should be returned
    assert_eq!(
        found.index, handle_a.index,
        "find_by_contract should return first registered implementation"
    );
    assert_eq!(
        found.generation, handle_a.generation,
        "generation should match first implementation"
    );

    // Verify the returned handle resolves to the first vtable
    let guard: PluginVTableGuard = registry
        .resolve_guard(found)
        .expect("resolve_guard should succeed");
    let vtable_ptr: *const PluginVTable = guard.vtable();
    // SAFETY: vtable_ptr points to VTABLE_IMPL_A which is 'static.
    let version: u32 = unsafe { (*vtable_ptr).contract_version };
    assert_eq!(
        version, VTABLE_IMPL_A.contract_version,
        "should resolve to first implementation's vtable"
    );

    // Verify find_all_by_contract returns all three
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
// Test 5: swap_vtable during active resolve_guard
// =============================================================================

/// Verifies that:
/// 1. An existing PluginVTableGuard remains valid after swap_vtable
/// 2. New resolve_guard calls return the new vtable after swap
#[test]
fn swap_vtable_during_active_resolve_guard() {
    const SWAP_TEST_CONTRACT_ID: u64 = 0xEEEE_3000_0000_0001_u64;
    const VERSION_V1: u32 = 1_u32 << 16;
    const VERSION_V2: u32 = 2_u32 << 16;

    static VTABLE_V1: PluginVTable = PluginVTable {
        contract_id: SWAP_TEST_CONTRACT_ID,
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    static VTABLE_V2: PluginVTable = PluginVTable {
        contract_id: SWAP_TEST_CONTRACT_ID,
        contract_version: VERSION_V2,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    let registry: Registry = Registry::new();

    // Register with V1
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

    // Resolve the guard BEFORE swap - this guard should remain valid
    let guard_before_swap: PluginVTableGuard = registry
        .resolve_guard(handle_v1)
        .expect("resolve_guard before swap should succeed");
    let vtable_ptr_before: *const PluginVTable = guard_before_swap.vtable();
    // SAFETY: vtable_ptr_before points to VTABLE_V1 which is 'static.
    let version_before: u32 = unsafe { (*vtable_ptr_before).contract_version };
    assert_eq!(
        version_before, VERSION_V1,
        "guard before swap should have V1"
    );

    // Perform the swap
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle_v1.index, new_arc)
        .expect("swap_vtable should succeed");

    // Verify old_arc points to V1
    // SAFETY: old_arc.0 points to VTABLE_V1 which is 'static.
    let old_version: u32 = unsafe { (*old_arc.0).contract_version };
    assert_eq!(old_version, VERSION_V1, "old_arc should point to V1");

    // The guard obtained BEFORE swap should STILL be valid and point to V1
    // (the Arc keeps the old vtable alive)
    // SAFETY: vtable_ptr_before still points to VTABLE_V1 (guard holds Arc to it).
    let version_after_swap_from_old_guard: u32 = unsafe { (*vtable_ptr_before).contract_version };
    assert_eq!(
        version_after_swap_from_old_guard, VERSION_V1,
        "old guard should still point to V1 after swap"
    );

    // New resolve_guard calls should fail with StaleHandle because generation was bumped
    let result_after_swap: Result<PluginVTableGuard, RegistryError> =
        registry.resolve_guard(handle_v1);
    assert!(
        matches!(result_after_swap, Err(RegistryError::StaleHandle { .. })),
        "old handle should be stale after swap_vtable bumps generation"
    );

    // Find the new handle via find_by_contract
    let new_handle: PluginHandle = registry
        .find_by_contract(SWAP_TEST_CONTRACT_ID, 0_u32)
        .expect("find_by_contract should find the swapped implementation");

    // New handle should have bumped generation
    assert_eq!(
        new_handle.generation,
        handle_v1.generation.wrapping_add(1_u32),
        "new handle should have incremented generation"
    );

    // Resolve the new handle - should get V2
    let guard_after_swap: PluginVTableGuard = registry
        .resolve_guard(new_handle)
        .expect("resolve_guard with new handle should succeed");
    let vtable_ptr_after: *const PluginVTable = guard_after_swap.vtable();
    // SAFETY: vtable_ptr_after points to VTABLE_V2 which is 'static.
    let version_after: u32 = unsafe { (*vtable_ptr_after).contract_version };
    assert_eq!(version_after, VERSION_V2, "new guard should point to V2");
}

// =============================================================================
// Helper functions
// =============================================================================

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    }
}
