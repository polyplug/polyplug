#![allow(clippy::expect_used)]

//! Concurrent reload of the same bundle — the post-quiesce resolvability
//! invariant and reload-critical-section mutual exclusion.
//!
//! Hot-reload uses a callback-based model:
//! - `Preparing` fires before a reload (host destroys instances)
//! - `Reloaded`/`Failed` fires after (host re-creates instances)
//!
//! These tests cover rapid alternating reload cycles on a single runtime, direct
//! interface swaps via the registry write guard, per-cycle reload callbacks, and
//! — most importantly — the deterministic proof that two reloads of the same
//! bundle never overlap their critical sections (the root cause of the historical
//! concurrent-reload flake).

use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex, MutexGuard};
use std::thread;
use std::thread::JoinHandle;

use crossbeam_epoch::{Guard, pin as epoch_pin};

use polyplug::error::{RegistryError, RuntimeError};
use polyplug::runtime::Runtime;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::runtime::{ReloadPhase, RuntimeConfig};
use polyplug_abi::{
    GuestContractHandle, GuestContractInterface, PluginDescriptor, ReloadPhaseType, StringView,
    Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

use crate::common::TestNativeLoader;
use crate::fixtures::{
    RELOAD_V1_DIR, hot_reload_config, make_hot_reload_runtime, resolve_version_fn, v1_so_path,
    v2_so_path,
};

// ─── Mock interfaces for the direct-swap tests ────────────────────────────────

static INTERFACE_V1: GuestContractInterface = make_interface!(
    GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    Version {
        major: 1,
        minor: 0,
        patch: 0,
    }
);

static INTERFACE_V2: GuestContractInterface = make_interface!(
    GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    Version {
        major: 2,
        minor: 0,
        patch: 0,
    }
);

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

// ─── Direct interface swap: pointer changes after swap ────────────────────────

/// Verifies that swap_interface directly swaps the interface and the new interface
/// is returned by resolve after the swap.
#[test]
fn test_swap_interface_changes_interface_pointer() {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_test_plugin", "swap.test.contract");

    // SAFETY: INTERFACE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "swap.test.contract".to_owned(),
            BundleId::from_u64(2_u64),
        )
    }
    .expect("registration should succeed");

    // Pin the epoch for the whole resolve→swap→deref sequence. The registry copies
    // each registered interface into an Arc, so a pointer resolved before the swap
    // points into the Arc the swap supersedes; holding this guard keeps that Arc alive
    // (epoch reclamation cannot run while a reader is pinned), so the pre-swap pointer
    // stays valid across the swap and its deref below.
    let _epoch_guard: Guard = epoch_pin();

    // The handle should be valid before the swap.
    let resolve_result_before: Result<*const GuestContractInterface, _> =
        registry.resolve_guest_contract(handle);
    assert!(
        resolve_result_before.is_ok(),
        "handle should be valid before swap"
    );

    let interface_ptr_before: *const GuestContractInterface =
        resolve_result_before.expect("resolve before swap should succeed");
    // SAFETY: the epoch guard pinned above keeps the pre-swap interface Arc alive across
    // this deref even though the swap below supersedes it.
    let version_before: &Version = unsafe { &(*interface_ptr_before).contract_version };
    assert_eq!(
        version_before.major, 1,
        "before swap: should have version 1"
    );

    // Perform the swap - direct swap_interface
    let new_arc: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
    registry
        .swap_guest_contract_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // The same handle should now resolve to INTERFACE_V2.
    let resolve_result_after: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve_guest_contract(handle);

    // With the new model (no generation), the handle should still be valid after swap
    assert!(
        resolve_result_after.is_ok(),
        "handle should still be valid after swap (no generation tracking)"
    );

    let interface_ptr_after: *const GuestContractInterface =
        resolve_result_after.expect("resolve after swap should succeed");
    // SAFETY: interface_ptr_after points at the live post-swap interface Arc, kept alive
    // by the epoch guard pinned above.
    let version_after: &Version = unsafe { &(*interface_ptr_after).contract_version };
    assert_eq!(version_after.major, 2, "after swap: should have version 2");
}

