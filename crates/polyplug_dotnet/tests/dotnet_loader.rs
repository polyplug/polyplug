//! Integration tests for polyplug_dotnet — .NET loader adapter.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use tempfile::NamedTempFile;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime_builder::RuntimeBuilder;
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_dotnet::version::read_target_framework;

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
            root.join("sdks")
                .join("csharp")
                .join("host")
                .join("bin")
                .join("Debug")
                .join("net10.0")
                .join("Polyplug.Host.dll")
        })
        .expect("CARGO_MANIFEST_DIR resolution failed")
}

fn test_runtime() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .build()
        .expect("failed to build test runtime")
}

fn make_manifest(path: &Path, name: &str) -> ManifestData {
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        runtime: "dotnet".to_owned(),
        file: path.file_name().unwrap().to_string_lossy().into_owned(),
        path: path.parent().unwrap().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// read_target_framework
// ---------------------------------------------------------------------------

#[test]
fn tfm_reader_nonexistent_file_returns_init_failed() {
    let result: Result<String, RuntimeError> =
        read_target_framework(Path::new("/nonexistent/path/that/does/not/exist.dll"));
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("PE") || error.contains("not found"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed, got {other:?}"),
    }
}

#[test]
fn tfm_reader_empty_file_returns_init_failed() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"");
    let result: Result<String, RuntimeError> = read_target_framework(tmp.path());
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("PE"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for empty file, got {other:?}"),
    }
}

#[test]
fn tfm_reader_random_bytes_returns_init_failed() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"\x00\x01\x02\x03this is not a valid PE binary");
    let result: Result<String, RuntimeError> = read_target_framework(tmp.path());
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("PE"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for junk bytes, got {other:?}"),
    }
}

#[test]
fn tfm_reader_elf_magic_returns_init_failed() {
    // ELF magic (0x7f 'E' 'L' 'F') is not a valid PE header — pelite rejects it.
    let tmp: NamedTempFile =
        temp_file_with_bytes(b"\x7fELF\x02\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00");
    let result: Result<String, RuntimeError> = read_target_framework(tmp.path());
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("PE"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for ELF magic, got {other:?}"),
    }
}

#[test]
fn tfm_reader_net10_dll_returns_correct_tfm() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(dll.exists(), "Polyplug.Host.dll not found at {dll:?}");
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
fn load_nonexistent_assembly_returns_init_failed() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let path: PathBuf = PathBuf::from("/does/not/exist/Plugin.dll");
    let manifest: ManifestData = make_manifest(&path, "nonexistent");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("not found"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed, got {other:?}"),
    }
}

#[test]
fn load_invalid_pe_file_returns_init_failed() {
    let tmp: NamedTempFile = temp_file_with_bytes(b"not a valid PE binary at all");
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(tmp.path(), "invalid_pe");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("assembly") || error.contains("PE"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for invalid PE, got {other:?}"),
    }
}

#[test]
fn load_with_invalid_hostfxr_path_and_missing_dll_returns_init_failed() {
    // InitFailed fires before hostfxr is consulted — verifies load() call ordering.
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Path(PathBuf::from("/nonexistent/libhostfxr.so")),
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let runtime: Arc<Runtime> = test_runtime();
    let path: PathBuf = PathBuf::from("/no/such/Plugin.dll");
    let manifest: ManifestData = make_manifest(&path, "missing_dll");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. }))
        ),
        "expected InitFailed (not a hostfxr error), got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// DotnetLoader::load — version mismatch (requires built Polyplug.dll)
// ---------------------------------------------------------------------------

#[test]
fn load_dll_net10_against_net6_requirement_returns_init_failed() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.Host.dll not found — build sdks/csharp/host first"
    );
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net6.0"),
        hostfxr: HostfxrLocation::Auto,
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(&dll, "Polyplug");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle: _, error })) => {
            assert!(
                error.contains("version") || error.contains("framework"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for version mismatch, got {other:?}"),
    }
}

#[test]
fn load_dll_with_matching_version_passes_tfm_check() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.Host.dll not found — build sdks/csharp/host first"
    );
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(&dll, "Polyplug");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        !matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. }))
        ),
        "must not get InitFailed for matching version"
    );
}

// ---------------------------------------------------------------------------
// DotnetLoader::load — hostfxr location errors (requires built Polyplug.dll)
// ---------------------------------------------------------------------------

