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
//           "functions": [callable, ...],       # ordered by fn_id
//       },
//   ]
//
// Each callable is invoked as fn(args_ptr_int, out_ptr_int, arena_ptr_int).
#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime_builder::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractId;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::RuntimeConfig;
use polyplug_abi::UnloadMode;
use polyplug_abi::dispatch::DispatchType;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use polyplug_utils::BundleId;
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
runtime = "python"
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
        runtime: "python".to_owned(),
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
    // SAFETY: resolved interface is a non-null, runtime-owned, retire-not-drop
    // GuestContractInterface; reading it and its VM dispatch fields is sound.
    let interface: &polyplug_abi::GuestContractInterface = unsafe { &*interface_ptr };
    assert_eq!(
        interface.dispatch_type,
        DispatchType::VirtualMachine,
        "Python contracts must register VM dispatch"
    );
    // SAFETY: dispatch_type == VirtualMachine guarantees the `vm` union arm is
    // active, so reading it is sound.
    let vm: polyplug_abi::dispatch::vm_dispatch::VmDispatch = unsafe { interface.dispatch.vm };
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
        )
    }
}

// ─── Plugin sources (VM dispatch / _polyplug_registrations) ─────────────────────

/// A plugin that registers one contract whose function 0 writes 0x2A into the
/// 4-byte int at `out` (via ctypes) and ignores `args`/`arena`.
const WRITE_OUT_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = 0x2A

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {
            "contract": "writeout@1",
            "plugin_name": "writeout_plugin",
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
def _fn0(args_ptr, out_ptr, arena_ptr):
    raise ValueError("dispatch boom")

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "raiser@1", "functions": [_fn0]},
    ]
"#;

fn raiser_contract_id() -> u64 {
    GuestContractId::new("raiser", 1).id()
}

/// A plugin whose function 0 forwards the arena pointer it received into `out`
/// (as an i64) so the test can assert the arena pointer is forwarded verbatim.
const ARENA_FORWARD_PLUGIN_SRC: &str = r#"
import ctypes

