//! Integration test: loader dispatch, duplicate detection, unknown runtime error, stub adapters.
//!
//! This test crate is the crate root for the `integration_loader_dispatch` test binary.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug_dotnet::DotnetLoader;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonLoader;

// ─── Helper: a minimal stub loader for testing ────────────────────────────────────────────

struct StubLoader {
    name: &'static str,
}

impl BundleLoader for StubLoader {
    fn loader_name(&self) -> &'static str {
        self.name
    }

    fn loader_language(&self) -> polyplug_abi::SupportedLanguage {
        polyplug_abi::SupportedLanguage::Rust
    }

    fn supports_hot_reload(&self) -> bool {
        true
    }

    fn load(
        &self,
        _manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        _runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        Ok(())
    }
}

// ─── ManifestData parsing ──────────────────────────────────────────────────────

#[test]
fn manifest_missing_loader_field_is_error() {
    let toml_src: &str = "";
    let result: Result<ManifestData, RuntimeError> =
        ManifestData::parse_from_str(toml_src).map_err(RuntimeError::from);
    match result {
        Err(RuntimeError::Loader(LoaderError::ManifestParse { .. })) => {}
        other => panic!("expected ManifestParse error for absent loader field, got: {other:?}"),
    }
}

#[test]
fn manifest_reads_loader_field() {
    let toml_src: &str = r#"loader = "lua""#;
    let data: ManifestData = ManifestData::parse_from_str(toml_src)
        .expect("TOML with loader = \"lua\" should parse successfully");
    assert_eq!(data.loader, "lua");
}

#[test]
fn manifest_reads_dotnet_loader_field() {
    let toml_src: &str = r#"loader = "dotnet""#;
    let data: ManifestData = ManifestData::parse_from_str(toml_src)
        .expect("TOML with loader = \"dotnet\" should parse successfully");
    assert_eq!(data.loader, "dotnet");
}

// ─── RuntimeBuilder loader dispatch ─────────────────────────────────────────────────────

#[test]
fn builder_builds_with_no_extra_loaders() {
    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder().build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder::build() with no extra loaders must succeed: {:?}",
        result.err()
    );
}

#[test]
fn builder_builds_with_stub_loader() {
    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(StubLoader {
            name: "my_custom_runtime",
        })
        .build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder::build() with a stub loader must succeed: {:?}",
        result.err()
    );
}

#[test]
fn duplicate_loader_detected_in_build() {
    // Registering two loaders with the same loader_name must fail at build().
    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(StubLoader {
            name: "conflict_runtime",
        })
        .loader(StubLoader {
            name: "conflict_runtime",
        })
        .build();
    match result {
        Err(RuntimeError::Loader(LoaderError::DuplicateLoader { loader_name })) => {
            assert_eq!(loader_name, "conflict_runtime");
        }
        Err(e) => panic!("expected DuplicateLoader error, got: {e:?}"),
        Ok(_) => panic!("expected DuplicateLoader error, got Ok"),
    }
}

#[test]
fn single_native_loader_succeeds_in_build() {
    let result: Result<Arc<Runtime>, RuntimeError> = Runtime::builder()
        .loader(StubLoader { name: "native" })
        .build();
    assert!(
        result.is_ok(),
        "single native loader must succeed: {:?}",
        result.err()
    );
}

// ─── Stub adapter crate behavior ─────────────────────────────────────────────────────

#[test]
fn dotnet_loader_load_nonexistent_dll_errors() {
    let loader: DotnetLoader = DotnetLoader::new(polyplug_dotnet::DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: polyplug_dotnet::HostfxrLocation::Auto,
    });
    assert_eq!(loader.loader_name(), "dotnet");

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(polyplug_dotnet::DotnetConfig::default()))
        .build()
        .expect("failed to build runtime");
    let manifest: ManifestData = ManifestData {
        id: 1,
        name: "dummy".to_owned(),
        loader: "dotnet".to_owned(),
        file: "dummy.dll".to_owned(),
        path: PathBuf::from("."),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        bundle_dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &rt,
    );
    match result {
        // .NET loader returns InitFailed for assembly not found or CLR init failures
        Err(LoaderError::InitFailed { bundle, error }) => {
            assert_eq!(bundle, "dummy");
            assert!(
                !error.is_empty(),
                "error message should describe the failure"
            );
        }
        other => panic!("expected InitFailed for dummy.dll, got: {other:?}"),
    }
}

#[test]
fn python_loader_loads_nonexistent_file_errors() {
    let loader: PythonLoader = PythonLoader::new(polyplug_python::PythonConfig::default());
    assert_eq!(loader.loader_name(), "python");

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(polyplug_python::PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let manifest: ManifestData = ManifestData {
        id: 1,
        name: "dummy".to_owned(),
        loader: "python".to_owned(),
        file: "dummy.py".to_owned(),
        path: PathBuf::from("."),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        bundle_dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &rt,
    );
    match result {
        // Python loader returns InitFailed for import errors (file not found or not accessible).
        Err(LoaderError::InitFailed { bundle, error }) => {
            assert_eq!(bundle, "dummy");
            assert!(
                !error.is_empty(),
                "error message should describe the failure"
            );
        }
        other => panic!("expected InitFailed for dummy.py, got: {other:?}"),
    }
}

#[test]
fn lua_loader_returns_error_for_missing_file() {
    let loader: LuaLoader = LuaLoader::new(polyplug_lua::LuaConfig::default());
    assert_eq!(loader.loader_name(), "lua");

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(polyplug_lua::LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let manifest: ManifestData = ManifestData {
        id: 1,
        name: "dummy".to_owned(),
        loader: "lua".to_owned(),
        file: "dummy.lua".to_owned(),
        path: PathBuf::from("."),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        bundle_dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &rt,
    );
    match result {
        // Lua loader returns InitFailed for script load failures (file not found).
        Err(LoaderError::InitFailed { bundle, error }) => {
            assert_eq!(bundle, "dummy");
            assert!(
                !error.is_empty(),
                "error message should describe the failure"
            );
        }
        other => panic!("expected InitFailed for missing file, got: {other:?}"),
    }
}

// ─── Error message content ───────────────────────────────────────────────────────────

#[test]
fn no_loader_error_message_is_actionable() {
    let err: LoaderError = LoaderError::NoLoaderForName {
        bundle: "my_plugin.dll".to_owned(),
        loader_name: "dotnet".to_owned(),
    };
    let msg: String = err.to_string();
    assert!(
        msg.contains("my_plugin.dll"),
        "error message must contain bundle name, got: {msg}"
    );
    assert!(
        msg.contains("dotnet"),
        "error message must contain runtime name, got: {msg}"
    );
    assert!(
        msg.contains("polyplug_dotnet"),
        "error message must reference the adapter crate name, got: {msg}"
    );
}

#[test]
fn duplicate_loader_error_message_contains_loader_name() {
    let err: LoaderError = LoaderError::DuplicateLoader {
        loader_name: "lua".to_owned(),
    };
    let msg: String = err.to_string();
    assert!(
        msg.contains("lua"),
        "DuplicateLoader error must contain runtime name, got: {msg}"
    );
}
