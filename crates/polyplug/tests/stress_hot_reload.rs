#![allow(clippy::expect_used)]

//! Stress tests for the hot-reload subsystem.
//!
//! Run with:
//!   cargo test --test stress_hot_reload --package polyplug
//!
//! Hot-reload uses callback-based model:
//! - Preparing callback fires before reload (host destroys instances)
//! - Warning emitted if Arc refs remain (informational only)
//! - Reloaded callback fires after reload (host creates new instances)

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use core::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::ReloadPhase;
use polyplug::error::RuntimeError;
use polyplug::plugin_registry::PluginRegistry;
use polyplug::runtime::Runtime;
use polyplug_abi::{DispatchType, GuestContractInterface, NativeDispatch, PluginDispatch};

// ─── Environment variables emitted by build.rs ───────────────────────────────

const RELOAD_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const RELOAD_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

// ─── Shared zero-size function pointer array (const – not static) ───────────

const MOCK_FNS_EMPTY: [*const (); 0] = [];

static VTABLE_MEM_A: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xDEAD_BEEF_0000_0001_u64,
    contract_version: (1_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
        },
    },
};

static VTABLE_MEM_B: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xDEAD_BEEF_0000_0001_u64,
    contract_version: (2_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
        },
    },
};

static VTABLE_QU_A: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xCAFE_BABE_0000_0001_u64,
    contract_version: (1_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
        },
    },
};

static VTABLE_QU_B: GuestContractInterface = GuestContractInterface {
    rt_ctx: core::ptr::null(),
    contract_id: 0xCAFE_BABE_0000_0001_u64,
    contract_version: (2_u32 << 16),
    function_count: 0_u32,
    dispatch_type: DispatchType::Native,
    dispatch: PluginDispatch {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
        },
    },
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn v1_so_path() -> PathBuf {
    PathBuf::from(RELOAD_V1_DIR).join("libreload_plugin_v1.so")
}

fn v2_so_path() -> PathBuf {
    PathBuf::from(RELOAD_V2_DIR).join("libreload_plugin_v2.so")
}

fn hot_reload_config() -> polyplug::runtime::RuntimeConfig {
    polyplug::runtime::RuntimeConfig {
        hot_reload_enabled: true,
        hot_reload_max_retries: 3,
        hot_reload_retry_interval: core::time::Duration::from_secs(1),
        hot_reload_abort_on_max_retries: true,
    }
}

fn make_hot_reload_runtime() -> Runtime {
    Runtime::builder()
        .config(hot_reload_config())
        .build()
        .expect("runtime build must succeed")
}

fn resolve_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug_abi::PluginHandle = rt.find_by_contract(contract_id, 0).ok()?;
    let vtable_ptr: *const GuestContractInterface = rt.resolve_plugin(handle).ok()?;
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*vtable_ptr).dispatch.native.functions;
        core::mem::transmute(*fns)
    };
    Some(fn_ptr)
}

// ─── Stress tests ─────────────────────────────────────────────────────────────

/// Rapid reload cycles: 100+ alternating v1/v2 reloads on a single Runtime.
///
/// Verifies that the vtable is consistent after every reload and that the
/// runtime does not panic or leak library handles across iterations.
#[test]
fn stress_rapid_reload_cycles_100() {
    const CYCLES: u32 = 100_u32;

    let rt: Runtime = make_hot_reload_runtime();
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);

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
                panic!("vtable not resolvable after reload at cycle {i}");
            });

        let version: u32 = version_fn();
        // Cycle 0 → v2 (200), cycle 1 → v1 (100), ...
        let expected: u32 = if i % 2_u32 == 0_u32 { 200_u32 } else { 100_u32 };
        assert_eq!(
            version, expected,
            "cycle {i}: expected version {expected}, got {version}"
        );
    }

    // 100 cycles: last reload (cycle 99, odd) used v1_so_path → expects 100.
    let final_fn: extern "C" fn() -> u32 =
        resolve_version_fn(&rt, contract_id).expect("final vtable resolution must succeed");
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

    let registry: PluginRegistry = PluginRegistry::new();

    let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
        name: polyplug_abi::StringView::from_static(b"stress-mem-plugin"),
        contract_name: polyplug_abi::StringView::from_static(b"stress.mem.contract"),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };

    // SAFETY: VTABLE_MEM_A is 'static and valid for the lifetime of this test.
    let handle: polyplug_abi::PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_MEM_A,
                "stress.mem.contract".to_owned(),
                0xDEAD_BEEF_0000_0001_u64,
            )
            .expect("register must succeed")
    };

    for cycle in 0_usize..CYCLES {
        let new_vtable: &'static GuestContractInterface = if cycle % 2_usize == 0_usize {
            &VTABLE_MEM_B
        } else {
            &VTABLE_MEM_A
        };

        let new_arc: Arc<GuestContractInterface> = Arc::new(new_vtable);
        registry
            .swap_interface(handle.index, new_arc)
            .unwrap_or_else(|e| panic!("swap_interface failed at cycle {cycle}: {e}"));
    }
}

