#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::ReloadPhase;
use polyplug::RuntimeConfig;
use polyplug::error::RuntimeError;
use polyplug::runtime::Runtime;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::runtime::ReloadPhaseType;
use polyplug_native::NativeLoader;

fn get_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
    let handle: polyplug_abi::GuestContractHandle = rt.find_guest_contract(contract_id, 0).ok()?;
    let vtable: *const GuestContractInterface = rt.resolve_guest_contract(handle).ok()?;
    // SAFETY: vtable is from resolve_guest_contract and points to a valid vtable while the
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

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                phases_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase);
            },
        )
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_utils::guest_contract_id("reload.test", 1);
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
        .find(|p| p.phase_type == ReloadPhaseType::Preparing);
    assert!(
        preparing_phase.is_some(),
        "Preparing phase must have been fired"
    );

    if let Some(phase) = preparing_phase {
        assert!(
            phase.bundle_name.len != 0,
            "bundle_name should not be empty"
        );
        assert!(phase.bundle_id.id() != 0, "bundle_id should be non-zero");
    }

    let reloaded_phase: Option<&ReloadPhase> = captured_phases
        .iter()
        .find(|p| p.phase_type == ReloadPhaseType::Reloaded);
    assert!(
        reloaded_phase.is_some(),
        "Reloaded phase must have been fired after successful reload"
    );
}

#[test]
fn test_reloaded_fires_after_vtable_swap() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                phases_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase);
            },
        )
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_utils::guest_contract_id("reload.test", 1);

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
        .find(|p| p.phase_type == ReloadPhaseType::Reloaded);
    assert!(
        reloaded_phase.is_some(),
        "Reloaded phase must have been fired"
    );

    if let Some(phase) = reloaded_phase {
        assert!(
            phase.bundle_name.len != 0,
            "bundle_name should not be empty"
        );
        assert!(phase.bundle_id.id() != 0, "bundle_id should be non-zero");
    }
}

#[test]
fn test_failed_fires_on_reload_error() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                phases_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase);
            },
        )
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_utils::guest_contract_id("reload.test", 1);
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
        .find(|p| p.phase_type == ReloadPhaseType::Failed);
    assert!(
        failed_phase.is_some(),
        "Failed phase must have been fired after error"
    );

    if let Some(phase) = failed_phase {
        assert!(
            phase.bundle_name.len != 0,
            "bundle_name should not be empty"
        );
        assert!(phase.bundle_id.id() != 0, "bundle_id should be non-zero");
        assert!(phase.reason.len != 0, "reason should not be empty");
    }
}

#[test]
fn test_old_vtable_kept_on_failure() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                phases_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase);
            },
        )
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let contract_id: u64 = polyplug_utils::guest_contract_id("reload.test", 1);
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
        .find(|p| p.phase_type == ReloadPhaseType::Failed);
    assert!(failed_phase.is_some(), "Failed phase must have been fired");
}

#[test]
fn test_notification_order_on_successful_reload() {
    let phases: Arc<Mutex<Vec<ReloadPhase>>> = Arc::new(Mutex::new(Vec::new()));
    let phases_clone: Arc<Mutex<Vec<ReloadPhase>>> = Arc::clone(&phases);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                phases_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase);
            },
        )
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

    assert_eq!(
        captured_phases[0].phase_type,
        ReloadPhaseType::Preparing,
        "First notification should be Preparing"
    );

    assert_eq!(
        captured_phases[1].phase_type,
        ReloadPhaseType::Reloaded,
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
    assert!(
        config.on_reload.is_none(),
        "default on_reload should be None"
    );
}

#[test]
fn test_callback_receives_correct_bundle_id() {
    let bundle_ids: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let bundle_ids_clone: Arc<Mutex<Vec<u64>>> = Arc::clone(&bundle_ids);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(polyplug_native::NativeConfig::default()))
        .config(RuntimeConfig {
            hot_reload_enabled: true,
            ..RuntimeConfig::default()
        })
        .on_reload(
            move |_user_data: *mut core::ffi::c_void, phase: ReloadPhase| {
                bundle_ids_clone
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(phase.bundle_id.id());
            },
        )
        .build()
        .expect("build runtime");

    let v1_path: &str = env!("RELOAD_PLUGIN_V1_DIR");
    rt.load_bundle(std::path::Path::new(v1_path))
        .expect("load v1");

    let expected_bundle_id: u64 = polyplug_utils::bundle_id("reload_plugin_v1");

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
