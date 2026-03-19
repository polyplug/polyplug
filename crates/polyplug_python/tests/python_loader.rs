// Integration tests for the polyplug_python PythonLoader.
//
// Covers: interpreter init, valid module loading, syntax error, import error,
// missing polyplug_init, vtable registration, GIL acquisition, and thread safety.
#![allow(clippy::expect_used)]

use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering;
use std::fs;
use std::sync::Arc;
use std::thread;

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use tempfile::TempDir;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Write `content` into `dir/<name>.py` and return the path.
fn write_plugin(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path: std::path::PathBuf = dir.path().join(format!("{name}.py"));
    fs::write(&path, content).expect("write plugin file");
    path
}

/// Minimal dummy `PluginRegistrar` that records how many times `register_plugin` is called.
///
/// The counter lives in an `Arc<AtomicU32>` so callers can read it after `load()` returns
/// even though the registrar was passed by mut-ref.  The Arc is leaked into the `host` field
/// so the pointer remains stable for the duration of the FFI callback.
fn make_registrar(counter: Arc<AtomicU32>) -> PluginRegistrar {
    // SAFETY: We leak the Arc and store its raw pointer in the `host` field (which is
    // normally `*const HostVTable`).  This is a test-only repurposing of the field;
    // the pointer is never dereferenced as a HostVTable — only `counting_register_plugin`
    // reads it, and it knows the actual type is `*const AtomicU32`.
    // The Arc is intentionally leaked so it lives for the duration of the test.
    let counter_ptr: *const AtomicU32 = Arc::into_raw(counter);
    PluginRegistrar {
        register_plugin: counting_register_plugin,
        host: counter_ptr as *const HostVTable,
    }
}

/// `register_plugin` implementation that bumps the counter stored in `registrar.host`.
///
/// # Safety
/// `registrar` must be non-null and its `host` field must point to an `AtomicU32`
/// placed there by `make_registrar`.  Descriptor and vtable pointers are ignored.
unsafe extern "C" fn counting_register_plugin(
    registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    // SAFETY: registrar is non-null (the loader passes the address we gave it).
    // host was stored as *const AtomicU32 by make_registrar() and is valid for the
    // lifetime of the test — the Arc was leaked, so it is never freed.
    let counter: &AtomicU32 = unsafe { &*((*registrar).host as *const AtomicU32) };
    counter.fetch_add(1, Ordering::SeqCst);
    AbiError::ok()
}

/// A Python plugin source that registers one vtable via `polyplug_init`.
///
/// The ctypes bridge mirrors the ABI contract expected by PythonLoader:
///   `polyplug_init(registrar_addr: int, ctx_addr: int) -> None`
///
/// `_AbiError` must match the Rust `AbiError` layout (24 bytes on x86_64):
///   `code: u32` + 4 bytes padding + `message: StringView { ptr, len }` (16 bytes).
/// ctypes uses the struct return type to emit the hidden-pointer calling convention
/// for structs larger than 16 bytes, matching the SysV x86_64 ABI.
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

class _PluginRegistrar(ctypes.Structure):
    pass

_RegisterFn = ctypes.CFUNCTYPE(
    _AbiError,
    ctypes.c_void_p,
    ctypes.c_void_p,
    ctypes.c_void_p,
)

_PluginRegistrar._fields_ = [
    ("register_plugin", _RegisterFn),
    ("host", ctypes.c_void_p),
]

class _PluginDescriptor(ctypes.Structure):
    _fields_ = [
        ("name",          _StringView),
        ("contract_name", _StringView),
        ("version_major", ctypes.c_uint32),
        ("version_minor", ctypes.c_uint32),
        ("version_patch", ctypes.c_uint32),
    ]

