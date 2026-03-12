//! Stress tests for the quiescence loop in reload.rs:212-228.
//!
//! The quiescence loop waits until every `Arc::strong_count(old_arc) == 1`
//! (meaning no in-flight caller holds a `PluginVTableGuard` backed by the old
//! vtable), or times out with `PolyplugError::QuiescenceTimeout`.
//!
//! These tests exercise that logic directly using `Registry::swap_vtable` and
//! `Registry::resolve_guard`, without requiring a full plugin bundle on disk.

#![allow(clippy::expect_used)]

use core::hint::spin_loop;
use core::time::Duration;
use std::sync::Arc;
use std::time::Instant;

use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::registry::PluginVTableGuard;
use polyplug::registry::Registry;
use polyplug::registry::VTableSlot;

// --- Constants matching reload.rs --------------------------------------------

/// Same timeout used by reload_bundle_impl.
const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5_u64);

// --- Static vtable fixtures --------------------------------------------------

const MOCK_FUNCTIONS: [*const (); 0] = [];

// Vtables for Test 1 (no-contention).
static VTABLE_V1: PluginVTable = PluginVTable {
    contract_id: 0xAA11_0000_0000_0001_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_V2: PluginVTable = PluginVTable {
    contract_id: 0xAA11_0000_0000_0001_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

// Vtables for Test 2 (guard released).
static VTABLE_V3: PluginVTable = PluginVTable {
    contract_id: 0xAA11_0000_0000_0002_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_V4: PluginVTable = PluginVTable {
    contract_id: 0xAA11_0000_0000_0002_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

// Vtables for Test 4 (multi-slot).
static VTABLE_SLOT0_V1: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0001_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_SLOT0_V2: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0001_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_SLOT1_V1: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0002_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_SLOT1_V2: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0002_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_SLOT2_V1: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0003_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VTABLE_SLOT2_V2: PluginVTable = PluginVTable {
    contract_id: 0xBB22_0000_0000_0003_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

// Vtables for Test 5 (concurrent resolvers).
static VT_CONC_A: PluginVTable = PluginVTable {
    contract_id: 0xCC33_0000_0000_0001_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VT_CONC_B: PluginVTable = PluginVTable {
    contract_id: 0xCC33_0000_0000_0001_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

// Vtables for Test 6 (last clone).
static VT_LAST_V1: PluginVTable = PluginVTable {
    contract_id: 0xDD44_0000_0000_0001_u64,
    contract_version: 1_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};
static VT_LAST_V2: PluginVTable = PluginVTable {
    contract_id: 0xDD44_0000_0000_0001_u64,
    contract_version: 2_u32 << 16,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

// --- Helpers -----------------------------------------------------------------

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    }
}

/// Run the quiescence loop from reload.rs:212-228 directly.
///
/// Returns `Ok(())` when all `old_arcs` reach strong_count == 1 before the
/// timeout, or `Err(())` on timeout.
fn run_quiescence_loop(old_arcs: &[Arc<VTableSlot>]) -> Result<(), ()> {
    let quiescence_start: Instant = Instant::now();
    for old_arc in old_arcs {
        loop {
            if Arc::strong_count(old_arc) == 1_usize {
                break;
            }
            if quiescence_start.elapsed() > QUIESCENCE_TIMEOUT {
                return Err(());
            }
            std::thread::sleep(Duration::from_millis(1_u64));
            spin_loop();
        }
    }
    Ok(())
}

// --- Tests -------------------------------------------------------------------

/// Test 1: quiescence completes immediately when no guard is held.
///
/// After `swap_vtable` the old Arc's only owner is `old_arcs` itself
/// (strong_count == 1). The loop must exit without sleeping.
#[test]
fn stress_quiescence_no_contention() {
    let registry: Registry = Registry::new();
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin", "quie.race.contract1");

    // SAFETY: VTABLE_V1 is 'static and valid for the Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_V1,
                "quie.race.contract1".to_owned(),
                0xAAAA_0001_u64,
            )
            .expect("register must succeed")
    };

    // Swap to v2 -- nobody holds a guard, so strong_count on old_arc will be 1.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    // Sanity: nobody else holds the old arc.
    assert_eq!(
        Arc::strong_count(&old_arc),
        1_usize,
        "old_arc must have strong_count == 1 before quiescence loop"
    );

    let old_arcs: Vec<Arc<VTableSlot>> = vec![old_arc];
    let start: Instant = Instant::now();
    let result: Result<(), ()> = run_quiescence_loop(&old_arcs);
    let elapsed: Duration = start.elapsed();

    assert!(
        result.is_ok(),
        "quiescence loop must succeed when no guard is held"
    );
    assert!(
        elapsed < Duration::from_millis(50_u64),
        "quiescence with no contention must complete in <50 ms, took {:?}",
        elapsed
    );
}

/// Test 2: quiescence succeeds once a short-lived guard is released.
///
/// The old slot Arc is cloned before swap. The clone is moved into a background
/// thread that holds it for 200 ms then drops it. The main thread's quiescence
/// loop must wait and then complete successfully well within the 5 s timeout.
#[test]
fn stress_quiescence_succeeds_after_guard_released() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin2", "quie.race.contract2");

    // SAFETY: VTABLE_V3 is 'static and valid for the Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_V3,
                "quie.race.contract2".to_owned(),
                0xAAAA_0002_u64,
            )
            .expect("register must succeed")
    };

    // Resolve a guard BEFORE the swap so its Arc points at the soon-to-be-old slot.
    let guard: PluginVTableGuard = registry
        .resolve_guard(handle)
        .expect("resolve_guard must succeed before swap");

    // After swap, old_arc == the Arc the guard is keeping alive.
    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_V4));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    // guard holds one clone; old_arc holds another.
    assert!(
        Arc::strong_count(&old_arc) >= 2_usize,
        "old_arc strong_count must be >= 2 while guard is alive"
    );

    // PluginVTableGuard is !Send; clone the underlying Arc<VTableSlot> instead.
    let arc_clone: Arc<VTableSlot> = Arc::clone(&old_arc);
    // Drop the guard locally -- the arc_clone keeps count elevated.
    drop(guard);

    let hold_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        // Simulate an in-flight call holding the slot alive for 200 ms.
        std::thread::sleep(Duration::from_millis(200_u64));
        drop(arc_clone);
    });

    // Quiescence must succeed within timeout -- count drops once thread wakes.
    let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
    hold_thread.join().expect("hold_thread must not panic");

    assert!(
        result.is_ok(),
        "quiescence loop must succeed after guard-holder thread releases the arc"
    );
}

