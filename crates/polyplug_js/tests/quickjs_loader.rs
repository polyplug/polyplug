//! Integration tests for the QuickJS bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), vtable registration, VM dispatch, memory management
//! helpers, and thread-safety of the shared QuickJS runtime.

#![allow(clippy::expect_used)]

use std::sync::Arc;
use std::sync::Mutex;

use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::DispatchType;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal bundle JS string that creates a global _VTABLE object.
fn make_bundle_js(contract_id: u64, fn_count: u32, contract_name: &str) -> String {
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    format!(
        r#"
globalThis.TEST_VTABLE = {{
    contractLo: {contract_lo},
    contractHi: {contract_hi},
    fnCount: {fn_count},
    contractName: "{contract_name}",
    functions: [
        // Stub functions that just return success
        function(args, out) {{ return 0; }}
    ]
}};
"#
    )
}

/// Write `content` to a temp file and return the path.
/// Also creates a minimal manifest.toml for the bundle.
fn write_temp_bundle(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    write_temp_bundle_with_name(content, "test.bundle")
}

/// Write `content` to a temp file with a specific bundle name.
fn write_temp_bundle_with_name(
    content: &str,
    name: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: std::path::PathBuf = dir.path().join("bundle.js");
    std::fs::write(&path, content).expect("write bundle.js");

    let bundle_id: u64 = polyplug_abi::bundle_id(name);
    let manifest: String = format!(
        r#"id = {}
name = "{}"
runtime = "js-quickjs"
file = "bundle.js"
"#,
        bundle_id, name
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    (dir, path)
}

/// Create a JsLoader.
fn make_loader() -> JsLoader {
    JsLoader::new(JsConfig {})
}

/// Create a minimal Runtime with the JsLoader registered.
fn make_runtime() -> Runtime {
    RuntimeBuilder::new()
        .loader(make_loader())
        .build()
        .expect("runtime build must succeed")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

// ── Runtime initialisation ────────────────────────────────────────────────────

#[test]
fn runtime_name_is_js_quickjs() {
    let loader: JsLoader = make_loader();
    assert_eq!(loader.runtime_name(), "js-quickjs");
}

// ── Valid bundle evaluation + vtable registration ─────────────────────────────

#[test]
fn load_valid_bundle_registers_vtable() {
    let contract_id: u64 = polyplug_abi::contract_id("test.noop", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.noop");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered by querying the registry.
    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

#[test]
fn load_bundle_with_functions_registers_correct_count() {
    let contract_id: u64 = polyplug_abi::contract_id("test.math", 1);
    let fn_count: u32 = 3;

    // Bundle with 3 functions
    let bundle: String = format!(
        r#"
globalThis.TEST_VTABLE = {{
    contractLo: {},
    contractHi: {},
    fnCount: {},
    contractName: "test.math",
    functions: [
        function(args, out) {{ return 0; }},
        function(args, out) {{ return 0; }},
        function(args, out) {{ return 0; }}
    ]
}};
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered.
    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    // Verify function_count
    let vtable_ptr: *const PluginVTable = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &PluginVTable = unsafe { &*vtable_ptr };
    assert_eq!(vtable_ref.function_count, fn_count);
}

// ── Directory path fallback ───────────────────────────────────────────────────

#[test]
fn load_accepts_directory_path() {
    let contract_id: u64 = polyplug_abi::contract_id("test.dir", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.dir");
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bundle.js"), &bundle).expect("write bundle.js");

    let bundle_id: u64 = polyplug_abi::bundle_id("test.dir");
    let manifest: String = format!(
        r#"id = {}
name = "test.dir"
runtime = "js-quickjs"
file = "bundle.js"
"#,
        bundle_id
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest).expect("write manifest.toml");

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    // Pass the directory — loader must append "bundle.js" automatically.
    let result: Result<(), polyplug::error::PolyplugError> = loader.load(dir.path(), &runtime);
    assert!(
        result.is_ok(),
        "load from directory path must succeed: {result:?}"
    );

    // Verify the plugin was registered.
    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

// ── Syntax error ──────────────────────────────────────────────────────────────

#[test]
fn load_syntax_error_returns_error() {
    let bundle: &str = "this is not valid javascript }{{{";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_err(), "syntax error bundle must return Err");

    // The error must be a JsRuntimePanic mentioning the eval failure.
    let err_str: String = result
        .expect_err("syntax error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("js-quickjs"),
        "error must mention runtime name: {err_str}"
    );
}

// ── Runtime error ─────────────────────────────────────────────────────────────

#[test]
fn load_runtime_error_returns_error() {
    // Valid JS syntax but throws at runtime.
    let bundle: &str = "throw new Error('intentional runtime error');";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_err(), "runtime error bundle must return Err");

    let err_str: String = result
        .expect_err("runtime error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("js-quickjs"),
        "error must mention runtime name: {err_str}"
    );
}

// ── Missing _VTABLE global ───────────────────────────────────────────────────

#[test]
fn load_bundle_without_vtable_returns_error() {
    // Valid JS that does not create a _VTABLE global.
    let bundle: &str = "var x = 1 + 2;";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_err(), "bundle without _VTABLE must return Err");

    let err_str: String = result
        .expect_err("bundle without _VTABLE must return Err")
        .to_string();
    assert!(
        err_str.contains("_VTABLE") || err_str.contains("vtable"),
        "error must mention vtable: {err_str}"
    );
}

// ── File not found ────────────────────────────────────────────────────────────

#[test]
fn load_nonexistent_file_returns_error() {
    let path: std::path::PathBuf =
        std::path::PathBuf::from("/tmp/polyplug_js_test_nonexistent_bundle_xyz.js");

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_err(), "non-existent file must return Err");
}

// ── BundlePath global injection ───────────────────────────────────────────────

#[test]
fn bundle_path_global_is_injected() {
    // The loader injects `globalThis.bundlePath` before evaluating the bundle.
    let contract_id: u64 = polyplug_abi::contract_id("test.bundlepath", 1);

    // Bundle reads bundlePath; if it is undefined the throw will surface as Err.
    let bundle: String = format!(
        r#"
if (typeof globalThis.bundlePath !== 'string') {{
    throw new Error('bundlePath not injected');
}}
globalThis.TEST_VTABLE = {{
    contractLo: {},
    contractHi: {},
    fnCount: 1,
    contractName: "test.bundlepath",
    functions: [function(args, out) {{ return 0; }}]
}};
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(
        result.is_ok(),
        "bundle reading bundlePath must succeed: {result:?}"
    );
}

// ── polyplug object is accessible in JS ───────────────────────────────────────

#[test]
fn polyplug_object_has_expected_methods() {
    // Verify all expected host methods are present on the polyplug global.
    let contract_id: u64 = polyplug_abi::contract_id("test.methods", 1);

    let bundle: String = format!(
        r#"
var methods = ['findByContract', 'findByBundle', 'findAllByContract',
                'resolvePlugin', 'getExtension', 'registerVtable', 'alloc', 'free'];
for (var i = 0; i < methods.length; i++) {{
    if (typeof polyplug[methods[i]] !== 'function') {{
        throw new Error('missing method: ' + methods[i]);
    }}
}}
globalThis.TEST_VTABLE = {{
    contractLo: {},
    contractHi: {},
    fnCount: 1,
    contractName: "test.methods",
    functions: [function(args, out) {{ return 0; }}]
}};
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(
        result.is_ok(),
        "all polyplug methods must be present: {result:?}"
    );
}

// ── VTable registration — contract_id roundtrip ───────────────────────────────

#[test]
fn vtable_contract_id_roundtrip() {
    // Use a well-known FNV-1a contract — contract_id("image.decode", 1).
    let contract_id: u64 = polyplug_abi::contract_id("image.decode", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "image.decode");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();
    loader.load(&path, &runtime).expect("load must succeed");

    // Verify the plugin was registered with the correct contract_id.
    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

// ── VM dispatch verification ──────────────────────────────────────────────────

#[test]
fn vtable_uses_vm_dispatch() {
    let contract_id: u64 = polyplug_abi::contract_id("test.vm_dispatch", 1);
    let fn_count: u32 = 2;

    let bundle: String = format!(
        r#"
globalThis.TEST_VTABLE = {{
    contractLo: {},
    contractHi: {},
    fnCount: {},
    contractName: "test.vm_dispatch",
    functions: [
        function(args, out) {{ return 0; }},
        function(args, out) {{ return 0; }}
    ]
}};
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    loader.load(&path, &runtime).expect("load must succeed");

    // Verify the plugin was registered and get its vtable.
    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    let vtable_ptr: *const PluginVTable = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &PluginVTable = unsafe { &*vtable_ptr };

    assert_eq!(vtable_ref.function_count, fn_count);
    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);
    // SAFETY: dispatch_type is VirtualMachine, so accessing .vm is valid.
    // The dispatch function pointer is always non-null (it's js_dispatch).
    assert!(!unsafe { vtable_ref.dispatch.vm.loader_data }.is_null());
}

// ── Memory management helpers ─────────────────────────────────────────────────

#[test]
fn js_alloc_and_free_calls_host_vtable() {
    let contract_id: u64 = polyplug_abi::contract_id("test.memory", 1);

    // Bundle calls alloc then free.
    // alloc returns [ptr_lo, ptr_hi] tuple; free takes (ptr_lo, ptr_hi)
    let bundle: String = format!(
        r#"
var result = polyplug.alloc(64);
var ptr_lo = result[0];
var ptr_hi = result[1];
if (ptr_lo !== 0 || ptr_hi !== 0) {{
    polyplug.free(ptr_lo, ptr_hi);
}}
globalThis.TEST_VTABLE = {{
    contractLo: {},
    contractHi: {},
    fnCount: 1,
    contractName: "test.memory",
    functions: [function(args, out) {{ return 0; }}]
}};
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(
        result.is_ok(),
        "bundle with alloc+free must succeed: {result:?}"
    );
}

// ── Thread safety ─────────────────────────────────────────────────────────────

#[test]
fn concurrent_loads_do_not_panic() {
    // Spawn multiple threads each loading a different bundle concurrently.
    // All bundles create _VTABLE globals so load() succeeds.
    // Tests that the shared QJS_RUNTIME and per-Context eval are thread-safe.
    let thread_count: usize = 4;
    let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<std::thread::JoinHandle<()>> = (0..thread_count)
        .map(|i: usize| {
            let errors_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&errors);
            std::thread::spawn(move || {
                let contract_id: u64 =
                    polyplug_abi::contract_id(&format!("test.concurrent.{i}"), 1);

                let bundle: String =
                    make_bundle_js(contract_id, 1, &format!("test.concurrent.{i}"));
                let (_dir, path) = write_temp_bundle(&bundle);

                let runtime: Runtime = RuntimeBuilder::new()
                    .loader(JsLoader::new(JsConfig {}))
                    .build()
                    .expect("runtime build must succeed");
                let loader: JsLoader = JsLoader::new(JsConfig {});

                if let Err(e) = loader.load(&path, &runtime) {
                    let mut guard: std::sync::MutexGuard<'_, Vec<String>> =
                        errors_clone.lock().unwrap_or_else(|e| e.into_inner());
                    guard.push(format!("thread {i}: {e}"));
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("thread must not panic");
    }

    let errs: std::sync::MutexGuard<'_, Vec<String>> =
        errors.lock().unwrap_or_else(|e| e.into_inner());
    assert!(
        errs.is_empty(),
        "concurrent loads must all succeed: {errs:?}"
    );
}

#[test]
fn sequential_loads_of_different_contracts_all_succeed() {
    // Sequential re-use of the same JsLoader for multiple bundles.
    let loader: JsLoader = JsLoader::new(JsConfig {});
    let runtime: Runtime = make_runtime();

    for i in 0..4_u32 {
        let contract_id: u64 = polyplug_abi::contract_id(&format!("test.sequential.{i}"), 1);

        let bundle: String = make_bundle_js(contract_id, 1, &format!("test.sequential.{i}"));
        let (_dir, path) = write_temp_bundle(&bundle);

        let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
        assert!(
            result.is_ok(),
            "sequential load {i} must succeed: {result:?}"
        );
    }
}

// ── VM Dispatch Call Tests ─────────────────────────────────────────────────────

#[test]
fn dispatch_vm_call_works_correctly() {
    // This test actually invokes dispatch.vm.call to verify the JS function
    // can be called through the ABI dispatch mechanism.
    use polyplug_abi::{AbiError, ABI_OK};

    let contract_id: u64 = polyplug_abi::contract_id("test.dispatch.call", 1);
    let bundle: String = make_bundle_js(contract_id, 1, "test.dispatch.call");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    let handle: PluginHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");

    let vtable_ptr: *const PluginVTable = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &PluginVTable = unsafe { &*vtable_ptr };

    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);

    // SAFETY: dispatch_type is VirtualMachine, so accessing .vm is valid.
    // dispatch.vm.call is js_dispatch, loader_data is valid.
    let call_result: AbiError = unsafe {
        (vtable_ref.dispatch.vm.call)(
            vtable_ref.dispatch.vm.loader_data,
            0, // fn_id = 0 (first function)
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "dispatch.vm.call must return ABI_OK, got code={}",
        call_result.code
    );
}
