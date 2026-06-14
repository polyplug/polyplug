// Integration tests for the polyplug_python PythonLoader (VM dispatch model).
//
// Python guests are VM-dispatch (like Lua/JS): the guest deposits its contract
// registrations in the module attribute `_polyplug_registrations`, and the
// loader registers each contract with DispatchType::VirtualMachine, routing
// per-call invocations through the `vm.call` transport.
//
// Registration shape (the spec the generator/SDK must emit):
//
//   _polyplug_registrations = [
//       {
//           "contract": "name@major" | "name@major.minor",
//           "plugin_name": "optional",          # defaults to bundle name
//           "factory": factory,                 # factory(host_ptr) -> impl
//           "functions": [callable, ...],       # ordered by fn_id
//       },
//   ]
//
// The loader calls `factory(host_ptr)` once per create_instance (and once at load
// for the stateless default impl) and passes the resolved impl as the first
// argument of each callable, so each callable is invoked as
// fn(impl, args_ptr_int, out_ptr_int, arena_ptr_int). The stateless plugins below
// take `impl` and ignore it.
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime_builder::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::dispatch::DispatchType;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;
use polyplug_utils::bundle_id;
use pyo3::Python;
use pyo3::types::PyAnyMethods;
use pyo3::types::PyDict;
use pyo3::types::PyDictMethods;
use pyo3::types::PyModule;

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Write `content` into a temp bundle directory with manifest.toml.
fn write_bundle(name: &str, content: &str) -> (TempDir, PathBuf) {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let path: PathBuf = dir.path().join("bundle.py");
    fs::write(&path, content).expect("write bundle.py");

    let bundle_id: u64 = bundle_id(name);
    let manifest: String = format!(
        r#"id = {}
name = "{}"
loader = "python"
file = "bundle.py"
"#,
        bundle_id, name
    );
    fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, path)
}

fn make_runtime() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .build()
        .expect("runtime build must succeed")
}

fn make_manifest(path: &Path, name: &str) -> ManifestData {
    ManifestData {
        id: bundle_id(name),
        name: name.to_owned(),
        loader: "python".to_owned(),
        file: path
            .file_name()
            .expect("bundle path must have a file name")
            .to_string_lossy()
            .into_owned(),
        path: path
            .parent()
            .expect("bundle path must have a parent directory")
            .to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// Resolve a registered contract's interface and call its VM dispatch for
/// `fn_id` with the given `args`/`out`/`arena` pointers.
///
/// # Safety
/// `out`/`args`/`arena` must be valid for whatever the guest callable does with
/// them; the test callables below only write to `out`.
unsafe fn dispatch(
    runtime: &Runtime,
    contract_id: u64,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let cid: GuestContractId = GuestContractId::from_u64(contract_id);
    let handle: GuestContractHandle = runtime
        .registry()
        .find(cid, 0)
        .expect("contract must be registered");
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("contract must resolve to an interface");
    // SAFETY: resolved interface is a non-null, runtime-owned GuestContractInterface
    // leaked for the runtime lifetime; reading it and its VM dispatch fields is sound.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::VirtualMachine,
        "Python contracts must register VM dispatch"
    );
    // SAFETY: dispatch_type == VirtualMachine guarantees the `vm` union arm is
    // active, so reading it is sound.
    let vm: polyplug_abi::dispatch::vm_dispatch::VmDispatch = unsafe { interface.dispatch.vm };
    let mut err: AbiError = AbiError::ok();
    // SAFETY: the call function is the loader's python_vm_dispatch; loader_data
    // wraps a live leaked PythonLoaderData; args/out are caller-provided.
    unsafe {
        (vm.call)(
            vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        );
    }
    err
}

/// Like [`dispatch`], but passes a real per-call `arena` pointer through to the
/// guest callable (the third dispatch argument). Used by the arena-isolation
/// tests to verify each call's allocations come from its OWN arena.
///
/// # Safety
/// `args`/`out` must be valid for the callable; `arena` must point at a valid,
/// caller-reset [`CallArena`] for this call.
unsafe fn dispatch_with_arena(
    runtime: &Runtime,
    contract_id: u64,
    fn_id: u32,
    args: *const (),
    out: *mut (),
    arena: *mut polyplug_abi::CallArena,
) -> AbiError {
    let cid: GuestContractId = GuestContractId::from_u64(contract_id);
    let handle: GuestContractHandle = runtime
        .registry()
        .find(cid, 0)
        .expect("contract must be registered");
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("contract must resolve to an interface");
    // SAFETY: resolved interface is a non-null, runtime-owned interface.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: Python contracts always register VM dispatch.
    let vm: polyplug_abi::dispatch::vm_dispatch::VmDispatch = unsafe { interface.dispatch.vm };
    let mut err: AbiError = AbiError::ok();
    // SAFETY: loader_data wraps a live leaked PythonLoaderData; args/out/arena are
    // caller-provided and valid for this call.
    unsafe {
        (vm.call)(
            vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            arena,
            &mut err as *mut AbiError,
        );
    }
    err
}

// ─── Plugin sources (VM dispatch / _polyplug_registrations) ─────────────────────

/// A plugin that registers one contract whose function 0 writes 0x2A into the
/// 4-byte int at `out` (via ctypes) and ignores `args`/`arena`.
const WRITE_OUT_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = 0x2A

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {
            "contract": "writeout@1",
            "plugin_name": "writeout_plugin",
            "factory": lambda host_ptr: None,
            "functions": [_fn0],
        },
    ]