/// Verifies that swap_interface directly swaps the interface under RwLock write guard.
#[test]
fn test_direct_swap_interface() {
    let registry: RuntimeStore = RuntimeStore::new();
    let descriptor: PluginDescriptor = make_descriptor("swap_plugin", "swap.direct.contract");

    // SAFETY: INTERFACE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "swap.direct.contract".to_owned(),
            BundleId::from_u64(3_u64),
        )
    }
    .expect("registration should succeed");

    // Pin the epoch across resolve→swap→deref so the pre-swap interface Arc the
    // registry copied stays alive across the swap that supersedes it.
    let _epoch_guard: Guard = epoch_pin();

    // Resolve before swap
    let interface_ptr_before: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve should succeed before swap");

    // SAFETY: the epoch guard pinned above keeps the pre-swap interface Arc alive
    // across this deref.
    let version_before: &Version = unsafe { &(*interface_ptr_before).contract_version };
    assert_eq!(version_before.major, 1, "before swap: V1");

    // Perform direct swap
    let new_arc: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
    registry
        .swap_guest_contract_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // Resolve after swap - the handle should still be valid
    let interface_ptr_after: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve should succeed after swap");

    // SAFETY: interface_ptr_after points at the live post-swap interface Arc, kept
    // alive by the epoch guard pinned above.
    let version_after: &Version = unsafe { &(*interface_ptr_after).contract_version };
    assert_eq!(version_after.major, 2, "after swap: V2");
}

// ─── Rapid reload cycles on a single runtime ──────────────────────────────────

/// Rapid reload cycles: 100+ alternating v1/v2 reloads on a single Runtime.
///
/// Verifies that the interface is consistent after every reload and that the
/// runtime does not panic or leak library handles across iterations.
#[test]
fn stress_rapid_reload_cycles_100() {
    const CYCLES: u32 = 100_u32;

    let rt: Arc<Runtime> = make_hot_reload_runtime();
    rt.load_bundle(Path::new(RELOAD_V1_DIR)).expect("load v1");

    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();

    for i in 0_u32..CYCLES {
        let so_path: PathBuf = if i % 2_u32 == 0_u32 {
            v2_so_path()
        } else {
            v1_so_path()
        };

        rt.reload_bundle(so_path.as_path())
            .unwrap_or_else(|e: RuntimeError| {
                panic!("reload failed at cycle {i}: {e}");
            });

        let version_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
            .unwrap_or_else(|| {
                panic!("interface not resolvable after reload at cycle {i}");
            });

        let version: u32 = version_fn();
        // Cycle 0 -> v2 (200), cycle 1 -> v1 (100), ...
        let expected: u32 = if i % 2_u32 == 0_u32 { 200_u32 } else { 100_u32 };
        assert_eq!(
            version, expected,
            "cycle {i}: expected version {expected}, got {version}"
        );
    }

    // 100 cycles: last reload (cycle 99, odd) used v1_so_path -> expects 100.
    let final_fn: extern "C" fn() -> u32 =
        resolve_version_fn(&rt, contract_id).expect("final interface resolution must succeed");
    assert_eq!(
        final_fn(),
        100_u32,
        "after 100 cycles (last = v1) version must be 100"
    );
}

/// Memory tracking during reload: Direct swap_interface swaps interfaces.
#[test]
fn stress_memory_interface_swap_cycles() {
    const CYCLES: usize = 50_usize;

    let registry: RuntimeStore = RuntimeStore::new();

    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"stress-mem-plugin"),
        contract_name: StringView::from_static(b"stress.mem.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };

    // SAFETY: INTERFACE_V1 is 'static and valid for the lifetime of this test.
    let handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_V1,
                "stress.mem.contract".to_owned(),
                BundleId::from_u64(0xDEAD_BEEF_0000_0001_u64),
            )
            .expect("register must succeed")
    };

    for cycle in 0_usize..CYCLES {
        let new_interface: &'static GuestContractInterface = if cycle % 2_usize == 0_usize {
            &INTERFACE_V2
        } else {
            &INTERFACE_V1
        };

        let new_arc: Arc<GuestContractInterface> = Arc::new(*new_interface);
        registry
            .swap_guest_contract_interface(handle.index, new_arc)
            .unwrap_or_else(|e| panic!("swap_interface failed at cycle {cycle}: {e}"));
    }
}

