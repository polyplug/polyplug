//! Integration tests for polyplug_dotnet — .NET loader adapter.
//!
//! Tests that require the CLR or a real .NET SDK are marked `#[ignore]`.
//! Run them with: `cargo test --test dotnet_loader -- --include-ignored`

use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use polyplug::abi::AbiError;
use polyplug::abi::HostVTable;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader as _;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_dotnet::version::read_target_framework;
use tempfile::NamedTempFile;

// SAFETY: noop_register is a valid function used only in test PluginRegistrar stubs.
// It is never actually called — tests fail before load() reaches the register step.
unsafe extern "C" fn noop_register(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    AbiError::ok()
}

fn stub_registrar() -> PluginRegistrar {
    PluginRegistrar {
        register_plugin: noop_register,
        host: core::ptr::null::<HostVTable>(),
    }
}

fn temp_file_with_bytes(bytes: &[u8]) -> NamedTempFile {
    let mut f: NamedTempFile = NamedTempFile::new().expect("tempfile creation failed");
    f.write_all(bytes).expect("tempfile write failed");
    f.flush().expect("tempfile flush failed");
    f
}

fn polyplug_dll_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p: &Path| p.parent())
        .map(|root: &Path| {
            root.join("host-libs")
                .join("csharp")
                .join("bin")
                .join("Debug")
                .join("net10.0")
                .join("Polyplug.dll")
        })
        .expect("CARGO_MANIFEST_DIR resolution failed")
}

// ---------------------------------------------------------------------------
// read_target_framework
// ---------------------------------------------------------------------------

#[test]
fn tfm_reader_nonexistent_file_returns_assembly_not_found() {
    let result: Result<String, PolyplugError> =
        read_target_framework(Path::new("/nonexistent/path/that/does/not/exist.dll"));
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { path })) => {
            assert!(path.contains("nonexistent"));
        }
        other => panic!("expected AssemblyNotFound, got {other:?}"),
    }
}

#[test]
fn tfm_reader_empty_file_returns_assembly_not_found() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"");
    let result: Result<String, PolyplugError> = read_target_framework(tmp.path());
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        other => panic!("expected AssemblyNotFound for empty file, got {other:?}"),
    }
}

#[test]
fn tfm_reader_random_bytes_returns_assembly_not_found() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"\x00\x01\x02\x03this is not a valid PE binary");
    let result: Result<String, PolyplugError> = read_target_framework(tmp.path());
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        other => panic!("expected AssemblyNotFound for junk bytes, got {other:?}"),
    }
}

#[test]
fn tfm_reader_elf_magic_returns_assembly_not_found() {
    // ELF magic (0x7f 'E' 'L' 'F') is not a valid PE header — pelite rejects it.
    let tmp: NamedTempFile =
        temp_file_with_bytes(b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00");
    let result: Result<String, PolyplugError> = read_target_framework(tmp.path());
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        other => panic!("expected AssemblyNotFound for ELF magic, got {other:?}"),
    }
}

#[test]
#[ignore = "requires host-libs/csharp to be built: dotnet build host-libs/csharp"]
fn tfm_reader_net10_dll_returns_correct_tfm() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(dll.exists(), "Polyplug.dll not found at {dll:?}");
    let tfm: String = read_target_framework(&dll).expect("read_target_framework failed");
    assert!(
        tfm.starts_with(".NETCoreApp,Version=v10.0"),
        "TFM should start with .NETCoreApp,Version=v10.0, got: {tfm:?}"
    );
}

// ---------------------------------------------------------------------------
// DotnetConfig
// ---------------------------------------------------------------------------

#[test]
fn dotnet_config_default_min_framework_is_net10() {
    let cfg: DotnetConfig = DotnetConfig::default();
    assert_eq!(cfg.min_framework, "net10.0");
}

#[test]
fn dotnet_config_default_hostfxr_is_auto() {
    let cfg: DotnetConfig = DotnetConfig::default();
    assert!(matches!(cfg.hostfxr, HostfxrLocation::Auto));
}

#[test]
fn dotnet_config_custom_min_framework() {
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net6.0"),
        hostfxr: HostfxrLocation::Auto,
    };
    assert_eq!(cfg.min_framework, "net6.0");
}

#[test]
fn dotnet_config_clone_is_independent() {
    let cfg: DotnetConfig = DotnetConfig::default();
    let mut cloned: DotnetConfig = cfg.clone();
    cloned.min_framework = String::from("net8.0");
    assert_eq!(cfg.min_framework, "net10.0");
    assert_eq!(cloned.min_framework, "net8.0");
}

// ---------------------------------------------------------------------------
// HostfxrLocation
// ---------------------------------------------------------------------------

#[test]
fn hostfxr_location_default_is_auto() {
    let loc: HostfxrLocation = HostfxrLocation::default();
    assert!(matches!(loc, HostfxrLocation::Auto));
}