#[test]
fn load_with_bad_hostfxr_path_and_valid_dll_is_rejected() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.Host.dll not found — build sdks/csharp/host first"
    );
    let cfg: DotnetConfig = DotnetConfig {
        min_framework: String::from("net10.0"),
        hostfxr: HostfxrLocation::Path(PathBuf::from("/nonexistent/libhostfxr.so")),
    };
    let loader: DotnetLoader = DotnetLoader::new(cfg);
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(&dll, "Polyplug");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    // CLR_CONTEXT is a process-wide OnceCell (see "Known Limitations" in CLAUDE.md):
    // the CLR is initialized exactly once per process by whichever load runs first.
    // This load can therefore fail at one of two stages, both of which are correct
    // loader rejections:
    //   1. If this is the first .NET load in the process, the bad hostfxr path is
    //      honored and CLR init fails → InitFailed.
    //   2. If the CLR was already initialized (by an earlier test in the same binary)
    //      the bad path is ignored — the existing context is reused — and the load
    //      reaches symbol resolution, where Polyplug.Host.dll is correctly rejected
    //      because it is a host SDK assembly with no guest `PolyplugInit` entry point
    //      → InitSymbolMissing.
    // The contract under test is that the loader rejects the bundle, not which of the
    // two ordering-dependent variants it produces.
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle, error })) => {
            assert!(
                error.contains("hostfxr")
                    || error.contains("host")
                    || bundle.contains("Polyplug")
                    || error.contains("init"),
                "InitFailed should mention the issue, got bundle={bundle}, error={error}"
            );
        }
        Err(RuntimeError::Loader(LoaderError::InitSymbolMissing { bundle })) => {
            assert!(
                bundle.contains("Polyplug"),
                "InitSymbolMissing should reference the assembly, got bundle={bundle}"
            );
        }
        other => {
            panic!("expected loader rejection (InitFailed or InitSymbolMissing), got {other:?}")
        }
    }
}

// ---------------------------------------------------------------------------
// CLR init + assembly loading (full integration — requires .NET SDK installed)
// ---------------------------------------------------------------------------

#[test]
fn full_clr_init_reaches_init_symbol_check() {
    let dll: PathBuf = polyplug_dll_path();
    assert!(
        dll.exists(),
        "Polyplug.Host.dll not found — build sdks/csharp/host first"
    );
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(&dll, "Polyplug");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        !matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. }))
        ),
        "must pass TFM and file checks for existing net10.0 DLL, got: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// BundleSource::Bytes — in-memory assembly loading
// ---------------------------------------------------------------------------

/// Args layout for the `test.add` contract's `add` function: two u32s.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

/// Path to the built `CsharpPlugin.dll` test fixture (Debug build).
///
/// The fixture lives at `<workspace>/tests/fixtures/csharp_plugin`. It is built by the
/// integration test build script (`crates/polyplug/build.rs`), not by this crate, so tests
/// that depend on it soft-skip when the DLL is absent.
fn csharp_fixture_dll_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p: &Path| p.parent())
        .map(|root: &Path| {
            root.join("tests")
                .join("fixtures")
                .join("csharp_plugin")
                .join("bin")
                .join("Debug")
                .join("net10.0")
                .join("CsharpPlugin.dll")
        })
        .expect("CARGO_MANIFEST_DIR resolution failed")
}

/// Build a manifest for the C# `test.add` fixture, matching `manifest.toml`. The
/// `id` MUST equal `bundle_id(name)` for `Runtime::load_bundle_from_source` validation.
fn make_csharp_fixture_manifest(dir: &Path) -> ManifestData {
    let name: &str = "csharp_test_adder";
    let mut function_count: HashMap<String, u32> = HashMap::new();
    function_count.insert("test.add@1".to_owned(), 4);
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        runtime: "dotnet".to_owned(),
        file: "CsharpPlugin.dll".to_owned(),
        path: dir.to_path_buf(),
        version: "1.0.0".to_owned(),
        provides: vec!["test.add@1".to_owned()],
        function_count,
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// `Code` is unsupported by the .NET loader (compiled language): it must yield a
/// structured `UnsupportedBundleSource` error rather than attempting any load.
#[test]
fn code_source_returns_unsupported_bundle_source() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData = make_manifest(Path::new("/tmp/x/Plugin.dll"), "csharp_code_x");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Code(String::from("// not loadable: .NET is compiled")),
        &runtime,
    );
    match result {
        Err(RuntimeError::Loader(LoaderError::UnsupportedBundleSource {
            loader: l,
            source_kind,
            bundle,
        })) => {
            assert_eq!(l, "dotnet");
            assert_eq!(source_kind, "code");
            assert_eq!(bundle, "csharp_code_x");
        }
        other => panic!("expected UnsupportedBundleSource for Code, got {other:?}"),
    }
}

