#![allow(clippy::expect_used)]

//! Concurrent registry operations: many threads registering, finding, resolving,
//! and swapping contracts at once.
//!
//! Covers concurrent `register_guest_contract` from many threads (distinct
//! contracts must all land and stay resolvable) and a single slot being swapped
//! repeatedly while resolver threads read it (the published-`ReadView` invariant:
//! a resolver observes either the old or the new interface, never a torn one).

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::{HostContractError, LoaderError, RegistryError, RuntimeError};
use polyplug::runtime::Runtime;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInterface,
    HostContractInstance, HostContractInterface, NativeDispatch, PluginDescriptor, Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;
use polyplug_utils::HostContractId;

use crate::common::TestNativeLoader;
use crate::fixtures::make_descriptor;

const THREADS: usize = 8_usize;
const RESOLVER_THREADS: usize = 6_usize;
const RESOLVE_ROUNDS: usize = 32_usize;
const SWAP_ROUNDS: usize = 24_usize;
const VERSION_V1: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};
const VERSION_V2: Version = Version {
    major: 2,
    minor: 0,
    patch: 0,
};

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

static INTERFACES_V1: [GuestContractInterface; THREADS] = [
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[0]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[1]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[2]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[3]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[4]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[5]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[6]), VERSION_V1),
    make_interface!(GuestContractId::from_u64(CONTRACT_IDS[7]), VERSION_V1),
];

const SWAP_CONTRACT_ID: u64 = 0x7171_0000_0000_2000_u64;

static INTERFACE_SWAP_V1: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(SWAP_CONTRACT_ID), VERSION_V1);

static INTERFACE_SWAP_V2: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(SWAP_CONTRACT_ID), VERSION_V2);

#[test]
fn stress_concurrent_register_find_resolve() {
    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(THREADS));
    let mut thread_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(THREADS);

    for idx in 0_usize..THREADS {
        let reg_clone: Arc<RuntimeStore> = Arc::clone(&registry);
        let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            let descriptor: PluginDescriptor =
                make_descriptor(PLUGIN_NAMES[idx], CONTRACT_NAMES[idx]);
            let interface: &'static GuestContractInterface = &INTERFACES_V1[idx];
            barrier_clone.wait();
            // SAFETY: interface is a static reference valid for the test lifetime.
            let handle: GuestContractHandle = unsafe {
                reg_clone
                    .register_guest_contract(
                        descriptor,
                        interface,
                        CONTRACT_NAMES[idx].to_owned(),
                        BundleId::from_u64(idx as u64),
                    )
                    .expect("register must succeed")
            };

            for _round in 0_usize..RESOLVE_ROUNDS {
                let found: GuestContractHandle = reg_clone
                    .find_guest_contract(GuestContractId::from_u64(CONTRACT_IDS[idx]), 0_u32)
                    .expect("find_guest_contract must succeed");
                let interface_ptr: *const GuestContractInterface = reg_clone
                    .resolve_guest_contract(found)
                    .expect("resolve must succeed");
                // SAFETY: interface_ptr is from the registry and valid.
                let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
                // SAFETY: interface_ptr is from the registry and valid.
                let version: &Version = unsafe { &(*interface_ptr).contract_version };
                assert_eq!(contract_id.id(), CONTRACT_IDS[idx]);
                assert_eq!(*version, VERSION_V1);
            }

            let resolved: Result<*const GuestContractInterface, RegistryError> =
                reg_clone.resolve_guest_contract(handle);
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
        let found: GuestContractHandle = registry
            .find_guest_contract(GuestContractId::from_u64(expected_cid), 0_u32)
            .expect("main-thread find must succeed");
        let interface_ptr: *const GuestContractInterface = registry
            .resolve_guest_contract(found)
            .expect("main-thread resolve must succeed");
        // SAFETY: interface_ptr is valid.
        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
        assert_eq!(contract_id.id(), CONTRACT_IDS[idx]);
    }
}

