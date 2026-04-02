#![allow(clippy::expect_used)]

//! Hot-reload safety tests.
//!
//! Verifies the safety guarantees of the hot-reload mechanism:
//! 1. In-flight calls complete with the old vtable during swap
//! 2. Generation increment makes old handles stale after swap
//! 3. Old Arc is kept alive until all guards are dropped (quiescence)

use core::time::Duration;
use std::sync::Arc;

use polyplug::plugin_registry::{PluginGuard, PluginRegistry, VTableSlot};
use polyplug_abi::{
    DispatchType, NativeDispatch, PluginDescriptor, PluginDispatch, PluginHandle, PluginInterface,
    StringView,
};

// ─── Static vtables for testing ──────────────────────────────────────────────

const MOCK_FNS: [*const (); 0] = [];

static VTABLE_V1: PluginInterface = PluginInterface {
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

static VTABLE_V2: PluginInterface = PluginInterface {
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

// ─── Test 1: VTable swap while plugin call in progress ───────────────────────

/// Verifies that an in-flight call completes with the OLD vtable even after
/// a swap occurs. The guard holds an Arc<VTableSlot> that keeps the old vtable
/// alive until the guard is dropped.
#[test]
fn test_vtable_swap_while_call_in_progress() {
    let registry: PluginRegistry = PluginRegistry::new();
    let descriptor: PluginDescriptor = make_descriptor("hot_reload_plugin", "hot.reload.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "hot.reload.contract".to_owned(),
            1_u64,
        )
    }
    .expect("registration should succeed");

    // Resolve a guard BEFORE the swap — this simulates an in-flight call.
    let guard: PluginGuard = registry
        .resolve_guard(handle)
        .expect("resolve_guard should succeed for valid handle");

    // Get the vtable pointer from the guard — this is what an in-flight call would use.
    let vtable_ptr_before: *const PluginInterface = guard.vtable();

    // Now swap the vtable while the guard is still held.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable should succeed");

    // The guard's vtable pointer should still point to VTABLE_V1 (the old vtable).
    // This proves the in-flight call would complete with the old vtable.
    assert_eq!(
        vtable_ptr_before,
        guard.vtable(),
        "guard should still reference the old vtable after swap"
    );

    // SAFETY: vtable_ptr_before points to VTABLE_V1 which is 'static.
    let contract_version_from_guard: u32 = unsafe { (*guard.vtable()).contract_version };
    assert_eq!(
        contract_version_from_guard,
        (1_u32 << 16),
        "guard should return v1 contract_version (1.0)"
    );

    // The old_arc should have strong_count == 2 (one from old_arc, one from guard).
    assert_eq!(
        Arc::strong_count(&old_arc),
        2_usize,
        "old_arc should have 2 strong references (old_arc + guard)"
    );

    // Drop the guard — simulates the in-flight call completing.
    drop(guard);

    // Now the old_arc should have strong_count == 1 (only old_arc itself).
    assert_eq!(
        Arc::strong_count(&old_arc),
        1_usize,
        "old_arc should have 1 strong reference after guard dropped"
    );
}

// ─── Test 2: Generation increment on swap ─────────────────────────────────────

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
    let resolve_result_before: Result<PluginGuard, _> = registry.resolve_guard(handle_before);
    assert!(
        resolve_result_before.is_ok(),
        "handle should be valid before swap"
    );

    // Perform the swap.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let _old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle_before.index, new_arc)
        .expect("swap_vtable should succeed");

    // The old handle should now be stale (generation mismatch).
    let resolve_result_after: Result<PluginGuard, polyplug::error::RegistryError> =
        registry.resolve_guard(handle_before);

    match resolve_result_after {
        Err(polyplug::error::RegistryError::StaleHandle {
            index,
            expected,
            found,
        }) => {
            assert_eq!(
                index, handle_before.index,
                "stale handle index should match"
            );
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
    let resolve_new: PluginGuard = registry
        .resolve_guard(handle_after)
        .expect("new handle should be valid");

    // SAFETY: resolve_new.vtable() points to VTABLE_V2 which is 'static.
    let version: u32 = unsafe { (*resolve_new.vtable()).contract_version };
    assert_eq!(
        version,
        (2_u32 << 16),
        "new handle should reference v2 vtable"
    );
}

// ─── Test 3: Arc reference count during quiescence ────────────────────────────

/// Verifies that the old Arc<VTableSlot> is kept alive until all guards are dropped.
/// This is the core quiescence mechanism: the caller holds the old Arc returned
/// by swap_vtable, and guards hold additional Arc references. The old vtable
/// is only truly released when strong_count drops to 1 (only the caller's Arc).
#[test]
fn test_arc_reference_count_during_quiescence() {
    let registry: Arc<PluginRegistry> = Arc::new(PluginRegistry::new());
    let descriptor: PluginDescriptor = make_descriptor("quiescence_plugin", "quiescence.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "quiescence.contract".to_owned(),
            3_u64,
        )
    }
    .expect("registration should succeed");

    // Spawn a background thread that holds a guard for a duration.
    // This simulates an in-flight call that takes time to complete.
    let registry_clone: Arc<PluginRegistry> = Arc::clone(&registry);
    let handle_for_thread: PluginHandle = PluginHandle {
        index: handle.index,
        generation: handle.generation,
    };

    let guard_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        // Resolve the guard on this thread (PluginGuard is !Send).
        let guard: PluginGuard = registry_clone
            .resolve_guard(handle_for_thread)
            .expect("resolve_guard should succeed in thread");

        // Hold the guard while sleeping to simulate an in-flight call.
        std::thread::sleep(Duration::from_millis(200_u64));

        drop(guard);
    });

    // Give the thread time to acquire the guard.
    std::thread::sleep(Duration::from_millis(50_u64));

    // Swap the vtable while the background thread holds a guard.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable should succeed");

    // At this point:
    // - old_arc has 1 reference (our variable)
    // - The background thread's guard has 1 reference
    // - Total strong_count should be 2
    assert_eq!(
        Arc::strong_count(&old_arc),
        2_usize,
        "old_arc should have 2 strong references (caller + background guard)"
    );

    // Wait for the background thread to finish (it will drop its guard).
    guard_thread.join().expect("guard thread should not panic");

    // Now the old_arc should have strong_count == 1 (only our reference).
    assert_eq!(
        Arc::strong_count(&old_arc),
        1_usize,
        "old_arc should have 1 strong reference after all guards dropped"
    );

    // The caller can now safely drop the old_arc, releasing the old vtable.
    drop(old_arc);

    // Verify the new vtable is active.
    let new_handle: PluginHandle = registry
        .find_by_contract(0xDEAD_BEEF_0000_0001_u64, 0_u32)
        .expect("find_by_contract should succeed");

    let guard: PluginGuard = registry
        .resolve_guard(new_handle)
        .expect("resolve_guard should succeed for new handle");

    // SAFETY: guard.vtable() points to VTABLE_V2 which is 'static.
    let version: u32 = unsafe { (*guard.vtable()).contract_version };
    assert_eq!(
        version,
        (2_u32 << 16),
        "active vtable should be v2 after swap"
    );
}

