#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;

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

fn create_runtime() -> Runtime {
    Runtime::builder()
        .loader(make_loader())
        .build()
        .expect("failed to build runtime")
}

fn load_fixture(rt: &Runtime) -> Result<(), RuntimeError> {
    rt.load_bundle(std::path::Path::new(PYTHON_PLUGIN))
}

fn get_vtable(rt: &Runtime) -> *const PluginInterface {
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = rt
        .find_by_contract(contract_id, 0)
        .expect("test.add must be registered after load_fixture()");
    rt.resolve_plugin(handle)
        .expect("handle must be valid")
        .vtable()
}

#[test]
fn integration_python_runtime_name() {
    let loader: PythonLoader = PythonLoader::default();
    assert_eq!(loader.runtime_name(), "python");
}

#[test]
fn integration_python_bundle_loads() {
    skip_if_no_python!();
    let rt: Runtime = create_runtime();
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Python module stays loaded for process lifetime.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 0 (add). args/out are correctly typed for the add function.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: cast to generic dispatch signature; arg types enforced by test (AddArgs matches).
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args is a valid AddArgs, out is a valid u32.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn integration_python_add_primitive() {
    skip_if_no_python!();
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Python module stays loaded.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 2,
        "test.add vtable must have at least 2 functions"
    );
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 1 (add_primitive). args/out are correctly typed.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(1) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature as add; arg types enforced by test.
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args and out are valid and correctly typed.
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add_primitive must return ABI_OK");
    assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
}

#[test]
fn integration_python_version_string() {
    skip_if_no_python!();
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 3,
        "test.add vtable must have at least 3 functions"
    );
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; version takes no args (null input accepted by Python side).
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: out_view is a valid StringView allocation on the stack.
    let result: AbiError = unsafe {
        dispatch_fn(
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "version must return ABI_OK");
}

#[test]
fn integration_python_exception_returns_abi_error() {
    skip_if_no_python!();
    // Create a temp bundle directory with manifest.toml and a Python script that raises an exception.
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("exception_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    // Write manifest.toml
    let manifest_content = r#"
name = "exception_test"
id = 8888888888888
version = "1.0.0"
runtime = "python"
file = "plugin.py"
provides = ["test.exception@1"]

[function_count]
"test.exception@1" = 1
"#;
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");

    // Write Python script that raises an exception in polyplug_init
    let plugin_content = r#"def polyplug_abi_version():
    return 1

def polyplug_init(registrar_addr):
    raise ValueError("test exception from polyplug_init")
"#;
    std::fs::write(tmp_dir.join("plugin.py"), plugin_content).expect("write plugin.py");

    let rt: Runtime = create_runtime();
    let result: Result<(), RuntimeError> = rt.load_bundle(&tmp_dir);
    match result {
        Err(RuntimeError::Loader(LoaderError::PythonInitRaisedException { .. })) => {}
        other => panic!("expected PythonInitRaisedException, got: {other:?}"),
    }

    // Cleanup
    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_python_utf8_roundtrip() {
    skip_if_no_python!();
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 3,
        "test.add vtable must have at least 3 functions"
    );
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
    let fn_ptr: *const () = unsafe { *vtable.dispatch.native.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; version takes no args (null input accepted by Python side).
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: out_view is a valid StringView allocation on the stack.
    let result: AbiError = unsafe {
        dispatch_fn(
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "version must return ABI_OK");
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
    let manifest_content = r#"
name = "version_test"
id = 7777777777777
version = "1.0.0"
runtime = "python"
file = "plugin.py"
provides = ["test.version@1"]

[function_count]
"test.version@1" = 1
"#;
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");
    std::fs::write(tmp_dir.join("plugin.py"), b"# empty plugin").expect("write plugin.py");

    let rt: Runtime = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig {
            min_version: (99, 0),
        }))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(&tmp_dir);
    match result {
        Err(RuntimeError::Loader(LoaderError::RuntimeVersionMismatch { .. })) => {}
        other => panic!("expected RuntimeVersionMismatch for Python 99.0, got: {other:?}"),
    }

    // Cleanup
    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_python_runtime_name_is_python() {
    let loader: PythonLoader = PythonLoader::new(PythonConfig::default());
    assert_eq!(loader.runtime_name(), "python");
}
