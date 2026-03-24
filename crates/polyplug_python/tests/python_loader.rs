// Integration tests for the polyplug_python PythonLoader.
//
// Covers: interpreter init, valid module loading, syntax error, import error,
// missing polyplug_init, vtable registration, GIL acquisition, and thread safety.
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::PluginHandle;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Write `content` into a temp bundle directory with manifest.toml.
/// Returns the directory (to keep it alive) and the path to bundle.py.
fn write_bundle(name: &str, content: &str) -> (TempDir, PathBuf) {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: PathBuf = dir.path().join("bundle.py");
    fs::write(&path, content).expect("write bundle.py");

    let bundle_id: u64 = polyplug_abi::bundle_id(name);
    let manifest: String = format!(
        r#"id = {}
name = "{}"
runtime = "python"
file = "bundle.py"
"#,
        bundle_id, name
    );
    fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, path)
}

/// Create a minimal Runtime with the PythonLoader registered.
fn make_runtime() -> Runtime {
    RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build must succeed")
}

/// Create a ManifestData for a Python bundle.
fn make_manifest(path: &PathBuf, name: &str) -> ManifestData {
    ManifestData {
        id: polyplug_abi::bundle_id(name),
        name: name.to_owned(),
        runtime: "python".to_owned(),
        file: path.file_name().unwrap().to_string_lossy().into_owned(),
        path: path.parent().unwrap().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    }
}

/// A Python plugin source that registers one vtable via `polyplug_init`.
///
/// The ctypes bridge mirrors the ABI contract expected by PythonLoader:
///   `polyplug_init(rt_ctx: int, host_vtable: int, ctx: int) -> None`
const VALID_PLUGIN_SRC: &str = r#"
import ctypes

class _StringView(ctypes.Structure):
    _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

class _AbiError(ctypes.Structure):
    _fields_ = [
        ("code",    ctypes.c_uint32),
        ("_pad",    ctypes.c_uint32),
        ("message", _StringView),
    ]

class _PluginDescriptor(ctypes.Structure):
    _fields_ = [
        ("name",          _StringView),
        ("contract_name", _StringView),
        ("version_major", ctypes.c_uint32),
        ("version_minor", ctypes.c_uint32),
        ("version_patch", ctypes.c_uint32),
    ]

# New PluginInterface structure matching the current ABI
class _NativeDispatch(ctypes.Structure):
    _fields_ = [("functions", ctypes.c_void_p)]

class _VmDispatch(ctypes.Structure):
    _fields_ = [
        ("call",        ctypes.c_void_p),
        ("loader_data", ctypes.c_void_p),
    ]

class _PluginDispatch(ctypes.Union):
    _fields_ = [
        ("native", _NativeDispatch),
        ("vm",     _VmDispatch),
    ]

class _PluginInterface(ctypes.Structure):
    _fields_ = [
        ("rt_ctx",          ctypes.c_void_p),
        ("contract_id",     ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count",  ctypes.c_uint32),
        ("dispatch_type",   ctypes.c_uint32),  # 0 = Native, 1 = VM
        ("_pad",            ctypes.c_uint32),
        ("dispatch",        _PluginDispatch),
    ]

_NAME_BYTES        = b"test_plugin\x00"
_CONTRACT_BYTES    = b"test.contract\x00"
_FUNCTIONS_ARR     = (ctypes.c_void_p * 0)()

_DESC = _PluginDescriptor()
_DESC.name.ptr          = ctypes.cast(ctypes.c_char_p(_NAME_BYTES), ctypes.c_void_p).value
_DESC.name.len          = len(_NAME_BYTES) - 1
_DESC.contract_name.ptr = ctypes.cast(ctypes.c_char_p(_CONTRACT_BYTES), ctypes.c_void_p).value
_DESC.contract_name.len = len(_CONTRACT_BYTES) - 1
_DESC.version_major = 1
_DESC.version_minor = 0
_DESC.version_patch = 0

_INTERFACE = _PluginInterface()
_INTERFACE.rt_ctx          = None
_INTERFACE.contract_id     = 0xDEADBEEFCAFEBABE
_INTERFACE.contract_version = 0
_INTERFACE.function_count  = 0
_INTERFACE.dispatch_type   = 0  # Native
_INTERFACE._pad            = 0
_INTERFACE.dispatch.native.functions = ctypes.cast(_FUNCTIONS_ARR, ctypes.c_void_p).value

# HostVTable function pointer types
_RegisterFn = ctypes.CFUNCTYPE(
    _AbiError,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_void_p,
)
_AllocFn = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t)
_FreeFn = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t)
_FindByContractFn = ctypes.CFUNCTYPE(ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32)
_FindByBundleFn = ctypes.CFUNCTYPE(ctypes.c_uint64, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint64, ctypes.c_uint32)
_FindAllByContractFn = ctypes.CFUNCTYPE(ctypes.c_size_t, ctypes.c_void_p, ctypes.c_uint64, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_size_t)
_ResolvePluginFn = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint64)
_GetExtensionFn = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint32)

