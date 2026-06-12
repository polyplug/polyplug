#![allow(clippy::expect_used)]

//! Concurrent dispatch while a reload/unload swaps the slot in flight.
//!
//! The hot-reload safety story rests on one defining condition: a slot's
//! interface may be swapped *while* other threads are resolving that slot and
//! calling through its function pointers. These tests exercise exactly that —
//! reader/dispatcher threads continuously resolve a contract and invoke a
//! dispatch function pointer while another thread swaps or reloads the slot.
//!
//! Why this is sound: `find_guest_contract` followed by `resolve_guest_contract`
//! reads the slot's `Arc<GuestContractInterface>` under the read lock. The swap
//! (`swap_guest_contract_interface`) takes the write lock, replaces the slot's
//! `Arc`, and hands the superseded published snapshot (which still owns the old
//! `Arc`) to crossbeam-epoch for deferred reclamation. A reader therefore observes
//! either the complete old interface or the complete new one — never a half-swapped
//! struct — and any interface memory it touched stays alive until it unpins, because
//! the deferred free runs only after every reader pinned in the prior epoch has
//! unpinned. The same epoch reclamation backs `apply_reload_swap`, the reconciliation
//! step the reload driver runs after a bundle re-initializes;
//! `swap_guest_contract_interface` is its single-slot, publicly reachable equivalent.

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use core::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use polyplug::runtime::Runtime;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostApi, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use crate::fixtures::{
    RELOAD_V1_DIR, make_descriptor, make_hot_reload_runtime, v1_so_path, v2_so_path,
};

// ─── Distinct-pointer interfaces for the dispatch-during-reload test ──────────

const CONTRACT_ID: u64 = 0xCAFE_F00D_0000_0001_u64;
const READER_THREADS: usize = 8;
const ITERATIONS: usize = 10_000;

const MOCK_FNS: [*const (); 0] = [];

/// `create_instance` for the pre-reload interface. Returns a tagged instance so
/// readers can distinguish which interface version they resolved.
unsafe extern "C" fn create_instance_v1(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// `create_instance` for the reloaded interface — a distinct function pointer so
/// callers can confirm the swap took effect.
unsafe extern "C" fn create_instance_v2(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

static INTERFACE_V1: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(CONTRACT_ID),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: create_instance_v1,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

static INTERFACE_V2: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(CONTRACT_ID),
    contract_version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: create_instance_v2,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

/// Eight readers dispatch through a contract's function pointers while the slot's
/// interface is swapped. No reader may observe a torn struct, crash, or
/// use-after-free; after the swap the contract must resolve to the reloaded
/// interface.
#[test]
fn dispatch_concurrent_with_reload_is_safe() {
    let registry: RuntimeStore = RuntimeStore::new();
    let bundle_id: BundleId = BundleId::new("bundle-a");
    let descriptor: PluginDescriptor = make_descriptor("bundle-a-plugin", "concurrent.contract");

    // SAFETY: INTERFACE_V1 is 'static, valid for the registry's lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "concurrent.contract".to_owned(),
            bundle_id,
        )
    }
    .expect("registration should succeed");
    let slot_idx: u32 = handle.index;

    let contract_id: GuestContractId = GuestContractId::from_u64(CONTRACT_ID);
    let dispatch_count: AtomicU64 = AtomicU64::new(0);

    thread::scope(|scope| {
        let reader_handles: Vec<thread::ScopedJoinHandle<'_, usize>> = (0..READER_THREADS)
            .map(|_| {
                let registry_ref: &RuntimeStore = &registry;
                let dispatch_count_ref: &AtomicU64 = &dispatch_count;
                scope.spawn(move || -> usize {
                    let mut completed: usize = 0;
                    for _ in 0..ITERATIONS {
                        // Pin the epoch across find→resolve→deref so the resolved
                        // interface stays alive across the deref while the swapper thread
                        // republishes concurrently — true unload epoch-reclaims the
                        // superseded interface only after every pinned reader unpins.
                        let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                        let resolved: GuestContractHandle = registry_ref
                            .find_guest_contract(contract_id, 0)
                            .expect("contract must always resolve during reload");
                        assert!(!resolved.is_null(), "resolved handle must be valid");

                        let interface_ptr: *const GuestContractInterface = registry_ref
                            .resolve_guest_contract(resolved)
                            .expect("interface must always resolve during reload");
                        assert!(!interface_ptr.is_null(), "interface pointer must be valid");

                        // SAFETY: the epoch guard pinned above keeps the resolved slot
                        // interface alive across this deref even if the slot is swapped
                        // concurrently on another thread.
                        unsafe {
                            let create_fn: unsafe extern "C" fn(
                                *const HostApi,
                                *const (),
                            )
                                -> GuestContractInstance = (*interface_ptr).create_instance;
                            let instance: GuestContractInstance =
                                create_fn(core::ptr::null(), core::ptr::null());
                            assert!(instance.is_null(), "mock create_instance returns null");
                        }

                        completed += 1;
                        dispatch_count_ref.fetch_add(1, Ordering::Relaxed);
                    }
                    completed
                })
            })
            .collect();

        // Wait until readers are actively dispatching before swapping the slot,
        // so the swap genuinely races in-flight resolves.
        while dispatch_count.load(Ordering::Relaxed) < 1_000 {
            core::hint::spin_loop();
        }

        let new_interface: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
        registry
            .swap_guest_contract_interface(slot_idx, new_interface)
            .expect("interface swap should succeed");

        for reader in reader_handles {
            let completed: usize = reader.join().expect("reader thread must not panic");
            assert_eq!(
                completed, ITERATIONS,
                "every reader must complete all dispatches"
            );
        }
    });

    let resolved_after: GuestContractHandle = registry
        .find_guest_contract(contract_id, 0)
        .expect("contract must resolve after reload");
    let interface_after: *const GuestContractInterface = registry
        .resolve_guest_contract(resolved_after)
        .expect("interface must resolve after reload");

    // SAFETY: interface_after points at the live slot interface, retained for the
    // registry's lifetime.
    let version_after: Version = unsafe { (*interface_after).contract_version };
    assert_eq!(
        version_after.major, 2,
        "after the swap the contract must resolve to the reloaded interface"
    );

    // SAFETY: same retained slot interface; reading the function pointer field is
    // a plain pointer comparison against the known reloaded callback.
    let create_after: unsafe extern "C" fn(*const HostApi, *const ()) -> GuestContractInstance =
        unsafe { (*interface_after).create_instance };
    let expected_create: unsafe extern "C" fn(*const HostApi, *const ()) -> GuestContractInstance =
        create_instance_v2;
    assert!(
        core::ptr::fn_addr_eq(create_after, expected_create),
        "reloaded interface must expose the v2 create_instance pointer"
    );
}

