#![allow(clippy::expect_used)]

use core::hint::spin_loop;
use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::time::Duration;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;
use std::time::Instant;

use polyplug::error::RegistryError;
use polyplug::registry::PluginVTableGuard;
use polyplug::registry::Registry;
use polyplug::registry::VTableSlot;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;

const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5_u64);
const VERSION_V1: u32 = 1_u32 << 16;
const VERSION_V2: u32 = 2_u32 << 16;

const CONTRACT_ID_RACE_1: u64 = 0x5151_0000_0000_0001_u64;
const CONTRACT_ID_RACE_2: u64 = 0x5151_0000_0000_0002_u64;
const CONTRACT_ID_TIMEOUT: u64 = 0x5151_0000_0000_0003_u64;
const CONTRACT_ID_SLOT_0: u64 = 0x5151_0000_0000_0101_u64;
const CONTRACT_ID_SLOT_1: u64 = 0x5151_0000_0000_0102_u64;
const CONTRACT_ID_SLOT_2: u64 = 0x5151_0000_0000_0103_u64;
const CONTRACT_ID_CONC: u64 = 0x5151_0000_0000_0201_u64;
const CONTRACT_ID_LAST: u64 = 0x5151_0000_0000_0301_u64;

const MOCK_FUNCTIONS: [*const (); 0] = [];

static VTABLE_RACE_1_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_RACE_1,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_RACE_1_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_RACE_1,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_RACE_2_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_RACE_2,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_RACE_2_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_RACE_2,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_TIMEOUT_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_TIMEOUT,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_TIMEOUT_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_TIMEOUT,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT0_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_0,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT0_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_0,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT1_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_1,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT1_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_1,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT2_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_2,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SLOT2_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_SLOT_2,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_CONC_A: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_CONC,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_CONC_B: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_CONC,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_LAST_V1: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_LAST,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_LAST_V2: PluginVTable = PluginVTable {
    contract_id: CONTRACT_ID_LAST,
    contract_version: VERSION_V2,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    }
}

fn run_quiescence_loop(old_arcs: &[Arc<VTableSlot>]) -> Result<(), ()> {
    let quiescence_start: Instant = Instant::now();
    for old_arc in old_arcs {
        loop {
            let strong_count: usize = Arc::strong_count(old_arc);
            if strong_count == 1_usize {
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

#[test]
fn stress_quiescence_no_contention() {
    let registry: Registry = Registry::new();
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin", "quie.race.contract1");

    // SAFETY: VTABLE_RACE_1_V1 is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_RACE_1_V1,
                "quie.race.contract1".to_owned(),
                0xAAAA_0001_u64,
            )
            .expect("register must succeed")
    };

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_RACE_1_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    let strong_count: usize = Arc::strong_count(&old_arc);
    assert_eq!(
        strong_count, 1_usize,
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
        "quiescence with no contention must complete in <50 ms, took {elapsed:?}"
    );
}

#[test]
fn stress_quiescence_succeeds_after_guard_released() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin2", "quie.race.contract2");

    // SAFETY: VTABLE_RACE_2_V1 is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_RACE_2_V1,
                "quie.race.contract2".to_owned(),
                0xAAAA_0002_u64,
            )
            .expect("register must succeed")
    };

    let (ready_tx, ready_rx): (Sender<()>, Receiver<()>) = std::sync::mpsc::channel();
    let reg_clone: Arc<Registry> = Arc::clone(&registry);
    let handle_for_thread: PluginHandle = handle;
    let hold_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        let guard: PluginVTableGuard = reg_clone
            .resolve_guard(handle_for_thread)
            .expect("resolve_guard must succeed before swap");
        ready_tx.send(()).expect("ready signal must send");
        std::thread::sleep(Duration::from_millis(200_u64));
        drop(guard);
    });

    ready_rx
        .recv()
        .expect("guard thread must signal readiness before swap");

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_RACE_2_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    let strong_count: usize = Arc::strong_count(&old_arc);
    assert!(
        strong_count >= 2_usize,
        "old_arc strong_count must be >= 2 while guard is alive"
    );

    let start: Instant = Instant::now();
    let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
    let elapsed: Duration = start.elapsed();

    hold_thread.join().expect("hold thread must not panic");

    assert!(
        result.is_ok(),
        "quiescence loop must succeed after guard-holder thread releases"
    );
    assert!(
        elapsed >= Duration::from_millis(100_u64),
        "quiescence loop must wait for guard release (>=100 ms), took {elapsed:?}"
    );
}

