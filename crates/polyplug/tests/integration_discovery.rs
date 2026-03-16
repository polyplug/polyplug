//! Integration tests: multi-bundle discovery, graph resolution, load order, error cases.

use polyplug::error::GraphError;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::error::RuntimeError;
use polyplug::graph::CapabilityGraph;
use polyplug::loader::manifest::ManifestData;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

/// Write a bundle directory: `<dir>/<stem>/manifest.toml` + `<dir>/<stem>/<stem>.so` stub.
/// The `toml_content` should NOT include `file = "..."` — this helper adds it.
fn write_bundle_dir(dir: &Path, stem: &str, toml_content: &str) {
    let bundle_dir: PathBuf = dir.join(stem);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    let so_name: String = format!("{stem}.so");
    fs::write(bundle_dir.join(&so_name), b"").expect("write stub so");
    let manifest_toml: String = format!("file = \"{}\"\n{}", so_name, toml_content);
    fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest.toml");
}

/// Write a script bundle (directory with manifest.toml inside).
#[allow(dead_code)]
fn write_script_bundle(
    dir: &Path,
    bundle_dir_name: &str,
    manifest_content: &str,
    script_name: &str,
) {
    let bundle_dir: PathBuf = dir.join(bundle_dir_name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(bundle_dir.join("manifest.toml"), manifest_content).expect("write manifest.toml");
    fs::write(bundle_dir.join(script_name), b"").expect("write script file");
}

#[test]
fn chain_loads_in_dependency_order() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    // Compute contract IDs to embed in manifests
    let cid_x: u64 = polyplug_abi::contract_id("contract.X", 1);
    let cid_y: u64 = polyplug_abi::contract_id("contract.Y", 1);

    write_bundle_dir(
        tmp.path(),
        "bundle_a",
        r#"
bundle_name = "bundle_a"
runtime = "native"
provides = ["contract.X"]
"#,
    );

    write_bundle_dir(
        tmp.path(),
        "bundle_b",
        &format!(
            r#"
bundle_name = "bundle_b"
runtime = "native"
provides = ["contract.Y"]

[[dependency]]
kind = "contract"
contract = "contract.X"
contract_id = {cid_x}
min_version = "1.0"
"#
        ),
    );

    write_bundle_dir(
        tmp.path(),
        "bundle_c",
        &format!(
            r#"
bundle_name = "bundle_c"
runtime = "native"
provides = []

[[dependency]]
kind = "contract"
contract = "contract.Y"
contract_id = {cid_y}
min_version = "1.0"
"#
        ),
    );

    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(tmp.path());
    assert_eq!(discovered.len(), 3, "expected 3 bundles");

    let graph: CapabilityGraph =
        CapabilityGraph::from_manifests(&discovered).expect("graph should build");

    let topo_order: Vec<String> = graph.topological_order().expect("topo order");

    let pos_a: usize = topo_order
        .iter()
        .position(|n| n == "bundle_a")
        .expect("bundle_a");
    let pos_b: usize = topo_order
        .iter()
        .position(|n| n == "bundle_b")
        .expect("bundle_b");
    let pos_c: usize = topo_order
        .iter()
        .position(|n| n == "bundle_c")
        .expect("bundle_c");

    assert!(pos_a < pos_b, "bundle_a must load before bundle_b");
    assert!(pos_b < pos_c, "bundle_b must load before bundle_c");
}

#[test]
fn missing_dep_fails_before_load() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    let cid_x: u64 = polyplug_abi::contract_id("contract.X", 1);

    // Bundle B requires contract.X, but nothing provides it
    write_bundle_dir(
        tmp.path(),
        "bundle_b",
        &format!(
            r#"
bundle_name = "bundle_b"
runtime = "native"
provides = []

[[dependency]]
kind = "contract"
contract = "contract.X"
contract_id = {cid_x}
min_version = "1.0"
"#
        ),
    );

    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(tmp.path());

    let result: Result<CapabilityGraph, GraphError> = CapabilityGraph::from_manifests(&discovered);
    assert!(
        matches!(result, Err(GraphError::UnsatisfiedCapability { .. })),
        "expected UnsatisfiedCapability, got Ok(_) or different error"
    );
}

