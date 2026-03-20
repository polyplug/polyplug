#![allow(clippy::expect_used)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use core::time::Duration;
use std::sync::Arc;
use std::sync::Barrier;
use std::time::Instant;

use polyplug::error::RegistryError;
use polyplug::registry::PluginVTableGuard;
use polyplug::registry::Registry;
use polyplug::registry::VTableSlot;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;

const THREADS: usize = 8_usize;
const RESOLVER_THREADS: usize = 6_usize;
const RESOLVE_ROUNDS: usize = 32_usize;
const SWAP_ROUNDS: usize = 24_usize;
const VERSION_V1: u32 = 1_u32 << 16;
const VERSION_V2: u32 = 2_u32 << 16;
const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(2_u64);

const CONTRACT_IDS: [u64; THREADS] = [
    0x7171_0000_0000_1000_u64,
    0x7171_0000_0000_1001_u64,
    0x7171_0000_0000_1002_u64,
    0x7171_0000_0000_1003_u64,
    0x7171_0000_0000_1004_u64,
    0x7171_0000_0000_1005_u64,
    0x7171_0000_0000_1006_u64,
    0x7171_0000_0000_1007_u64,
];

const PLUGIN_NAMES: [&str; THREADS] = [
    "stress_reg_0",
    "stress_reg_1",
    "stress_reg_2",
    "stress_reg_3",
    "stress_reg_4",
    "stress_reg_5",
    "stress_reg_6",
    "stress_reg_7",
];

const CONTRACT_NAMES: [&str; THREADS] = [
    "stress.registry.contract0",
    "stress.registry.contract1",
    "stress.registry.contract2",
    "stress.registry.contract3",
    "stress.registry.contract4",
    "stress.registry.contract5",
    "stress.registry.contract6",
    "stress.registry.contract7",
];

const MOCK_FUNCTIONS: [*const (); 0] = [];

static VTABLES_V1: [PluginVTable; THREADS] = [
    PluginVTable {
        contract_id: CONTRACT_IDS[0],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[1],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[2],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[3],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[4],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[5],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[6],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
    PluginVTable {
        contract_id: CONTRACT_IDS[7],
        contract_version: VERSION_V1,
        function_count: 0_u32,
        functions: MOCK_FUNCTIONS.as_ptr(),
    },
];

const SWAP_CONTRACT_ID: u64 = 0x7171_0000_0000_2000_u64;

static VTABLE_SWAP_V1: PluginVTable = PluginVTable {
    contract_id: SWAP_CONTRACT_ID,
    contract_version: VERSION_V1,
    function_count: 0_u32,
    functions: MOCK_FUNCTIONS.as_ptr(),
};

static VTABLE_SWAP_V2: PluginVTable = PluginVTable {
    contract_id: SWAP_CONTRACT_ID,
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

fn wait_for_quiescence(old_arc: &Arc<VTableSlot>, timeout: Duration) -> bool {
    let start: Instant = Instant::now();
    loop {
        let strong_count: usize = Arc::strong_count(old_arc);
        if strong_count == 1_usize {
            return true;
        }
        if start.elapsed() > timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1_u64));
    }
}

