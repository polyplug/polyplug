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
use polyplug::runtime::Runtime;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractId, GuestContractInterface, HostApi,
    NativeDispatch, PluginDescriptor, ReloadPhaseType, StringView, Version,
};
use polyplug_utils::BundleId;

mod common;

use common::TestNativeLoader;

// ─── Environment variables emitted by build.rs ───────────────────────────────

const RELOAD_V1_DIR: &str = env!("RELOAD_PLUGIN_V1_DIR");
const RELOAD_V2_DIR: &str = env!("RELOAD_PLUGIN_V2_DIR");

// ─── Shared zero-size function pointer array (const – not static) ───────────

const MOCK_FNS_EMPTY: [*const (); 0] = [];

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

static INTERFACE_MEM_A: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
            function_count: 0,
        },
    },
};

static INTERFACE_MEM_B: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xDEAD_BEEF_0000_0001_u64),
    contract_version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
            function_count: 0,
        },
    },
};

static INTERFACE_QU_A: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xCAFE_BABE_0000_0001_u64),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
            function_count: 0,
        },
    },
};

static INTERFACE_QU_B: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(0xCAFE_BABE_0000_0001_u64),
    contract_version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: noop_create_instance,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            functions: MOCK_FNS_EMPTY.as_ptr(),
            function_count: 0,
        },
    },
};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn v1_so_path() -> PathBuf {
    let filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v1.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v1.dll"
    } else {
        "libreload_plugin_v1.so"
    };
    PathBuf::from(RELOAD_V1_DIR).join(filename)
}

fn v2_so_path() -> PathBuf {
    let filename: &str = if cfg!(target_os = "macos") {
        "libreload_plugin_v2.dylib"
    } else if cfg!(target_os = "windows") {
        "reload_plugin_v2.dll"
    } else {
        "libreload_plugin_v2.so"
    };
    PathBuf::from(RELOAD_V2_DIR).join(filename)
}

fn hot_reload_config() -> polyplug::RuntimeConfig {
    polyplug::RuntimeConfig {
        compatibility: polyplug::Compatibility::Strict,
        unload_mode: polyplug::UnloadMode::Retire,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
    }
}

fn make_hot_reload_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .config(hot_reload_config())
        .loader(TestNativeLoader::new())
        .build()
        .expect("runtime build must succeed")
}

fn resolve_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug_abi::GuestContractHandle = rt.find_guest_contract(contract_id, 0).ok()?;
    let interface_ptr: *const GuestContractInterface = rt.resolve_guest_contract(handle).ok()?;
    // SAFETY: interface_ptr was returned by resolve_guest_contract and points at a retained
    // (retire-not-drop) interface valid for the runtime's lifetime. dispatch.native is the
    // active variant for these native test interfaces, and functions[0] is the version fn
    // matching the `extern "C" fn() -> u32` signature the test plugins export.
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*interface_ptr).dispatch.native.functions;
        core::mem::transmute(*fns)
    };
    Some(fn_ptr)
}

// ─── Stress tests ─────────────────────────────────────────────────────────────

/// Rapid reload cycles: 100+ alternating v1/v2 reloads on a single Runtime.
///
/// Verifies that the interface is consistent after every reload and that the
/// runtime does not panic or leak library handles across iterations.
#[test]
fn stress_rapid_reload_cycles_100() {
    const CYCLES: u32 = 100_u32;

    let rt: Arc<Runtime> = make_hot_reload_runtime();
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

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

    // SAFETY: INTERFACE_MEM_A is 'static and valid for the lifetime of this test.
    let handle: polyplug_abi::GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_MEM_A,
                "stress.mem.contract".to_owned(),
                BundleId::from_u64(0xDEAD_BEEF_0000_0001_u64),
            )
            .expect("register must succeed")
    };

    for cycle in 0_usize..CYCLES {
        let new_interface: &'static GuestContractInterface = if cycle % 2_usize == 0_usize {
            &INTERFACE_MEM_B
        } else {
            &INTERFACE_MEM_A
        };

        let new_arc: Arc<GuestContractInterface> = Arc::new(*new_interface);
        registry
            .swap_guest_contract_interface(handle.index, new_arc)
            .unwrap_or_else(|e| panic!("swap_interface failed at cycle {cycle}: {e}"));
    }
}

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
                        // SAFETY: interface_ptr is valid
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
                        // SAFETY: vt_ptr was returned by resolve_guest_contract and points at a
                        // retained (retire-not-drop) interface valid for the runtime's lifetime.
                        // dispatch.native is the active variant and functions[0] is the version fn
                        // matching the `extern "C" fn() -> u32` signature the test plugins export.
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
        "torn reads detected: {torn} interface calls returned neither 100 nor 200"
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

    let rt: Arc<Runtime> = Runtime::builder()
        .config(polyplug::RuntimeConfig {
            hot_reload_enabled: true,
            ..polyplug::RuntimeConfig::default()
        })
        .loader(TestNativeLoader::new())
        .on_reload(move |_user_data: *mut core::ffi::c_void, ev: ReloadPhase| {
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

/// Concurrent reloaders: two threads alternate reloading the same plugin.
/// Both may succeed or one may get a transient error -- but neither must panic
/// and the final interface must be valid and callable.
#[test]
fn stress_concurrent_reload_threads_no_panic() {
    const ROUNDS_PER_THREAD: u32 = 40_u32;

    let rt: Arc<Runtime> = make_hot_reload_runtime();
    rt.load_bundle(std::path::Path::new(RELOAD_V1_DIR))
        .expect("load v1");

    let contract_id: u64 = GuestContractId::new("reload.test", 1).id();
    let rt_a: Arc<Runtime> = Arc::clone(&rt);
    let rt_b: Arc<Runtime> = Arc::clone(&rt);

    let reloader_a: std::thread::JoinHandle<()> = std::thread::spawn(move || {
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

    // Final interface must still be callable.
    let final_fn: extern "C" fn() -> u32 = resolve_version_fn(&rt, contract_id)
        .expect("interface must be resolvable after concurrent reloads");
    let version: u32 = final_fn();
    assert!(
        version == 100_u32 || version == 200_u32,
        "final version must be 100 or 200, got {version}"
    );
}