class _PluginVTable(ctypes.Structure):
    _fields_ = [
        ("contract_id",      ctypes.c_uint64),
        ("contract_version", ctypes.c_uint32),
        ("function_count",   ctypes.c_uint32),
        ("functions",        ctypes.c_void_p),
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

_VTABLE = _PluginVTable()
_VTABLE.contract_id      = 0xDEADBEEFCAFEBABE
_VTABLE.contract_version = 0
_VTABLE.function_count   = 0
_VTABLE.functions        = ctypes.cast(_FUNCTIONS_ARR, ctypes.c_void_p).value

def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
    registrar = _PluginRegistrar.from_address(registrar_addr)
    registrar.register_plugin(
        ctypes.c_void_p(registrar_addr),
        ctypes.addressof(_DESC),
        ctypes.addressof(_VTABLE),
    )
"#;

/// A Python plugin that defines `polyplug_init` but raises an exception inside it.
const RAISING_PLUGIN_SRC: &str = r#"
def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
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

def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
    pass
"#;

/// A minimal no-op plugin (defines polyplug_init but does nothing).
const NOOP_PLUGIN_SRC: &str = r#"
def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
    pass
"#;

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Python interpreter is initialized exactly once and does not panic on the first call.
#[test]
fn test_interpreter_initializes_without_panic() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "noop_init", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "unexpected error: {result:?}");
}

/// `PythonConfig::default()` requires Python ≥ 3.11.  Loading a valid plugin
/// must succeed when the host Python satisfies this requirement.
#[test]
fn test_default_config_version_check_passes() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "ver_check", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_ok(),
        "version check failed unexpectedly: {result:?}"
    );
}

/// A pathologically high minimum version requirement (`(99, 0)`) must fail with
/// `LoaderError::RuntimeVersionMismatch`.
#[test]
fn test_version_too_old_returns_version_mismatch() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "ver_mismatch", NOOP_PLUGIN_SRC);
    let config: PythonConfig = PythonConfig {
        min_version: (99, 0),
    };
    let loader: PythonLoader = PythonLoader::new(config);
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
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
/// invoke the registrar callback exactly once.
#[test]
fn test_valid_plugin_calls_registrar_once() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "valid_plugin", VALID_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let counter_clone: Arc<AtomicU32> = Arc::clone(&counter);
    let mut registrar: PluginRegistrar = make_registrar(counter_clone);
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "load failed: {result:?}");
    let calls: u32 = counter.load(Ordering::SeqCst);
    assert_eq!(
        calls, 1,
        "expected exactly 1 register_plugin call, got {calls}"
    );
}

/// A plugin with a syntax error must return `PythonModuleImportFailed`.
#[test]
fn test_syntax_error_returns_module_import_failed() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "syntax_err", SYNTAX_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected failure for syntax error plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { path: p, reason }) => {
            assert!(
                p.contains("syntax_err"),
                "path should mention the file name; got: {p}"
            );
            assert!(!reason.is_empty(), "reason must not be empty");
        }
        other => panic!("expected PythonModuleImportFailed, got: {other:?}"),
    }
}