/// Test 3: quiescence times out when a guard is held indefinitely.
///
/// A background thread holds an Arc clone of the old slot forever (longer than
/// the 5 s QUIESCENCE_TIMEOUT). The loop must return `Err(())` (timeout).
///
/// Because this takes >=5 s, it is `#[ignore]` by default.
#[test]
#[ignore] // Takes ~5 s -- run with: cargo test -- --ignored stress_quiescence_timeout_fires
fn stress_quiescence_timeout_fires() {
    // Use vtables with a unique contract_id not shared with other tests.
    static VTABLE_T1: PluginVTable = PluginVTable {
        contract_id: 0xEE55_0000_FFFF_0001_u64,
        contract_version: 1_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };
    static VTABLE_T2: PluginVTable = PluginVTable {
        contract_id: 0xEE55_0000_FFFF_0001_u64,
        contract_version: 2_u32 << 16,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    };

    let registry: Registry = Registry::new();
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin3", "quie.race.contract3");

    // SAFETY: VTABLE_T1 is 'static and valid for the Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_T1,
                "quie.race.contract3".to_owned(),
                0xAAAA_0003_u64,
            )
            .expect("register must succeed")
    };

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_T2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    // Clone the old arc and move it into a background thread that holds it
    // for 7 s -- well beyond the 5 s QUIESCENCE_TIMEOUT.
    let arc_clone: Arc<VTableSlot> = Arc::clone(&old_arc);
    let hold_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(7_u64));
        drop(arc_clone);
    });

    let start: Instant = Instant::now();
    let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
    let elapsed: Duration = start.elapsed();

    // Must have timed out.
    assert!(
        result.is_err(),
        "quiescence loop must time out when arc is held forever"
    );
    assert!(
        elapsed >= QUIESCENCE_TIMEOUT,
        "elapsed must be >= QUIESCENCE_TIMEOUT ({:?}), got {:?}",
        QUIESCENCE_TIMEOUT,
        elapsed
    );
    // Should not wait much longer than the timeout.
    assert!(
        elapsed < QUIESCENCE_TIMEOUT + Duration::from_millis(200_u64),
        "elapsed must not overshoot timeout by >200 ms, got {:?}",
        elapsed
    );

    hold_thread.join().expect("hold_thread must not panic");
}