#[test]
#[ignore]
fn stress_quiescence_timeout_fires() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let descriptor: PluginDescriptor = make_descriptor("qtest_plugin3", "quie.race.contract3");

    // SAFETY: VTABLE_TIMEOUT_V1 is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_TIMEOUT_V1,
                "quie.race.contract3".to_owned(),
                0xAAAA_0003_u64,
            )
            .expect("register must succeed")
    };

    let (ready_tx, ready_rx): (Sender<()>, Receiver<()>) = std::sync::mpsc::channel();
    let reg_clone: Arc<Registry> = Arc::clone(&registry);
    let handle_for_thread: PluginHandle = handle;
    let hold_thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        let guard: PluginVTableGuard = reg_clone
            .resolve_guard(handle_for_thread)
            .expect("resolve_guard must succeed before swap");
        ready_tx.send(()).expect("ready signal must send");
        std::thread::sleep(Duration::from_secs(7_u64));
        drop(guard);
    });

    ready_rx
        .recv()
        .expect("guard thread must signal readiness before swap");

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_TIMEOUT_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    let start: Instant = Instant::now();
    let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
    let elapsed: Duration = start.elapsed();

    assert!(
        result.is_err(),
        "quiescence loop must time out when guard is held beyond timeout"
    );
    assert!(
        elapsed >= QUIESCENCE_TIMEOUT,
        "elapsed must be >= QUIESCENCE_TIMEOUT ({QUIESCENCE_TIMEOUT:?}), got {elapsed:?}"
    );
    assert!(
        elapsed < QUIESCENCE_TIMEOUT + Duration::from_millis(200_u64),
        "elapsed must not overshoot timeout by >200 ms, got {elapsed:?}"
    );

    hold_thread.join().expect("hold thread must not panic");
}

#[test]
fn stress_quiescence_multiple_arcs_all_must_quiesce() {
    let registry: Registry = Registry::new();

    // SAFETY: VTABLE_SLOT0_V1 is a static reference valid for the test lifetime.
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
    // SAFETY: VTABLE_SLOT1_V1 is a static reference valid for the test lifetime.
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
    // SAFETY: VTABLE_SLOT2_V1 is a static reference valid for the test lifetime.
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

    let old0: Arc<VTableSlot> = registry
        .swap_vtable(h0.index, Arc::new(VTableSlot(&VTABLE_SLOT0_V2)))
        .expect("swap slot0 must succeed");
    let old1: Arc<VTableSlot> = registry
        .swap_vtable(h1.index, Arc::new(VTableSlot(&VTABLE_SLOT1_V2)))
        .expect("swap slot1 must succeed");
    let old2: Arc<VTableSlot> = registry
        .swap_vtable(h2.index, Arc::new(VTableSlot(&VTABLE_SLOT2_V2)))
        .expect("swap slot2 must succeed");

    let clone0: Arc<VTableSlot> = Arc::clone(&old0);
    let clone1: Arc<VTableSlot> = Arc::clone(&old1);
    let clone2: Arc<VTableSlot> = Arc::clone(&old2);

    let hold0: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50_u64));
        drop(clone0);
    });
    let hold1: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100_u64));
        drop(clone1);
    });
    let hold2: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150_u64));
        drop(clone2);
    });

    let old_arcs: Vec<Arc<VTableSlot>> = vec![old0, old1, old2];
    let result: Result<(), ()> = run_quiescence_loop(&old_arcs);

    hold0.join().expect("hold0 thread must not panic");
    hold1.join().expect("hold1 thread must not panic");
    hold2.join().expect("hold2 thread must not panic");

    assert!(
        result.is_ok(),
        "quiescence loop must succeed once all three arcs are released"
    );
}

