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
        loader: "dotnet".to_owned(),
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
fn dotnet_loader_loader_name_is_dotnet() {
    let loader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    assert_eq!(loader.loader_name(), "dotnet");
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
        loader: "dotnet".to_owned(),
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

    // The fixture is NOT self-contained: CsharpPlugin.dll references Polyplug.Abi and
    // Polyplug.Guest. Pre-load those dependencies from bytes into the SAME (runtime, bundle)
    // collectible ALC so the in-memory plugin can resolve them. The runtime must therefore
    // exist BEFORE preloading — the ALC key includes the runtime id. CLR_CONTEXT is
    // process-global, so this standalone loader and the runtime's loader share the same
    // context + byte bridge.
    let manifest: ManifestData = make_csharp_fixture_manifest(bundle_dir);

    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .expect("failed to build runtime with dotnet loader");

    let preloader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    for dep_name in ["Polyplug.Abi.dll", "Polyplug.Guest.dll"] {
        let dep_dll: PathBuf = bundle_dir.join(dep_name);
        if !dep_dll.exists() {
            eprintln!("skipping: {dep_name} dependency not built next to fixture");
            return;
        }
        let dep_bytes: Vec<u8> = std::fs::read(&dep_dll)
            .unwrap_or_else(|e: std::io::Error| panic!("failed to read {dep_name}: {e}"));
        preloader
            .preload_dependency_from_bytes(&runtime, manifest.id, &dep_bytes)
            .unwrap_or_else(|e: RuntimeError| {
                panic!("preload {dep_name} dependency from bytes: {e:?}")
            });
    }

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

/// Build a fixture manifest with a caller-chosen bundle `name` (and thus a unique bundle id).
/// Per-bundle collectible ALCs are keyed by bundle id and the CLR is process-global, so each
/// unload test must use its own id to stay isolated from other tests running concurrently.
fn make_named_fixture_manifest(dir: &Path, name: &str) -> ManifestData {
    let mut function_count: HashMap<String, u32> = HashMap::new();
    function_count.insert("test.add@1".to_owned(), 4);
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        loader: "dotnet".to_owned(),
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

/// Load the C# fixture (Bytes source) under a chosen `name`, returning the runtime
/// and bundle id, or `None` (soft-skip) if the fixture/dependency is not built.
fn load_named_fixture(name: &str) -> Option<(Arc<Runtime>, u64)> {
    let dll: PathBuf = csharp_fixture_dll_path();
    if !dll.exists() {
        eprintln!("skipping: CsharpPlugin.dll fixture not built at {dll:?}");
        return None;
    }
    let bytes: Vec<u8> = std::fs::read(&dll).expect("failed to read fixture dll");
    let bundle_dir: &Path = dll.parent().expect("fixture dll must have a parent dir");
    let manifest: ManifestData = make_named_fixture_manifest(bundle_dir, name);
    let bundle_id: u64 = manifest.id;

    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .expect("failed to build runtime");

    // Preload the shared dependencies into THIS (runtime, bundle) collectible ALC — the
    // runtime must exist first because the ALC key includes the runtime id.
    let preloader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    for dep_name in ["Polyplug.Abi.dll", "Polyplug.Guest.dll"] {
        let dep_dll: PathBuf = bundle_dir.join(dep_name);
        if !dep_dll.exists() {
            eprintln!("skipping: {dep_name} dependency not built next to fixture");
            return None;
        }
        let dep_bytes: Vec<u8> = std::fs::read(&dep_dll)
            .unwrap_or_else(|e: std::io::Error| panic!("failed to read {dep_name}: {e}"));
        preloader
            .preload_dependency_from_bytes(&runtime, bundle_id, &dep_bytes)
            .unwrap_or_else(|e: RuntimeError| panic!("preload {dep_name} dependency: {e:?}"));
    }
    runtime
        .load_bundle_from_source(manifest, polyplug::loader::BundleSource::Bytes(bytes))
        .expect("load_bundle_from_source(Bytes) failed");
    Some((runtime, bundle_id))
}

/// Dispatch `add(3,5)` on the loaded fixture, asserting the result is 8 — proves the bundle's
/// collectible ALC is live and dispatch through it works.
fn assert_add_dispatch_works(runtime: &Runtime) {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);
    let handle: polyplug_abi::GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .resolve_guest_contract(handle)
        .expect("handle must resolve");
    // SAFETY: interface_ptr is a live, non-null interface for the loaded bundle.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0;
    // SAFETY: functions[0] is the `add` wrapper; its ABI is the generic native dispatch signature.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is the add wrapper; its ABI is the generic native dispatch signature.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> polyplug_abi::AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args/out point to valid storage for AddArgs/u32.
    let result: polyplug_abi::AbiError = unsafe {
        dispatch_fn(
            core::ptr::addr_of!(args) as *const (),
            core::ptr::addr_of_mut!(out) as *mut (),
        )
    };
    assert_eq!(result.code, polyplug_abi::AbiErrorCode::Ok as u32);
    assert_eq!(out, 8, "add(3,5) must equal 8 before unload");
}

/// Unloading a .NET bundle truly unloads its collectible `AssemblyLoadContext` — the
/// managed assemblies become eligible for GC and the liveness probe reports the ALC
/// reclaimed. The contract is also gone from the registry. Reclaim is uniform: there is
/// no retire-not-drop branch, so unload always reclaims rather than parking the ALC.
#[test]
fn unload_reclaims_alc() {
    let Some((runtime, bundle_id)) = load_named_fixture("csharp_reclaim_probe") else {
        return; // soft-skip: fixture not built
    };

    assert_add_dispatch_works(&runtime);
    assert!(
        polyplug_dotnet::bundle_alc_alive(&runtime, bundle_id).expect("alc probe"),
        "ALC must be alive while the bundle is loaded"
    );

    runtime
        .unload_bundle(polyplug_utils::BundleId::from_u64(bundle_id))
        .expect("unload_bundle must succeed");

    assert!(
        !polyplug_dotnet::bundle_alc_alive(&runtime, bundle_id).expect("alc probe"),
        "ALC must be reclaimed after unload"
    );
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);
    assert!(
        runtime.find_guest_contract(contract_id, 0).is_err(),
        "the contract must no longer resolve after unload"
    );
}

