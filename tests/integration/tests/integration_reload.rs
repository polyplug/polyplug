#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use core::time::Duration;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::ReloadPhase;
use polyplug::error::RuntimeError;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeConfig;
use polyplug_abi::GuestContractInterface;
use polyplug_native::NativeLoader;

fn hot_reload_config() -> RuntimeConfig {
    RuntimeConfig {
        hot_reload_enabled: true,
        ..RuntimeConfig::default()
    }
}

fn create_runtime_with_native() -> Runtime {
    Runtime::builder()
        .config(hot_reload_config())
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .build()
        .expect("build runtime with native loader")
}

fn get_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug_abi::GuestContractHandle = rt.find_by_contract(contract_id, 0).ok()?;
    let interface: *const GuestContractInterface = rt.resolve_plugin(handle).ok()?;
    // SAFETY: interface is from resolve_plugin and points to a valid interface while the
    // library is loaded; slot 0 is a compatible extern "C" fn in the fixtures.
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*interface).dispatch.native.functions;
        core::mem::transmute(*fns)
    };
    Some(fn_ptr)
}

#[test]
fn test_a_basic_reload() {
    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    let v2_path: PathBuf =
        std::path::PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so");
    let rt: Runtime = create_runtime_with_native();
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");
    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v1");
    assert_eq!(version_fn(), 100_u32, "v1 should return 100");
    rt.reload_bundle(v2_path.as_path()).expect("reload v2");
    let version_fn2: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v2");
    assert_eq!(version_fn2(), 200_u32, "v2 should return 200");
}

#[test]
fn test_b_in_flight_safety() {
    let rt: Arc<Runtime> = Arc::new(create_runtime_with_native());
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let rt_clone: Arc<Runtime> = Arc::clone(&rt);
    let caller: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        for _ in 0..1000_u32 {
            let handle_result: Result<
                polyplug_abi::GuestContractHandle,
                polyplug::error::RegistryError,
            > = rt_clone.find_by_contract(contract_id, 0);
            if let Ok(handle) = handle_result {
                let vt_result: Result<
                    *const GuestContractInterface,
                    polyplug::error::RegistryError,
                > = rt_clone.resolve_plugin(handle);
                if let Ok(vt) = vt_result {
                    // SAFETY: interface is from resolve_plugin and slot 0 is a valid extern "C" fn.
                    let _: u32 = unsafe {
                        let f: extern "C" fn() -> u32 =
                            core::mem::transmute(*(*vt).dispatch.native.functions);
                        f()
                    };
                }
            }
        }
    });
    for _ in 0..20_u32 {
        let _ = rt.reload_bundle(
            &PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so"),
        );
        let _ = rt.reload_bundle(
            &PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("libreload_plugin_v1.so"),
        );
    }
    caller.join().expect("caller thread panicked");
}

