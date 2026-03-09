//! Integration tests: multi-bundle discovery, graph resolution, load order, error cases.
//!
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)
#![allow(clippy::expect_used)]

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

/// Write a dummy bundle file (`.so`) and companion manifest TOML.
fn write_manifest(dir: &Path, stem: &str, toml_content: &str) {
    let bundle_path: PathBuf = dir.join(format!("{stem}.so"));
    fs::write(&bundle_path, b"").expect("write dummy so");
    let manifest_path: PathBuf = dir.join(format!("{stem}.manifest.toml"));
    fs::write(&manifest_path, toml_content).expect("write manifest");
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
    let cid_x: u64 = polyplug::abi::contract_id("contract.X", 1);
    let cid_y: u64 = polyplug::abi::contract_id("contract.Y", 1);

    write_manifest(
        tmp.path(),
        "bundle_a",
        r#"
bundle_name = "bundle_a"
runtime = "native"
provides = ["contract.X"]
"#,
    );

    write_manifest(
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

    write_manifest(
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

    let cid_x: u64 = polyplug::abi::contract_id("contract.X", 1);

    // Bundle B requires contract.X, but nothing provides it
    write_manifest(
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

    let cid_a: u64 = polyplug::abi::contract_id("contract.A", 1);
    let cid_b: u64 = polyplug::abi::contract_id("contract.B", 1);

    write_manifest(
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

    write_manifest(
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
            panic!("expected DependencyCycle from from_manifests, got: {}", err_str);
        }
    }
}

#[test]
fn malformed_manifest_skips_bundle() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    // Write a valid bundle A
    write_manifest(
        tmp.path(),
        "bundle_a",
        r#"
bundle_name = "bundle_a"
runtime = "native"
"#,
    );

    // Write a malformed manifest for bundle B (invalid TOML)
    fs::write(tmp.path().join("bundle_b.so"), b"").expect("create bundle_b.so");
    fs::write(
        tmp.path().join("bundle_b.manifest.toml"),
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

    write_manifest(
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

    // Create a dummy .so file WITHOUT a companion manifest
    let plugin_path: PathBuf = tmp.path().join("test_plugin.so");
    fs::write(&plugin_path, b"").expect("create dummy so");

    // Build a Runtime with no plugin dirs (no directory scanning)
    let runtime: Runtime = Runtime::builder().build().expect("build should succeed");

    // Explicitly load the bundle — should fail with manifest-not-found
    let result: Result<(), PolyplugError> = runtime.load_bundle(&plugin_path);
    assert!(
        result.is_err(),
        "load_bundle without manifest must return Err"
    );

    match result {
        Err(PolyplugError::Loader(LoaderError::ManifestParse { reason, .. })) => {
            assert!(
                reason.contains("not found") || reason.contains("manifest"),
                "error reason should mention manifest not found, got: {reason}"
            );
        }
        other => {
            let err_str: String = match other {
                Err(e) => format!("wrong error variant: {:?}", e),
                Ok(()) => "unexpected Ok(())".to_owned(),
            };
            panic!("expected ManifestParse error, got: {}", err_str);
        }
    }
}