/// A plugin that imports a missing module must return `PythonModuleImportFailed`.
#[test]
fn test_import_error_returns_module_import_failed() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "import_err", IMPORT_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected failure for import-error plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { path: p, reason }) => {
            assert!(
                p.contains("import_err"),
                "path should mention the file name; got: {p}"
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
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "no_init", MISSING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected failure for plugin missing polyplug_init");
    match err {
        PolyplugError::Loader(LoaderError::InitSymbolMissing { bundle }) => {
            assert!(
                bundle.contains("no_init"),
                "bundle name should contain 'no_init'; got: {bundle}"
            );
        }
        other => panic!("expected InitSymbolMissing, got: {other:?}"),
    }
}

/// A `polyplug_init` that raises a Python exception must return `PythonInitRaisedException`.
#[test]
fn test_raising_init_returns_init_raised_exception() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "raising_init", RAISING_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected failure for raising plugin");
    match err {
        PolyplugError::Loader(LoaderError::PythonInitRaisedException { bundle, message }) => {
            assert!(
                bundle.contains("raising_init"),
                "bundle name should contain 'raising_init'; got: {bundle}"
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
    let path: std::path::PathBuf = dir.path().join("does_not_exist.py");
    // Intentionally do NOT create the file.
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected failure for nonexistent path");
    match err {
        PolyplugError::Loader(LoaderError::PythonModuleImportFailed { .. }) => {}
        other => panic!("expected PythonModuleImportFailed, got: {other:?}"),
    }
}

/// The GIL is properly acquired and released: multiple sequential loads must all succeed.
#[test]
fn test_gil_released_between_sequential_loads() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let loader: PythonLoader = PythonLoader::default();
    for i in 0u32..4u32 {
        let name: String = format!("gil_seq_{i}");
        let path: std::path::PathBuf = write_plugin(&dir, &name, NOOP_PLUGIN_SRC);
        let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
        let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
        assert!(result.is_ok(), "sequential load {i} failed: {result:?}");
    }
}

/// Multiple threads may each create their own `PythonLoader` and call `load()`
/// concurrently.  The interpreter `OnceLock` must be thread-safe: all threads
/// must succeed (no panics, no data races).
#[test]
fn test_thread_safety_concurrent_loads() {
    let dir: Arc<TempDir> = Arc::new(TempDir::new().expect("tempdir"));

    // Pre-write all plugin files before spawning threads to avoid racing on writes.
    let paths: Vec<std::path::PathBuf> = (0u32..8u32)
        .map(|i: u32| {
            let name: String = format!("thread_safe_{i}");
            write_plugin(&dir, &name, NOOP_PLUGIN_SRC)
        })
        .collect::<Vec<std::path::PathBuf>>();

    let paths: Arc<Vec<std::path::PathBuf>> = Arc::new(paths);

    let handles: Vec<thread::JoinHandle<Result<(), PolyplugError>>> = (0usize..8usize)
        .map(|i: usize| {
            let path: std::path::PathBuf = paths[i].clone();
            thread::spawn(move || {
                let loader: PythonLoader = PythonLoader::default();
                let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
                let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
                loader.load(&path, &mut registrar)
            })
        })
        .collect::<Vec<thread::JoinHandle<Result<(), PolyplugError>>>>();

    for (i, handle) in handles.into_iter().enumerate() {
        let join_result: thread::Result<Result<(), PolyplugError>> = handle.join();
        let result: Result<(), PolyplugError> = join_result.expect("thread panicked");
        assert!(result.is_ok(), "thread {i} load failed: {result:?}");
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
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path1: std::path::PathBuf = write_plugin(&dir, "reuse1", NOOP_PLUGIN_SRC);
    let path2: std::path::PathBuf = write_plugin(&dir, "reuse2", NOOP_PLUGIN_SRC);

    let loader_a: PythonLoader = PythonLoader::default();
    let loader_b: PythonLoader = PythonLoader::default();

    let counter_a: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let counter_b: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));

    let mut reg_a: PluginRegistrar = make_registrar(Arc::clone(&counter_a));
    let mut reg_b: PluginRegistrar = make_registrar(Arc::clone(&counter_b));

    let result_a: Result<(), PolyplugError> = loader_a.load(&path1, &mut reg_a);
    let result_b: Result<(), PolyplugError> = loader_b.load(&path2, &mut reg_b);

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
    // Python's importlib may not handle non-ASCII module names on all platforms.
    // This test verifies that names with underscores and digits (common edge case) work.
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "plugin_42_ok", NOOP_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
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
    let helper_path: std::path::PathBuf = dir.path().join("my_helper.py");
    fs::write(&helper_path, helper_src).expect("write helper");

    let plugin_src: &str = r#"
import my_helper

def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
    assert my_helper.HELPER_VALUE == 42
"#;
    let path: std::path::PathBuf = write_plugin(&dir, "imports_helper", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "sibling import failed: {result:?}");
}

/// Verify that a `site-packages/` sub-directory (if present) is also added to
/// `sys.path`, allowing the plugin to import packages from it.
#[test]
fn test_site_packages_dir_added_to_sys_path() {
    let dir: TempDir = TempDir::new().expect("tempdir");

    let sp_dir: std::path::PathBuf = dir.path().join("site-packages");
    fs::create_dir_all(&sp_dir).expect("create site-packages");
    fs::write(sp_dir.join("fakelib.py"), "FAKE = 99\n").expect("write fakelib");

    let plugin_src: &str = r#"
import fakelib

def polyplug_init(registrar_addr: int, _ctx_addr: int) -> None:
    assert fakelib.FAKE == 99
"#;
    let path: std::path::PathBuf = write_plugin(&dir, "uses_site_pkg", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "site-packages import failed: {result:?}");
}

/// Verify error from `polyplug_init` contains the bundle name (file stem) so
/// consumers can identify which plugin caused the failure.
#[test]
fn test_error_contains_bundle_name() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: std::path::PathBuf = write_plugin(&dir, "named_raising_plugin", RAISING_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let err: PolyplugError = loader
        .load(&path, &mut registrar)
        .expect_err("expected error");
    let display: String = err.to_string();
    assert!(
        display.contains("named_raising_plugin"),
        "error message should include bundle name 'named_raising_plugin'; got: {display}"
    );
}

/// Confirm the `PluginContext` `bundle_path` pointer received by `polyplug_init`
/// is a valid non-empty string.  The plugin reads it via ctypes; we verify no exception is raised.
#[test]
fn test_plugin_context_bundle_path_accessible() {
    let dir: TempDir = TempDir::new().expect("tempdir");

    let plugin_src: &str = r#"
import ctypes

class _StringView(ctypes.Structure):
    _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

class _PluginContext(ctypes.Structure):
    _fields_ = [("bundle_path", _StringView)]

def polyplug_init(registrar_addr: int, ctx_addr: int) -> None:
    ctx = _PluginContext.from_address(ctx_addr)
    # ptr must be non-null and len must be > 0
    assert ctx.bundle_path.ptr is not None and ctx.bundle_path.ptr != 0
    assert ctx.bundle_path.len > 0
"#;

    let path: std::path::PathBuf = write_plugin(&dir, "ctx_check", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
    let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "context check plugin failed: {result:?}");
}

/// Stress test: load 16 different no-op plugins sequentially on the same loader
/// to verify no GIL leak, no `sys.path` contamination, and no accumulation of error state.
#[test]
fn test_many_sequential_loads_no_state_leak() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let loader: PythonLoader = PythonLoader::default();
    for i in 0u32..16u32 {
        let name: String = format!("stress_{i}");
        let path: std::path::PathBuf = write_plugin(&dir, &name, NOOP_PLUGIN_SRC);
        let counter: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
        let mut registrar: PluginRegistrar = make_registrar(Arc::clone(&counter));
        let result: Result<(), PolyplugError> = loader.load(&path, &mut registrar);
        assert!(result.is_ok(), "stress load {i} failed: {result:?}");
    }
}

/// After a failed load (syntax error), subsequent loads of valid plugins on the
/// same loader must still succeed.  Ensures error paths do not corrupt interpreter state.
#[test]
fn test_valid_load_after_failed_load_succeeds() {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let loader: PythonLoader = PythonLoader::default();

    // First load — should fail.
    let bad_path: std::path::PathBuf = write_plugin(&dir, "bad_recover", SYNTAX_ERROR_PLUGIN_SRC);
    let counter_bad: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut reg_bad: PluginRegistrar = make_registrar(Arc::clone(&counter_bad));
    assert!(
        loader.load(&bad_path, &mut reg_bad).is_err(),
        "bad load should fail"
    );

    // Second load — should succeed.
    let good_path: std::path::PathBuf = write_plugin(&dir, "good_after_bad", NOOP_PLUGIN_SRC);
    let counter_good: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    let mut reg_good: PluginRegistrar = make_registrar(Arc::clone(&counter_good));
    let result: Result<(), PolyplugError> = loader.load(&good_path, &mut reg_good);
    assert!(result.is_ok(), "recovery load failed: {result:?}");
}