#[test]
fn stress_concurrent_register_find_resolve() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(THREADS));
    let mut thread_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(THREADS);

    for idx in 0_usize..THREADS {
        let reg_clone: Arc<Registry> = Arc::clone(&registry);
        let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            let descriptor: PluginDescriptor =
                make_descriptor(PLUGIN_NAMES[idx], CONTRACT_NAMES[idx]);
            let vtable: &'static PluginVTable = &VTABLES_V1[idx];
            barrier_clone.wait();
            // SAFETY: vtable is a static reference valid for the test lifetime.
            let handle: PluginHandle = unsafe {
                reg_clone
                    .register(
                        descriptor,
                        vtable,
                        CONTRACT_NAMES[idx].to_owned(),
                        idx as u64,
                    )
                    .expect("register must succeed")
            };

            for _round in 0_usize..RESOLVE_ROUNDS {
                let found: PluginHandle = reg_clone
                    .find_by_contract(CONTRACT_IDS[idx], VERSION_V1)
                    .expect("find_by_contract must succeed");
                let guard: PluginVTableGuard = reg_clone
                    .resolve_guard(found)
                    .expect("resolve_guard must succeed");
                let vtable_ptr: *const PluginVTable = guard.vtable();
                // SAFETY: vtable_ptr is from the registry and valid for the guard lifetime.
                let contract_id: u64 = unsafe { (*vtable_ptr).contract_id };
                // SAFETY: vtable_ptr is from the registry and valid for the guard lifetime.
                let version: u32 = unsafe { (*vtable_ptr).contract_version };
                assert_eq!(contract_id, CONTRACT_IDS[idx]);
                assert_eq!(version, VERSION_V1);
            }

            let resolved: Result<*const PluginVTable, RegistryError> = reg_clone.resolve(handle);
            assert!(
                resolved.is_ok(),
                "resolve must succeed for registered handle"
            );
        });
        thread_handles.push(thread_handle);
    }

    for handle in thread_handles {
        handle.join().expect("thread must not panic");
    }

    for (idx, &expected_cid) in CONTRACT_IDS.iter().enumerate().take(THREADS) {
        let found: PluginHandle = registry
            .find_by_contract(expected_cid, VERSION_V1)
            .expect("main-thread find must succeed");
        let guard: PluginVTableGuard = registry
            .resolve_guard(found)
            .expect("main-thread resolve_guard must succeed");
        let vtable_ptr: *const PluginVTable = guard.vtable();
        // SAFETY: vtable_ptr is valid while guard is alive.
        let contract_id: u64 = unsafe { (*vtable_ptr).contract_id };
        assert_eq!(contract_id, CONTRACT_IDS[idx]);
    }
}

#[test]
fn stress_concurrent_swaps_with_resolvers() {
    let registry: Arc<Registry> = Arc::new(Registry::new());
    let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "stress.swap.contract");
    // SAFETY: VTABLE_SWAP_V1 is a static reference valid for the test lifetime.
    let handle: PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_SWAP_V1,
                "stress.swap.contract".to_owned(),
                0xABCD_0001_u64,
            )
            .expect("initial register must succeed")
    };

    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let ready: Arc<Barrier> = Arc::new(Barrier::new(RESOLVER_THREADS + 1_usize));
    let resolve_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let mut resolver_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(RESOLVER_THREADS);

    for _thread_idx in 0_usize..RESOLVER_THREADS {
        let reg_clone: Arc<Registry> = Arc::clone(&registry);
        let stop_clone: Arc<AtomicBool> = Arc::clone(&stop);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let resolve_counter: Arc<AtomicUsize> = Arc::clone(&resolve_count);
        let resolver_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            ready_clone.wait();
            while !stop_clone.load(Ordering::Relaxed) {
                let handle_result: Result<PluginHandle, RegistryError> =
                    reg_clone.find_by_contract(SWAP_CONTRACT_ID, 0_u32);
                if let Ok(found) = handle_result {
                    let guard_result: Result<PluginVTableGuard, RegistryError> =
                        reg_clone.resolve_guard(found);
                    if let Ok(guard) = guard_result {
                        let vtable_ptr: *const PluginVTable = guard.vtable();
                        // SAFETY: vtable_ptr is valid for the guard lifetime.
                        let version: u32 = unsafe { (*vtable_ptr).contract_version };
                        assert!(
                            version == VERSION_V1 || version == VERSION_V2,
                            "version must be V1 or V2"
                        );
                        resolve_counter.fetch_add(1_usize, Ordering::Relaxed);
                    }
                }
            }
        });
        resolver_handles.push(resolver_handle);
    }

    ready.wait();

    for round in 0_usize..SWAP_ROUNDS {
        let new_vtable: &'static PluginVTable = if round % 2_usize == 0_usize {
            &VTABLE_SWAP_V2
        } else {
            &VTABLE_SWAP_V1
        };
        let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(new_vtable));
        let old_arc: Arc<VTableSlot> = registry
            .swap_vtable(handle.index, new_arc)
            .expect("swap_vtable must succeed");
        let quiesced: bool = wait_for_quiescence(&old_arc, QUIESCENCE_TIMEOUT);
        assert!(quiesced, "round {round}: old arc must quiesce");
    }

    stop.store(true, Ordering::Relaxed);
    for handle in resolver_handles {
        handle.join().expect("resolver thread must not panic");
    }

    let resolved_total: usize = resolve_count.load(Ordering::Relaxed);
    assert!(
        resolved_total > 0_usize,
        "resolver threads must observe at least one resolve"
    );
}
