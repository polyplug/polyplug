//! Integration test: loader dispatch, duplicate detection, unknown runtime error, stub adapters.
//!
//! This test crate is the crate root for the `integration_loader_dispatch` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use std::path::Path;

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
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
    fn runtime_name(&self) -> &'static str {
        self.name
    }

    fn load(&self, _path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
        Ok(())
    }
}

// ─── ManifestData parsing ──────────────────────────────────────────────────────

#[test]
fn manifest_defaults_to_native_when_field_absent() {
    let toml_src: &str = "";
    let data: ManifestData =
        toml::from_str(toml_src).expect("empty TOML should parse to ManifestData with defaults");
    assert_eq!(
        data.runtime, "native",
        "absent runtime field must default to \"native\""
    );
}

#[test]
fn manifest_reads_runtime_field() {
    let toml_src: &str = r#"runtime = "lua""#;
    let data: ManifestData =
        toml::from_str(toml_src).expect("TOML with runtime = \"lua\" should parse successfully");
    assert_eq!(data.runtime, "lua");
}

#[test]
fn manifest_reads_dotnet_runtime_field() {
    let toml_src: &str = r#"runtime = "dotnet""#;
    let data: ManifestData =
        toml::from_str(toml_src).expect("TOML with runtime = \"dotnet\" should parse successfully");
    assert_eq!(data.runtime, "dotnet");
}

// ─── RuntimeBuilder loader dispatch ─────────────────────────────────────────────────────

#[test]
fn builder_builds_with_no_extra_loaders() {
    let result: Result<Runtime, RuntimeError> = Runtime::builder().build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder::build() with no extra loaders must succeed: {:?}",
        result.err()
    );
}

#[test]
fn builder_builds_with_stub_loader() {
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
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
    // Registering two loaders with the same runtime_name must fail at build().
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .loader(StubLoader {
            name: "conflict_runtime",
        })
        .loader(StubLoader {
            name: "conflict_runtime",
        })
        .build();
    match result {
        Err(RuntimeError::Loader(LoaderError::DuplicateLoader { runtime_name })) => {
            assert_eq!(runtime_name, "conflict_runtime");
        }
        Err(e) => panic!("expected DuplicateLoader error, got: {e:?}"),
        Ok(_) => panic!("expected DuplicateLoader error, got Ok"),
    }
}

#[test]
fn duplicate_native_loader_detected_in_build() {
    // Attempting to register a loader for "native" duplicates the built-in.
    let result: Result<Runtime, RuntimeError> = Runtime::builder()
        .loader(StubLoader { name: "native" })
        .build();
    match result {
        Err(RuntimeError::Loader(LoaderError::DuplicateLoader { runtime_name })) => {
            assert_eq!(runtime_name, "native");
        }
        Err(e) => panic!("expected DuplicateLoader for \"native\", got: {e:?}"),
        Ok(_) => panic!("expected DuplicateLoader for \"native\", got Ok"),
    }
}

// ─── Stub adapter crate behavior ─────────────────────────────────────────────────────

#[test]
fn dotnet_loader_load_nonexistent_dll_errors() {
    let loader: DotnetLoader = DotnetLoader::new(polyplug_dotnet::DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: polyplug_dotnet::HostfxrLocation::Auto,
    });
    assert_eq!(loader.runtime_name(), "dotnet");

    let dummy_path: &Path = Path::new("dummy.dll");
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: dummy_registrar_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> = loader.load(dummy_path, &mut registrar);
    let result: Result<(), PolyplugError> = loader.load(dummy_path, &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        Err(PolyplugError::Loader(LoaderError::ClrInitFailed { .. })) => {}
        other => panic!("expected AssemblyNotFound or ClrInitFailed for dummy.dll, got: {other:?}"),
    }
}

#[test]
fn python_loader_returns_not_implemented() {
    let loader: PythonLoader = PythonLoader::new(polyplug_python::PythonConfig::default());
    assert_eq!(loader.runtime_name(), "python");

    let dummy_path: &Path = Path::new("dummy.py");
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: dummy_registrar_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> = loader.load(dummy_path, &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented { runtime_name })) => {
            assert_eq!(runtime_name, "python");
        }
        other => panic!("expected RuntimeNotImplemented, got: {other:?}"),
    }
}

#[test]
fn lua_loader_returns_not_implemented() {
    let loader: LuaLoader = LuaLoader::new();
    assert_eq!(loader.runtime_name(), "lua");

    let dummy_path: &Path = Path::new("dummy.lua");
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: dummy_registrar_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> = loader.load(dummy_path, &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented { runtime_name })) => {
            assert_eq!(runtime_name, "lua");
        }
        other => panic!("expected RuntimeNotImplemented, got: {other:?}"),
    }
}

// ─── Error message content ───────────────────────────────────────────────────────────

#[test]
fn no_loader_error_message_is_actionable() {
    let err: LoaderError = LoaderError::NoLoaderForRuntime {
        bundle: "my_plugin.dll".to_owned(),
        runtime_name: "dotnet".to_owned(),
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
        msg.contains("polyplug-dotnet"),
        "error message must reference the adapter crate name, got: {msg}"
    );
}

#[test]
fn duplicate_loader_error_message_contains_runtime_name() {
    let err: LoaderError = LoaderError::DuplicateLoader {
        runtime_name: "lua".to_owned(),
    };
    let msg: String = err.to_string();
    assert!(
        msg.contains("lua"),
        "DuplicateLoader error must contain runtime name, got: {msg}"
    );
}

// ─── Helpers ───────────────────────────────────────────────────────────────────────────────────

/// Minimal registrar callback for constructing a PluginRegistrar in tests.
///
/// # Safety
/// Parameters are valid for the duration of the call. Not actually called
/// in these tests (stub loaders return Err immediately).
unsafe extern "C" fn dummy_registrar_callback(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}