#[test]
fn stress_concurrent_swaps_with_resolvers() {
    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "stress.swap.contract");
    // SAFETY: INTERFACE_SWAP_V1 is a static reference valid for the test lifetime.
    let handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_SWAP_V1,
                "stress.swap.contract".to_owned(),
                BundleId::from_u64(0xABCD_0001_u64),
            )
            .expect("initial register must succeed")
    };

    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let ready: Arc<Barrier> = Arc::new(Barrier::new(RESOLVER_THREADS + 1_usize));
    let resolve_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let mut resolver_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(RESOLVER_THREADS);

    for _thread_idx in 0_usize..RESOLVER_THREADS {
        let reg_clone: Arc<RuntimeStore> = Arc::clone(&registry);
        let stop_clone: Arc<AtomicBool> = Arc::clone(&stop);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let resolve_counter: Arc<AtomicUsize> = Arc::clone(&resolve_count);
        let resolver_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            ready_clone.wait();
            // Each resolver guarantees at least one successful resolve before
            // honoring `stop` — on a loaded runner the swap loop can finish and
            // set `stop` before this thread is ever scheduled, and a plain
            // `while !stop` loop would then exit with zero resolves and fail
            // the test's "must observe at least one resolve" assertion.
            let mut local_resolves: usize = 0_usize;
            loop {
                // Pin the epoch across find→resolve→deref. Under true unload the
                // superseded interface is reclaimed via epoch-deferred reclamation once
                // no reader is pinned; pinning before resolving keeps the interface this
                // iteration touches alive across the deref even if the swapper thread
                // republishes concurrently.
                let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                let handle_result: Result<GuestContractHandle, RegistryError> = reg_clone
                    .find_guest_contract(GuestContractId::from_u64(SWAP_CONTRACT_ID), 0_u32);
                if let Ok(found) = handle_result {
                    let resolve_result: Result<*const GuestContractInterface, RegistryError> =
                        reg_clone.resolve_guest_contract(found);
                    if let Ok(interface_ptr) = resolve_result {
                        // SAFETY: the epoch guard pinned above keeps the resolved
                        // interface alive across this deref despite a concurrent swap.
                        let version: &Version = unsafe { &(*interface_ptr).contract_version };
                        assert!(
                            *version == VERSION_V1 || *version == VERSION_V2,
                            "version must be V1 or V2"
                        );
                        resolve_counter.fetch_add(1_usize, Ordering::Relaxed);
                        local_resolves += 1_usize;
                    }
                }
                if stop_clone.load(Ordering::Relaxed) && local_resolves >= 1_usize {
                    break;
                }
            }
        });
        resolver_handles.push(resolver_handle);
    }

    ready.wait();

    for round in 0_usize..SWAP_ROUNDS {
        let new_interface: &'static GuestContractInterface = if round % 2_usize == 0_usize {
            &INTERFACE_SWAP_V2
        } else {
            &INTERFACE_SWAP_V1
        };
        let new_arc: Arc<GuestContractInterface> = Arc::new(*new_interface);
        registry
            .swap_guest_contract_interface(handle.index, new_arc)
            .expect("swap_interface must succeed");
        // No quiescence wait needed - direct swap model
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

// ─── Registration races: exactly-one-winner under contention ──────────────────

const REGISTER_RACE_THREADS: usize = 8_usize;

/// Many threads register a loader with the SAME name ("native") at once. The
/// loaders map is an `RwLock<HashMap>` with a check-then-insert: exactly one
/// registration must win, every other must see `DuplicateLoader`. No insert may
/// be lost or double-counted under the race.
#[test]
fn concurrent_register_loader_duplicate_is_exactly_one_winner() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("runtime build must succeed");
    let ready: Arc<Barrier> = Arc::new(Barrier::new(REGISTER_RACE_THREADS));
    let ok_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let dup_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(REGISTER_RACE_THREADS);
    for _ in 0_usize..REGISTER_RACE_THREADS {
        let rt: Arc<Runtime> = Arc::clone(&runtime);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let ok_clone: Arc<AtomicUsize> = Arc::clone(&ok_count);
        let dup_clone: Arc<AtomicUsize> = Arc::clone(&dup_count);
        handles.push(std::thread::spawn(move || {
            ready_clone.wait();
            match rt.register_loader(Box::new(TestNativeLoader::new())) {
                Ok(()) => {
                    ok_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. })) => {
                    dup_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(other) => panic!("unexpected error registering loader: {other:?}"),
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread must not panic");
    }

    assert_eq!(
        ok_count.load(Ordering::Relaxed),
        1_usize,
        "exactly one loader registration must win the race"
    );
    assert_eq!(
        dup_count.load(Ordering::Relaxed),
        REGISTER_RACE_THREADS - 1_usize,
        "every other registration must observe DuplicateLoader"
    );
}

// ─── Concurrent host-contract registration ────────────────────────────────────

/// No-op host-contract `create_instance`.
///
/// # Safety
/// Matches the ABI host-contract `create_instance` signature.
unsafe extern "C" fn hc_create(
    _this: *const HostContractInterface,
    _args: *const (),
    out_instance: *mut HostContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(HostContractInstance::null()) };
    }
}

/// No-op host-contract `destroy_instance`.
///
/// # Safety
/// Matches the ABI host-contract `destroy_instance` signature.
unsafe extern "C" fn hc_destroy(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
}