/// Test 4: quiescence with multiple old arcs -- all must reach count 1.
///
/// Swap vtables for three separate slots. Hold arcs for each with different
/// release delays (50 ms, 100 ms, 150 ms). The loop must wait for all three.
#[test]
fn stress_quiescence_multiple_arcs_all_must_quiesce() {
    let registry: Registry = Registry::new();

    // Register three plugins in three different slots.
    // SAFETY: all static vtables are valid for the Registry lifetime.
    let h0: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("slot0_plugin", "quie.slot0"),
                &VTABLE_SLOT0_V1,
                "quie.slot0".to_owned(),
                0xBBBB_0001_u64,
            )
            .expect("slot0 register must succeed")
    };
    // SAFETY: VTABLE_SLOT1_V1 is 'static and valid for the Registry lifetime.
    let h1: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("slot1_plugin", "quie.slot1"),
                &VTABLE_SLOT1_V1,
                "quie.slot1".to_owned(),
                0xBBBB_0002_u64,
            )
            .expect("slot1 register must succeed")
    };
    // SAFETY: VTABLE_SLOT2_V1 is 'static and valid for the Registry lifetime.
    let h2: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("slot2_plugin", "quie.slot2"),
                &VTABLE_SLOT2_V1,
                "quie.slot2".to_owned(),
                0xBBBB_0003_u64,
            )
            .expect("slot2 register must succeed")
    };

    // Swap all three slots.
    let old0: Arc<VTableSlot> = registry
        .swap_vtable(h0.index, Arc::new(VTableSlot(&VTABLE_SLOT0_V2)))
        .expect("swap slot0 must succeed");
    let old1: Arc<VTableSlot> = registry
        .swap_vtable(h1.index, Arc::new(VTableSlot(&VTABLE_SLOT1_V2)))
        .expect("swap slot1 must succeed");
    let old2: Arc<VTableSlot> = registry
        .swap_vtable(h2.index, Arc::new(VTableSlot(&VTABLE_SLOT2_V2)))
        .expect("swap slot2 must succeed");

    // Create clones to hold in background threads at different delays.
    let clone0: Arc<VTableSlot> = Arc::clone(&old0);
    let clone1: Arc<VTableSlot> = Arc::clone(&old1);
    let clone2: Arc<VTableSlot> = Arc::clone(&old2);

    std::thread::scope(|s| {
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(50_u64));
            drop(clone0);
        });
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(100_u64));
            drop(clone1);
        });
        s.spawn(move || {
            std::thread::sleep(Duration::from_millis(150_u64));
            drop(clone2);
        });

        let old_arcs: Vec<Arc<VTableSlot>> = vec![old0, old1, old2];
        let result: Result<(), ()> = run_quiescence_loop(&old_arcs);
        assert!(
            result.is_ok(),
            "quiescence loop must succeed once all three arcs are released"
        );
    });
}