"#;

/// Contract id for `writeout@1`.
fn writeout_contract_id() -> u64 {
    GuestContractId::new("writeout", 1).id()
}

/// A plugin whose function 0 raises a Python exception.
const RAISING_FN_PLUGIN_SRC: &str = r#"
def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    raise ValueError("dispatch boom")

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "raiser@1", "factory": lambda host_ptr: None, "functions": [_fn0]},
    ]
"#;

fn raiser_contract_id() -> u64 {
    GuestContractId::new("raiser", 1).id()
}

/// A plugin whose function 0 forwards the arena pointer it received into `out`
/// (as an i64) so the test can assert the arena pointer is forwarded verbatim.
const ARENA_FORWARD_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int64))[0] = arena_ptr

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "arenafwd@1", "factory": lambda host_ptr: None, "functions": [_fn0]},
    ]
"#;

fn arenafwd_contract_id() -> u64 {
    GuestContractId::new("arenafwd", 1).id()
}

/// `polyplug_init` raises an exception.
const RAISING_INIT_PLUGIN_SRC: &str = r#"
def polyplug_init(_host_interface: int, _ctx: int) -> None:
    raise RuntimeError("intentional test error")
"#;

/// Missing `polyplug_init` entirely.
const MISSING_INIT_PLUGIN_SRC: &str = r#"
def not_polyplug_init():
    pass
"#;

/// `polyplug_init` runs but never deposits `_polyplug_registrations`.
const NO_REGISTRATIONS_PLUGIN_SRC: &str = r#"
def polyplug_init(_host_interface: int, _ctx: int) -> None:
    pass
"#;

/// `_polyplug_registrations` is an empty list (no contracts).
const EMPTY_REGISTRATIONS_PLUGIN_SRC: &str = r#"
def polyplug_init(_host_interface: int, _ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = []
"#;

/// Syntax error.
const SYNTAX_ERROR_PLUGIN_SRC: &str = r#"
def polyplug_init(:
    pass
"#;

/// Imports a nonexistent module.
const IMPORT_ERROR_PLUGIN_SRC: &str = r#"
import _polyplug_nonexistent_module_xyz_123456

def polyplug_init(_host_interface: int, _ctx: int) -> None:
    pass
"#;

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_interpreter_initializes_without_panic() {
    let (_dir, path) = write_bundle("noop_init", WRITE_OUT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "noop_init");
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "unexpected error: {result:?}");
}

#[test]
fn test_default_config_version_check_passes() {
    let (_dir, path) = write_bundle("ver_check", WRITE_OUT_PLUGIN_SRC);
    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("runtime build must succeed");
    let manifest: ManifestData = make_manifest(&path, "ver_check");
    let result: Result<(), LoaderError> = PythonLoader::new(PythonConfig::default()).load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "version check failed: {result:?}");
}

#[test]
fn test_version_too_old_returns_version_mismatch() {
    let (_dir, path) = write_bundle("ver_mismatch", WRITE_OUT_PLUGIN_SRC);
    let config: PythonConfig = PythonConfig {
        min_version: (99, 0),
    };
    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::new(config.clone()))
        .build()
        .expect("runtime build must succeed");
    let manifest: ManifestData = make_manifest(&path, "ver_mismatch");
    let err: LoaderError = PythonLoader::new(config)
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected version mismatch");
    match err {
        LoaderError::InitFailed { bundle, error } => {
            assert_eq!(bundle, "python");
            assert!(
                error.contains("version"),
                "error should mention version: {error}"
            );
            assert!(
                error.contains("99"),
                "error should mention required version: {error}"
            );
        }
        other => panic!("expected InitFailed for version mismatch, got: {other:?}"),
    }
}

/// Loading a valid plugin registers its contract with VM dispatch.
#[test]
fn test_valid_plugin_registers_vm_contract() {
    let (_dir, path) = write_bundle("valid_plugin", WRITE_OUT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "valid_plugin");
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "load failed: {result:?}");

    let cid: GuestContractId = GuestContractId::from_u64(writeout_contract_id());
    let handle: GuestContractHandle = runtime
        .registry()
        .find(cid, 0)
        .expect("contract must be registered");
    let interface_ptr: *const polyplug_abi::GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("contract must resolve");
    // SAFETY: runtime-owned interface; reading dispatch_type is sound.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::VirtualMachine,
        "Python must register VM dispatch"
    );
}