#[test]
fn cycle_detected_with_clear_error() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    let cid_a: u64 = polyplug_abi::contract_id("contract.A", 1);
    let cid_b: u64 = polyplug_abi::contract_id("contract.B", 1);

    write_bundle_dir(
        tmp.path(),
        "bundle_a",
        &format!(
            r#"
bundle_name = "bundle_a"
runtime = "native"
provides = ["contract.A"]

[[dependency]]
kind = "contract"
contract = "contract.B"
contract_id = {cid_b}
min_version = "1.0"
"#
        ),
    );

    write_bundle_dir(
        tmp.path(),
        "bundle_b",
        &format!(
            r#"
bundle_name = "bundle_b"
runtime = "native"
provides = ["contract.B"]

[[dependency]]
kind = "contract"
contract = "contract.A"
contract_id = {cid_a}
min_version = "1.0"
"#
        ),
    );

    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(tmp.path());
    assert_eq!(discovered.len(), 2);

    let result: Result<CapabilityGraph, GraphError> = CapabilityGraph::from_manifests(&discovered);
    match result {
        Err(GraphError::DependencyCycle { participants }) => {
            assert!(
                participants.len() >= 2,
                "expected at least 2 participants in cycle"
            );
            let all_participants: String = participants.join(",");
            assert!(
                all_participants.contains("bundle_a")
                    || participants.iter().any(|p| p == "bundle_a"),
                "bundle_a must appear in cycle participants"
            );
        }
        other => {
            let err_str: String = match other {
                Err(e) => format!("wrong error variant: {:?}", e),
                Ok(_) => "unexpected Ok".to_owned(),
            };
            panic!(
                "expected DependencyCycle from from_manifests, got: {}",
                err_str
            );
        }
    }
}

#[test]
fn malformed_manifest_skips_bundle() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    // Write a valid bundle A
    write_bundle_dir(
        tmp.path(),
        "bundle_a",
        r#"
bundle_name = "bundle_a"
runtime = "native"
"#,
    );

    // Write a malformed manifest for bundle B (invalid TOML) in a directory
    let bundle_b_dir: PathBuf = tmp.path().join("bundle_b");
    fs::create_dir_all(&bundle_b_dir).expect("create bundle_b dir");
    fs::write(
        bundle_b_dir.join("manifest.toml"),
        b"NOT VALID TOML ===== [[[",
    )
    .expect("write bad manifest");

    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dir(tmp.path());

    // Only bundle_a should be discovered; bundle_b was skipped
    assert_eq!(
        discovered.len(),
        1,
        "expected exactly 1 bundle (bundle_b skipped)"
    );
    assert_eq!(
        discovered[0].1.bundle_name, "bundle_a",
        "only bundle_a should be in results"
    );
}

#[test]
fn unknown_runtime_fails_build() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    write_bundle_dir(
        tmp.path(),
        "zigzag_plugin",
        r#"
bundle_name = "zigzag_plugin"
runtime = "zigzag_unknown"
"#,
    );

    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .plugin_dir(tmp.path().to_path_buf())
        .build();

    match result {
        Err(RuntimeError::Loader(LoaderError::NoLoaderForRuntime { runtime_name, .. })) => {
            assert_eq!(runtime_name, "zigzag_unknown");
        }
        other => {
            let err_str: String = match other {
                Err(e) => format!("wrong error variant: {:?}", e),
                Ok(_) => "unexpected Ok(Runtime)".to_owned(),
            };
            panic!("expected NoLoaderForRuntime, got: {}", err_str);
        }
    }
}

#[test]
fn explicit_load_bundle_missing_manifest_errors() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    // Create a plain file (not a directory) — load_bundle() should reject it
    let plain_file: PathBuf = tmp.path().join("not_a_dir.so");
    fs::write(&plain_file, b"").expect("write plain file");

    // Build a Runtime with no plugin dirs (no directory scanning)
    let rt: Runtime = Runtime::builder().build().expect("build should succeed");

    // Explicitly load the bundle — should fail with BundleNotADirectory
    let result: Result<(), PolyplugError> = rt.load_bundle(&plain_file);
    assert!(
        matches!(
            result,
            Err(PolyplugError::Loader(
                LoaderError::BundleNotADirectory { .. }
            ))
        ),
        "expected BundleNotADirectory, got {:?}",
        result
    );
}