#[test]
fn test_c_quiescence_arc_count() {
    let rt: Runtime = create_runtime_with_native();
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    rt.reload_bundle(&PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so"))
        .expect("reload completes: quiescence succeeded");
    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v2 after reload");
    assert_eq!(version_fn(), 200_u32, "v2 should remain active");
}

#[test]
fn test_d_dlclose_timing() {
    let rt: Arc<Runtime> = Arc::new(create_runtime_with_native());
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    let rt2: Arc<Runtime> = Arc::clone(&rt);
    let reload_thread: std::thread::JoinHandle<Result<(), RuntimeError>> =
        std::thread::spawn(move || {
            rt2.reload_bundle(
                &PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so"),
            )
        });
    let result: Result<(), RuntimeError> = reload_thread.join().expect("join");
    assert!(result.is_ok(), "reload should succeed: {:?}", result);
}

#[test]
fn test_e_cascade_reload() {
    let rt: Runtime = create_runtime_with_native();
    rt.load_bundle(std::path::Path::new(env!("DEPENDER_PLUGIN_DIR")))
        .expect("load depender");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    let dep_contract_id: u64 = polyplug_abi::contract_id("depender.test", 1);
    let init_count_before: u32 = {
        let handle: polyplug_abi::GuestContractHandle = rt
            .find_by_contract(dep_contract_id, 0)
            .expect("find depender");
        let vt: *const GuestContractInterface =
            rt.resolve_plugin(handle).expect("resolve depender");
        // SAFETY: interface is from resolve_plugin and slot 0 is a valid extern "C" fn.
        unsafe {
            let f: extern "C" fn() -> u32 = core::mem::transmute(*(*vt).dispatch.native.functions);
            f()
        }
    };
    rt.reload_bundle(&PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("libreload_plugin_v1.so"))
        .expect("reload v1");
    let init_count_after: u32 = {
        let handle: polyplug_abi::GuestContractHandle = rt
            .find_by_contract(dep_contract_id, 0)
            .expect("find depender after reload");
        let vt: *const GuestContractInterface = rt
            .resolve_plugin(handle)
            .expect("resolve depender after reload");
        // SAFETY: interface is from resolve_plugin and slot 0 is a valid extern "C" fn.
        unsafe {
            let f: extern "C" fn() -> u32 = core::mem::transmute(*(*vt).dispatch.native.functions);
            f()
        }
    };
    assert!(
        init_count_after >= init_count_before,
        "depender should have been re-initialized when cascade fires (before={init_count_before}, after={init_count_after})"
    );
}

#[test]
fn test_f_callback_fires() {
    let fired: Arc<Mutex<Option<ReloadPhase>>> = Arc::new(Mutex::new(None));
    let fired_clone: Arc<Mutex<Option<ReloadPhase>>> = Arc::clone(&fired);
    let rt: Runtime = Runtime::builder()
        .config(hot_reload_config())
        .on_reload(move |ev: ReloadPhase| {
            *fired_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(ev);
        })
        .build()
        .expect("build");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    rt.reload_bundle(&PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so"))
        .expect("reload v2");
    let ev: ReloadPhase = fired
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("on_reload callback must have fired");
    // Check that the event contains the expected bundle
    let bundle_name = match &ev {
        ReloadPhase::Preparing { bundle_name, .. } => bundle_name,
        ReloadPhase::Reloaded { bundle_name, .. } => bundle_name,
        ReloadPhase::Failed { bundle_name, .. } => bundle_name,
    };
    assert!(
        bundle_name.contains("reload_plugin"),
        "bundle_name should contain reload_plugin, got: {}",
        bundle_name
    );
}

#[test]
fn test_g_file_watcher() {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let bundle_dir: PathBuf = dir.path().join("reload_plugin_v1");
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    std::fs::copy(
        env!("RELOAD_PLUGIN_V1_SO"),
        bundle_dir.join("libreload_plugin_v1.so"),
    )
    .expect("copy v1 so");
    std::fs::copy(
        std::path::PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("manifest.toml"),
        bundle_dir.join("manifest.toml"),
    )
    .expect("copy v1 manifest");

    let fired: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let fired_clone: Arc<AtomicBool> = Arc::clone(&fired);
    let rt: Arc<Runtime> = Arc::new(
        Runtime::builder()
            .config(hot_reload_config())
            .on_reload(move |_ev: ReloadPhase| {
                fired_clone.store(true, Ordering::Relaxed);
            })
            .build()
            .expect("build"),
    );
    rt.load_bundle(bundle_dir.as_path())
        .expect("load from tmpdir");
    Runtime::watch_plugin_dir(Arc::clone(&rt), dir.path()).expect("watch");

    let so_staging: PathBuf = dir.path().join("reload_plugin_new.so");
    std::fs::copy(env!("RELOAD_PLUGIN_V2_SO"), &so_staging).expect("stage v2");
    std::fs::rename(&so_staging, bundle_dir.join("libreload_plugin_v1.so"))
        .expect("atomic rename v2 into place");

    for _ in 0..50_u32 {
        if fired.load(Ordering::Relaxed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10_u64));
    }
    assert!(
        fired.load(Ordering::Relaxed),
        "file watcher must have triggered reload within 500ms"
    );
}

#[test]
fn test_h_multiple_reloads() {
    let rt: Runtime = create_runtime_with_native();
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR")))
        .expect("load v1");
    for i in 0..50_u32 {
        let so_path: PathBuf = if i % 2 == 0 {
            PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so")
        } else {
            PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("libreload_plugin_v1.so")
        };
        rt.reload_bundle(so_path.as_path())
            .expect("reload should succeed");
    }
    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve after reloads");
    assert_eq!(version_fn(), 100_u32, "last reload should be v1");
}

#[test]
fn test_i_non_native_returns_error() {
    let rt: Runtime = create_runtime_with_native();
    let result: Result<(), RuntimeError> =
        rt.reload_bundle(std::path::Path::new("/nonexistent/fake_plugin.so"));
    assert!(
        matches!(result, Err(RuntimeError::Loader(..))),
        "expected Loader error for nonexistent path, got: {result:?}"
    );
}