/// VM dispatch happy path: the callable writes into `out` and returns Ok.
#[test]
fn test_vm_dispatch_writes_out_and_returns_ok() {
    let (_dir, path) = write_bundle("writeout_disp", WRITE_OUT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "writeout_disp");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    let mut out_buf: i32 = 0;
    // SAFETY: out points at a valid i32; the callable writes a 4-byte int there.
    let err: AbiError = unsafe {
        dispatch(
            &runtime,
            writeout_contract_id(),
            0,
            core::ptr::null(),
            &mut out_buf as *mut i32 as *mut (),
        )
    };
    assert!(
        err.is_ok(),
        "dispatch should return Ok, got code {}",
        err.code
    );
    assert_eq!(out_buf, 0x2A, "callable must have written 0x2A into out");
}

/// A Python exception inside the callable maps to AbiErrorCode::Generic.
#[test]
fn test_vm_dispatch_exception_maps_to_generic() {
    let (_dir, path) = write_bundle("raiser_disp", RAISING_FN_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "raiser_disp");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // SAFETY: the callable ignores args/out, so null is sound.
    let err: AbiError = unsafe {
        dispatch(
            &runtime,
            raiser_contract_id(),
            0,
            core::ptr::null(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Generic as u32,
        "a guest exception must map to Generic"
    );
}

/// An out-of-range fn_id maps to AbiErrorCode::FunctionNotAvailable.
#[test]
fn test_vm_dispatch_fn_id_out_of_range() {
    let (_dir, path) = write_bundle("range_disp", WRITE_OUT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "range_disp");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // Only fn_id 0 exists; fn_id 5 is out of range.
    // SAFETY: out-of-range fn_id returns before touching args/out.
    let err: AbiError = unsafe {
        dispatch(
            &runtime,
            writeout_contract_id(),
            5,
            core::ptr::null(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::FunctionNotAvailable as u32,
        "out-of-range fn_id must map to FunctionNotAvailable"
    );
}

/// The arena pointer is forwarded to the callable; when null it arrives as 0.
#[test]
fn test_vm_dispatch_arena_forwarded_zero_when_null() {
    let (_dir, path) = write_bundle("arena_disp", ARENA_FORWARD_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "arena_disp");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    let mut out_buf: i64 = -1;
    // `dispatch` always passes a null arena, so the callable must observe 0.
    // SAFETY: out points at a valid i64; callable writes the arena int there.
    let err: AbiError = unsafe {
        dispatch(
            &runtime,
            arenafwd_contract_id(),
            0,
            core::ptr::null(),
            &mut out_buf as *mut i64 as *mut (),
        )
    };
    assert!(
        err.is_ok(),
        "dispatch should return Ok, got code {}",
        err.code
    );
    assert_eq!(out_buf, 0, "null arena must be forwarded as integer 0");
}

#[test]
fn test_syntax_error_returns_init_failed() {
    let (_dir, path) = write_bundle("syntax_err", SYNTAX_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "syntax_err");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for syntax error plugin");
    assert!(matches!(err, LoaderError::InitFailed { .. }));
}

#[test]
fn test_import_error_returns_init_failed() {
    let (_dir, path) = write_bundle("import_err", IMPORT_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "import_err");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for import-error plugin");
    assert!(matches!(err, LoaderError::InitFailed { .. }));
}

#[test]
fn test_missing_init_returns_init_symbol_missing() {
    let (_dir, path) = write_bundle("no_init", MISSING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "no_init");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure");
    assert!(matches!(err, LoaderError::InitSymbolMissing { .. }));
}

#[test]
fn test_raising_init_returns_init_failed() {
    let (_dir, path) = write_bundle("raising_init", RAISING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "raising_init");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure");
    match err {
        LoaderError::InitFailed { error, .. } => {
            assert!(
                error.contains("intentional test error"),
                "error should contain the Python exception text; got: {error}"
            );
        }
        other => panic!("expected InitFailed, got: {other:?}"),
    }
}

/// A plugin that runs init but never deposits `_polyplug_registrations` fails.
#[test]
fn test_missing_registrations_attr_fails() {
    let (_dir, path) = write_bundle("no_regs", NO_REGISTRATIONS_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "no_regs");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for missing registrations");
    match err {
        LoaderError::InitFailed { error, .. } => {
            assert!(
                error.contains("_polyplug_registrations"),
                "error should mention the missing attribute; got: {error}"
            );
        }
        other => panic!("expected InitFailed, got: {other:?}"),
    }
}

/// An empty `_polyplug_registrations` list registers no contracts and fails.
#[test]
fn test_empty_registrations_fails() {
    let (_dir, path) = write_bundle("empty_regs", EMPTY_REGISTRATIONS_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "empty_regs");
    let err: LoaderError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for empty registrations");
    assert!(matches!(err, LoaderError::InitFailed { .. }));
}

#[test]
fn test_loader_name() {
    let loader: PythonLoader = PythonLoader::default();
    assert_eq!(loader.loader_name(), "python");
}

#[test]
fn test_loader_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<PythonLoader>();
}

/// Sequential loads on the same loader all succeed (no GIL leak / state leak).
#[test]
fn test_many_sequential_loads() {
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    for i in 0u32..8u32 {
        let name: String = format!("seq_{i}");
        let (_dir, path) = write_bundle(&name, WRITE_OUT_PLUGIN_SRC);
        let manifest: ManifestData = make_manifest(&path, &name);
        let result: Result<(), LoaderError> = loader.load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        );
        // Only the first load registers `writeout@1`; later loads collide on the
        // contract id. The point of this test is interpreter stability, so accept
        // either Ok or a duplicate-registration InitFailed without panicking.
        assert!(
            result.is_ok() || matches!(result, Err(LoaderError::InitFailed { .. })),
            "sequential load {i} produced an unexpected error: {result:?}"
        );
    }
}

/// After a failed load, a subsequent valid load still succeeds.
#[test]
fn test_valid_load_after_failed_load_succeeds() {
    let (_dir1, bad_path) = write_bundle("bad_recover", SYNTAX_ERROR_PLUGIN_SRC);
    let (_dir2, good_path) = write_bundle("good_after_bad", WRITE_OUT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();

    let bad_manifest: ManifestData = make_manifest(&bad_path, "bad_recover");
    let good_manifest: ManifestData = make_manifest(&good_path, "good_after_bad");

    assert!(
        loader
            .load(
                &bad_manifest,
                &polyplug::loader::BundleSource::Path(bad_manifest.path.clone()),
                &runtime
            )
            .is_err(),
        "bad load should fail"
    );

    let result: Result<(), LoaderError> = loader.load(
        &good_manifest,
        &polyplug::loader::BundleSource::Path(good_manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "recovery load failed: {result:?}");
}

/// `BundleInitContext.bundle_path` is a valid non-empty string for path loads.
#[test]
fn test_plugin_context_bundle_path_accessible() {
    let plugin_src: &str = r#"
import ctypes

class _StringView(ctypes.Structure):
    _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

class _BundleInitContext(ctypes.Structure):
    _fields_ = [("bundle_id", ctypes.c_uint64), ("bundle_path", _StringView)]

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    pass

def polyplug_init(_host_interface: int, ctx_addr: int) -> None:
    ctx = _BundleInitContext.from_address(ctx_addr)
    assert ctx.bundle_path.ptr is not None and ctx.bundle_path.ptr != 0
    assert ctx.bundle_path.len > 0
    global _polyplug_registrations
    _polyplug_registrations = [{"contract": "ctxcheck@1", "factory": lambda host_ptr: None, "functions": [_fn0]}]
"#;

    let (_dir, path) = write_bundle("ctx_check", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "ctx_check");
    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "context check plugin failed: {result:?}");
}

/// Split-module bundle: a helper module defines `polyplug_init` and deposits
/// `_polyplug_registrations` into its own namespace, and the entry module only
/// `from helper import polyplug_init`. This mirrors the generated layout, where
/// the entry file imports `polyplug_init` from `generated/guest/contracts.py`.
///
/// The loader must collect the registrations from the namespace of the module
/// that *defines* `polyplug_init` (its `__globals__`), not from the entry
/// module — load + register + dispatch must all work.
#[test]
fn test_split_module_registrations_via_init_globals() {
    // The helper's callable calls `_polyplug_arena_alloc(16, arena_ptr)` during
    // dispatch. The bridge must resolve from the helper module's globals (where
    // the ABI functions are defined), not only from the entry module — this locks
    // the injection point at polyplug_init.__globals__. The dispatch passes a null
    // arena, so the bridge takes the host-alloc fallback and must return a
    // nonzero address; the callable writes that address (truthiness) into `out`.
    let helper_src: &str = r#"
import ctypes

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    buf = _polyplug_arena_alloc(16, arena_ptr)
    if buf == 0:
        raise RuntimeError("_polyplug_arena_alloc returned 0")
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = 0x2A

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {
            "contract": "splitmod@1",
            "plugin_name": "splitmod_plugin",
            "factory": lambda host_ptr: None,
            "functions": [_fn0],
        },
    ]
"#;
    let entry_src: &str = r#"
from _splitmod_helper import polyplug_init
"#;

    let dir: TempDir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("_splitmod_helper.py"), helper_src).expect("write helper module");
    let entry_path: PathBuf = dir.path().join("bundle.py");
    fs::write(&entry_path, entry_src).expect("write entry module");

    let manifest: ManifestData = make_manifest(&entry_path, "splitmod");
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();

    let result: Result<(), LoaderError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "split-module load failed: {result:?}");

    let mut out_buf: i32 = 0;
    // SAFETY: out points at a valid i32; the callable writes a 4-byte int there.
    let err: AbiError = unsafe {
        dispatch(
            &runtime,
            GuestContractId::new("splitmod", 1).id(),
            0,
            core::ptr::null(),
            &mut out_buf as *mut i32 as *mut (),
        )
    };
    assert_eq!(err.code, AbiErrorCode::Ok as u32, "dispatch should succeed");
    assert_eq!(out_buf, 0x2A, "callable should write 0x2A into out");
}