def _fn0(args_ptr, out_ptr, arena_ptr):
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int64))[0] = arena_ptr

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {"contract": "arenafwd@1", "functions": [_fn0]},
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
    let result: Result<(), RuntimeError> = loader.load(
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
    let result: Result<(), RuntimeError> = PythonLoader::new(PythonConfig::default()).load(
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
    let err: RuntimeError = PythonLoader::new(config)
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected version mismatch");
    match err {
        RuntimeError::Loader(LoaderError::InitFailed { bundle, error }) => {
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
    let result: Result<(), RuntimeError> = loader.load(
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
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for syntax error plugin");
    assert!(matches!(
        err,
        RuntimeError::Loader(LoaderError::InitFailed { .. })
    ));
}

#[test]
fn test_import_error_returns_init_failed() {
    let (_dir, path) = write_bundle("import_err", IMPORT_ERROR_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "import_err");
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for import-error plugin");
    assert!(matches!(
        err,
        RuntimeError::Loader(LoaderError::InitFailed { .. })
    ));
}

#[test]
fn test_missing_init_returns_init_symbol_missing() {
    let (_dir, path) = write_bundle("no_init", MISSING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "no_init");
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure");
    assert!(matches!(
        err,
        RuntimeError::Loader(LoaderError::InitSymbolMissing { .. })
    ));
}

#[test]
fn test_raising_init_returns_init_failed() {
    let (_dir, path) = write_bundle("raising_init", RAISING_INIT_PLUGIN_SRC);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "raising_init");
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure");
    match err {
        RuntimeError::Loader(LoaderError::InitFailed { error, .. }) => {
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
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for missing registrations");
    match err {
        RuntimeError::Loader(LoaderError::InitFailed { error, .. }) => {
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
    let err: RuntimeError = loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect_err("expected failure for empty registrations");
    assert!(matches!(
        err,
        RuntimeError::Loader(LoaderError::InitFailed { .. })
    ));
}

#[test]
fn test_runtime_name() {
    let loader: PythonLoader = PythonLoader::default();
    assert_eq!(loader.runtime_name(), "python");
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
        let result: Result<(), RuntimeError> = loader.load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        );
        // Only the first load registers `writeout@1`; later loads collide on the
        // contract id. The point of this test is interpreter stability, so accept
        // either Ok or a duplicate-registration InitFailed without panicking.
        assert!(
            result.is_ok()
                || matches!(
                    result,
                    Err(RuntimeError::Loader(LoaderError::InitFailed { .. }))
                ),
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

    let result: Result<(), RuntimeError> = loader.load(
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

def _fn0(args_ptr, out_ptr, arena_ptr):
    pass

def polyplug_init(_host_interface: int, ctx_addr: int) -> None:
    ctx = _BundleInitContext.from_address(ctx_addr)
    assert ctx.bundle_path.ptr is not None and ctx.bundle_path.ptr != 0
    assert ctx.bundle_path.len > 0
    global _polyplug_registrations
    _polyplug_registrations = [{"contract": "ctxcheck@1", "functions": [_fn0]}]
"#;

    let (_dir, path) = write_bundle("ctx_check", plugin_src);
    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = make_manifest(&path, "ctx_check");
    let result: Result<(), RuntimeError> = loader.load(
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
    // The helper's callable calls `_polyplug_arena_alloc(16)` during dispatch.
    // The bridge must resolve from the helper module's globals (where the ABI
    // functions are defined), not only from the entry module — this locks the
    // injection point at polyplug_init.__globals__. The dispatch passes a null
    // arena, so the bridge takes the host-alloc fallback and must return a
    // nonzero address; the callable writes that address (truthiness) into `out`.
    let helper_src: &str = r#"
import ctypes

def _fn0(args_ptr, out_ptr, arena_ptr):
    buf = _polyplug_arena_alloc(16)
    if buf == 0:
        raise RuntimeError("_polyplug_arena_alloc returned 0")
    ctypes.cast(out_ptr, ctypes.POINTER(ctypes.c_int32))[0] = 0x2A

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {
            "contract": "splitmod@1",
            "plugin_name": "splitmod_plugin",
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

    let result: Result<(), RuntimeError> = loader.load(
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

// ─── Unload: sys.modules purge under UnloadMode::Reclaim ─────────────────────────

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
def _fn0(args_ptr, out_ptr, arena_ptr):
    pass

def polyplug_init(host_interface: int, ctx: int) -> None:
    global _polyplug_registrations
    _polyplug_registrations = [
        {{
            "contract": "{contract}",
            "plugin_name": "{name}_plugin",
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

/// Under `UnloadMode::Reclaim`, unloading a bundle purges its re-keyed
/// `sys.modules` entries so a later load re-imports fresh source.
#[test]
fn unload_reclaim_purges_bundle_modules_from_sys_modules() {
    let bundle_name: &str = "reclaim_purge";
    let helper_module: &str = "_reclaim_purge_helper";
    let (_dir, path) = write_split_bundle(bundle_name, helper_module, "reclaimpurge@1");

    let loader: PythonLoader = PythonLoader::default();
    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .loader(PythonLoader::default())
        .config(RuntimeConfig {
            unload_mode: UnloadMode::Reclaim,
            ..RuntimeConfig::default()
        })
        .build()
        .expect("runtime build must succeed");
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
        .unload(BundleId::from_u64(bundle_id(bundle_name)), &runtime, true)
        .expect("unload must succeed");

    let after: usize = count_bundle_modules(helper_module);
    assert_eq!(
        after, 0,
        "Reclaim unload must purge the bundle's re-keyed sys.modules entries"
    );
}

/// Under `UnloadMode::Retire` (default), unloading a bundle keeps its re-keyed
/// `sys.modules` entries mapped (retire-not-drop).
#[test]
fn unload_retire_keeps_bundle_modules_in_sys_modules() {
    let bundle_name: &str = "retire_keep";
    let helper_module: &str = "_retire_keep_helper";
    let (_dir, path) = write_split_bundle(bundle_name, helper_module, "retirekeep@1");

    let loader: PythonLoader = PythonLoader::default();
    // Default config => UnloadMode::Retire.
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
        .unload(BundleId::from_u64(bundle_id(bundle_name)), &runtime, true)
        .expect("unload must succeed");

    let after: usize = count_bundle_modules(helper_module);
    assert_eq!(
        after, before,
        "Retire unload must keep the bundle's re-keyed sys.modules entries"
    );
}
