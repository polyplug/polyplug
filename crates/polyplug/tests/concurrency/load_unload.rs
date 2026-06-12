#![allow(clippy::expect_used)]

//! Concurrent load + unload of the same and different bundles.
//!
//! The invariant under test is the epoch-reclamation guarantee: a reader that
//! pins the epoch and observes a handle concurrently with an unload must EITHER
//! resolve successfully — in which case the resolved interface stays alive for as
//! long as the reader holds its pin, even though the bundle was invalidated — OR
//! fail cleanly with `StaleHandle`/`PluginNotFound`. It must never produce a
//! use-after-free. Run under ThreadSanitizer (see `.github/workflows/nightly.yml`)
//! to also assert the registry's locking is race-free.

use core::sync::atomic::AtomicBool;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::RegistryError;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{GuestContractHandle, GuestContractInterface, PluginDescriptor, Version};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use crate::fixtures::make_descriptor;

const RESOLVER_THREADS: usize = 6_usize;
const VERSION_V1: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};

const UNLOAD_CONTRACT_ID: u64 = 0x7171_0000_0000_3000_u64;

static INTERFACE_UNLOAD: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(UNLOAD_CONTRACT_ID), VERSION_V1);

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

// ─── Multi-writer load/unload churn ───────────────────────────────────────────

const MW_CONTRACT_ID: u64 = 0x7171_0000_0000_6000_u64;
const MW_BUNDLE_ID: u64 = 0xABCD_0000_0000_0006_u64;

static INTERFACE_MW: GuestContractInterface =
    make_interface!(GuestContractId::from_u64(MW_CONTRACT_ID), VERSION_V1);

/// Several writer threads concurrently churn the SAME bundle — each loops
/// `invalidate` then `register` — while resolver threads read. This drives the
/// retire/reclaim path under genuine multi-writer contention (the single-writer
/// `stress_concurrent_unload_with_resolvers` only ever has one thread mutating).
///
/// Writers tolerate `DuplicateProvider` (another writer re-registered first) and a
/// zero-removed `invalidate` (another writer already unloaded) — those are
/// expected outcomes of the race, not failures. The invariants are: no writer or
/// resolver panics or hits a use-after-free, every resolved interface that a
/// pinned reader observes is intact (its contract id is correct), and after the
/// churn quiesces the contract is in a consistent resolvable state.
#[test]
fn concurrent_load_unload_same_bundle_multiwriter_no_uaf() {
    const WRITERS: usize = 4_usize;
    const RESOLVERS: usize = 4_usize;
    const ROUNDS_PER_WRITER: usize = 200_usize;

    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    // SAFETY: INTERFACE_MW is 'static, valid for the test lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("mw_plugin", "mw.contract"),
                &INTERFACE_MW,
                "mw.contract".to_owned(),
                BundleId::from_u64(MW_BUNDLE_ID),
            )
            .expect("initial register must succeed");
    }

    let ready: Arc<Barrier> = Arc::new(Barrier::new(WRITERS + RESOLVERS));
    let stop: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let resolve_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));

    let mut writer_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(WRITERS);
    for _ in 0_usize..WRITERS {
        let reg: Arc<RuntimeStore> = Arc::clone(&registry);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        writer_handles.push(std::thread::spawn(move || {
            ready_clone.wait();
            for _ in 0_usize..ROUNDS_PER_WRITER {
                // A zero-removed invalidate is fine — another writer beat us to it.
                let _: Result<u32, RegistryError> =
                    reg.invalidate_bundle(BundleId::from_u64(MW_BUNDLE_ID));
                // SAFETY: INTERFACE_MW is 'static, valid for the test lifetime.
                let result: Result<GuestContractHandle, RegistryError> = unsafe {
                    reg.register_guest_contract(
                        make_descriptor("mw_plugin", "mw.contract"),
                        &INTERFACE_MW,
                        "mw.contract".to_owned(),
                        BundleId::from_u64(MW_BUNDLE_ID),
                    )
                };
                // DuplicateProvider means another writer re-registered first — an
                // expected race outcome, not a failure. Anything else is a bug.
                if let Err(err) = result
                    && !matches!(err, RegistryError::DuplicateProvider { .. })
                {
                    panic!("unexpected registry error during churn: {err:?}");
                }
            }
        }));
    }

    let mut resolver_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(RESOLVERS);
    for _ in 0_usize..RESOLVERS {
        let reg: Arc<RuntimeStore> = Arc::clone(&registry);
        let ready_clone: Arc<Barrier> = Arc::clone(&ready);
        let stop_clone: Arc<AtomicBool> = Arc::clone(&stop);
        let counter: Arc<AtomicUsize> = Arc::clone(&resolve_count);
        resolver_handles.push(std::thread::spawn(move || {
            ready_clone.wait();
            while !stop_clone.load(Ordering::Relaxed) {
                // Pin the epoch across find→resolve→deref so a resolved interface
                // stays alive across the deref even as writers invalidate and
                // re-register concurrently — the superseded interface is
                // epoch-reclaimed only after every pinned reader has unpinned.
                let _epoch_guard: crossbeam_epoch::Guard = crossbeam_epoch::pin();
                if let Ok(found) =
                    reg.find_guest_contract(GuestContractId::from_u64(MW_CONTRACT_ID), 0_u32)
                    && let Ok(interface_ptr) = reg.resolve_guest_contract(found)
                {
                    // SAFETY: the epoch guard pinned above keeps the resolved
                    // interface alive across this deref despite concurrent churn.
                    let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
                    assert_eq!(
                        contract_id.id(),
                        MW_CONTRACT_ID,
                        "a resolved interface must always be the churned contract, never torn"
                    );
                    counter.fetch_add(1_usize, Ordering::Relaxed);
                }
            }
        }));
    }

    for handle in writer_handles {
        handle.join().expect("writer thread must not panic");
    }
    stop.store(true, Ordering::Relaxed);
    for handle in resolver_handles {
        handle.join().expect("resolver thread must not panic");
    }

    // After the churn, drive the bundle to a known-present state and assert it
    // resolves — the multi-writer race must leave the registry consistent.
    let _: Result<u32, RegistryError> =
        registry.invalidate_bundle(BundleId::from_u64(MW_BUNDLE_ID));
    // SAFETY: INTERFACE_MW is 'static, valid for the test lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("mw_plugin", "mw.contract"),
                &INTERFACE_MW,
                "mw.contract".to_owned(),
                BundleId::from_u64(MW_BUNDLE_ID),
            )
            .expect("final re-register must succeed");
    }
    assert!(
        registry
            .find_guest_contract(GuestContractId::from_u64(MW_CONTRACT_ID), 0_u32)
            .is_ok(),
        "contract must resolve after multi-writer churn settles"
    );
}