/// Reload callback fires on every cycle and records the sequence of events.
/// Verifies that the bundle metadata are correct for all 100 reload events.
#[test]
fn stress_reload_callback_fires_on_every_cycle() {
    const CYCLES: u32 = 100_u32;

    let events: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&events);

    let rt: Arc<Runtime> = Runtime::builder()
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .loader(TestNativeLoader::new())
        .on_reload(move |_user_data: *mut c_void, ev: ReloadPhase| {
            events_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(ev);
        })
        .build()
        .expect("build runtime");

    rt.load_bundle(Path::new(RELOAD_V1_DIR)).expect("load v1");

    for i in 0_u32..CYCLES {
        let so_path: PathBuf = if i % 2_u32 == 0_u32 {
            v2_so_path()
        } else {
            v1_so_path()
        };

        rt.reload_bundle(so_path.as_path())
            .unwrap_or_else(|e: RuntimeError| {
                panic!("reload failed at cycle {i}: {e}");
            });
    }

    let recorded_events: MutexGuard<'_, Vec<ReloadPhase>> =
        events.lock().unwrap_or_else(|e| e.into_inner());

    // Count only Reloaded events (Preparing fires before each attempt)
    let reloaded_count: usize = recorded_events
        .iter()
        .filter(|ev| ev.phase_type == ReloadPhaseType::Reloaded)
        .count();

    assert_eq!(
        reloaded_count as u32, CYCLES,
        "expected {CYCLES} Reloaded callbacks, got {}",
        reloaded_count
    );

    for (idx, ev) in recorded_events.iter().enumerate() {
        if ev.phase_type == ReloadPhaseType::Reloaded {
            assert!(
                !(ev.bundle_name.ptr.is_null() || ev.bundle_name.len == 0),
                "event {idx}: bundle_name must not be empty"
            );
        }
    }
}

// ─── Concurrent reloads of the same bundle ────────────────────────────────────

/// Concurrent reloaders: two threads alternate reloading the same plugin.
/// Both may succeed or one may get a transient error -- but neither must panic
/// and the final interface must be valid and callable.
#[test]
fn stress_concurrent_reload_threads_no_panic() {
    const ROUNDS_PER_THREAD: u32 = 40_u32;

    let rt: Arc<Runtime> = make_hot_reload_runtime();
    rt.load_bundle(Path::new(RELOAD_V1_DIR)).expect("load v1");

    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();
    let rt_a: Arc<Runtime> = Arc::clone(&rt);
    let rt_b: Arc<Runtime> = Arc::clone(&rt);

    let reloader_a: JoinHandle<()> = thread::spawn(move || {
        for i in 0_u32..ROUNDS_PER_THREAD {
            let so_path: PathBuf = if i % 2_u32 == 0_u32 {
                v2_so_path()
            } else {
                v1_so_path()
            };
            // Ignore errors -- concurrent reloads may race; what matters is no panic.
            let _: Result<(), RuntimeError> = rt_a.reload_bundle(so_path.as_path());
        }
    });

    let reloader_b: JoinHandle<()> = thread::spawn(move || {
        for i in 0_u32..ROUNDS_PER_THREAD {
            let so_path: PathBuf = if i % 2_u32 == 0_u32 {
                v1_so_path()
            } else {
                v2_so_path()
            };
            let _: Result<(), RuntimeError> = rt_b.reload_bundle(so_path.as_path());
        }
    });

    reloader_a.join().expect("reloader_a must not panic");
    reloader_b.join().expect("reloader_b must not panic");

    // Final interface must still be callable.
    let final_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
        .expect("interface must be resolvable after concurrent reloads");
    let version: u32 = final_fn();
    assert!(
        version == 100_u32 || version == 200_u32,
        "final version must be 100 or 200, got {version}"
    );
}

