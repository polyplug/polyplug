#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::LogLevel;
use polyplug_abi::StringView;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

const PYTHON_PLUGIN: &str = env!("TEST_PYTHON_PLUGIN");
const SKIP_PYTHON: bool = {
    let a: &[u8] = PYTHON_PLUGIN.as_bytes();
    let b: &[u8] = b"PYTHON_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

macro_rules! skip_if_no_python {
    () => {
        if SKIP_PYTHON {
            return;
        }
    };
}

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

fn make_loader() -> PythonLoader {
    PythonLoader::new(PythonConfig::default())
}

fn create_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(make_loader())
        .build()
        .expect("failed to build runtime")
}

fn load_fixture(rt: &Runtime) -> Result<(), RuntimeError> {
    rt.load_bundle(std::path::Path::new(PYTHON_PLUGIN))
}

fn get_vtable(rt: &Runtime) -> *const GuestContractInterface {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load_fixture()");
    rt.resolve_guest_contract(handle)
        .expect("handle must be valid")
}

/// Invoke a function on a VM-dispatch vtable through `dispatch.vm.call`.
///
/// Python guests now register a `DispatchType::VirtualMachine` interface (the
/// previous ctypes native-fn-ptr path is gone), so dispatch flows through the
/// loader-provided 6-arg `call` exactly like the Lua and JS guests.
///
/// # Safety
/// `fn_id` must be a valid slot declared by the fixture's `function_count`, and
/// `args`/`out` must point to live, correctly-typed buffers for that function's
/// ABI layout that outlive the synchronous call.
unsafe fn call_vm_function(
    vtable: &GuestContractInterface,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "python loader must use VM dispatch"
    );
    // SAFETY: the dispatch_type assertion above proves the active union variant is `vm`, so reading
    // `dispatch.vm.{call,loader_data}` is the correct field of the union. The runtime populates
    // `vm.call` with a non-null loader-provided dispatcher and `vm.loader_data` with the matching
    // loader context during registration; `vtable` is a live borrow held by the caller for the
    // duration of this call, so both remain valid. A null arena selects the `host->alloc` fallback
    // for variable-size returns. `fn_id`, `args`, and `out` are forwarded under the caller's
    // invariants (see the call sites).
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            core::ptr::null_mut(),
        )
    }
}

#[test]
fn integration_python_runtime_name() {
    let loader: PythonLoader = PythonLoader::default();
    assert_eq!(loader.runtime_name(), "python");
}

#[test]
fn integration_python_bundle_loads() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = load_fixture(&rt);
    assert!(
        result.is_ok(),
        "PythonLoader::load() must succeed for fixture: {:?}",
        result.err()
    );
}

#[test]
fn integration_python_add() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Python module stays loaded for process lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_id 0 is the `add` slot declared by the fixture's `function_count`. `args` points to
    // a live `AddArgs` (`#[repr(C)]`, matching the guest's `add` parameter layout) and `out` points
    // to a live `u32` matching the declared return; both outlive the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            0,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn integration_python_add_primitive() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Python module stays loaded.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_id 1 is the `add_primitive` slot declared by the fixture. `args` points to a live
    // `AddArgs` (`#[repr(C)]`, matching the guest's parameter layout) and `out` points to a live
    // `u32` matching the declared return; both outlive the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            1,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add_primitive must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
}

#[test]
fn integration_python_version_string() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_id 2 is the `version` slot. It takes no args, so a null `args` pointer is accepted
    // by the Python side; `out` points to a live `StringView` slot for the return.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "version must return AbiErrorCode::Ok"
    );
}