// ─── Additional test: Multiple guards during quiescence ───────────────────────

/// Verifies that multiple concurrent guards all keep the old Arc alive,
/// and the Arc is only released when ALL guards are dropped.
#[test]
fn test_multiple_guards_keep_arc_alive() {
    let registry: Arc<PluginRegistry> = Arc::new(PluginRegistry::new());
    let descriptor: PluginDescriptor =
        make_descriptor("multi_guard_plugin", "multi.guard.contract");

    // SAFETY: VTABLE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry.register(
            descriptor,
            &VTABLE_V1,
            "multi.guard.contract".to_owned(),
            4_u64,
        )
    }
    .expect("registration should succeed");

    const NUM_GUARDS: usize = 4_usize;
    let mut guard_threads: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(NUM_GUARDS);

    // Spawn multiple threads, each holding a guard.
    for _ in 0_usize..NUM_GUARDS {
        let registry_clone: Arc<PluginRegistry> = Arc::clone(&registry);
        let handle_for_thread: PluginHandle = PluginHandle {
            index: handle.index,
            generation: handle.generation,
        };

        let guard_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            // Resolve the guard on this thread (PluginGuard is !Send).
            let guard: PluginGuard = registry_clone
                .resolve_guard(handle_for_thread)
                .expect("resolve_guard should succeed in thread");

            // Hold the guard while sleeping to simulate an in-flight call.
            std::thread::sleep(Duration::from_millis(200_u64));

            drop(guard);
        });

        guard_threads.push(guard_thread);
    }

    // Give threads time to acquire their guards.
    std::thread::sleep(Duration::from_millis(30_u64));

    // Swap the vtable.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable should succeed");

    // old_arc should have strong_count = 1 (caller) + NUM_GUARDS (threads).
    assert_eq!(
        Arc::strong_count(&old_arc),
        1_usize + NUM_GUARDS,
        "old_arc should have {} strong references (caller + {} guards)",
        1_usize + NUM_GUARDS,
        NUM_GUARDS
    );

    // Wait for all threads to finish.
    for thread_handle in guard_threads {
        thread_handle.join().expect("guard thread should not panic");
    }

    // Now old_arc should have strong_count == 1.
    assert_eq!(
        Arc::strong_count(&old_arc),
        1_usize,
        "old_arc should have 1 strong reference after all guards dropped"
    );
}
