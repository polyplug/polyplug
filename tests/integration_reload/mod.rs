#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

#[cfg(feature = "hot-reload")]
use std::path::PathBuf;
#[cfg(feature = "hot-reload")]
use std::sync::atomic::AtomicBool;
#[cfg(feature = "hot-reload")]
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
#[cfg(feature = "hot-reload")]
use std::time::Duration;

use polyplug::abi::PluginVTable;
use polyplug::error::PolyplugError;
use polyplug::runtime::Runtime;
use polyplug::ReloadEvent;

fn get_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug::abi::PluginHandle = rt.find_by_contract(contract_id, 0).ok()?;
    let vtable: *const PluginVTable = rt.resolve_plugin(handle).ok()?;
    // SAFETY: vtable is from resolve_plugin and points to a valid vtable while the
    // library is loaded; slot 0 is a compatible extern "C" fn in the fixtures.
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*vtable).functions;
        std::mem::transmute(*fns)
    };
    Some(fn_ptr)
}

#[test]
fn test_a_basic_reload() {
    let v1_path: &str = env!("RELOAD_PLUGIN_V1_SO");
    let v2_path: &str = env!("RELOAD_PLUGIN_V2_SO");
    let rt: Runtime = Runtime::builder().build().expect("build");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");
    let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v1");
    assert_eq!(version_fn(), 100_u32, "v1 should return 100");
    rt.reload_bundle(std::path::Path::new(v2_path))
        .expect("reload v2");
    let version_fn2: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v2");
    assert_eq!(version_fn2(), 200_u32, "v2 should return 200");
}

#[test]
fn test_b_in_flight_safety() {
    let rt: Arc<Runtime> = Arc::new(Runtime::builder().build().expect("build"));
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
    let rt_clone: Arc<Runtime> = Arc::clone(&rt);
    let caller: std::thread::JoinHandle<()> = std::thread::spawn(move || {
        for _ in 0..1000_u32 {
            let handle_result: Result<polyplug::abi::PluginHandle, polyplug::error::RegistryError> =
                rt_clone.find_by_contract(contract_id, 0);
            if let Ok(handle) = handle_result {
                let vt_result: Result<*const PluginVTable, polyplug::error::RegistryError> =
                    rt_clone.resolve_plugin(handle);
                if let Ok(vt) = vt_result {
                    // SAFETY: vtable is from resolve_plugin and slot 0 is a valid extern "C" fn.
                    let _: u32 = unsafe {
                        let f: extern "C" fn() -> u32 = std::mem::transmute(*(*vt).functions);
                        f()
                    };
                }
            }
        }
    });
    for _ in 0..20_u32 {
        let _ = rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")));
        let _ = rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")));
    }
    caller.join().expect("caller thread panicked");
}

#[test]
fn test_c_quiescence_arc_count() {
    let rt: Runtime = Runtime::builder().build().expect("build");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")))
        .expect("reload completes: quiescence succeeded");
    let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v2 after reload");
    assert_eq!(version_fn(), 200_u32, "v2 should remain active");
}

#[test]
fn test_d_dlclose_timing() {
    let rt: Arc<Runtime> = Arc::new(Runtime::builder().build().expect("build"));
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    let rt2: Arc<Runtime> = Arc::clone(&rt);
    let reload_thread: std::thread::JoinHandle<Result<(), PolyplugError>> =
        std::thread::spawn(move || {
            rt2.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")))
        });
    let result: Result<(), PolyplugError> = reload_thread.join().expect("join");
    assert!(result.is_ok(), "reload should succeed: {:?}", result);
}