/// Python guests now dispatch through `DispatchType::VirtualMachine` (the
/// previous ctypes native-fn-ptr path is gone, since closure trampolines are
/// undefined behaviour on arm64). The VM `call` signature carries an optional
/// per-call `CallArena`; this test passes a null arena, which selects the
/// `host->alloc` fallback for the variable-size string return. This test pins
/// that the string return stays correct across many calls on the null-arena
/// path — no stale buffer reuse, no aliasing between iterations.
#[test]
fn integration_python_string_return_vm_null_arena() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid for the runtime lifetime.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "python guests use VM dispatch"
    );

    // The same value must come back correctly on every call on the null-arena path.
    for i in 0..64 {
        let mut out_view: StringView = StringView::null();
        // SAFETY: fn_id 2 is the `version` slot; it reads no input, so a null `args` pointer is
        // accepted. `out` points to a live `StringView` slot for this iteration.
        let result: AbiError = unsafe {
            call_vm_function(
                vtable,
                2,
                core::ptr::null::<()>(),
                &mut out_view as *mut StringView as *mut (),
            )
        };
        assert_eq!(
            result.code,
            AbiErrorCode::Ok as u32,
            "version must return Ok (iter {i})"
        );
        // SAFETY: Ok return guarantees out_view points at a valid UTF-8 buffer.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
        let version: &str = core::str::from_utf8(bytes).expect("version must be UTF-8");
        assert!(version.starts_with("1.0"), "version value (iter {i})");
    }
}

#[test]
fn integration_python_exception_returns_abi_error() {
    skip_if_no_python!();
    // Create a temp bundle directory with manifest.toml and a Python script that raises an exception.
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("exception_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    // Write manifest.toml
    let manifest_content: String = format!(
        r#"
name = "exception_test"
id = {}
version = "1.0.0"
runtime = "python"
file = "plugin.py"
provides = ["test.exception@1"]

[function_count]
"test.exception@1" = 1
"#,
        polyplug_utils::bundle_id("exception_test")
    );
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");

    // Write Python script that raises an exception in polyplug_init
    // The loader calls polyplug_init(host_interface, ctx) (self-passing pattern).
    let plugin_content = r#"def polyplug_abi_version():
    return 1

def polyplug_init(host_interface, ctx):
    raise ValueError("test exception from polyplug_init")
"#;
    std::fs::write(tmp_dir.join("plugin.py"), plugin_content).expect("write plugin.py");

    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = rt.load_bundle(&tmp_dir);
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle, error })) => {
            // The loader reports the failure against the module file stem
            // (the `file` entry, "plugin.py" -> "plugin"), since the exception
            // is raised inside `polyplug_init` after the module path is resolved.
            assert_eq!(
                bundle, "plugin",
                "init failure should be reported against the module file stem: {bundle}"
            );
            assert!(
                error.contains("exception")
                    || error.contains("ValueError")
                    || error.contains("test exception"),
                "error should mention exception details: {error}"
            );
        }
        other => panic!("expected InitFailed for raised exception, got: {other:?}"),
    }

    // Cleanup
    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_python_utf8_roundtrip() {
    skip_if_no_python!();
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_id 2 is the `version` slot; it reads no input, so a null `args` pointer is accepted
    // by the Python side. `out` points to a live `StringView` slot for the return.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "version must return AbiErrorCode::Ok"
    );
    // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes.
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let version_str: &str = core::str::from_utf8(version_bytes).expect("version must be UTF-8");
    assert!(
        !version_str.is_empty(),
        "version() must return non-empty UTF-8"
    );
    let starts_with: bool = version_str.starts_with("1.0");
    assert!(starts_with, "version() must start with 1.0");
}

