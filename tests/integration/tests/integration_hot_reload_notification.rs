#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

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

fn get_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug_abi::GuestContractHandle = rt.find_by_contract(contract_id, 0).ok()?;
    let vtable: *const GuestContractInterface = rt.resolve_plugin(handle).ok()?.vtable();
    // SAFETY: vtable is from resolve_plugin and points to a valid vtable while the
    // library is loaded; slot 0 is a compatible extern "C" fn in the fixtures.
    let fn_ptr: extern "C" fn() -> u32 = unsafe {
        let fns: *const *const () = (*vtable).dispatch.native.functions;
        core::mem::transmute(*fns)
    };
    Some(fn_ptr)
}

#[test]
fn test_preparing_fires_before_vtable_swap() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn_v1: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v1");
    assert_eq!(
        version_fn_v1(),
        100_u32,
        "v1 should return 100 before reload"
    );

    phases.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let v2_path: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so");
    rt.reload_bundle(v2_path.as_path()).expect("reload v2");

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let preparing_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| matches!(p, ReloadPhase::Preparing { .. }));
    assert!(
        preparing_phase.is_some(),
        "Preparing phase must have been fired"
    );

    if let Some(ReloadPhase::Preparing {
        bundle_id,
        bundle_name,
        retry_count,
    }) = preparing_phase
    {
        assert_eq!(*bundle_name, "reload_plugin_v1", "bundle_name should match");
        assert_eq!(
            *retry_count, 0_u32,
            "first attempt should have retry_count=0"
        );
        assert!(*bundle_id != 0, "bundle_id should be non-zero");
    }

    let reloaded_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| matches!(p, ReloadPhase::Reloaded { .. }));
    assert!(
        reloaded_phase.is_some(),
        "Reloaded phase must have been fired after successful reload"
    );
}

#[test]
fn test_reloaded_fires_after_vtable_swap() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);

    let v2_path: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so");
    rt.reload_bundle(v2_path.as_path()).expect("reload v2");

    let version_fn_v2: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v2");
    assert_eq!(
        version_fn_v2(),
        200_u32,
        "v2 should return 200 after reload"
    );

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let reloaded_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| matches!(p, ReloadPhase::Reloaded { .. }));
    assert!(
        reloaded_phase.is_some(),
        "Reloaded phase must have been fired"
    );

    if let Some(ReloadPhase::Reloaded {
        bundle_id,
        bundle_name,
    }) = reloaded_phase
    {
        assert_eq!(*bundle_name, "reload_plugin_v1", "bundle_name should match");
        assert!(*bundle_id != 0, "bundle_id should be non-zero");
    }
}

#[test]
fn test_failed_fires_on_abort_after_max_retries() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            hot_reload_max_retries: 1_u32,
            hot_reload_retry_interval: Duration::from_millis(10_u64),
            hot_reload_abort_on_max_retries: true,
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn_before: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v1 before");
    assert_eq!(
        version_fn_before(),
        100_u32,
        "v1 should return 100 before failed reload"
    );

    phases.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let nonexistent_so: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("nonexistent.so");
    let result: Result<(), RuntimeError> = rt.reload_bundle(nonexistent_so.as_path());

    assert!(result.is_err(), "reload of nonexistent .so should fail");

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let failed_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| matches!(p, ReloadPhase::Failed { .. }));
    assert!(
        failed_phase.is_some(),
        "Failed phase must have been fired after abort"
    );

    if let Some(ReloadPhase::Failed {
        bundle_id,
        bundle_name,
        reason,
    }) = failed_phase
    {
        assert_eq!(
            *bundle_name, "reload_plugin_v1",
            "bundle_name should match the loaded bundle"
        );
        assert!(*bundle_id != 0, "bundle_id should be non-zero");
        assert!(!reason.is_empty(), "reason should not be empty");
    }
}