/// Deterministic enforcement that concurrent reloads of the same bundle are
/// mutually exclusive — the invariant the `reload_serialize` mutex establishes
/// and the root cause of the historical concurrent-reload flake.
///
/// A reload's pre-reload slot snapshot and the `apply_reload_swap` that consumes
/// it straddle `loader.reload()`; the registry lock is dropped between them. Two
/// reloads of the same bundle that overlap in that window can leave one reload
/// with a stale snapshot, whose swap then finds no freshly-registered slot for a
/// contract the other reload already consumed, tears down that contract's only live
/// slot and removes it from the find index — leaving a
/// contract BOTH versions provide intermittently unresolvable.
///
/// The reload callback fires `Preparing` at the start of a reload's critical
/// section and `Reloaded`/`Failed` at its end, both *inside* `reload_serialize`.
/// We use that bracket as a probe with no production instrumentation: a shared
/// counter is incremented on `Preparing` and decremented on `Reloaded`/`Failed`.
/// The fixture bundle has no cascade dependents, so the callbacks bracket 1:1
/// with no same-thread nesting. If the counter ever exceeds 1, two reloads were
/// in their critical sections at once — the precise corrupting interleave.
///
/// With the mutex the counter never exceeds 1 (deterministic: a thread cannot
/// fire `Preparing` until the previous reload fired `Reloaded`/`Failed` and
/// released the lock) and the contract is resolvable after every reload. Without
/// it, the counter reaches 2 under this barrier-synchronized contention and the
/// per-round resolution intermittently fails.
#[test]
fn concurrent_reloads_are_mutually_exclusive() {
    const THREADS: usize = 4_usize;
    const ROUNDS_PER_THREAD: u32 = 50_u32;

    let in_section: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let max_seen: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0_usize));
    let overlap: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let cb_in: Arc<AtomicUsize> = Arc::clone(&in_section);
    let cb_max: Arc<AtomicUsize> = Arc::clone(&max_seen);
    let cb_overlap: Arc<AtomicBool> = Arc::clone(&overlap);

    let rt: Arc<Runtime> = Runtime::builder()
        .config(hot_reload_config())
        .loader(TestNativeLoader::new())
        .on_reload(move |_ud: *mut c_void, phase: ReloadPhase| {
            match phase.phase_type {
                ReloadPhaseType::Preparing => {
                    let now: usize = cb_in.fetch_add(1_usize, Ordering::SeqCst) + 1_usize;
                    if now > 1_usize {
                        cb_overlap.store(true, Ordering::SeqCst);
                    }
                    cb_max.fetch_max(now, Ordering::SeqCst);
                }
                ReloadPhaseType::Reloaded | ReloadPhaseType::Failed => {
                    // Saturating: the manifest-validate and missing-file fast-fail
                    // paths fire `Failed` without a preceding `Preparing`; never
                    // underflow the counter (those paths do not occur here, but the
                    // probe must stay sound regardless).
                    let _: Result<usize, usize> =
                        cb_in.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v: usize| {
                            Some(v.saturating_sub(1_usize))
                        });
                }
                _ => {}
            }
        })
        .build()
        .expect("runtime build must succeed");

    rt.load_bundle(Path::new(RELOAD_V1_DIR)).expect("load v1");
    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();

    let barrier: Arc<Barrier> = Arc::new(Barrier::new(THREADS));
    let mut handles: Vec<JoinHandle<()>> = Vec::with_capacity(THREADS);
    for t in 0_usize..THREADS {
        let rt_t: Arc<Runtime> = Arc::clone(&rt);
        let bar: Arc<Barrier> = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            bar.wait();
            for i in 0_u32..ROUNDS_PER_THREAD {
                let so_path: PathBuf = if (i as usize + t) % 2_usize == 0_usize {
                    v2_so_path()
                } else {
                    v1_so_path()
                };
                let _: Result<(), RuntimeError> = rt_t.reload_bundle(so_path.as_path());
                // The contract is provided by BOTH versions, so under correct
                // serialization the old slot is never removed from the find index
                // (only swapped in place). It must therefore resolve after every
                // reload, even while another thread is mid-reload.
                assert!(
                    resolve_version_fn(&rt_t, contract_id).is_some(),
                    "thread {t} round {i}: contract must stay resolvable across concurrent reloads"
                );
            }
        }));
    }
    for handle in handles {
        handle.join().expect("reloader thread must not panic");
    }

    assert_eq!(
        in_section.load(Ordering::SeqCst),
        0_usize,
        "every reload critical section must have exited"
    );
    assert!(
        !overlap.load(Ordering::SeqCst),
        "two reloads were in their critical sections at once (max concurrency = {}) — reload serialization is broken",
        max_seen.load(Ordering::SeqCst)
    );

    let final_fn: extern "C" fn() -> u32 =
        resolve_version_fn(&rt, contract_id).expect("final interface must resolve");
    let version: u32 = final_fn();
    assert!(
        version == 100_u32 || version == 200_u32,
        "final version must be 100 or 200, got {version}"
    );
}