class _HostVTable(ctypes.Structure):
    _fields_ = [
        ("register_plugin", _RegisterFn),
        ("alloc", _AllocFn),
        ("free", _FreeFn),
        ("find_by_contract", _FindByContractFn),
        ("find_by_bundle", _FindByBundleFn),
        ("find_all_by_contract", _FindAllByContractFn),
        ("resolve_plugin", _ResolvePluginFn),
        ("get_extension", _GetExtensionFn),
    ]

def polyplug_init(rt_ctx: int, host_vtable: int, _ctx: int) -> None:
    host = _HostVTable.from_address(host_vtable)
    host.register_plugin(
        ctypes.c_void_p(rt_ctx),
        ctypes.addressof(_DESC),
        ctypes.addressof(_INTERFACE),
    )
"#;

/// A Python plugin that defines `polyplug_init` but raises an exception inside it.
const RAISING_PLUGIN_SRC: &str = r#"
def polyplug_init(_rt_ctx: int, _host_vtable: int, _ctx: int) -> None:
    raise RuntimeError("intentional test error")
"#;

/// A Python plugin that is missing `polyplug_init` entirely.
const MISSING_INIT_PLUGIN_SRC: &str = r#"
def not_polyplug_init():
    pass
"#;

/// A Python file with a syntax error.
const SYNTAX_ERROR_PLUGIN_SRC: &str = r#"
def polyplug_init(:
    pass
"#;

/// A Python plugin that imports a module that does not exist.
const IMPORT_ERROR_PLUGIN_SRC: &str = r#"
import _polyplug_nonexistent_module_xyz_123456

def polyplug_init(_rt_ctx: int, _host_vtable: int, _ctx: int) -> None:
    pass
"#;

/// A minimal no-op plugin (defines polyplug_init but does nothing).
const NOOP_PLUGIN_SRC: &str = r#"
def polyplug_init(_rt_ctx: int, _host_vtable: int, _ctx: int) -> None:
    pass
"#;

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Python interpreter is initialized exactly once and does not panic on the first call.
#[test]
fn test_interpreter_initializes_without_panic() {
    let (_dir, path) = write_bundle("noop_init", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "noop_init");
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "unexpected error: {result:?}");
}

