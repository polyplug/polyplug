#![allow(clippy::expect_used)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::RegistryError;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInterface, HostApi,
    NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

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

const MOCK_FUNCTIONS: [*const (); 0] = [];

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

macro_rules! make_interface {
    ($contract_id:expr, $version:expr) => {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64($contract_id),
            contract_version: $version,
            dispatch_type: DispatchType::Native,
            create_instance: noop_create_instance,
            destroy_instance: noop_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: MOCK_FUNCTIONS.as_ptr(),
                },
            },
        }
    };
}

static INTERFACES_V1: [GuestContractInterface; THREADS] = [
    make_interface!(CONTRACT_IDS[0], VERSION_V1),
    make_interface!(CONTRACT_IDS[1], VERSION_V1),
    make_interface!(CONTRACT_IDS[2], VERSION_V1),
    make_interface!(CONTRACT_IDS[3], VERSION_V1),
    make_interface!(CONTRACT_IDS[4], VERSION_V1),
    make_interface!(CONTRACT_IDS[5], VERSION_V1),
    make_interface!(CONTRACT_IDS[6], VERSION_V1),
    make_interface!(CONTRACT_IDS[7], VERSION_V1),
];

const SWAP_CONTRACT_ID: u64 = 0x7171_0000_0000_2000_u64;

static INTERFACE_SWAP_V1: GuestContractInterface = make_interface!(SWAP_CONTRACT_ID, VERSION_V1);

static INTERFACE_SWAP_V2: GuestContractInterface = make_interface!(SWAP_CONTRACT_ID, VERSION_V2);

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

const UNLOAD_CONTRACT_ID: u64 = 0x7171_0000_0000_3000_u64;

static INTERFACE_UNLOAD: GuestContractInterface = make_interface!(UNLOAD_CONTRACT_ID, VERSION_V1);

const UNLOAD_BUNDLE_ID: u64 = 0xABCD_0002_u64;

const UNLOAD_ROUNDS: usize = 24_usize;

/// Exercises the resolve↔invalidate (resolve→dispatch) race: resolver threads
/// continuously `find` + `resolve` + read the interface while one thread
/// repeatedly invalidates (unloads) and re-registers the bundle.
///
/// The invariant under test is the epoch-reclamation guarantee: a reader that pins
/// the epoch and observes a handle concurrently with an unload must EITHER resolve
/// successfully — in which case the resolved interface stays alive for as long as the
/// reader holds its pin, even though the bundle was invalidated — OR fail cleanly with
/// `StaleHandle`/`PluginNotFound`. It must never produce a use-after-free. Run this
/// under ThreadSanitizer (see `.github/workflows/nightly.yml`) to also assert the
/// registry's locking is race-free.
#[test]
fn stress_concurrent_unload_with_resolvers() {
    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let descriptor: PluginDescriptor = make_descriptor("unload_plugin", "stress.unload.contract");
    // SAFETY: INTERFACE_UNLOAD is a static reference valid for the test lifetime.
    unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_UNLOAD,
                "stress.unload.contract".to_owned(),
                BundleId::from_u64(UNLOAD_BUNDLE_ID),
            )
            .expect("initial register must succeed");
    }

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
            // Guarantee at least one successful resolve before honoring `stop`
            // (mirrors the swap test): the unload loop ends on a re-register, so
            // the contract is present once the loop stops and every resolver can
            // make progress before exiting.
            let mut local_resolves: usize = 0_usize;
            loop {
                // Pin the epoch across find→resolve→deref so the resolved interface
                // stays alive across the deref even though another thread unloads the
                // bundle concurrently — under true unload the superseded interface is
                // epoch-reclaimed only after every pinned reader has unpinned.
                let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                let handle_result: Result<GuestContractHandle, RegistryError> = reg_clone
                    .find_guest_contract(GuestContractId::from_u64(UNLOAD_CONTRACT_ID), 0_u32);
                if let Ok(found) = handle_result {
                    let resolve_result: Result<*const GuestContractInterface, RegistryError> =
                        reg_clone.resolve_guest_contract(found);
                    if let Ok(interface_ptr) = resolve_result {
                        // SAFETY: the epoch guard pinned above keeps the resolved
                        // interface alive across this deref even after a concurrent
                        // unload on another thread.
                        let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
                        assert_eq!(contract_id.id(), UNLOAD_CONTRACT_ID);
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

    for _round in 0_usize..UNLOAD_ROUNDS {
        registry
            .invalidate_bundle(BundleId::from_u64(UNLOAD_BUNDLE_ID))
            .expect("invalidate must succeed");
        let descriptor: PluginDescriptor =
            make_descriptor("unload_plugin", "stress.unload.contract");
        // SAFETY: INTERFACE_UNLOAD is a static reference valid for the test lifetime.
        unsafe {
            registry
                .register_guest_contract(
                    descriptor,
                    &INTERFACE_UNLOAD,
                    "stress.unload.contract".to_owned(),
                    BundleId::from_u64(UNLOAD_BUNDLE_ID),
                )
                .expect("re-register must succeed");
        }
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
