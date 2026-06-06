#![allow(clippy::expect_used)]

//! Integration tests: multi-bundle discovery, graph resolution, load order, error cases.

use std::collections::HashMap;
use std::sync::Arc;

use polyplug::compatibility::CapabilityGraph;
use polyplug::error::GraphError;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::ManifestData;
use polyplug::loader::RawManifestDependency;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_utils::GuestContractId;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_bundle_with_manifest(dir: &Path, manifest: &ManifestData) {
    let bundle_dir: PathBuf = dir.join(&manifest.name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(bundle_dir.join(&manifest.file), b"").expect("write stub so");
    fs::write(
        bundle_dir.join("manifest.toml"),
        toml::to_string(manifest).expect("serialize manifest"),
    )
    .expect("write manifest.toml");
}

#[test]
fn chain_loads_in_dependency_order() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    let cid_x: GuestContractId = GuestContractId::new("contract.X", 1);
    let cid_y: GuestContractId = GuestContractId::new("contract.Y", 1);

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 1,
            name: "bundle_a".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_a.so".to_owned(),
            provides: vec!["contract.X".to_owned()],
            ..empty_manifest()
        },
    );

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 2,
            name: "bundle_b".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_b.so".to_owned(),
            provides: vec!["contract.Y".to_owned()],
            dependencies: vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "contract.X".to_owned(),
                contract_id: cid_x,
                min_version: "1.0".to_owned(),
                bundle: None,
                bundle_id: None,
            }],
            ..empty_manifest()
        },
    );

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 3,
            name: "bundle_c".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_c.so".to_owned(),
            provides: vec![],
            dependencies: vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "contract.Y".to_owned(),
                contract_id: cid_y,
                min_version: "1.0".to_owned(),
                bundle: None,
                bundle_id: None,
            }],
            ..empty_manifest()
        },
    );

    // scan_dirs takes a slice of PathBufs
    let dirs: &[PathBuf] = &[tmp.path().to_path_buf()];
    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dirs(dirs).found;
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

fn empty_manifest() -> ManifestData {
    ManifestData {
        id: 0,
        name: String::new(),
        runtime: "native".to_owned(),
        file: String::new(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
        path: PathBuf::new(),
    }
}

#[test]
fn missing_dep_fails_before_load() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    let cid_x: GuestContractId = GuestContractId::new("contract.X", 1);

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 1,
            name: "bundle_b".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_b.so".to_owned(),
            provides: vec![],
            dependencies: vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "contract.X".to_owned(),
                contract_id: cid_x,
                min_version: "1.0".to_owned(),
                bundle: None,
                bundle_id: None,
            }],
            ..empty_manifest()
        },
    );

    let dirs: &[PathBuf] = &[tmp.path().to_path_buf()];
    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dirs(dirs).found;

    let result: Result<CapabilityGraph, GraphError> = CapabilityGraph::from_manifests(&discovered);
    assert!(
        matches!(result, Err(GraphError::UnsatisfiedCapability { .. })),
        "expected UnsatisfiedCapability, got Ok(_) or different error"
    );
}

#[test]
fn cycle_detected_with_clear_error() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    let cid_a: GuestContractId = GuestContractId::new("contract.A", 1);
    let cid_b: GuestContractId = GuestContractId::new("contract.B", 1);

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 1,
            name: "bundle_a".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_a.so".to_owned(),
            provides: vec!["contract.A".to_owned()],
            dependencies: vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "contract.B".to_owned(),
                contract_id: cid_b,
                min_version: "1.0".to_owned(),
                bundle: None,
                bundle_id: None,
            }],
            ..empty_manifest()
        },
    );

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 2,
            name: "bundle_b".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_b.so".to_owned(),
            provides: vec!["contract.B".to_owned()],
            dependencies: vec![RawManifestDependency {
                kind: "contract".to_owned(),
                contract: "contract.A".to_owned(),
                contract_id: cid_a,
                min_version: "1.0".to_owned(),
                bundle: None,
                bundle_id: None,
            }],
            ..empty_manifest()
        },
    );

    let dirs: &[PathBuf] = &[tmp.path().to_path_buf()];
    let discovered: Vec<(PathBuf, ManifestData)> = scanner::scan_dirs(dirs).found;
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

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 1,
            name: "bundle_a".to_owned(),
            runtime: "native".to_owned(),
            file: "bundle_a.so".to_owned(),
            ..empty_manifest()
        },
    );

    let bundle_b_dir: PathBuf = tmp.path().join("bundle_b");
    fs::create_dir_all(&bundle_b_dir).expect("create bundle_b dir");
    fs::write(
        bundle_b_dir.join("manifest.toml"),
        b"NOT VALID TOML ===== [[[",
    )
    .expect("write bad manifest");

    let dirs: &[PathBuf] = &[tmp.path().to_path_buf()];
    let scan: scanner::ScanResult = scanner::scan_dirs(dirs);

    assert_eq!(
        scan.found.len(),
        1,
        "expected exactly 1 bundle (bundle_b skipped)"
    );
    assert_eq!(
        scan.found[0].1.name, "bundle_a",
        "only bundle_a should be in results"
    );

    // The corrupt manifest must not be silently swallowed: it surfaces as a
    // parse diagnostic that names the offending bundle.
    let parse_diags: Vec<&scanner::ScanDiagnostic> = scan
        .diagnostics
        .iter()
        .filter(|d| matches!(d, scanner::ScanDiagnostic::ManifestParseFailed { .. }))
        .collect();
    assert_eq!(parse_diags.len(), 1, "expected one parse diagnostic");
    assert!(
        parse_diags[0].to_string().contains("bundle_b"),
        "diagnostic must name bundle_b: {}",
        parse_diags[0]
    );
}

#[test]
fn unknown_runtime_fails_build() {
    let tmp: TempDir = TempDir::new().expect("tmp dir");

    write_bundle_with_manifest(
        tmp.path(),
        &ManifestData {
            id: 1,
            name: "zigzag_plugin".to_owned(),
            runtime: "zigzag_unknown".to_owned(),
            file: "zigzag_plugin.so".to_owned(),
            ..empty_manifest()
        },
    );

    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
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

    // Create a directory without a manifest.toml
    let bundle_dir: PathBuf = tmp.path().join("no_manifest_bundle");
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");

    let rt: Arc<Runtime> = Runtime::builder().build().expect("build should succeed");

    let result: Result<(), RuntimeError> = rt.load_bundle(&bundle_dir);
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::ManifestParse { .. }))
        ),
        "expected ManifestParse error, got {:?}",
        result
    );
}