// ─── Unload: sys.modules purge ───────────────────────────────────────────────────

/// Count `sys.modules` keys that are re-keyed bundle modules (prefix
/// `__polyplug_bundle_`) AND reference `helper_substr` (the bundle's sibling
/// module name), holding the GIL via `Python::attach` to mirror the crate.
fn count_bundle_modules(helper_substr: &str) -> usize {
    Python::attach(|py: Python<'_>| -> usize {
        let sys_mod: pyo3::Bound<'_, PyModule> = PyModule::import(py, "sys").expect("sys import");
        let modules: pyo3::Bound<'_, pyo3::PyAny> =
            sys_mod.getattr("modules").expect("sys.modules");
        let dict: pyo3::Bound<'_, PyDict> =
            modules.cast_into::<PyDict>().expect("sys.modules dict");
        let mut count: usize = 0;
        for (key, _value) in dict.iter() {
            let key_str: String = match key.extract::<String>() {
                Ok(s) => s,
                Err(_) => continue,
            };
            if key_str.starts_with("__polyplug_bundle_") && key_str.contains(helper_substr) {
                count += 1;
            }
        }
        count
    })
}

/// Write a split-module bundle (entry imports a sibling helper that physically
/// lives under the bundle dir) so the isolation pass actually re-keys at least one
/// in-bundle module into `sys.modules`. Returns the temp dir and entry path.
fn write_split_bundle(name: &str, helper_module: &str, contract: &str) -> (TempDir, PathBuf) {
    let helper_src: String = format!(
        r#"
def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    pass

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {{
            "contract": "{contract}",
            "plugin_name": "{name}_plugin",
            "factory": lambda host_ptr: None,
            "functions": [_fn0],
        }},
    ]
"#
    );
    let entry_src: String = format!("from {helper_module} import polyplug_init\n");

    let dir: TempDir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join(format!("{helper_module}.py")), helper_src).expect("write helper");
    let entry_path: PathBuf = dir.path().join("bundle.py");
    fs::write(&entry_path, entry_src).expect("write entry");
    (dir, entry_path)
}

