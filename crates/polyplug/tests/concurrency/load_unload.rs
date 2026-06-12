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