#[test]
fn test_retry_count_increments_correctly() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            hot_reload_max_retries: 2_u32,
            hot_reload_retry_interval: Duration::from_millis(10_u64),
            hot_reload_abort_on_max_retries: true,
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    phases.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let nonexistent_so: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("nonexistent.so");
    let _result: Result<(), RuntimeError> = rt.reload_bundle(nonexistent_so.as_path());

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();

    let preparing_phases: Vec<&ReloadPhase> = captured_phases
        .iter()
        .filter(|p| matches!(p, ReloadPhase::Preparing { .. }))
        .collect();

    assert!(
        preparing_phases.len() >= 2,
        "Should have at least 2 Preparing phases (initial + retries)"
    );

    let retry_counts: Vec<u32> = preparing_phases
        .iter()
        .filter_map(|p| {
            if let ReloadPhase::Preparing { retry_count, .. } = p {
                Some(*retry_count)
            } else {
                None
            }
        })
        .collect();

    for (i, &count) in retry_counts.iter().enumerate() {
        assert_eq!(
            count, i as u32,
            "retry_count at position {} should be {}",
            i, i
        );
    }
}

#[test]
fn test_old_vtable_kept_on_abort() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            hot_reload_max_retries: 1_u32,
            hot_reload_retry_interval: Duration::from_millis(10_u64),
            hot_reload_abort_on_max_retries: true,
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_abi::contract_id("reload.test", 1);
    let version_fn_before: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v1 before");
    assert_eq!(
        version_fn_before(),
        100_u32,
        "v1 should return 100 before failed reload"
    );

    let nonexistent_so: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("nonexistent.so");
    let result: Result<(), RuntimeError> = rt.reload_bundle(nonexistent_so.as_path());
    assert!(result.is_err(), "reload should fail");

    let version_fn_after: extern "C" fn() -> u32 =
        get_version_fn(&rt, contract_id).expect("resolve v1 after");
    assert_eq!(
        version_fn_after(),
        100_u32,
        "v1 should still return 100 after failed reload - old vtable preserved"
    );

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let failed_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| matches!(p, ReloadPhase::Failed { .. }));
    assert!(failed_phase.is_some(), "Failed phase must have been fired");
}

#[test]
fn test_notification_order_on_successful_reload() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(move |phase: ReloadPhase| {
            phases_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(phase);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    phases.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let v2_path: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so");
    rt.reload_bundle(v2_path.as_path()).expect("reload v2");

    let captured_phases: Vec<ReloadPhase> =
        phases.lock().unwrap_or_else(|e| e.into_inner()).clone();

    assert_eq!(
        captured_phases.len(),
        2,
        "Should have exactly 2 phases for successful reload"
    );

    assert!(
        matches!(captured_phases[0], ReloadPhase::Preparing { .. }),
        "First notification should be Preparing"
    );

    assert!(
        matches!(captured_phases[1], ReloadPhase::Reloaded { .. }),
        "Second notification should be Reloaded"
    );
}

#[test]
fn test_runtime_config_defaults() {
    let config: RuntimeConfig = RuntimeConfig::default();

    assert!(
        !config.hot_reload_enabled,
        "default hot_reload_enabled should be false"
    );
    assert_eq!(
        config.hot_reload_max_retries, 3_u32,
        "default max_retries should be 3"
    );
    assert_eq!(
        config.hot_reload_retry_interval,
        Duration::from_secs(1_u64),
        "default retry_interval should be 1 second"
    );
    assert!(
        config.hot_reload_abort_on_max_retries,
        "default abort_on_max_retries should be true"
    );
}

#[test]
fn test_callback_receives_correct_bundle_id() {
    let bundle_ids: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let bundle_ids_clone: Arc<Mutex<Vec<u64>>> = Arc::clone(&bundle_ids);

    let rt: Runtime = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(move |phase: ReloadPhase| {
            let id: u64 = match phase {
                ReloadPhase::Preparing { bundle_id, .. } => bundle_id,
                ReloadPhase::Reloaded { bundle_id, .. } => bundle_id,
                ReloadPhase::Failed { bundle_id, .. } => bundle_id,
            };
            bundle_ids_clone
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(id);
        })
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let expected_bundle_id: u64 = polyplug_abi::bundle_id("reload_plugin_v1");

    let v2_path: PathBuf =
        PathBuf::from(env!("RELOAD_PLUGIN_V2_DIR")).join("libreload_plugin_v2.so");
    rt.reload_bundle(v2_path.as_path()).expect("reload v2");

    let captured_ids: Vec<u64> = bundle_ids.lock().unwrap_or_else(|e| e.into_inner()).clone();

    assert!(!captured_ids.is_empty(), "Should have captured bundle_ids");
    for id in captured_ids {
        assert_eq!(
            id, expected_bundle_id,
            "All bundle_ids should match expected value"
        );
    }
}
