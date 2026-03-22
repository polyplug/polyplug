#![allow(clippy::expect_used)]

//! Integration tests: version negotiation, compatibility modes, and warning callbacks.

use std::collections::HashMap;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::manifest::RawManifestDependency;
use polyplug::runtime::Runtime;
use polyplug::version::Compatibility;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use tempfile::TempDir;

struct NoopLoader;

impl BundleLoader for NoopLoader {
    fn runtime_name(&self) -> &'static str {
        "test-noop"
    }

    fn load(&self, _path: &Path, _runtime: &Runtime) -> Result<(), polyplug::error::PolyplugError> {
        Ok(())
    }
}

static WARNING_SINK: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

fn shared_warning_sink() -> Arc<Mutex<Vec<String>>> {
    Arc::clone(WARNING_SINK.get_or_init(|| Arc::new(Mutex::new(Vec::new()))))
}

fn ensure_warning_registered() {
    static REGISTER: OnceLock<()> = OnceLock::new();
    REGISTER.get_or_init(|| {
        let sink: Arc<Mutex<Vec<String>>> = shared_warning_sink();
        let _: Result<_, _> = Runtime::builder()
            .on_warning(move |msg: &str| {
                sink.lock().expect("lock").push(msg.to_owned());
            })
            .build();
    });
}

fn write_bundle_manifest(
    dir: &TempDir,
    bundle_name: &str,
    version: &str,
    provides: &[&str],
    function_count_entries: &[(&str, u32)],
    deps: &[(&str, u64, &str)],
) -> PathBuf {
    let bundle_dir: PathBuf = dir.path().join(bundle_name);
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    let so_name: String = format!("{bundle_name}.so");
    std::fs::write(bundle_dir.join(&so_name), b"").expect("write stub so");

    let function_count: HashMap<String, u32> = function_count_entries
        .iter()
        .map(|(k, v)| (k.to_string(), *v))
        .collect();

    let dependencies: Vec<RawManifestDependency> = deps
        .iter()
        .map(|(contract, cid, min_ver)| RawManifestDependency {
            kind: "contract".to_owned(),
            contract: contract.to_string(),
            contract_id: *cid,
            min_version: min_ver.to_string(),
            bundle: None,
            bundle_id: None,
        })
        .collect();

    let manifest: ManifestData = ManifestData {
        id: 1,
        name: bundle_name.to_owned(),
        runtime: "test-noop".to_owned(),
        file: so_name,
        version: version.to_owned(),
        provides: provides.iter().map(|s| s.to_string()).collect(),
        function_count,
        dependencies,
        needs_reinit_on_dep_reload: false,
        path: PathBuf::new(),
    };

    fs::write(bundle_dir.join("manifest.toml"), manifest.to_toml()).expect("write manifest.toml");

    bundle_dir
}

// ────────────────────────────────────────────────────────────────────────────
// Tests 1–4: version compatibility — exact and superset
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn compatible_exact_version_strict_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    // Consumer (depends on test.contract with min_version 1.0)
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

#[test]
fn compatible_superset_version_strict_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider: version 1.2 satisfies min_version 1.0 (same major, higher minor)
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.2",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

#[test]
fn compatible_superset_version_relaxed_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.2",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Relaxed)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

#[test]
fn compatible_superset_version_yolo_loads_ok() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.2",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Yolo)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

// ────────────────────────────────────────────────────────────────────────────
// Tests 5–7: provider too old (minor mismatch)
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn too_old_strict_returns_version_mismatch() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider at 1.0, consumer requires 1.2
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.2")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::VersionMismatch { .. }))
        ),
        "expected VersionMismatch but got an unexpected value"
    );
}

#[test]
fn too_old_relaxed_warns_and_loads() {
    let sink: Arc<Mutex<Vec<String>>> = shared_warning_sink();
    sink.lock().expect("lock").clear();
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider at 1.0, consumer requires 1.2
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.2")],
    );
    let sink_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&sink);
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Relaxed)
        .loader(NoopLoader)
        .on_warning(move |msg: &str| {
            sink_clone.lock().expect("lock").push(msg.to_owned());
        })
        .build();
    assert!(result.is_ok(), "expected Ok");
    assert!(
        sink.lock()
            .expect("lock")
            .iter()
            .any(|w: &String| w.to_lowercase().contains("version mismatch")),
        "expected a version mismatch warning in sink"
    );
}