/// Unloading a bundle ALWAYS purges its re-keyed `sys.modules` entries so a later
/// load re-imports fresh source. Purge is uniform: unload always reclaims the import
/// cache rather than parking it alive.
#[test]
fn unload_purges_bundle_modules_from_sys_modules() {
    let bundle_name: &str = "reclaim_purge";
    let helper_module: &str = "_reclaim_purge_helper";
    let (_dir, path) = write_split_bundle(bundle_name, helper_module, "reclaimpurge@1");

    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, bundle_name);

    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    let before: usize = count_bundle_modules(helper_module);
    assert!(
        before > 0,
        "bundle's helper module must be re-keyed into sys.modules after load"
    );

    loader
        .unload(BundleId::from_u64(bundle_id(bundle_name)), &runtime)
        .expect("unload must succeed");

    let after: usize = count_bundle_modules(helper_module);
    assert_eq!(
        after, 0,
        "unload must purge the bundle's re-keyed sys.modules entries"
    );
}

// ─── Arena isolation (parameterized-arena protocol) ─────────────────────────────

/// Plugin for the nested-reentrancy arena test.
///
/// fn_id 0 (OUTER) re-enters the SAME bundle through `python_vm_dispatch` to run
/// fn_id 1 (INNER) — a genuine same-thread nested dispatch — by calling the
/// test-injected builtin `__polyplug_test_nested_dispatch()` (a Rust closure that
/// resolves this contract and calls its VM dispatch with a NULL arena). After the
/// nested call returns, OUTER allocates a 64-byte buffer from its OWN `arena_ptr`
/// via `_polyplug_arena_alloc(size, arena)` and writes the address into `out_ptr`.
/// fn_id 1 (INNER) allocates from its own (null) arena and ignores `out`.
///
/// Going through a real `python_vm_dispatch` reentry — rather than a plain Python
/// call — is what would have triggered the old shared-cell bug: the INNER call's
/// exit-time clear-to-0 wiped the cell, so OUTER's post-nested allocation (reading
/// the cell) fell back to host->alloc and escaped the outer arena. With the arena
/// threaded as an explicit argument, OUTER always uses its own `arena_ptr`.
///
/// The nested re-entry is driven from Rust (no ctypes struct-return calls, which
/// are UB on some targets) so the test exercises the loader's actual dispatch path.
const NESTED_ARENA_PLUGIN_SRC: &str = r#"
import ctypes

def _inner(impl, args_ptr, out_ptr, arena_ptr):
    _polyplug_arena_alloc(16, arena_ptr)