/// `PythonConfig::default()` requires Python ≥ 3.11.  Loading a valid plugin
/// must succeed when the host Python satisfies this requirement.
#[test]
fn test_default_config_version_check_passes() {
    let (_dir, path) = write_bundle("ver_check", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let runtime: Runtime = RuntimeBuilder::new()
        .loader(loader)
        .build()
        .expect("runtime build must succeed");
    let manifest: ManifestData = make_manifest(&path, "ver_check");
    let result: Result<(), PolyplugError> =
        PythonLoader::new(PythonConfig::default()).load(&manifest, &runtime);
    assert!(
        result.is_ok(),
        "version check failed unexpectedly: {result:?}"
    );
}

/// A pathologically high minimum version requirement (`(99, 0)`) must fail with
/// `LoaderError::RuntimeVersionMismatch`.
#[test]
fn test_version_too_old_returns_version_mismatch() {
    let (_dir, path) = write_bundle("ver_mismatch", NOOP_PLUGIN_SRC);
    let config: PythonConfig = PythonConfig {
        min_version: (99, 0),
    };
    let loader: PythonLoader = PythonLoader::new(config.clone());
    let runtime: Runtime = RuntimeBuilder::new()
        .loader(loader)
        .build()
        .expect("runtime build must succeed");
    let manifest: ManifestData = make_manifest(&path, "ver_mismatch");
    let err: PolyplugError = PythonLoader::new(config)
        .load(&manifest, &runtime)
        .expect_err("expected version mismatch");
    match err {
        PolyplugError::Loader(LoaderError::RuntimeVersionMismatch { required, found }) => {
            assert_eq!(required, "99.0", "required mismatch");
            assert!(
                !found.is_empty(),
                "found version string must not be empty; got: {found}"
            );
        }
        other => panic!("expected RuntimeVersionMismatch, got: {other:?}"),
    }
}

/// Loading a valid plugin whose `polyplug_init` calls `register_plugin` once must
/// register the plugin in the registry.
#[test]
fn test_valid_plugin_registers_in_registry() {
    let (_dir, path) = write_bundle("valid_plugin", VALID_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "valid_plugin");
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "load failed: {result:?}");
    // The plugin registers with contract_id = 0xDEADBEEFCAFEBABE
    let contract_id: u64 = 0xDEADBEEFCAFEBABE;
    let handle: Result<PluginHandle, polyplug::error::RegistryError> =
        runtime.registry().find(contract_id, 0);
    assert!(handle.is_ok(), "plugin must be registered in registry");
}

/// A plugin with a syntax error must return `PythonModuleImportFailed`.
#[test]
fn test_syntax_error_returns_module_import_failed() {
    let (_dir, path) = write_bundle("syntax_err", SYNTAX_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "syntax_err");
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected failure for syntax error plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { path: p, reason }) => {
            assert!(
                p.contains("bundle.py"),
                "path should mention bundle.py; got: {p}"
            );
            assert!(!reason.is_empty(), "reason must not be empty");
        }
        other => panic!("expected PythonModuleImportFailed, got: {other:?}"),
    }
}

/// A plugin that imports a missing module must return `PythonModuleImportFailed`.
#[test]
fn test_import_error_returns_module_import_failed() {
    let (_dir, path) = write_bundle("import_err", IMPORT_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "import_err");
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected failure for import-error plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { path: p, reason }) => {
            assert!(
                p.contains("bundle.py"),
                "path should mention bundle.py; got: {p}"
            );
            assert!(
                reason.contains("_polyplug_nonexistent_module_xyz_123456")
                    || reason.contains("No module named"),
                "reason should mention the missing module; got: {reason}"
            );
        }
        other => panic!("expected PythonModuleImportFailed, got: {other:?}"),
    }
}

/// A plugin that is missing `polyplug_init` must return `InitSymbolMissing`.
#[test]
fn test_missing_init_returns_init_symbol_missing() {
    let (_dir, path) = write_bundle("no_init", MISSING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "no_init");
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected failure for plugin missing polyplug_init");
    match err {
        PolyplugError::Loader(LoaderError::InitSymbolMissing { bundle }) => {
            assert!(
                bundle.contains("bundle"),
                "bundle name should contain 'bundle'; got: {bundle}"
            );
        }
        other => panic!("expected InitSymbolMissing, got: {other:?}"),
    }
}

/// A `polyplug_init` that raises a Python exception must return `PythonInitRaisedException`.
#[test]
fn test_raising_init_returns_init_raised_exception() {
    let (_dir, path) = write_bundle("raising_init", RAISING_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "raising_init");
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected failure for raising plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonInitRaisedException { bundle, message }) => {
            assert!(
                bundle.contains("bundle"),
                "bundle name should contain 'bundle'; got: {bundle}"
            );
            assert!(
                message.contains("intentional test error"),
                "message should contain the Python exception text; got: {message}"
            );
        }
        other => panic!("expected PythonInitRaisedException, got: {other:?}"),
    }
}

/// Loading a `.py` file that does not exist must return `PythonModuleImportFailed`
/// (path canonicalization fails).
#[test]
fn test_nonexistent_path_returns_import_failed() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = ManifestData {
        id: 0,
        name: "does_not_exist".to_owned(),
        runtime: "python".to_owned(),
        file: "does_not_exist.py".to_owned(),
        path: dir.path().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected failure for nonexistent path");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { .. }) => {}
        other => panic!("expected PythonModuleImportFailed, got: {other:?}"),
    }
}