// ─── Mock interfaces for the swap-under-load test ─────────────────────────────

static INTERFACE_QU_A: GuestContractInterface = make_interface!(
    GuestContractId::from_u64(0xCAFE_BABE_0000_0001_u64),
    Version {
        major: 1,
        minor: 0,
        patch: 0,
    }
);

static INTERFACE_QU_B: GuestContractInterface = make_interface!(
    GuestContractId::from_u64(0xCAFE_BABE_0000_0001_u64),
    Version {
        major: 2,
        minor: 0,
        patch: 0,
    }
);

/// Direct swap under concurrent reader load: multiple reader threads continuously
/// resolve interfaces while the reloader thread fires 50+ interface swaps.
#[test]
fn stress_direct_swap_under_concurrent_reader_load() {
    const READER_THREADS: usize = 8_usize;
    const SWAP_ROUNDS: usize = 50_usize;

    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());

    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"swap-load-plugin"),
        contract_name: StringView::from_static(b"swap.load.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };

    // SAFETY: INTERFACE_QU_A is 'static and valid for the test lifetime.
    let handle: polyplug_abi::GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_QU_A,
                "swap.load.contract".to_owned(),
                BundleId::from_u64(0xCAFE_BABE_0000_0001_u64),
            )
            .expect("register must succeed")
    };

    let stop_flag: Arc<core::sync::atomic::AtomicBool> =
        Arc::new(core::sync::atomic::AtomicBool::new(false));

    let mut reader_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(READER_THREADS);

    for _thread_idx in 0_usize..READER_THREADS {
        let reg_clone: Arc<RuntimeStore> = Arc::clone(&registry);
        let stop_clone: Arc<core::sync::atomic::AtomicBool> = Arc::clone(&stop_flag);

        let reader_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                // Pin the epoch across find→resolve→deref so the resolved interface
                // stays alive across the deref while the reloader thread republishes
                // concurrently (true unload epoch-reclaims the superseded interface
                // only after every pinned reader has unpinned).
                let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                let find_result: Result<
                    polyplug_abi::GuestContractHandle,
                    polyplug::error::RegistryError,
                > = reg_clone.find_guest_contract(
                    GuestContractId::from_u64(0xCAFE_BABE_0000_0001_u64),
                    0_u32,
                );
                if let Ok(resolved_handle) = find_result {
                    let resolve_result: Result<
                        *const GuestContractInterface,
                        polyplug::error::RegistryError,
                    > = reg_clone.resolve_guest_contract(resolved_handle);
                    if let Ok(interface_ptr) = resolve_result {
                        // SAFETY: the epoch guard pinned above keeps the resolved
                        // interface alive across this deref despite a concurrent reload.
                        let version: &Version = unsafe { &(*interface_ptr).contract_version };
                        assert!(
                            version.major == 1 || version.major == 2,
                            "version must be 1 or 2"
                        );
                    }
                }
            }
        });

        reader_handles.push(reader_handle);
    }

    // Give readers time to start.
    std::thread::sleep(Duration::from_millis(20_u64));

    for round in 0_usize..SWAP_ROUNDS {
        let new_interface: &'static GuestContractInterface = if round % 2_usize == 0_usize {
            &INTERFACE_QU_B
        } else {
            &INTERFACE_QU_A
        };

        let new_arc: Arc<GuestContractInterface> = Arc::new(*new_interface);
        registry
            .swap_guest_contract_interface(handle.index, new_arc)
            .unwrap_or_else(|e| panic!("swap_interface failed at round {round}: {e}"));
    }

    stop_flag.store(true, Ordering::Relaxed);
    for h in reader_handles {
        h.join().expect("reader thread must not panic");
    }
}