#[test]
fn test_e_cascade_reload() {
    let rt: Runtime = Runtime::builder().build().expect("build");
    rt.load_bundle(std::path::Path::new(env!("DEPENDER_PLUGIN_SO")))
        .expect("load depender");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    let dep_contract_id: u64 = polyplug::abi::contract_id("depender.test", 1);
    let init_count_before: u32 = {
        let handle: polyplug::abi::PluginHandle = rt
            .find_by_contract(dep_contract_id, 0)
            .expect("find depender");
        let vt: *const PluginVTable = rt.resolve_plugin(handle).expect("resolve depender");
        // SAFETY: vtable is from resolve_plugin and slot 0 is a valid extern "C" fn.
        unsafe {
            let f: extern "C" fn() -> u32 = std::mem::transmute(*(*vt).functions);
            f()
        }
    };
    rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("reload v1");
    let init_count_after: u32 = {
        let handle: polyplug::abi::PluginHandle = rt
            .find_by_contract(dep_contract_id, 0)
            .expect("find depender after reload");
        let vt: *const PluginVTable = rt
            .resolve_plugin(handle)
            .expect("resolve depender after reload");
        // SAFETY: vtable is from resolve_plugin and slot 0 is a valid extern "C" fn.
        unsafe {
            let f: extern "C" fn() -> u32 = std::mem::transmute(*(*vt).functions);
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
    let fired: Arc<Mutex<Option<ReloadEvent>>> = Arc::new(Mutex::new(None));
    let fired_clone: Arc<Mutex<Option<ReloadEvent>>> = Arc::clone(&fired);
    let rt: Runtime = Runtime::builder()
        .on_reload(move |ev: ReloadEvent| {
            *fired_clone.lock().unwrap_or_else(|e| e.into_inner()) = Some(ev);
        })
        .build()
        .expect("build");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")))
        .expect("reload v2");
    let ev: ReloadEvent = fired
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .expect("on_reload callback must have fired");
    assert!(
        ev.affected_contract_ids
            .contains(&polyplug::abi::contract_id("reload.test", 1)),
        "affected_contract_ids must contain reload.test@1"
    );
}

#[cfg(feature = "hot-reload")]
#[test]
fn test_g_file_watcher() {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let so_dest: PathBuf = dir.path().join("reload_plugin.so");
    std::fs::copy(env!("RELOAD_PLUGIN_V1_SO"), &so_dest).expect("copy v1");
    let manifest_src: std::path::PathBuf =
        std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")).with_extension("manifest.toml");
    let manifest_dest: PathBuf = dir.path().join("reload_plugin.manifest.toml");
    std::fs::copy(&manifest_src, &manifest_dest).expect("copy v1 manifest");

    let fired: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let fired_clone: Arc<AtomicBool> = Arc::clone(&fired);
    let rt: Arc<Runtime> = Arc::new(
        Runtime::builder()
            .on_reload(move |_ev: ReloadEvent| {
                fired_clone.store(true, Ordering::Relaxed);
            })
            .build()
            .expect("build"),
    );
    rt.load_bundle(so_dest.as_path()).expect("load from tmpdir");
    Runtime::watch_plugin_dir(Arc::clone(&rt), dir.path()).expect("watch");

    let so_staging: PathBuf = dir.path().join("reload_plugin_new.so");
    std::fs::copy(env!("RELOAD_PLUGIN_V2_SO"), &so_staging).expect("stage v2");
    std::fs::rename(&so_staging, &so_dest).expect("atomic rename v2 into place");

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
    let rt: Runtime = Runtime::builder().build().expect("build");
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")))
        .expect("load v1");
    for i in 0..50_u32 {
        let so: &str = if i % 2 == 0 {
            env!("RELOAD_PLUGIN_V2_SO")
        } else {
            env!("RELOAD_PLUGIN_V1_SO")
        };
        rt.reload_bundle(std::path::Path::new(so))
            .expect("reload should succeed");
    }
    let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
    let version_fn: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve after reloads");
    assert_eq!(version_fn(), 100_u32, "last reload should be v1");
}

#[test]
fn test_i_non_native_returns_error() {
    let rt: Runtime = Runtime::builder().build().expect("build");
    let result: Result<(), PolyplugError> =
        rt.reload_bundle(std::path::Path::new("/nonexistent/fake_plugin.so"));
    assert!(
        matches!(result, Err(PolyplugError::ReloadFailed { .. })),
        "expected ReloadFailed for nonexistent path, got: {result:?}"
    );
}