/// The GIL is properly acquired and released: multiple sequential loads must all succeed.
#[test]
fn test_gil_released_between_sequential_loads() {
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    for i in 0u32..4u32 {
        let name: String = format!("gil_seq_{i}");
        let (_dir, path) = write_bundle(&name, NOOP_PLUGIN_SRC);
        let manifest: ManifestData = make_manifest(&path, &name);
        let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
        assert!(result.is_ok(), "sequential load {i} failed: {result:?}");
    }
}

/// `PythonLoader` is `Send + Sync` — verify at compile time.
#[test]
fn test_loader_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PythonLoader>();
}

/// Loading a valid plugin from two different loaders in the same process must both
/// succeed (interpreter already initialized on the second call — OnceLock no-op).
#[test]
fn test_second_loader_reuses_interpreter() {
    let (_dir1, path1) = write_bundle("reuse1", NOOP_PLUGIN_SRC);
    let (_dir2, path2) = write_bundle("reuse2", NOOP_PLUGIN_SRC);

    let loader_a: PythonLoader = PythonLoader::default();
    let loader_b: PythonLoader = PythonLoader::default();

    let runtime_a: Runtime = make_runtime();
    let runtime_b: Runtime = make_runtime();

    let manifest1: ManifestData = make_manifest(&path1, "reuse1");
    let manifest2: ManifestData = make_manifest(&path2, "reuse2");
    let result_a: Result<(), PolyplugError> = loader_a.load(&manifest1, &runtime_a);
    let result_b: Result<(), PolyplugError> = loader_b.load(&manifest2, &runtime_b);

    assert!(result_a.is_ok(), "first loader failed: {result_a:?}");
    assert!(
        result_b.is_ok(),
        "second loader failed (should reuse init): {result_b:?}"
    );
}

/// `runtime_name()` returns `"python"`.
#[test]
fn test_runtime_name() {
    let loader: PythonLoader = PythonLoader::default();
    assert_eq!(loader.runtime_name(), "python");
}

/// A plugin file whose name contains an underscore and digits loads correctly.
#[test]
fn test_plugin_path_with_underscore_digits_loads() {
    let (_dir, path) = write_bundle("plugin_42_ok", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "plugin_42_ok");
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(
        result.is_ok(),
        "underscore-digit stem load failed: {result:?}"
    );
}

/// Verify that `bundle_dir` is inserted into `sys.path` by the loader, allowing
/// a plugin to `import` a sibling `.py` file placed in the same directory.
#[test]
fn test_bundle_dir_added_to_sys_path() {
    let dir: TempDir = TempDir::new().expect("tempdir");

    let helper_src: &str = "HELPER_VALUE = 42\n";
    let helper_path: PathBuf = dir.path().join("my_helper.py");
    fs::write(&helper_path, helper_src).expect("write helper");

    let plugin_src: &str = r#"
import my_helper

def polyplug_init(_rt_ctx: int, _host_vtable: int, _ctx: int) -> None:
    assert my_helper.HELPER_VALUE == 42
"#;
    let path: PathBuf = dir.path().join("bundle.py");
    fs::write(&path, plugin_src).expect("write bundle.py");

    let manifest_toml: String = format!(
        r#"id = {}
name = "imports_helper"
runtime = "python"
file = "bundle.py"
"#,
        polyplug_abi::bundle_id("imports_helper")
    );
    fs::write(dir.path().join("manifest.toml"), &manifest_toml).expect("write manifest.toml");

    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = ManifestData {
        id: polyplug_abi::bundle_id("imports_helper"),
        name: "imports_helper".to_owned(),
        runtime: "python".to_owned(),
        file: "bundle.py".to_owned(),
        path: dir.path().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "sibling import failed: {result:?}");
}