#[test]
fn stress_quiescence_concurrent_resolvers_and_swaps() {
    const RESOLVER_THREADS: usize = 8_usize;
    const SWAP_ROUNDS: usize = 20_usize;
    const HOLD_MILLIS: u64 = 5_u64;

    let registry: Arc<Registry> = Arc::new(Registry::new());

    // SAFETY: VTABLE_CONC_A is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("conc_plugin", "quie.conc.contract"),
                &VTABLE_CONC_A,
                "quie.conc.contract".to_owned(),
                0xCCCC_0001_u64,
            )
            .expect("register must succeed")
    };

    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let mut resolver_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(RESOLVER_THREADS);

    for _thread_idx in 0_usize..RESOLVER_THREADS {
        let reg_clone: Arc<Registry> = Arc::clone(&registry);
        let stop_clone: Arc<AtomicBool> = Arc::clone(&stop);
        let resolver_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                let handle_result: Result<PluginHandle, RegistryError> =
                    reg_clone.find_by_contract(CONTRACT_ID_CONC, 0_u32);
                if let Ok(resolved_handle) = handle_result {
                    let guard_result: Result<PluginVTableGuard, RegistryError> =
                        reg_clone.resolve_guard(resolved_handle);
                    if let Ok(guard) = guard_result {
                        std::thread::sleep(Duration::from_millis(HOLD_MILLIS));
                        drop(guard);
                    }
                }
            }
        });
        resolver_handles.push(resolver_handle);
    }

    std::thread::sleep(Duration::from_millis(20_u64));

    for round in 0_usize..SWAP_ROUNDS {
        let new_vtable: &'static PluginVTable = if round % 2_usize == 0_usize {
            &VTABLE_CONC_B
        } else {
            &VTABLE_CONC_A
        };
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(new_vtable));
        let old_arc: Arc<VTableSlot> = registry
            .swap_vtable(handle.index, new_arc)
            .expect("swap_vtable must succeed in concurrent test");

        let result: Result<(), ()> = run_quiescence_loop(&[old_arc]);
        assert!(
            result.is_ok(),
            "round {round}: quiescence must succeed while resolver threads hold guards briefly"
        );
    }

    stop.store(true, Ordering::Relaxed);
    for h in resolver_handles {
        h.join().expect("resolver thread must not panic");
    }
}

#[test]
fn stress_quiescence_waits_for_last_clone() {
    let registry: Registry = Registry::new();

    // SAFETY: VTABLE_LAST_V1 is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                make_descriptor("last_clone_plugin", "quie.last.clone"),
                &VTABLE_LAST_V1,
                "quie.last.clone".to_owned(),
                0xDDDD_0001_u64,
            )
            .expect("register must succeed")
    };

    let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_LAST_V2));
    let old_arc: Arc<VTableSlot> = registry
        .swap_vtable(handle.index, new_arc)
        .expect("swap_vtable must succeed");

    const EXTRA_CLONES: usize = 4_usize;
    let mut clones: Vec<Arc<VTableSlot>> = Vec::with_capacity(EXTRA_CLONES);
    for _i in 0_usize..EXTRA_CLONES {
        clones.push(Arc::clone(&old_arc));
    }
    let strong_count: usize = Arc::strong_count(&old_arc);
    assert_eq!(
        strong_count,
        EXTRA_CLONES + 1_usize,
        "strong_count must be {} before quiescence",
        EXTRA_CLONES + 1_usize
    );

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
    assert!(
        elapsed >= Duration::from_millis(100_u64),
        "quiescence must wait for last clone (>=100 ms), took {elapsed:?}"
    );
}