#[test]
fn integration_python_version_too_old() {
    // Create a temp bundle directory to test version mismatch
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("version_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    // Write manifest.toml
    let manifest_content: String = format!(
        r#"
name = "version_test"
id = {}
version = "1.0.0"
runtime = "python"
file = "plugin.py"
provides = ["test.version@1"]

[function_count]
"test.version@1" = 1
"#,
        polyplug_utils::bundle_id("version_test")
    );
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");
    std::fs::write(tmp_dir.join("plugin.py"), b"# empty plugin").expect("write plugin.py");

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig {
            min_version: (99, 0),
        }))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(&tmp_dir);
    match result {
        Err(RuntimeError::Loader(LoaderError::InitFailed { bundle, error })) => {
            // The version gate runs at interpreter initialization, before any
            // bundle-specific context exists, so the failure is reported against
            // the "python" runtime rather than the manifest name.
            assert_eq!(
                bundle, "python",
                "runtime version mismatch should be reported against the python runtime: {bundle}"
            );
            assert!(
                error.contains("version"),
                "error should mention version: {error}"
            );
        }
        other => panic!("expected InitFailed for version mismatch, got: {other:?}"),
    }

    // Cleanup
    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_python_runtime_name_is_python() {
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    assert_eq!(loader.runtime_name(), "python");
}

// ─── Guest logging (HostApi.log via the python guest SDK) ───────────────────────

/// Vendor the CURRENT python guest SDK (`polyplug_guest` + `polyplug_abi` + the
/// canonical generated ABI module) into `<bundle>/site-packages/`, mirroring
/// `integration_peer_caller_python.rs`. The fixture's vendored copy is NOT used:
/// it can lag the live SDK, and this test must exercise the current `log` helper.
fn vendor_current_python_sdk(bundle_dir: &Path) {
    let workspace_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of tests/integration")
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let sdk_root: PathBuf = workspace_root.join("sdks").join("python");
    let site: PathBuf = bundle_dir.join("site-packages");

    let guest_dst: PathBuf = site.join("polyplug_guest");
    std::fs::create_dir_all(&guest_dst).expect("create polyplug_guest dir");
    std::fs::copy(
        sdk_root
            .join("guest")
            .join("polyplug_guest")
            .join("__init__.py"),
        guest_dst.join("__init__.py"),
    )
    .expect("vendor polyplug_guest");

    let abi_src: PathBuf = sdk_root.join("polyplug_abi").join("polyplug_abi");
    let abi_dst: PathBuf = site.join("polyplug_abi");
    std::fs::create_dir_all(&abi_dst).expect("create polyplug_abi dir");
    for name in ["__init__.py", "abi.py", "string_view_helper.py"] {
        std::fs::copy(abi_src.join(name), abi_dst.join(name))
            .unwrap_or_else(|e| panic!("vendor polyplug_abi/{name}: {e}"));
    }

    // polyplug_abi.abi falls back to `from polyplug.abi.abi import *`, so the
    // canonical generated ABI module must be reachable as the `polyplug` package.
    let polyplug_abi_pkg: PathBuf = site.join("polyplug").join("abi");
    std::fs::create_dir_all(&polyplug_abi_pkg).expect("create polyplug/abi dir");
    std::fs::write(site.join("polyplug").join("__init__.py"), b"").expect("polyplug __init__");
    std::fs::write(polyplug_abi_pkg.join("__init__.py"), b"").expect("polyplug/abi __init__");
    std::fs::copy(
        sdk_root.join("abi").join("abi.py"),
        polyplug_abi_pkg.join("abi.py"),
    )
    .expect("vendor polyplug/abi/abi.py");
}

