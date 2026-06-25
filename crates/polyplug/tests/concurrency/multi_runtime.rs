#![allow(clippy::expect_used)]

//! Multi-runtime parallelism (Rule 12: multiple runtimes in one process must
//! stay fully independent).
//!
//! The runtime holds no global or thread-local state — every `RuntimeStore`,
//! loaded bundle, `HostApi`, and configuration is instance-owned. These tests
//! prove that by building many runtimes across threads simultaneously and
//! asserting that nothing one runtime does is observable in another, and that
//! building, using, and destroying runtimes concurrently is panic- and
//! UAF-free.
//!
//! The native loader is used because it is the only loader with no
//! once-per-process interpreter constraint (CPython and the CLR are documented
//! exceptions in CLAUDE.md), so it is the honest surface for an isolation claim.
//!
//! Sustained memory behaviour (flat RSS across load→dispatch→unload→drop cycles)
//! is the dedicated `soak_load_unload` harness's job; here the no-leak angle is
//! the structural one — distinct per-instance `HostApi` pointers and clean
//! concurrent teardown of every runtime built.

use core::ptr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use polyplug::runtime::Runtime;
use polyplug_utils::GuestContractId;

use crate::common::TestNativeLoader;
use crate::fixtures::{
    RELOAD_V1_DIR, hot_reload_config, make_hot_reload_runtime, resolve_version_fn,
};

const RUNTIMES: usize = 8_usize;

/// Two runtimes in the same process must not share a registry: a bundle loaded
/// into one is invisible to the other, and their `HostApi` tables are distinct
/// heap objects.
#[test]
fn two_runtimes_do_not_share_registry_or_host_api() {
    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();

    let rt_loaded: Arc<Runtime> = make_hot_reload_runtime();
    let rt_empty: Arc<Runtime> = make_hot_reload_runtime();

    rt_loaded
        .load_bundle(Path::new(RELOAD_V1_DIR))
        .expect("load into rt_loaded must succeed");

    // The loader runtime resolves the contract...
    assert!(
        resolve_version_fn(&rt_loaded, contract_id).is_some(),
        "rt_loaded must resolve the contract it loaded"
    );
    // ...the other runtime, which loaded nothing, must NOT — proving the
    // registries are per-instance, not a process-global shared one.
    assert!(
        rt_empty.find_guest_contract(contract_id, 0).is_err(),
        "rt_empty must NOT see a contract loaded into a different runtime (Rule 12 isolation)"
    );

    // Each runtime owns its own HostApi (distinct heap addresses).
    assert!(
        !ptr::eq(rt_loaded.host_abi(), rt_empty.host_abi()),
        "each runtime must own a distinct HostApi — no shared global table"
    );
}

/// N runtimes built and loaded concurrently, each in its own thread. Half load
/// the bundle and must resolve it; half load nothing and must NOT see the
/// contract. Asserting both directions under concurrency proves there is no
/// shared registry that a racing load could leak across.
#[test]
fn parallel_runtimes_have_isolated_registries() {
    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(RUNTIMES));

    let handles: Vec<thread::JoinHandle<()>> = (0_usize..RUNTIMES)
        .map(|idx| {
            let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
            thread::spawn(move || {
                // Every thread builds its OWN runtime with its OWN loader.
                let rt: Arc<Runtime> = make_hot_reload_runtime();
                let loads_bundle: bool = idx % 2_usize == 0_usize;

                // Release all threads together so the loads genuinely overlap.
                barrier_clone.wait();

                if loads_bundle {
                    rt.load_bundle(Path::new(RELOAD_V1_DIR))
                        .unwrap_or_else(|e| panic!("thread {idx}: load must succeed: {e}"));
                    let version_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
                        .unwrap_or_else(|| panic!("thread {idx}: loaded contract must resolve"));
                    assert_eq!(
                        version_fn(),
                        100_u32,
                        "thread {idx}: own bundle must resolve to v1 (100)"
                    );
                } else {
                    assert!(
                        rt.find_guest_contract(contract_id, 0).is_err(),
                        "thread {idx}: a runtime that loaded nothing must not observe another \
                         runtime's concurrently-loaded contract"
                    );
                }
                // rt drops here — concurrent teardown of one of N runtimes.
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("runtime thread must not panic");
    }
}

/// Build → load → dispatch → destroy churn from many threads at once. Each
/// thread fully tears down its runtime every iteration (which drops the loader,
/// which `dlclose`s the library). The point is that concurrent runtime lifecycle
/// — construction and destruction overlapping across threads — never panics,
/// deadlocks, or produces a use-after-free; every dispatch sees a correct,
/// isolated interface.
#[test]
fn parallel_runtime_build_use_destroy_churn() {
    const THREADS: usize = 6_usize;
    const CYCLES_PER_THREAD: usize = 8_usize;

    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(THREADS));

    let handles: Vec<thread::JoinHandle<()>> = (0_usize..THREADS)
        .map(|t| {
            let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier_clone.wait();
                for cycle in 0_usize..CYCLES_PER_THREAD {
                    // Fresh runtime + fresh loader every cycle.
                    let rt: Arc<Runtime> = Runtime::builder()
                        .config(hot_reload_config())
                        .loader(TestNativeLoader::new())
                        .build()
                        .unwrap_or_else(|e| panic!("thread {t} cycle {cycle}: build: {e}"));

                    rt.load_bundle(Path::new(RELOAD_V1_DIR))
                        .unwrap_or_else(|e| panic!("thread {t} cycle {cycle}: load: {e}"));

                    let version_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
                        .unwrap_or_else(|| {
                            panic!("thread {t} cycle {cycle}: contract must resolve")
                        });
                    assert_eq!(
                        version_fn(),
                        100_u32,
                        "thread {t} cycle {cycle}: dispatch must return v1 (100)"
                    );
                    // rt dropped at end of scope → full teardown under concurrency.
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("churn thread must not panic");
    }
}