def _outer(impl, args_ptr, out_ptr, arena_ptr):
    # Real nested dispatch into fn_id 1 via the test-injected Rust trampoline.
    __polyplug_test_nested_dispatch()
    # After the nested call returned, allocate from the OUTER arena_ptr and report.
    addr = _polyplug_arena_alloc(64, arena_ptr)
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int64))[0] = addr

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "nestedarena@1", "factory": lambda host_ptr: None, "functions": [_outer, _inner]},
    ]
"#;

fn nestedarena_contract_id() -> u64 {
    GuestContractId::new("nestedarena", 1).id()
}

/// Nested same-thread cross-call must NOT corrupt the outer call's arena.
///
/// The outer guest fn re-enters the same bundle (a real `python_vm_dispatch`
/// nested dispatch of fn_id 1 via a Rust trampoline injected into `builtins`),
/// then allocates from its OWN `arena_ptr`. With the arena threaded explicitly
/// (not via a shared cell), the post-nested allocation lands inside the OUTER
/// arena's buffer. Under the old shared-cell protocol the nested call's
/// clear-to-0 wiped the cell and the outer allocation fell back to host->alloc
/// (outside the arena buffer) — the bug this test pins.
#[test]
fn test_nested_dispatch_preserves_outer_arena() {
    let contract_id: u64 = nestedarena_contract_id();
    let (_dir, path) = write_bundle("nested_arena", NESTED_ARENA_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "nested_arena");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // Inject a builtin `__polyplug_test_nested_dispatch()` that performs a real
    // nested VM dispatch of fn_id 1 (null arena) from Rust. Putting it in
    // `builtins` makes it reachable from the bundle module without touching the
    // bundle's re-keyed sys.modules entry. The runtime pointer is captured as a
    // usize (raw pointers are not Send, but it stays valid for the test).
    let runtime_addr: usize = Arc::as_ptr(&runtime) as usize;
    Python::attach(|py: Python<'_>| {
        let trampoline = pyo3::types::PyCFunction::new_closure(
            py,
            None,
            None,
            move |_args: &pyo3::Bound<'_, pyo3::types::PyTuple>,
                  _kwargs: Option<&pyo3::Bound<'_, PyDict>>|
                  -> i64 {
                // SAFETY: runtime_addr is the live Runtime for the test's lifetime.
                let rt: &Runtime = unsafe { &*(runtime_addr as *const Runtime) };
                // SAFETY: fn_id 1 (inner) only allocates from its (null) arena.
                let err: AbiError = unsafe {
                    dispatch_with_arena(
                        rt,
                        contract_id,
                        1,
                        core::ptr::null(),
                        core::ptr::null_mut(),
                        core::ptr::null_mut(),
                    )
                };
                err.code as i64
            },
        )
        .expect("trampoline creation must succeed");
        let builtins: pyo3::Bound<'_, PyModule> =
            PyModule::import(py, "builtins").expect("builtins import");
        builtins
            .setattr("__polyplug_test_nested_dispatch", trampoline)
            .expect("inject trampoline builtin");
    });

    // Outer arena over a 4 KiB buffer (null host: 64-byte alloc fits the primary
    // region, so no overflow / host needed). The buffer must outlive the dispatch.
    let mut buf: Vec<u8> = vec![0_u8; 4096];
    let lo: usize = buf.as_ptr() as usize;
    let hi: usize = lo + buf.len();
    let mut arena: polyplug_abi::CallArena =
        polyplug_abi::CallArena::new(&mut buf, core::ptr::null());

    let mut out: i64 = 0;
    // SAFETY: out is a valid local i64; arena is a valid CallArena over `buf`,
    // which outlives this call; the guest writes the allocated address into out.
    let err: AbiError = unsafe {
        dispatch_with_arena(
            &runtime,
            contract_id,
            0,
            core::ptr::null(),
            &mut out as *mut i64 as *mut (),
            &mut arena as *mut polyplug_abi::CallArena,
        )
    };
    assert!(
        err.is_ok(),
        "outer dispatch must succeed: code={}",
        err.code
    );

    let p: usize = out as usize;
    assert!(
        p >= lo && p < hi,
        "outer allocation {p:#x} must land inside the OUTER arena buffer [{lo:#x}, {hi:#x}); a value outside means the nested call's clear wiped the outer arena (shared-cell bug)"
    );
}

/// Plugin for the two-thread arena test: one fn that allocates 64 bytes from its
/// `arena_ptr` and writes the address into `out_ptr`.
const ARENA_ECHO_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    addr = _polyplug_arena_alloc(64, arena_ptr)
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int64))[0] = addr

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "arenaecho@1", "factory": lambda host_ptr: None, "functions": [_fn0]},
    ]
"#;

fn arenaecho_contract_id() -> u64 {
    GuestContractId::new("arenaecho", 1).id()
}