/// `Bytes` with non-PE content must fail PE parsing before any CLR interaction.
#[test]
fn bytes_source_invalid_pe_returns_init_failed() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let manifest: ManifestData =
        make_manifest(Path::new("/tmp/x/CsharpPlugin.dll"), "csharp_bytes_bad");
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Bytes(b"not a PE file".to_vec()),
        &runtime,
    );
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { error, .. })) => {
            assert!(
                error.contains("PE") || error.contains("assembly"),
                "error: {error}"
            );
        }
        other => panic!("expected InitFailed for invalid PE bytes, got {other:?}"),
    }
}

/// `Bytes` missing `manifest.file` cannot derive the assembly simple name → structured error.
#[test]
fn bytes_source_missing_manifest_file_returns_manifest_missing_file() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    let runtime: Arc<Runtime> = test_runtime();
    let mut manifest: ManifestData = make_manifest(Path::new("/tmp/x/x.dll"), "csharp_no_file");
    manifest.file = String::new();
    let result: Result<(), RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Bytes(vec![0u8; 16]),
        &runtime,
    );
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(
                LoaderError::ManifestMissingFile { .. }
            ))
        ),
        "expected ManifestMissingFile, got {result:?}"
    );
}

/// Full Bytes integration: load the built C# fixture from raw bytes through
/// `Runtime::load_bundle_from_source`, then resolve `test.add@1` and dispatch
/// `add(3, 5)`, asserting parity with the Path-loaded result (== 8).
///
/// Soft-skips when the fixture DLL is not built (it is produced by the integration
/// test build script, not by this crate's build).
#[test]
fn bytes_source_loads_fixture_and_dispatches() {
    let dll: PathBuf = csharp_fixture_dll_path();
    if !dll.exists() {
        eprintln!("skipping: CsharpPlugin.dll fixture not built at {dll:?}");
        return;
    }
    let bytes: Vec<u8> = std::fs::read(&dll).expect("failed to read fixture dll");
    let bundle_dir: &Path = dll.parent().expect("fixture dll must have a parent dir");

    // The fixture is NOT self-contained: CsharpPlugin.dll references Polyplug.Abi. Pre-load
    // that dependency from bytes into the shared CLR default load context so the in-memory
    // plugin can resolve it. CLR_CONTEXT is process-global, so this standalone loader and
    // the runtime's loader (built below) share the same context + byte bridge.
    let abi_dll: PathBuf = bundle_dir.join("Polyplug.Abi.dll");
    if !abi_dll.exists() {
        eprintln!("skipping: Polyplug.Abi.dll dependency not built next to fixture");
        return;
    }
    let abi_bytes: Vec<u8> = std::fs::read(&abi_dll).expect("failed to read Polyplug.Abi.dll");
    let preloader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    preloader
        .preload_dependency_from_bytes(&abi_bytes)
        .expect("preload Polyplug.Abi dependency from bytes");

    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .expect("failed to build runtime with dotnet loader");

    let manifest: ManifestData = make_csharp_fixture_manifest(bundle_dir);
    let load_result: Result<(), RuntimeError> =
        runtime.load_bundle_from_source(manifest, polyplug::loader::BundleSource::Bytes(bytes));
    assert!(
        load_result.is_ok(),
        "load_bundle_from_source(Bytes) failed: {:?}",
        load_result.err()
    );

    // Resolve test.add@1 and dispatch add(3, 5) == 8 (native dispatch parity with Path).
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);
    let handle: polyplug_abi::GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after byte-source load");
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .resolve_guest_contract(handle)
        .expect("handle must resolve to an interface");
    assert!(!interface_ptr.is_null(), "interface must be non-null");

    // SAFETY: interface_ptr is valid; the CLR keeps the byte-loaded assembly alive for the
    // process lifetime. The C# fixture uses native dispatch (function pointer table).
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        polyplug_abi::DispatchType::Native,
        "C# fixture must use native dispatch"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0;
    // SAFETY: functions[0] is the `add` wrapper for the test.add contract.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is the add wrapper; its ABI is the generic native dispatch signature.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> polyplug_abi::AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args and out point to valid, correctly-typed storage for AddArgs/u32.
    let result: polyplug_abi::AbiError = unsafe {
        dispatch_fn(
            core::ptr::addr_of!(args) as *const (),
            core::ptr::addr_of_mut!(out) as *mut (),
        )
    };
    assert_eq!(
        result.code,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(
        out, 8,
        "add(3, 5) must equal 8 (Bytes-source dispatch parity)"
    );
}
