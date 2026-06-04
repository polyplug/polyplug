#![allow(clippy::expect_used)]

//! Tests for the native loader.
//!
//! These tests verify error handling paths for the native loader.
//! Integration tests in `tests/integration/` cover end-to-end loading.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_manifest(name: &str, file: &str) -> ManifestData {
    ManifestData {
        runtime: "native".to_owned(),
        id: 1,
        name: name.to_owned(),
        file: file.to_owned(),
        path: PathBuf::from("/fake/path"),
        version: String::new(),
        provides: vec![],
        function_count: HashMap::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: vec![],
        dependencies: vec![],
    }
}

fn make_runtime() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .build()
        .expect("runtime build must succeed")
}

// ─── Error Handling Tests ─────────────────────────────────────────────────

#[test]
fn test_loader_rejects_missing_file() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = make_manifest("missing_plugin", "nonexistent.so");

    let result = loader.load(&manifest, &runtime);
    assert!(result.is_err());
}

#[test]
fn test_loader_rejects_empty_file() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = ManifestData {
        runtime: "native".to_owned(),
        id: 1,
        name: "empty_file_plugin".to_owned(),
        file: String::new(),
        path: PathBuf::from("/fake/path"),
        version: String::new(),
        provides: vec![],
        function_count: HashMap::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: vec![],
        dependencies: vec![],
    };

    let result = loader.load(&manifest, &runtime);
    assert!(result.is_err());
}

#[test]
fn test_loader_rejects_zero_id() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = ManifestData {
        runtime: "native".to_owned(),
        id: 0,
        name: "zero_id_plugin".to_owned(),
        file: "fake.so".to_owned(),
        path: PathBuf::from("/fake/path"),
        version: String::new(),
        provides: vec![],
        function_count: HashMap::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: vec![],
        dependencies: vec![],
    };

    let result = loader.load(&manifest, &runtime);
    assert!(result.is_err());
}

#[test]
fn test_loader_runtime_name() {
    let loader = NativeLoader::new(NativeConfig::default());
    assert_eq!(loader.runtime_name(), "native");
}

#[test]
fn test_loader_new() {
    let loader = NativeLoader::new(NativeConfig::default());
    let _ = loader;
}