/// Leak a minimal native host-contract interface to obtain the `&'static`
/// reference `register_host_contract` requires. Test-only: the leak lives for the
/// process, which is the whole test run.
fn leak_host_contract_interface() -> &'static HostContractInterface {
    Box::leak(Box::new(HostContractInterface {
        contract_id: HostContractId::from(0_u64),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton: false,
        dispatch_type: DispatchType::Native,
        runtime: core::ptr::null_mut(),
        user_data: core::ptr::null_mut(),
        create_instance: hc_create,
        destroy_instance: hc_destroy,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: crate::fixtures::MOCK_FNS_EMPTY.as_ptr(),
            },
        },
    }))
}

/// Many threads register host contracts under DISTINCT ids concurrently; every
/// one must land and be retrievable. Exercises the `RwLock<HashMap>` host-contract
/// map under parallel inserts with no lost writes.
#[test]
fn concurrent_register_host_contract_distinct_ids_all_land() {
    const BASE: u64 = 0xB055_0000_0000_0000_u64;
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("runtime build must succeed");
    let ready: Arc<Barrier> = Arc::new(Barrier::new(REGISTER_RACE_THREADS));

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(REGISTER_RACE_THREADS);
    for i in 0_usize..REGISTER_RACE_THREADS {
        let rt: Arc<Runtime> = Arc::clone(&runtime);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        handles.push(std::thread::spawn(move || {
            let iface: &'static HostContractInterface = leak_host_contract_interface();
            ready_clone.wait();
            rt.register_host_contract(BASE + i as u64, iface)
                .expect("distinct-id host contract registration must succeed");
        }));
    }
    for handle in handles {
        handle.join().expect("thread must not panic");
    }

    for i in 0_usize..REGISTER_RACE_THREADS {
        assert!(
            runtime.get_host_contract(BASE + i as u64, 0_u32).is_some(),
            "every distinctly-registered host contract must be retrievable"
        );
    }
}

