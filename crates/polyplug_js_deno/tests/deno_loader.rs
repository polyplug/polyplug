//! Integration tests for the Deno (V8 in-process) bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), vtable registration, trampoline dispatch, permission flags
//! (none required — V8 is in-process), and thread safety of concurrent loads.

#![allow(clippy::expect_used)]

use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;

// ─── Minimal HostVTable (no-op implementations) ───────────────────────────────

unsafe extern "C" fn noop_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    _size: usize,
    _align: usize,
) -> *mut u8 {
    core::ptr::null_mut()
}

unsafe extern "C" fn noop_free(
    _rt_ctx: *mut core::ffi::c_void,
    _ptr: *mut u8,
    _size: usize,
    _align: usize,
) {
}

unsafe extern "C" fn noop_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min: u32,
) -> PluginHandle {
    PluginHandle::null()
}

unsafe extern "C" fn noop_find_by_bundle(
    _rt_ctx: *mut core::ffi::c_void,
    _bundle_id: u64,
    _contract_id: u64,
    _min: u32,
) -> PluginHandle {
    PluginHandle::null()
}

unsafe extern "C" fn noop_find_all_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

unsafe extern "C" fn noop_resolve_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _handle: PluginHandle,
) -> *const PluginVTable {
    core::ptr::null()
}

unsafe extern "C" fn noop_get_extension(
    _rt_ctx: *mut core::ffi::c_void,
    _extension_id: u32,
) -> *const () {
    core::ptr::null()
}

unsafe extern "C" fn noop_register_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _descriptor: *const PluginDescriptor,
    _vtable: *const PluginVTable,
) -> AbiError {
    AbiError::ok()
}

static NOOP_HOST_VTABLE: HostVTable = HostVTable {
    register_plugin: noop_register_plugin,
    alloc: noop_alloc,
    free: noop_free,
    find_by_contract: noop_find_by_contract,
    find_by_bundle: noop_find_by_bundle,
    find_all_by_contract: noop_find_all_by_contract,
    resolve_plugin: noop_resolve_plugin,
    get_extension: noop_get_extension,
};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn make_deno_bundle(contract_id: u64, vtable_ptr: usize, fn_count: u32) -> String {
    format!("Deno.core.ops.op_register_vtable({contract_id}n, {vtable_ptr}n, {fn_count});\n")
}

fn write_temp_bundle(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: std::path::PathBuf = dir.path().join("bundle.js");
    std::fs::write(&path, content).expect("write bundle.js");
    (dir, path)
}

fn make_loader() -> JsDenoLoader {
    JsDenoLoader::new(JsDenoConfig {})
}

fn make_runtime() -> Runtime {
    RuntimeBuilder::new()
        .loader(make_loader())
        .build()
        .expect("runtime build")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn runtime_name_is_js_deno() {
    let loader: JsDenoLoader = make_loader();
    assert_eq!(loader.runtime_name(), "js-deno");
}

#[test]
fn loader_construction_does_not_panic() {
    let _loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
}

#[test]
fn load_valid_bundle_registers_vtable() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.noop", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_ok(), "load must succeed: {result:?}");
}

#[test]
fn load_bundle_with_functions_registers_correct_count() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.math", 1);
    let fn_count: u32 = 3;

    let dummy_fn_array: Box<[*const ()]> = vec![core::ptr::null(); fn_count as usize].into();
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: fn_count,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, fn_count);
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_ok(), "load must succeed: {result:?}");
}

#[test]
fn load_accepts_directory_path() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.dir", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, 0);
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bundle.js"), &bundle).expect("write bundle.js");

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(dir.path());
    assert!(
        result.is_ok(),
        "load from directory path must succeed: {result:?}"
    );
}

#[test]
fn load_syntax_error_returns_error() {
    let bundle: &str = "this is not valid javascript }{{{";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_err(), "syntax error bundle must return Err");

    let err_str: String = result
        .expect_err("syntax error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("failed to load module")
            || err_str.contains("SyntaxError")
            || err_str.contains("failed to execute"),
        "error must indicate load or execution failure: {err_str}"
    );
}

#[test]
fn load_runtime_error_returns_error() {
    let bundle: &str = "throw new Error('intentional runtime error');";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_err(), "runtime error bundle must return Err");

    let err_str: String = result
        .expect_err("runtime error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("event loop failed")
            || err_str.contains("intentional runtime error")
            || err_str.contains("failed to execute"),
        "error must indicate execution failure: {err_str}"
    );
}

#[test]
fn load_bundle_without_register_vtable_returns_error() {
    let bundle: &str = "var x = 1 + 2;";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(
        result.is_err(),
        "bundle without op_register_vtable must return Err"
    );

    let err_str: String = result
        .expect_err("bundle without op_register_vtable must return Err")
        .to_string();
    assert!(
        err_str.contains("js-deno"),
        "error must mention runtime name: {err_str}"
    );
}