/// Verify that a `site-packages/` sub-directory (if present) is also added to
/// `sys.path`, allowing the plugin to import packages from it.
#[test]
fn test_site_packages_dir_added_to_sys_path() {
    let dir: TempDir = TempDir::new().expect("tempdir");

    let sp_dir: PathBuf = dir.path().join("site-packages");
    fs::create_dir_all(&sp_dir).expect("create site-packages");
    fs::write(sp_dir.join("fakelib.py"), "FAKE = 99\n").expect("write fakelib");

    let plugin_src: &str = r#"
import fakelib

def polyplug_init(_rt_ctx: int, _host_vtable: int, _ctx: int) -> None:
    assert fakelib.FAKE == 99
"#;
    let path: PathBuf = dir.path().join("bundle.py");
    fs::write(&path, plugin_src).expect("write bundle.py");

    let manifest_toml: String = format!(
        r#"id = {}
name = "uses_site_pkg"
runtime = "python"
file = "bundle.py"
"#,
        polyplug_abi::bundle_id("uses_site_pkg")
    );
    fs::write(dir.path().join("manifest.toml"), &manifest_toml).expect("write manifest.toml");

    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = ManifestData {
        id: polyplug_abi::bundle_id("uses_site_pkg"),
        name: "uses_site_pkg".to_owned(),
        runtime: "python".to_owned(),
        file: "bundle.py".to_owned(),
        path: dir.path().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
    };
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "site-packages import failed: {result:?}");
}

/// Verify error from `polyplug_init` contains the bundle name (file stem) so
/// consumers can identify which plugin caused the failure.
#[test]
fn test_error_contains_bundle_name() {
    let (_dir, path) = write_bundle("named_raising_plugin", RAISING_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "named_raising_plugin");
    let err: PolyplugError = loader
        .load(&manifest, &runtime)
        .expect_err("expected error");
    let display: String = err.to_string();
    assert!(
        display.contains("bundle"),
        "error message should include bundle name 'bundle'; got: {display}"
    );
}

/// Confirm the `PluginContext` `bundle_path` pointer received by `polyplug_init`
/// is a valid non-empty string.  The plugin reads it via ctypes; we verify no exception is raised.
#[test]
fn test_plugin_context_bundle_path_accessible() {
    let plugin_src: &str = r#"
import ctypes

class _StringView(ctypes.Structure):
    _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

class _PluginContext(ctypes.Structure):
    _fields_ = [("bundle_path", _StringView), ("host_abi_version", ctypes.c_uint32), ("bundle_id", ctypes.c_uint64)]

def polyplug_init(_rt_ctx: int, _host_vtable: int, ctx_addr: int) -> None:
    ctx = _PluginContext.from_address(ctx_addr)
    # ptr must be non-null and len must be > 0
    assert ctx.bundle_path.ptr is not None and ctx.bundle_path.ptr != 0
    assert ctx.bundle_path.len > 0
"#;

    let (_dir, path) = write_bundle("ctx_check", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "ctx_check");
    let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "context check plugin failed: {result:?}");
}

/// Stress test: load 16 different no-op plugins sequentially on the same loader
/// to verify no GIL leak, no `sys.path` contamination, and no accumulation of error state.
#[test]
fn test_many_sequential_loads_no_state_leak() {
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();
    for i in 0u32..16u32 {
        let name: String = format!("stress_{i}");
        let (_dir, path) = write_bundle(&name, NOOP_PLUGIN_SRC);
        let manifest: ManifestData = make_manifest(&path, &name);
        let result: Result<(), PolyplugError> = loader.load(&manifest, &runtime);
        assert!(result.is_ok(), "stress load {i} failed: {result:?}");
    }
}

/// After a failed load (syntax error), subsequent loads of valid plugins on the
/// same loader must still succeed.  Ensures error paths do not corrupt interpreter state.
#[test]
fn test_valid_load_after_failed_load_succeeds() {
    let (_dir1, bad_path) = write_bundle("bad_recover", SYNTAX_ERROR_PLUGIN_SRC);
    let (_dir2, good_path) = write_bundle("good_after_bad", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Runtime = make_runtime();

    let bad_manifest: ManifestData = make_manifest(&bad_path, "bad_recover");
    let good_manifest: ManifestData = make_manifest(&good_path, "good_after_bad");

    // First load — should fail.
    assert!(
        loader.load(&bad_manifest, &runtime).is_err(),
        "bad load should fail"
    );

    // Second load — should succeed.
    let result: Result<(), PolyplugError> = loader.load(&good_manifest, &runtime);
    assert!(result.is_ok(), "recovery load failed: {result:?}");
}