/// Two `Runtime`s loading the SAME bundle id get structurally isolated collectible ALCs:
/// the managed bridge keys contexts by (runtime id, bundle id), so unloading the bundle
/// from one runtime reclaims only that runtime's ALC while the other runtime's copy stays
/// alive and dispatchable. This is the regression test for the last-writer-wins ALC race
/// the composite key removes.
#[test]
fn two_runtimes_same_bundle_id_have_isolated_alcs() {
    let dll: PathBuf = csharp_fixture_dll_path();
    if !dll.exists() {
        eprintln!("skipping: CsharpPlugin.dll fixture not built at {dll:?}");
        return;
    }
    let bytes: Vec<u8> = std::fs::read(&dll).expect("failed to read fixture dll");
    let bundle_dir: &Path = dll.parent().expect("fixture dll must have a parent dir");
    // ONE shared name → ONE shared bundle id across both runtimes.
    let manifest_a: ManifestData = make_named_fixture_manifest(bundle_dir, "csharp_two_rt_probe");
    let manifest_b: ManifestData = make_named_fixture_manifest(bundle_dir, "csharp_two_rt_probe");
    let bundle_id: u64 = manifest_a.id;

    let rt_a: Arc<Runtime> = RuntimeBuilder::new()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .expect("failed to build runtime A");
    let rt_b: Arc<Runtime> = RuntimeBuilder::new()
        .loader(DotnetLoader::new(DotnetConfig::default()))
        .build()
        .expect("failed to build runtime B");

    // Preload the shared dependencies into EACH runtime's own (runtime, bundle)
    // ALC — the composite key means each runtime needs its own preload pass.
    let preloader: DotnetLoader = DotnetLoader::new(DotnetConfig::default());
    for rt in [&rt_a, &rt_b] {
        for dep_name in ["Polyplug.Abi.dll", "Polyplug.Guest.dll"] {
            let dep_dll: PathBuf = bundle_dir.join(dep_name);
            if !dep_dll.exists() {
                eprintln!("skipping: {dep_name} dependency not built next to fixture");
                return;
            }
            let dep_bytes: Vec<u8> = std::fs::read(&dep_dll)
                .unwrap_or_else(|e: std::io::Error| panic!("failed to read {dep_name}: {e}"));
            preloader
                .preload_dependency_from_bytes(rt, bundle_id, &dep_bytes)
                .unwrap_or_else(|e: RuntimeError| panic!("preload {dep_name}: {e:?}"));
        }
    }

    rt_a.load_bundle_from_source(
        manifest_a,
        polyplug::loader::BundleSource::Bytes(bytes.clone()),
    )
    .expect("runtime A must load the bundle");
    rt_b.load_bundle_from_source(manifest_b, polyplug::loader::BundleSource::Bytes(bytes))
        .expect("runtime B must load the same bundle id");

    // Both runtimes hold their own live ALC for the same bundle id.
    assert!(
        polyplug_dotnet::bundle_alc_alive(&rt_a, bundle_id).expect("alc probe A"),
        "runtime A's ALC must be alive after load"
    );
    assert!(
        polyplug_dotnet::bundle_alc_alive(&rt_b, bundle_id).expect("alc probe B"),
        "runtime B's ALC must be alive after load"
    );
    assert_add_dispatch_works(&rt_a);
    assert_add_dispatch_works(&rt_b);

    // Reclaim the bundle in runtime A only.
    rt_a.unload_bundle(polyplug_utils::BundleId::from_u64(bundle_id))
        .expect("runtime A unload must succeed");

    assert!(
        !polyplug_dotnet::bundle_alc_alive(&rt_a, bundle_id).expect("alc probe A"),
        "runtime A's ALC must be reclaimed after its unload"
    );
    assert!(
        polyplug_dotnet::bundle_alc_alive(&rt_b, bundle_id).expect("alc probe B"),
        "runtime B's ALC must survive runtime A's unload — composite keying"
    );
    // Runtime B's copy still dispatches.
    assert_add_dispatch_works(&rt_b);
}