/// Two threads dispatching into the SAME bundle with DISTINCT arenas must each
/// allocate from their OWN arena.
///
/// CPython's GIL serializes Python execution, so the threads do not truly run the
/// guest body in parallel; the invariant this pins is that allocation reads the
/// arena from the per-call ARGUMENT (which differs per thread) and never from a
/// shared cell that the other thread could have overwritten. Each thread runs
/// many iterations resetting its own arena and asserts every allocation lands in
/// its own buffer. The two buffers are disjoint so a misattribution is detectable.
#[test]
fn test_concurrent_dispatch_distinct_arenas_isolated() {
    let contract_id: u64 = arenaecho_contract_id();
    let (_dir, path) = write_bundle("arena_echo", ARENA_ECHO_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "arena_echo");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // Two disjoint 4 KiB buffers + arenas, leaked so their addresses are valid
    // across both worker threads (null host: 64-byte allocs fit the primary region).
    let buf_a: &'static mut [u8] = Box::leak(vec![0_u8; 4096].into_boxed_slice());
    let buf_b: &'static mut [u8] = Box::leak(vec![0_u8; 4096].into_boxed_slice());
    let a_lo: usize = buf_a.as_ptr() as usize;
    let a_hi: usize = a_lo + buf_a.len();
    let b_lo: usize = buf_b.as_ptr() as usize;
    let b_hi: usize = b_lo + buf_b.len();
    let arena_a: &'static mut polyplug_abi::CallArena = Box::leak(Box::new(
        polyplug_abi::CallArena::new(buf_a, core::ptr::null()),
    ));
    let arena_b: &'static mut polyplug_abi::CallArena = Box::leak(Box::new(
        polyplug_abi::CallArena::new(buf_b, core::ptr::null()),
    ));
    let arena_a_addr: usize = arena_a as *mut polyplug_abi::CallArena as usize;
    let arena_b_addr: usize = arena_b as *mut polyplug_abi::CallArena as usize;

    const ITERS: usize = 500;
    let runtime_a: Arc<Runtime> = Arc::clone(&runtime);
    let runtime_b: Arc<Runtime> = Arc::clone(&runtime);

    let handle_a: std::thread::JoinHandle<Result<(), String>> = std::thread::spawn(move || {
        for i in 0..ITERS {
            // SAFETY: arena_a_addr is the live leaked arena for this worker.
            let arena: &mut polyplug_abi::CallArena =
                unsafe { &mut *(arena_a_addr as *mut polyplug_abi::CallArena) };
            arena.reset();
            let mut out: i64 = 0;
            // SAFETY: out is a valid local; arena_a is valid; contract is loaded.
            let err: AbiError = unsafe {
                dispatch_with_arena(
                    &runtime_a,
                    contract_id,
                    0,
                    core::ptr::null(),
                    &mut out as *mut i64 as *mut (),
                    arena_a_addr as *mut polyplug_abi::CallArena,
                )
            };
            if !err.is_ok() {
                return Err(format!("A iter {i}: dispatch failed code={}", err.code));
            }
            let p: usize = out as usize;
            if !(p >= a_lo && p < a_hi) {
                return Err(format!(
                    "A iter {i}: allocation {p:#x} escaped arena A buffer [{a_lo:#x}, {a_hi:#x})"
                ));
            }
        }
        Ok(())
    });

    let handle_b: std::thread::JoinHandle<Result<(), String>> = std::thread::spawn(move || {
        for i in 0..ITERS {
            // SAFETY: arena_b_addr is the live leaked arena for this worker.
            let arena: &mut polyplug_abi::CallArena =
                unsafe { &mut *(arena_b_addr as *mut polyplug_abi::CallArena) };
            arena.reset();
            let mut out: i64 = 0;
            // SAFETY: out is a valid local; arena_b is valid; contract is loaded.
            let err: AbiError = unsafe {
                dispatch_with_arena(
                    &runtime_b,
                    contract_id,
                    0,
                    core::ptr::null(),
                    &mut out as *mut i64 as *mut (),
                    arena_b_addr as *mut polyplug_abi::CallArena,
                )
            };
            if !err.is_ok() {
                return Err(format!("B iter {i}: dispatch failed code={}", err.code));
            }
            let p: usize = out as usize;
            if !(p >= b_lo && p < b_hi) {
                return Err(format!(
                    "B iter {i}: allocation {p:#x} escaped arena B buffer [{b_lo:#x}, {b_hi:#x})"
                ));
            }
        }
        Ok(())
    });

    let a_outcome: Result<(), String> = handle_a.join().expect("thread A must not panic");
    let b_outcome: Result<(), String> = handle_b.join().expect("thread B must not panic");
    if let Err(e) = a_outcome {
        panic!("{e}");
    }
    if let Err(e) = b_outcome {
        panic!("{e}");
    }
}

// ─── Multi-runtime isolation proof (process-global SharedState / nonce) ──────────