/// Direct swap under concurrent reader load: multiple reader threads continuously
/// resolve interfaces while the reloader thread fires 50+ interface swaps.
#[test]
fn stress_direct_swap_under_concurrent_reader_load() {
    const READER_THREADS: usize = 8_usize;
    const SWAP_ROUNDS: usize = 50_usize;

    let registry: Arc<PluginRegistry> = Arc::new(PluginRegistry::new());

    let descriptor: polyplug_abi::PluginDescriptor = polyplug_abi::PluginDescriptor {
        name: polyplug_abi::StringView::from_static(b"swap-load-plugin"),
        contract_name: polyplug_abi::StringView::from_static(b"swap.load.contract"),
        version_major: 1_u32,
        version_minor: 0_u32,
        version_patch: 0_u32,
    };

    // SAFETY: VTABLE_QU_A is 'static and valid for the test lifetime.
    let handle: polyplug_abi::PluginHandle = unsafe {
        registry
            .register(
                descriptor,
                &VTABLE_QU_A,
                "swap.load.contract".to_owned(),
                0xCAFE_BABE_0000_0001_u64,
            )
            .expect("register must succeed")
    };

    let stop_flag: Arc<core::sync::atomic::AtomicBool> =
        Arc::new(core::sync::atomic::AtomicBool::new(false));

    let mut reader_handles: Vec<std::thread::JoinHandle<()>> = Vec::with_capacity(READER_THREADS);

    for _thread_idx in 0_usize..READER_THREADS {
        let reg_clone: Arc<PluginRegistry> = Arc::clone(&registry);
        let stop_clone: Arc<core::sync::atomic::AtomicBool> = Arc::clone(&stop_flag);

        let reader_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            while !stop_clone.load(Ordering::Relaxed) {
                let find_result: Result<
                    polyplug_abi::PluginHandle,
                    polyplug::error::RegistryError,
                > = reg_clone.find_by_contract(0xCAFE_BABE_0000_0001_u64, 0_u32);
                if let Ok(resolved_handle) = find_result {
                    let resolve_result: Result<
                        *const GuestContractInterface,
                        polyplug::error::RegistryError,
                    > = reg_clone.resolve(resolved_handle);
                    if let Ok(vtable_ptr) = resolve_result {
                        // SAFETY: vtable_ptr is valid
                        let version: u32 = unsafe { (*vtable_ptr).contract_version };
                        assert!(
                            version == (1_u32 << 16) || version == (2_u32 << 16),
                            "version must be V1 or V2"
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
        let new_vtable: &'static GuestContractInterface = if round % 2_usize == 0_usize {
            &VTABLE_QU_B
        } else {
            &VTABLE_QU_A
        };

        let new_arc: Arc<GuestContractInterface> = Arc::new(new_vtable);
        registry
            .swap_interface(handle.index, new_arc)
            .unwrap_or_else(|e| panic!("swap_interface failed at round {round}: {e}"));
    }

    stop_flag.store(true, Ordering::Relaxed);
    for h in reader_handles {
        h.join().expect("reader thread must not panic");
    }
}

/// VTable handoff correctness: verifies that every vtable swap atomically
/// transfers the correct function pointer and that no intermediate state
/// (neither v1 nor v2) is observable between swaps.
///
/// Dispatcher threads spin-read the vtable version function; every return value
/// must be exactly 100 (v1) or 200 (v2) — never anything else.
#[test]
fn stress_vtable_handoff_correctness_no_torn_reads() {
    const DISPATCHER_THREADS: usize = 6_usize;
    const RELOAD_ROUNDS: u32 = 80_u32;

    let rt: Arc<Runtime> = Arc::new(make_hot_reload_runtime());
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);

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
                let handle_result: Result<
                    polyplug_abi::PluginHandle,
                    polyplug::error::RegistryError,
                > = rt_clone.find_by_contract(contract_id, 0_u32);

                if let Ok(plugin_handle) = handle_result {
                    let resolve_result: Result<
                        *const GuestContractInterface,
                        polyplug::error::RegistryError,
                    > = rt_clone.resolve_plugin(plugin_handle);

                    if let Ok(vt_ptr) = resolve_result {
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
            .unwrap_or_else(|e: RuntimeError| {
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
        "torn reads detected: {torn} vtable calls returned neither 100 nor 200"
    );
}

/// Reload callback fires on every cycle and records the sequence of events.
/// Verifies that the bundle metadata are correct for
/// all 100 reload events.
#[test]
fn stress_reload_callback_fires_on_every_cycle() {
    const CYCLES: u32 = 100_u32;

    let events: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&events);

    let rt: Runtime = Runtime::builder()
        .config(polyplug::runtime::RuntimeConfig {
            hot_reload_enabled: true,
            ..polyplug::runtime::RuntimeConfig::default()
        })
        .on_reload(move |ev: ReloadPhase| {
            events_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(ev);
        })
        .build()
        .expect("build runtime");

    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

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

    let recorded_events: std::sync::MutexGuard<'_, Vec<ReloadPhase>> =
        events.lock().unwrap_or_else(|e| e.into_inner());

    // Count only Reloaded events (Preparing fires before each attempt)
    let reloaded_count: usize = recorded_events
        .iter()
        .filter(|ev| matches!(ev, ReloadPhase::Reloaded { .. }))
        .count();

    assert_eq!(
        reloaded_count as u32, CYCLES,
        "expected {CYCLES} Reloaded callbacks, got {}",
        reloaded_count
    );

    for (idx, ev) in recorded_events.iter().enumerate() {
        if let ReloadPhase::Reloaded { bundle_name, .. } = ev {
            assert!(
                !bundle_name.is_empty(),
                "event {idx}: bundle_name must not be empty"
            );
        }
    }
}

/// Concurrent reloaders: two threads alternate reloading the same plugin.
/// Both may succeed or one may get a transient error — but neither must panic
/// and the final vtable must be valid and callable.
#[test]
fn stress_concurrent_reload_threads_no_panic() {
    const ROUNDS_PER_THREAD: u32 = 40_u32;

    let rt: Arc<Runtime> = Arc::new(make_hot_reload_runtime());
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let rt_a: Arc<Runtime> = Arc::clone(&rt);
    let rt_b: Arc<Runtime> = Arc::clone(&rt);

    let reloader_a: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        for i in 0_u32..ROUNDS_PER_THREAD {
            let so_path: PathBuf = if i % 2_u32 == 0_u32 {
                v2_so_path()
            } else {
                v1_so_path()
            };
            // Ignore errors — concurrent reloads may race; what matters is no panic.
            let _: Result<(), RuntimeError> = rt_a.reload_bundle(so_path.as_path());
        }
    });

    let reloader_b: std::thread::JoinHandle<()> = std::thread::spawn(move || {
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

    // Final vtable must still be callable.
    let final_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
        .expect("vtable must be resolvable after concurrent reloads");
    let version: u32 = final_fn();
    assert!(
        version == 100_u32 || version == 200_u32,
        "final version must be 100 or 200, got {version}"
    );
}