/// Many threads register a host contract under the SAME id at once: exactly one
/// wins, the rest see `DuplicateContract`.
#[test]
fn concurrent_register_host_contract_same_id_is_exactly_one_winner() {
    const ID: u64 = 0xB055_0000_DEAD_BEEF_u64;
    let runtime: Arc<Runtime> = Runtime::builder()
        .build()
        .expect("runtime build must succeed");
    let ready: Arc<Barrier> = Arc::new(Barrier::new(REGISTER_RACE_THREADS));
    let ok_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let dup_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(REGISTER_RACE_THREADS);
    for _ in 0_usize..REGISTER_RACE_THREADS {
        let rt: Arc<Runtime> = Arc::clone(&runtime);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let ok_clone: Arc<AtomicUsize> = Arc::clone(&ok_count);
        let dup_clone: Arc<AtomicUsize> = Arc::clone(&dup_count);
        handles.push(std::thread::spawn(move || {
            let iface: &'static HostContractInterface = leak_host_contract_interface();
            ready_clone.wait();
            match rt.register_host_contract(ID, iface) {
                Ok(()) => {
                    ok_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(HostContractError::DuplicateContract { .. }) => {
                    dup_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(other) => panic!("unexpected host-contract error: {other:?}"),
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread must not panic");
    }

    assert_eq!(
        ok_count.load(Ordering::Relaxed),
        1_usize,
        "exactly one host-contract registration must win"
    );
    assert_eq!(
        dup_count.load(Ordering::Relaxed),
        REGISTER_RACE_THREADS - 1_usize,
        "every other registration must observe DuplicateContract"
    );
}

// ─── Concurrent guest DuplicateProvider race ──────────────────────────────────

const DUP_CONTRACT_ID: u64 = 0x7171_0000_0000_4000_u64;
const DUP_BUNDLE_ID: u64 = 0xABCD_0000_0000_0009_u64;

static INTERFACE_DUP: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(DUP_CONTRACT_ID), VERSION_V1);

/// Many threads race to register the SAME (bundle_id, contract): the
/// `DuplicateProvider` guard runs under the registry write lock, so exactly one
/// must win and the rest must see `DuplicateProvider`. The contract must resolve
/// afterward — the race must not corrupt the slot or the find index.
#[test]
fn concurrent_register_guest_duplicate_provider_is_exactly_one_winner() {
    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let ready: Arc<Barrier> = Arc::new(Barrier::new(REGISTER_RACE_THREADS));
    let ok_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let dup_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));

    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(REGISTER_RACE_THREADS);
    for _ in 0_usize..REGISTER_RACE_THREADS {
        let reg: Arc<RuntimeStore> = Arc::clone(&registry);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let ok_clone: Arc<AtomicUsize> = Arc::clone(&ok_count);
        let dup_clone: Arc<AtomicUsize> = Arc::clone(&dup_count);
        handles.push(std::thread::spawn(move || {
            let descriptor: PluginDescriptor = make_descriptor("dup_plugin", "stress.dup.contract");
            ready_clone.wait();
            // SAFETY: INTERFACE_DUP is 'static, valid for the test lifetime.
            let result: Result<GuestContractHandle, RegistryError> = unsafe {
                reg.register_guest_contract(
                    descriptor,
                    &INTERFACE_DUP,
                    "stress.dup.contract".to_owned(),
                    BundleId::from_u64(DUP_BUNDLE_ID),
                )
            };
            match result {
                Ok(_) => {
                    ok_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(RegistryError::DuplicateProvider { .. }) => {
                    dup_clone.fetch_add(1_usize, Ordering::Relaxed);
                }
                Err(other) => panic!("unexpected registry error: {other:?}"),
            }
        }));
    }
    for handle in handles {
        handle.join().expect("thread must not panic");
    }

    assert_eq!(
        ok_count.load(Ordering::Relaxed),
        1_usize,
        "exactly one provider registration must win the race"
    );
    assert_eq!(
        dup_count.load(Ordering::Relaxed),
        REGISTER_RACE_THREADS - 1_usize,
        "every other registration must observe DuplicateProvider"
    );
    assert!(
        registry
            .find_guest_contract(GuestContractId::from_u64(DUP_CONTRACT_ID), 0_u32)
            .is_ok(),
        "the contract must resolve after the registration race"
    );
}

// ─── Concurrent find_all during multi-provider registration ───────────────────

const MULTI_CONTRACT_ID: u64 = 0x7171_0000_0000_5000_u64;
const PROVIDERS: usize = 8_usize;
const FIND_ALL_READERS: usize = 4_usize;

static INTERFACE_MULTI: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(MULTI_CONTRACT_ID), VERSION_V1);

/// While writer threads register `PROVIDERS` distinct bundles that all provide
/// the SAME contract id (different bundle ids → multiple providers, not
/// duplicates), reader threads call `find_all_guest_contracts` continuously. Each
/// read must observe a count in `0..=PROVIDERS` — never a torn or phantom view of
/// the published `ReadView` as it is republished — and the final count must equal
/// `PROVIDERS`.
#[test]
fn concurrent_find_all_during_multi_provider_registration() {
    const BASE_BUNDLE: u64 = 0xABCD_0000_0005_0000_u64;

    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let contract_id: GuestContractId = GuestContractId::from_u64(MULTI_CONTRACT_ID);
    let ready: Arc<Barrier> = Arc::new(Barrier::new(PROVIDERS + FIND_ALL_READERS));
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let mut writer_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(PROVIDERS);
    for i in 0_usize..PROVIDERS {
        let reg: Arc<RuntimeStore> = Arc::clone(&registry);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        writer_handles.push(std::thread::spawn(move || {
            // Same contract id AND name for every provider — only the bundle id
            // differs, which is what makes them distinct providers rather than a
            // DuplicateProvider (same bundle) or a ContractIdCollision (same id,
            // different name).
            let descriptor: PluginDescriptor = make_descriptor("multi_plugin", "multi.provider");
            ready_clone.wait();
            // SAFETY: INTERFACE_MULTI is 'static, valid for the test lifetime.
            unsafe {
                reg.register_guest_contract(
                    descriptor,
                    &INTERFACE_MULTI,
                    "multi.provider".to_owned(),
                    BundleId::from_u64(BASE_BUNDLE + i as u64),
                )
                .expect("each distinct-bundle provider registration must succeed");
            }
        }));
    }

    let mut reader_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(FIND_ALL_READERS);
    for _ in 0_usize..FIND_ALL_READERS {
        let reg: Arc<RuntimeStore> = Arc::clone(&registry);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let stop_clone: Arc<AtomicBool> = Arc::clone(&stop);
        reader_handles.push(std::thread::spawn(move || {
            ready_clone.wait();
            let mut buffer: [GuestContractHandle; PROVIDERS + 4] =
                [GuestContractHandle::null(); PROVIDERS + 4];
            loop {
                let count: usize = reg.find_all_guest_contracts(contract_id, 0_u32, &mut buffer);
                assert!(
                    count <= PROVIDERS,
                    "find_all reported {count} providers — more than the {PROVIDERS} ever registered (torn ReadView)"
                );
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
            }
        }));
    }

    for handle in writer_handles {
        handle.join().expect("writer thread must not panic");
    }
    stop.store(true, Ordering::Relaxed);
    for handle in reader_handles {
        handle.join().expect("reader thread must not panic");
    }

    let mut final_buffer: [GuestContractHandle; PROVIDERS + 4] =
        [GuestContractHandle::null(); PROVIDERS + 4];
    let final_count: usize =
        registry.find_all_guest_contracts(contract_id, 0_u32, &mut final_buffer);
    assert_eq!(
        final_count, PROVIDERS,
        "after all providers register, find_all must return exactly {PROVIDERS}"
    );
}