/// Write a python bundle whose single contract function calls the guest SDK's
/// `log()` helper, plus a module-top-level `log()` probe that runs BEFORE
/// `polyplug_init` stores the host (must be a graceful no-op).
fn write_log_demo_bundle(tmp: &Path) -> PathBuf {
    let dir: PathBuf = tmp.join("log_demo_python");
    std::fs::create_dir_all(&dir).expect("create log demo bundle dir");

    let id_val: u64 = polyplug_utils::bundle_id("log_demo_python");
    let manifest: String = format!(
        "name = \"log_demo_python\"\n\
         id = {id_val}\n\
         bundle_name = \"log_demo_python\"\n\
         version = \"1.0.0\"\n\
         runtime = \"python\"\n\
         file = \"logdemo.py\"\n\
         provides = [\"test.logdemo@1\"]\n\
         needs_reinit_on_dep_reload = false\n\n\
         [function_count]\n\
         \"test.logdemo@1\" = 1\n",
    );
    std::fs::write(dir.join("manifest.toml"), manifest).expect("write manifest.toml");

    let plugin_py: &str = "from polyplug_guest import LogLevel, log, register_contract, store_host_interface\n\
         \n\
         # Module top level runs BEFORE polyplug_init stores the host: log() must\n\
         # be a graceful no-op here, never a crash or a delivered record.\n\
         log(LogLevel.Error, \"guest.logdemo\", \"before-init must not be delivered\")\n\
         \n\
         \n\
         def _do_log(args_ptr: int, out_ptr: int, arena_ptr: int) -> None:\n\
         \x20   log(LogLevel.Info, \"guest.logdemo\", \"héllo from python ✓\")\n\
         \n\
         \n\
         def polyplug_init(host_ptr: int, ctx_ptr: int) -> None:\n\
         \x20   store_host_interface(host_ptr)\n\
         \x20   register_contract(\n\
         \x20       globals(),\n\
         \x20       contract=\"test.logdemo@1\",\n\
         \x20       functions=[_do_log],\n\
         \x20       plugin_name=\"logdemo\",\n\
         \x20   )\n";
    std::fs::write(dir.join("logdemo.py"), plugin_py).expect("write logdemo.py");

    vendor_current_python_sdk(&dir);
    dir
}

/// End-to-end guest logging: a python guest calls the SDK `log()` helper, which
/// crosses `HostApi.log` and lands verbatim in the host logger installed via
/// `RuntimeBuilder::logger`. Also proves the pre-init no-op (the bundle logs once
/// at module top level before `polyplug_init` — that record must NOT appear).
#[test]
fn integration_python_guest_log_routes_to_host_logger() {
    let tmp: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let bundle_dir: PathBuf = write_log_demo_bundle(tmp.path());

    let records: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::clone(&records);
    let rt: Arc<Runtime> = Runtime::builder()
        .loader(make_loader())
        .logger(move |level: LogLevel, scope: &str, message: &str| {
            sink.lock()
                .expect("logger mutex must not be poisoned")
                .push((level, scope.to_owned(), message.to_owned()));
        })
        .build()
        .expect("failed to build runtime");

    rt.load_bundle(&bundle_dir).expect("log bundle must load");

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.logdemo", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.logdemo must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    // SAFETY: vtable_ptr is non-null and stays valid for the runtime lifetime
    // (retire-not-drop keeps resolved interfaces alive).
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    let args: u32 = 0;
    let mut out: u32 = 0;
    // SAFETY: fn_id 0 is declared by the manifest's function_count; `_do_log`
    // reads neither args nor out, and both pointers reference live stack
    // buffers that outlive the synchronous call.
    let err: AbiError = unsafe {
        call_vm_function(
            vtable,
            0,
            &args as *const u32 as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::Ok as u32,
        "log dispatch must return Ok; got code={}",
        err.code
    );

    let captured: Vec<(LogLevel, String, String)> = records
        .lock()
        .expect("logger mutex must not be poisoned")
        .clone();
    let guest_records: Vec<&(LogLevel, String, String)> = captured
        .iter()
        .filter(|(_, scope, _)| scope == "guest.logdemo")
        .collect();
    assert_eq!(
        guest_records.len(),
        1,
        "exactly one guest.logdemo record expected (the pre-init log must be a no-op); got: {captured:?}"
    );
    let (level, scope, message): &(LogLevel, String, String) = guest_records[0];
    assert_eq!(*level, LogLevel::Info, "level must arrive verbatim");
    assert_eq!(scope, "guest.logdemo", "scope must arrive verbatim");
    assert_eq!(
        message, "héllo from python ✓",
        "message must arrive verbatim, UTF-8 intact"
    );
}
