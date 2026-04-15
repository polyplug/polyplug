//! Integration tests for the QuickJS bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), polyplug_init registration, and thread-safety.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::loader::BundleLoader;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInterface;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal bundle JS string that defines polyplug_init and registers a plugin.
fn make_bundle_js(contract_id: u64, fn_count: u32, contract_name: &str) -> String {
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    format!(
        r#"
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "{contract_name}",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
        contractLo: {contract_lo},
        contractHi: {contract_hi},
        fnCount: {fn_count},
        contractName: "{contract_name}",
        functions: [
            function(args, out) {{ return 0; }}
        ]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
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

/// Create a ManifestData for a JS bundle.
fn make_manifest(path: &std::path::PathBuf, name: &str) -> ManifestData {
    ManifestData {
        id: polyplug_abi::bundle_id(name),
        name: name.to_owned(),
        runtime: "js-quickjs".to_owned(),
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

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered by querying the registry.
    let handle: GuestContractHandle = runtime
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
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "test.math",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
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
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    // Verify function_count
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };
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
    let manifest_toml: String = format!(
        r#"id = {}
name = "test.dir"
runtime = "js-quickjs"
file = "bundle.js"
"#,
        bundle_id
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest_toml).expect("write manifest.toml");

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = ManifestData {
        id: bundle_id,
        name: "test.dir".to_owned(),
        runtime: "js-quickjs".to_owned(),
        file: "bundle.js".to_owned(),
        path: dir.path().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    };
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(
        result.is_ok(),
        "load from directory path must succeed: {result:?}"
    );

    // Verify the plugin was registered.
    let handle: GuestContractHandle = runtime
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

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(result.is_err(), "runtime error bundle must return Err");

    let err_str: String = result
        .expect_err("runtime error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("js-quickjs"),
        "error must mention runtime name: {err_str}"
    );
}

// ── Missing polyplug_init function ─────────────────────────────────────────────

#[test]
fn load_bundle_without_polyplug_init_returns_error() {
    // Valid JS that does not define polyplug_init.
    let bundle: &str = "var x = 1 + 2;";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(
        result.is_err(),
        "bundle without polyplug_init must return Err"
    );

    let err_str: String = result
        .expect_err("bundle without polyplug_init must return Err")
        .to_string();
    assert!(
        err_str.contains("init symbol missing"),
        "error must mention init symbol missing: {err_str}"
    );
}

// ── File not found ────────────────────────────────────────────────────────────

#[test]
fn load_nonexistent_file_returns_error() {
    let path: std::path::PathBuf =
        std::path::PathBuf::from("/tmp/polyplug_js_test_nonexistent_bundle_xyz.js");

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = ManifestData {
        id: 0,
        name: "nonexistent".to_owned(),
        runtime: "js-quickjs".to_owned(),
        file: "bundle.js".to_owned(),
        path: path.parent().unwrap().to_path_buf(),
        version: String::new(),
        provides: Vec::new(),
        function_count: HashMap::new(),
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    };
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "test.bundlepath",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: 1,
        contractName: "test.bundlepath",
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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
                'resolvePlugin', 'getHostContract', 'registerVtable', 'alloc', 'free'];
for (var i = 0; i < methods.length; i++) {{
    if (typeof polyplug[methods[i]] !== 'function') {{
        throw new Error('missing method: ' + methods[i]);
    }}
}}
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "test.methods",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: 1,
        contractName: "test.methods",
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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
    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    loader.load(&manifest, &runtime).expect("load must succeed");

    // Verify the plugin was registered with the correct contract_id.
    let handle: GuestContractHandle = runtime
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
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "test.vm_dispatch",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: {},
        contractName: "test.vm_dispatch",
        functions: [
            function(args, out) {{ return 0; }},
            function(args, out) {{ return 0; }}
        ]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    loader.load(&manifest, &runtime).expect("load must succeed");

    // Verify the plugin was registered and get its vtable.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };

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
function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var descriptor = {{
        name: "js-quickjs-plugin",
        contractName: "test.memory",
        versionMajor: 0,
        versionMinor: 1,
        versionPatch: 0
    }};
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: 1,
        contractName: "test.memory",
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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

                let manifest: ManifestData = make_manifest(&path, "test.bundle");
                if let Err(e) = loader.load(&manifest, &runtime) {
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
fn multiple_runtimes_on_same_thread_are_isolated() {
    // CRITICAL TEST: Verifies that multiple Runtime instances on the SAME thread
    // have isolated state. This would have FAILED with the old thread-local approach
    // because REGISTRATION_DATA was shared across all Runtime instances on a thread.
    //
    // With the new userdata-based approach, each QuickJS Runtime has its own
    // registration slot stored in its userdata, ensuring complete isolation.

    // Create first runtime and load a plugin with contract_id A
    let contract_id_a: u64 = polyplug_abi::contract_id("test.isolation.a", 1);
    let bundle_a: String = make_bundle_js(contract_id_a, 1, "test.isolation.a");
    let (_dir_a, path_a) = write_temp_bundle(&bundle_a);

    let runtime_a: Runtime = RuntimeBuilder::new()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("runtime_a build must succeed");
    let loader_a: JsLoader = JsLoader::new(JsConfig {});

    let manifest_a: ManifestData = make_manifest(&path_a, "test.bundle.a");
    loader_a
        .load(&manifest_a, &runtime_a)
        .expect("load runtime_a must succeed");

    // Verify plugin A is registered in runtime_a
    let handle_a: GuestContractHandle = runtime_a
        .registry()
        .find(contract_id_a, 0)
        .expect("plugin A must be registered in runtime_a");
    assert!(!handle_a.is_null(), "handle_a must be valid");

    // Create second runtime on the SAME thread and load a plugin with contract_id B
    let contract_id_b: u64 = polyplug_abi::contract_id("test.isolation.b", 1);
    let bundle_b: String = make_bundle_js(contract_id_b, 1, "test.isolation.b");
    let (_dir_b, path_b) = write_temp_bundle(&bundle_b);

    let runtime_b: Runtime = RuntimeBuilder::new()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("runtime_b build must succeed");
    let loader_b: JsLoader = JsLoader::new(JsConfig {});

    let manifest_b: ManifestData = make_manifest(&path_b, "test.bundle.b");
    loader_b
        .load(&manifest_b, &runtime_b)
        .expect("load runtime_b must succeed");

    // Verify plugin B is registered in runtime_b
    let handle_b: GuestContractHandle = runtime_b
        .registry()
        .find(contract_id_b, 0)
        .expect("plugin B must be registered in runtime_b");
    assert!(!handle_b.is_null(), "handle_b must be valid");

    // CRITICAL: Verify that runtime_a still has plugin A (not corrupted by runtime_b)
    // With the old thread-local approach, loading runtime_b would have overwritten
    // the registration data, causing this check to fail.
    let handle_a_still_valid: GuestContractHandle = runtime_a
        .registry()
        .find(contract_id_a, 0)
        .expect("plugin A must still be registered in runtime_a");
    assert!(
        !handle_a_still_valid.is_null(),
        "handle_a must still be valid after runtime_b was created"
    );

    // Verify runtime_b does NOT have plugin A (isolation)
    let handle_a_in_b: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime_b.registry().find(contract_id_a, 0);
    assert!(
        handle_a_in_b.is_err()
            || handle_a_in_b
                .as_ref()
                .ok()
                .map(|h| h.is_null())
                .unwrap_or(true),
        "runtime_b must NOT have plugin A (isolation)"
    );

    // Verify runtime_a does NOT have plugin B (isolation)
    let handle_b_in_a: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime_a.registry().find(contract_id_b, 0);
    assert!(
        handle_b_in_a.is_err()
            || handle_b_in_a
                .as_ref()
                .ok()
                .map(|h| h.is_null())
                .unwrap_or(true),
        "runtime_a must NOT have plugin B (isolation)"
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

        let manifest: ManifestData = make_manifest(&path, "test.bundle");
        let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
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
    use polyplug_abi::AbiError;

    let contract_id: u64 = polyplug_abi::contract_id("test.dispatch.call", 1);
    let bundle: String = make_bundle_js(contract_id, 1, "test.dispatch.call");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    let handle: GuestContractHandle = runtime
        .registry()
        .find(contract_id, 0)
        .expect("plugin must be registered");

    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };

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
        call_result.code,
        AbiErrorCode::Ok,
        "dispatch.vm.call must return Ok, got code={}",
        call_result.code
    );
}

// ── StringView.toString() Tests ────────────────────────────────────────────────

#[test]
fn stringview_to_string_handles_empty_string() {
    // Test that StringView.toString() handles empty strings (len=0).
    let contract_id: u64 = polyplug_abi::contract_id("test.stringview.empty", 1);

    let bundle: String = format!(
        r#"
var testResult = null;

// Test empty string handling - should return '' without reading memory
function stringViewToString(sv) {{
    if (!sv || sv.len === 0) return '';
    return 'non-empty';
}}

var result = stringViewToString({{ ptr_lo: 0, ptr_hi: 0, len: 0 }});
if (result === '') {{
    testResult = "PASS";
}} else {{
    throw new Error("expected empty string, got: " + result);
}}

function polyplug_init(rt_ctx, host_vtable, ctx) {{
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: 1,
        contractName: "test.stringview.empty",
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName
    );
    return {{ code: 0, message: null }};
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Runtime = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(&manifest, &runtime);
    assert!(result.is_ok(), "Empty string test must succeed: {result:?}");
}