/// Test 5: concurrent resolvers and swap race -- checks that arc strong_count
/// accounting is correct across many parallel `resolve_guard` calls.
///
/// This stress test:
/// - Spawns N resolver threads that each call `resolve_guard` in a tight loop.
/// - On the main thread, swaps the vtable 20 times.
/// - After each swap, verifies that the old_arc eventually reaches count 1
///   (quiescence succeeds within the timeout).
#[test]
fn stress_quiescence_concurrent_resolvers_and_swaps() {
    const RESOLVER_THREADS: usize = 8_usize;
    const SWAP_ROUNDS: usize = 20_usize;
    // Short sleep per resolve to maximise contention window.
    const HOLD_MILLIS: u64 = 5_u64;

    let registry: Arc<Registry> = Arc::new(Registry::new());

    // SAFETY: VT_CONC_A is 'static and valid for the Registry lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("conc_plugin", "quie.conc.contract"),
                &VT_CONC_A,
                "quie.conc.contract".to_owned(),
                0xCCCC_0001_u64,
            )
            .expect("register must succeed")
    };

    let stop: Arc<core::sync::atomic::AtomicBool> =
        Arc::new(core::sync::atomic::AtomicBool::new(false));

    // Spawn resolver threads. Each thread repeatedly resolves a guard, sleeps
    // briefly (holding the Arc), then drops the guard.
    let mut resolver_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(RESOLVER_THREADS);
    for _thread_idx in 0_usize..RESOLVER_THREADS {
        let reg_clone: Arc<Registry> = Arc::clone(&registry);
        let stop_clone: Arc<core::sync::atomic::AtomicBool> = Arc::clone(&stop);
        let resolver_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            while !stop_clone.load(core::sync::atomic::Ordering::Relaxed) {
                // resolve_guard uses handle generation 0; the generation bumps on each swap.
                // We use the slot index directly and accept StaleHandle errors gracefully.
                let guard_result: Result<PluginVTableGuard, _> =
                    reg_clone.resolve_guard(PluginHandle {
                        index: handle.index,
                        generation: 0,
                    });
                if let Ok(guard) = guard_result {
                    // Hold it briefly to create contention for quiescence.
                    std::thread::sleep(Duration::from_millis(HOLD_MILLIS));
                    drop(guard);
                }
            }
        });
        resolver_handles.push(resolver_handle);
    }

    // Give resolvers time to spin up and grab their first guards.
    std::thread::sleep(Duration::from_millis(20_u64));

    // Alternate between VT_CONC_A and VT_CONC_B for each swap round.
    for round in 0_usize..SWAP_ROUNDS {
        let new_vtable: &'static PluginVTable = if round % 2_usize == 0_usize {
            &VT_CONC_B
        } else {
            &VT_CONC_A
        };
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(new_vtable));

        let old_arc: Arc<VTableSlot> = registry
            .swap_vtable(handle.index, new_arc)
            .expect("swap_vtable must succeed in concurrent test");

        // Run the quiescence loop. Resolvers each hold the guard for HOLD_MILLIS,
        // so quiescence will take at most a few HOLD_MILLIS windows.
        let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
        assert!(
            result.is_ok(),
            "round {}: quiescence must succeed while resolver threads hold guards briefly",
            round
        );
    }

    // Stop resolver threads.
    stop.store(true, core::sync::atomic::Ordering::Relaxed);
    for h in resolver_handles {
        h.join().expect("resolver thread must not panic");
    }
}

/// Test 6: strong_count monotonically approaches 1 -- a single arc clone
/// is released in stages by sub-threads; the loop must not exit early.
///
/// Verifies the loop does NOT exit until ALL clones are dropped, not just the
/// first one.
#[test]
fn stress_quiescence_waits_for_last_clone() {
    let registry: Registry = Registry::new();

    // SAFETY: VT_LAST_V1 is 'static.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("last_clone_plugin", "quie.last.clone"),
                &VT_LAST_V1,
                "quie.last.clone".to_owned(),
                0xDDDD_0001_u64,
            )
            .expect("register must succeed")
    };

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VT_LAST_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    // Create 4 extra clones so strong_count == 5 initially.
    const EXTRA_CLONES: usize = 4_usize;
    let mut clones: Vec<Arc<VTableSlot>> = Vec::with_capacity(EXTRA_CLONES);
    for _i in 0_usize..EXTRA_CLONES {
        clones.push(Arc::clone(&old_arc));
    }
    assert_eq!(
        Arc::strong_count(&old_arc),
        EXTRA_CLONES + 1_usize,
        "strong_count must be {} before quiescence",
        EXTRA_CLONES + 1_usize
    );

    // Release one clone every 30 ms in a background thread.
    // The main quiescence loop must wait until all 4 extra clones are gone.
    let release_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        for clone in clones {
            std::thread::sleep(Duration::from_millis(30_u64));
            drop(clone);
        }
    });

    let start: Instant = Instant::now();
    let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
    let elapsed: Duration = start.elapsed();

    release_thread
        .join()
        .expect("release_thread must not panic");

    assert!(
        result.is_ok(),
        "quiescence must succeed once all clones are dropped"
    );
    // Must have waited at least for the last clone release (4 x 30 ms = 120 ms).
    assert!(
        elapsed >= Duration::from_millis(100_u64),
        "quiescence must have waited for the last clone (>=100 ms), took {:?}",
        elapsed
    );
}