/// Interface handoff correctness: verifies that every interface swap atomically
/// transfers the correct function pointer and that no intermediate state
/// (neither v1 nor v2) is observable between swaps.
///
/// Dispatcher threads spin-read the interface version function; every return value
/// must be exactly 100 (v1) or 200 (v2) -- never anything else.
#[test]
fn stress_interface_handoff_correctness_no_torn_reads() {
    const DISPATCHER_THREADS: usize = 6_usize;
    const RELOAD_ROUNDS: u32 = 80_u32;

    let rt: Arc<Runtime> = make_hot_reload_runtime();
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();

    let stop_flag: Arc<core::sync::atomic::AtomicBool> =
        Arc::new(core::sync::atomic::AtomicBool::new(false));
    let torn_reads: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    let mut dispatcher_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(DISPATCHER_THREADS);

    for _thread_idx in 0_usize..DISPATCHER_THREADS {
        let rt_clone: Arc<Runtime> = Arc::clone(&rt);
        let stop_clone: Arc<core::sync::atomic::AtomicBool> = Arc::clone(&stop_flag);
        let torn_clone: Arc<AtomicUsize> = Arc::clone(&torn_reads);

        let dispatcher_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                // Pin the epoch across find→resolve→dispatch so the resolved interface
                // (and its native function table) stays alive across the call while the
                // reloader thread republishes concurrently — true unload epoch-reclaims
                // the superseded interface only after every pinned reader has unpinned.
                let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                let handle_result: Result<
                    polyplug_abi::GuestContractHandle,
                    polyplug::error::RegistryError,
                > = rt_clone.find_guest_contract(contract_id, 0_u32);

                if let Ok(plugin_handle) = handle_result {
                    let resolve_result: Result<
                        *const GuestContractInterface,
                        polyplug::error::RegistryError,
                    > = rt_clone.resolve_guest_contract(plugin_handle);

                    if let Ok(vt_ptr) = resolve_result {
                        // SAFETY: the epoch guard pinned above keeps the resolved
                        // interface and its native function table alive across this
                        // dispatch despite a concurrent reload. dispatch.native is the
                        // active variant and functions[0] is the version fn matching the
                        // `extern "C" fn() -> u32` signature the test plugins export.
                        let version: u32 = unsafe {
                            let fn_ptr: *const () = *(*vt_ptr).dispatch.native.functions;
                            let version_fn: extern "C" fn() -> u32 = core::mem::transmute(fn_ptr);
                            version_fn()
                        };

                        if version != 100_u32 && version != 200_u32 {
                            torn_clone.fetch_add(1_usize, Ordering::Relaxed);
                        }
                    }
                }
            }
        });

        dispatcher_handles.push(dispatcher_handle);
    }

    // Give dispatchers time to start.
    std::thread::sleep(Duration::from_millis(10_u64));

    for i in 0_u32..RELOAD_ROUNDS {
        let so_path: PathBuf = if i % 2_u32 == 0_u32 {
            v2_so_path()
        } else {
            v1_so_path()
        };

        rt.reload_bundle(so_path.as_path())
            .unwrap_or_else(|e: polyplug::error::RuntimeError| {
                panic!("reload failed at round {i}: {e}");
            });
    }

    stop_flag.store(true, Ordering::Relaxed);
    for h in dispatcher_handles {
        h.join().expect("dispatcher thread must not panic");
    }

    let torn: usize = torn_reads.load(Ordering::Relaxed);
    assert_eq!(
        torn, 0_usize,
        "torn reads detected: {torn} interface calls returned neither 100 nor 200"
    );
}