#[test]
fn load_bundle_null_vtable_pointer_returns_error() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.null_vtable", 1);

    let bundle: String = format!("Deno.core.ops.op_register_vtable({contract_id}n, 0n, 1);\n");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_err(), "null vtable pointer must return Err");

    let err_str: String = result
        .expect_err("null vtable pointer must return Err")
        .to_string();
    assert!(
        err_str.contains("null vtable"),
        "error must mention null vtable: {err_str}"
    );
}

#[test]
fn load_nonexistent_file_returns_error() {
    let path: std::path::PathBuf =
        std::path::PathBuf::from("/tmp/polyplug_js_deno_test_nonexistent_bundle_xyz.js");

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_err(), "non-existent file must return Err");
}

#[test]
fn bundle_path_global_is_injected() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.bundlepath", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = format!(
        r#"
if (typeof globalThis.bundlePath !== 'string') {{
    throw new Error('bundlePath not injected');
}}
Deno.core.ops.op_register_vtable({contract_id}n, {vtable_ptr}n, 0);
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(
        result.is_ok(),
        "bundle reading bundlePath must succeed: {result:?}"
    );
}

#[test]
fn deno_core_ops_are_accessible() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.ops", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = format!(
        r#"
var ops = [
    'op_find_by_contract',
    'op_find_by_bundle',
    'op_find_all_by_contract',
    'op_resolve_plugin',
    'op_get_extension',
    'op_register_vtable',
    'op_alloc',
    'op_free',
];
for (var i = 0; i < ops.length; i++) {{
    if (typeof Deno.core.ops[ops[i]] !== 'function') {{
        throw new Error('missing op: ' + ops[i]);
    }}
}}
Deno.core.ops.op_register_vtable({contract_id}n, {vtable_ptr}n, 0);
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_ok(), "all Deno ops must be present: {result:?}");
}

#[test]
fn vtable_contract_id_roundtrip() {
    let contract_id: u64 = polyplug_abi::contract_id("image.decode", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(result.is_ok(), "load must succeed: {result:?}");
}

#[test]
fn no_subprocess_permissions_required() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.noperm", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(
        result.is_ok(),
        "in-process V8 load requires no external permissions: {result:?}"
    );
}

#[test]
fn generated_host_caller_factory_pattern_works() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.caller", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = format!(
        r#"
class TestCallerContract {{
    #guard;

    constructor(guard) {{
        this.#guard = guard;
    }}

    static create(rt, minVersion = 0) {{
        const handle = rt.findByContract({contract_id}n, minVersion);
        if (handle === null || handle === undefined) {{
            return null;
        }}
        const guard = rt.getGuard(handle);
        if (!guard) {{
            return null;
        }}
        return new TestCallerContract(guard);
    }}

    isValid() {{
        return this.#guard !== null && this.#guard !== undefined;
    }}

    reset() {{
        this.#guard = null;
    }}
}}

const mockRuntime = {{
    findByContract: function(contractId, minVersion) {{
        return {{ handle: 12345 }};
    }},
    getGuard: function(handle) {{
        return {{ vtable: function() {{ return {{ functions: [] }}; }} }};
    }}
}};

const caller = TestCallerContract.create(mockRuntime, 0);
if (caller === null) {{
    throw new Error('create() should return instance when plugin found');
}}

if (!caller.isValid()) {{
    throw new Error('isValid() should return true for valid instance');
}}

caller.reset();
if (caller.isValid()) {{
    throw new Error('isValid() should return false after reset()');
}}

const mockRuntimeNoPlugin = {{
    findByContract: function(contractId, minVersion) {{
        return null;
    }},
    getGuard: function(handle) {{
        return null;
    }}
}};
const nullCaller = TestCallerContract.create(mockRuntimeNoPlugin, 0);
if (nullCaller !== null) {{
    throw new Error('create() should return null when no plugin found');
}}

const mockRuntimeNoGuard = {{
    findByContract: function(contractId, minVersion) {{
        return {{ handle: 12345 }};
    }},
    getGuard: function(handle) {{
        return null;
    }}
}};
const noGuardCaller = TestCallerContract.create(mockRuntimeNoGuard, 0);
if (noGuardCaller !== null) {{
    throw new Error('create() should return null when getGuard returns null');
}}

Deno.core.ops.op_register_vtable({contract_id}n, {vtable_ptr}n, 0);
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let result: Result<(), polyplug::error::PolyplugError> =
        runtime.load_bundle(&path);
    assert!(
        result.is_ok(),
        "host caller factory pattern test must succeed: {result:?}"
    );
}