/// Entry-module source for a bundle whose contract function 0 writes the integer
/// returned by a **bundle-local** sibling module `shared_helper.VALUE` into `out`.
///
/// Two runtimes load two different bundles that share the *same name* (hence the
/// same `bundle_id` name-hash) and each ship their *own* `shared_helper.py` with
/// a different `VALUE`. Because CPython's `sys.modules` is process-global, the
/// per-bundle module-isolation pass must re-key each bundle's `shared_helper`
/// under a prefix made unique by the runtime-vended nonce — otherwise the second
/// runtime would import the first's cached `shared_helper` and observe the wrong
/// value. The `{contract}` placeholder lets each runtime register a distinct
/// contract id so both contracts coexist in the (separate) registries.
const SHARED_NAME_ENTRY_SRC: &str = r#"
import ctypes
from shared_helper import VALUE

def _fn0(impl, args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = VALUE

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "{contract}", "factory": lambda host_ptr: None, "functions": [_fn0]},
    ]
"#;

/// Write a same-named bundle whose `shared_helper.VALUE == value` and whose
/// contract is `"{contract_name}@1"`. Returns the temp dir (kept alive by the
/// caller) and the entry-file path.
fn write_shared_name_bundle(contract_name: &str, value: i32) -> (TempDir, PathBuf) {
    let dir: TempDir = TempDir::new().expect("tempdir");
    let entry: PathBuf = dir.path().join("bundle.py");
    let entry_src: String =
        SHARED_NAME_ENTRY_SRC.replace("{contract}", &format!("{contract_name}@1"));
    fs::write(&entry, entry_src).expect("write bundle.py");
    fs::write(
        dir.path().join("shared_helper.py"),
        format!("VALUE = {value}\n"),
    )
    .expect("write shared_helper.py");

    // Both bundles deliberately share the SAME name so their bundle_id collides.
    let name: &str = "shared_name";
    let manifest: String = format!(
        r#"id = {}
name = "{}"
loader = "python"
file = "bundle.py"
"#,
        bundle_id(name),
        name
    );
    fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, entry)
}

/// Two `Runtime` instances in ONE process each load a same-named Python bundle
/// (identical `bundle_id`) carrying its own `shared_helper.py`, then dispatch.
///
/// This is the runtime proof for Wave C1's static-free Python loader:
/// - A double `Py_Initialize` (if the once-per-process init guard regressed)
///   would panic/abort here.
/// - A nonce collision (if the per-bundle `sys.modules` re-key prefix were not
///   made unique by the runtime-vended nonce) would make runtime B import
///   runtime A's cached `shared_helper`, so B would observe A's `VALUE`.
///
/// Both must dispatch their OWN value, proving the process-global `SharedState`
/// vends a unique nonce per load across distinct runtimes.
#[test]
fn two_runtimes_same_named_bundle_do_not_collide_in_sys_modules() {
    let name: &str = "shared_name";

    let (_dir_a, path_a) = write_shared_name_bundle("alpha_contract", 0x11);
    let (_dir_b, path_b) = write_shared_name_bundle("beta_contract", 0x22);

    let loader_a: PythonLoader = PythonLoader::default();
    let loader_b: PythonLoader = PythonLoader::default();
    let runtime_a: Arc<Runtime> = make_runtime();
    let runtime_b: Arc<Runtime> = make_runtime();

    let manifest_a: ManifestData = make_manifest(&path_a, name);
    let manifest_b: ManifestData = make_manifest(&path_b, name);

    loader_a
        .load(
            &manifest_a,
            &polyplug::loader::BundleSource::Path(manifest_a.path.clone()),
            &runtime_a,
        )
        .expect("runtime A load must succeed");
    loader_b
        .load(
            &manifest_b,
            &polyplug::loader::BundleSource::Path(manifest_b.path.clone()),
            &runtime_b,
        )
        .expect("runtime B load must succeed");

    let alpha_cid: u64 = GuestContractId::new("alpha_contract", 1).id();
    let beta_cid: u64 = GuestContractId::new("beta_contract", 1).id();

    let mut out_a: i32 = 0;
    // SAFETY: out_a is a valid i32; the callable writes a 4-byte int there.
    let err_a: AbiError = unsafe {
        dispatch(
            &runtime_a,
            alpha_cid,
            0,
            core::ptr::null(),
            &mut out_a as *mut i32 as *mut (),
        )
    };
    assert!(err_a.is_ok(), "A dispatch failed code={}", err_a.code);

    let mut out_b: i32 = 0;
    // SAFETY: out_b is a valid i32; the callable writes a 4-byte int there.
    let err_b: AbiError = unsafe {
        dispatch(
            &runtime_b,
            beta_cid,
            0,
            core::ptr::null(),
            &mut out_b as *mut i32 as *mut (),
        )
    };
    assert!(err_b.is_ok(), "B dispatch failed code={}", err_b.code);

    assert_eq!(
        out_a, 0x11,
        "runtime A must read its OWN shared_helper.VALUE (0x11)"
    );
    assert_eq!(
        out_b, 0x22,
        "runtime B must read its OWN shared_helper.VALUE (0x22), not A's cached module"
    );
}
