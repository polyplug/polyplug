#![allow(clippy::expect_used)]

//! Tests for the native loader.
//!
//! These tests verify error handling paths for the native loader.
//! Integration tests in `tests/integration/` cover end-to-end loading.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_native::NativeConfig;
use polyplug_native::NativeLoader;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_manifest(name: &str, file: &str) -> ManifestData {
    ManifestData {
        loader: "native".to_owned(),
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

    let result = loader.load(
        &manifest,
        &BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err());
}

#[test]
fn test_loader_rejects_empty_file() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = ManifestData {
        loader: "native".to_owned(),
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

    let result = loader.load(
        &manifest,
        &BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err());
}

#[test]
fn test_loader_rejects_code_source() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = make_manifest("code_plugin", "plugin.so");
    let source = BundleSource::Code("return {}".to_owned());

    let result = loader.load(&manifest, &source, &runtime);
    match result {
        Err(LoaderError::UnsupportedBundleSource {
            loader,
            source_kind,
            bundle,
        }) => {
            assert_eq!(loader, "native");
            assert_eq!(source_kind, "code");
            assert_eq!(bundle, "code_plugin");
        }
        other => panic!("expected UnsupportedBundleSource, got {other:?}"),
    }
}

#[test]
fn test_loader_rejects_bytes_source() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = make_manifest("bytes_plugin", "plugin.so");
    let source = BundleSource::Bytes(vec![0u8, 1, 2, 3]);

    let result = loader.load(&manifest, &source, &runtime);
    match result {
        Err(LoaderError::UnsupportedBundleSource {
            loader,
            source_kind,
            bundle,
        }) => {
            assert_eq!(loader, "native");
            assert_eq!(source_kind, "bytes");
            assert_eq!(bundle, "bytes_plugin");
        }
        other => panic!("expected UnsupportedBundleSource, got {other:?}"),
    }
}

#[test]
fn test_loader_rejects_zero_id() {
    let loader = NativeLoader::new(NativeConfig::default());
    let runtime = make_runtime();

    let manifest = ManifestData {
        loader: "native".to_owned(),
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

    let result = loader.load(
        &manifest,
        &BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err());
}

#[test]
fn test_loader_loader_name() {
    let loader = NativeLoader::new(NativeConfig::default());
    assert_eq!(loader.loader_name(), "native");
}

#[test]
fn test_loader_new() {
    let loader = NativeLoader::new(NativeConfig::default());
    let _ = loader;
}