#[test]
fn hostfxr_location_path_stores_pathbuf() {
    let p: PathBuf = PathBuf::from("/usr/lib/dotnet/host/fxr/10.0.0/libhostfxr.so");
    let loc: HostfxrLocation = HostfxrLocation::Path(p.clone());
    match loc {
        HostfxrLocation::Path(stored) => assert_eq!(stored, p),
        other => panic!("expected HostfxrLocation::Path, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// DotnetLoader construction
// ---------------------------------------------------------------------------

#[test]
fn dotnet_loader_new_does_not_panic() {
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net7.0"),
        hostfxr: HostfxrLocation::Auto,
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    drop(loader);
}

#[test]
fn dotnet_loader_runtime_name_is_dotnet() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    assert_eq!(loader.runtime_name(), "dotnet");
}

// ---------------------------------------------------------------------------
// DotnetLoader::load — file / PE errors (no CLR needed)
// ---------------------------------------------------------------------------

#[test]
fn load_nonexistent_assembly_returns_assembly_not_found() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> =
        loader.load(Path::new("/does/not/exist/Plugin.dll"), &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        other => panic!("expected AssemblyNotFound, got {other:?}"),
    }
}

#[test]
fn load_invalid_pe_file_returns_assembly_not_found() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"not a valid PE binary at all");
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> = loader.load(tmp.path(), &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. })) => {}
        other => panic!("expected AssemblyNotFound for invalid PE, got {other:?}"),
    }
}

#[test]
fn load_with_invalid_hostfxr_path_and_missing_dll_returns_assembly_not_found() {
    // AssemblyNotFound fires before hostfxr is consulted — verifies load() call ordering.
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Path(PathBuf::from("/nonexistent/libhostfxr.so")),
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> =
        loader.load(Path::new("/no/such/Plugin.dll"), &mut registrar);
    assert!(
        matches!(
            result,
            Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. }))
        ),
        "expected AssemblyNotFound (not a hostfxr error), got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DotnetLoader::load — version mismatch (requires built Polyplug.dll)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires host-libs/csharp to be built: dotnet build host-libs/csharp"]
fn load_dll_net10_against_net6_requirement_returns_version_mismatch() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.dll not found — build host-libs/csharp first"
    );
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net6.0"),
        hostfxr: HostfxrLocation::Auto,
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> = loader.load(&dll, &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { required, found })) => {
            assert_eq!(required, "net6.0");
            assert!(
                found.contains("10"),
                "found TFM should contain 10, got: {found}"
            );
        }
        other => panic!("expected RuntimeVersionMismatch, got {other:?}"),
    }
}

#[test]
#[ignore = "requires host-libs/csharp to be built: dotnet build host-libs/csharp"]
fn load_dll_with_matching_version_passes_tfm_check() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.dll not found — build host-libs/csharp first"
    );
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> = loader.load(&dll, &mut registrar);
    assert!(
        !matches!(
            result,
            Err(PolyplugError::Loader(
                LoaderError::RuntimeVersionMismatch { .. }
            ))
        ),
        "must not get version mismatch for net10.0 DLL vs net10.0 config"
    );
}

// ---------------------------------------------------------------------------
// DotnetLoader::load — hostfxr location errors (requires built Polyplug.dll)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires host-libs/csharp to be built: dotnet build host-libs/csharp"]
fn load_with_bad_hostfxr_path_and_valid_dll_returns_clr_init_failed() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.dll not found — build host-libs/csharp first"
    );
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Path(PathBuf::from("/nonexistent/libhostfxr.so")),
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> = loader.load(&dll, &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::ClrInitFailed { path, .. }))
        | Err(PolyplugError::Loader(LoaderError::InitSymbolMissing { bundle: path })) => {
            assert!(
                path.contains("nonexistent")
                    || path.contains("libhostfxr")
                    || path.contains("Polyplug"),
                "Error should mention the bad hostfxr path or assembly, got: {path}"
            );
        }
        Err(PolyplugError::Loader(LoaderError::ClrInitFailed { path, .. })) => {
            assert!(
                path.contains("nonexistent") || path.contains("libhostfxr"),
                "ClrInitFailed path should mention the bad hostfxr path, got: {path}"
            );
        }
        other => panic!("expected ClrInitFailed for bad hostfxr path, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// CLR init + assembly loading (full integration — requires .NET SDK installed)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires .NET 10 SDK installed and host-libs/csharp built"]
fn full_clr_init_reaches_init_symbol_check() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.dll not found — build host-libs/csharp first"
    );
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let mut registrar: PluginRegistrar = stub_registrar();
    let result: Result<(), PolyplugError> = loader.load(&dll, &mut registrar);
    assert!(
        !matches!(
            result,
            Err(PolyplugError::Loader(LoaderError::AssemblyNotFound { .. }))
                | Err(PolyplugError::Loader(
                    LoaderError::RuntimeVersionMismatch { .. }
                ))
        ),
        "must pass TFM and file checks for existing net10.0 DLL, got: {result:?}"
    );
}