#[test]
fn too_old_yolo_loads_silently() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.2")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Yolo)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

// ────────────────────────────────────────────────────────────────────────────
// Tests 8–10: major version mismatch
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn major_mismatch_strict_returns_version_mismatch() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider at 1.0, consumer requires 2.0
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "2.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::VersionMismatch { .. }))
        ),
        "expected VersionMismatch but got an unexpected value"
    );
}

#[test]
fn major_mismatch_relaxed_warns_and_loads() {
    let sink: Arc<Mutex<Vec<String>>> = shared_warning_sink();
    sink.lock().expect("lock").clear();
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider at 1.0, consumer requires 2.0
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "2.0")],
    );
    let sink_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&sink);
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Relaxed)
        .loader(NoopLoader)
        .on_warning(move |msg: &str| {
            sink_clone.lock().expect("lock").push(msg.to_owned());
        })
        .build();
    assert!(result.is_ok(), "expected Ok");
    assert!(
        sink.lock()
            .expect("lock")
            .iter()
            .any(|w: &String| w.to_lowercase().contains("version mismatch")),
        "expected a version mismatch warning in sink"
    );
}

#[test]
fn major_mismatch_yolo_loads_silently() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[("test.contract@1", 2)],
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "2.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Yolo)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

// ────────────────────────────────────────────────────────────────────────────
// Tests 11–13: function_count mismatch
// function_count = {} (empty) → no entry for "test.contract@1" → FunctionCountMismatch
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn function_count_mismatch_strict_returns_error() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider: NO function_count entry (empty {}) — will trigger FunctionCountMismatch
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[], // intentionally empty function_count
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::FunctionCountMismatch { .. }
            ))
        ),
        "expected FunctionCountMismatch but got an unexpected value"
    );
}

#[test]
fn function_count_mismatch_relaxed_warns_and_loads() {
    let sink: Arc<Mutex<Vec<String>>> = shared_warning_sink();
    sink.lock().expect("lock").clear();
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider: NO function_count entry (empty {})
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[], // intentionally empty function_count
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let sink_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&sink);
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Relaxed)
        .loader(NoopLoader)
        .on_warning(move |msg: &str| {
            sink_clone.lock().expect("lock").push(msg.to_owned());
        })
        .build();
    assert!(result.is_ok(), "expected Ok");
    assert!(
        !sink.lock().expect("lock").is_empty(),
        "expected at least one warning in sink for function count mismatch"
    );
}

#[test]
fn function_count_mismatch_yolo_ignored() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider: NO function_count entry (empty {})
    write_bundle_manifest(
        &tmp,
        "provider",
        "1.0",
        &["test.contract"],
        &[], // intentionally empty function_count
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Yolo)
        .loader(NoopLoader)
        .build();
    assert!(result.is_ok(), "expected Ok");
}

// ────────────────────────────────────────────────────────────────────────────
// Test 14: malformed version string in provider manifest
// ────────────────────────────────────────────────────────────────────────────

#[test]
fn malformed_version_returns_manifest_parse_error() {
    let tmp: TempDir = TempDir::new().expect("tmp");
    let cid: u64 = polyplug_abi::contract_id("test.contract", 1);
    // Provider: version = "not_a_version"
    // split_once('.') on "not_a_version" returns None → major_str = "0" → key = "test.contract@0"
    // Include function_count with key "test.contract@0" so function_count check passes,
    // then the version parse will trigger ManifestParse.
    write_bundle_manifest(
        &tmp,
        "provider",
        "not_a_version",
        &["test.contract"],
        &[("test.contract@0", 2)], // key "test.contract@0" matches what code computes
        &[],
    );
    write_bundle_manifest(
        &tmp,
        "consumer",
        "1.0",
        &[],
        &[],
        &[("test.contract", cid, "1.0")],
    );
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .compatibility(Compatibility::Strict)
        .loader(NoopLoader)
        .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::ManifestParse { .. }))
        ),
        "expected ManifestParse but got an unexpected value"
    );
}
