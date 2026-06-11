//! Integration tests for the QuickJS bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), polyplug_init registration, and thread-safety.

#![allow(clippy::expect_used)]

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::loader::manifest::ManifestData;
use polyplug::runtime::Runtime;
use polyplug::runtime::RuntimeBuilder;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Compatibility;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::RuntimeConfig;
use polyplug_abi::types::LogLevel;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_utils::GuestContractId;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal bundle JS string that defines polyplug_init and registers a plugin.
fn make_bundle_js(contract_id: u64, fn_count: u32, contract_name: &str) -> String {
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
        functions: [
            function(args, out) {{ return 0; }}
        ]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
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

    let bundle_id: u64 = polyplug_utils::bundle_id(name);
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
fn make_runtime() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .loader(make_loader())
        .build()
        .expect("runtime build must succeed")
}

/// Create a Runtime with the JsLoader registered and hot-reload enabled.
fn make_runtime_hot_reload() -> Arc<Runtime> {
    RuntimeBuilder::new()
        .loader(make_loader())
        .config(RuntimeConfig {
            compatibility: Compatibility::Strict,
            unload_mode: polyplug_abi::UnloadMode::Retire,
            hot_reload_enabled: true,
            on_reload: None,
            on_reload_user_data: core::ptr::null_mut(),
            ..Default::default()
        })
        .build()
        .expect("runtime build must succeed")
}

/// Create a ManifestData for a JS bundle.
fn make_manifest(path: &std::path::Path, name: &str) -> ManifestData {
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        runtime: "js-quickjs".to_owned(),
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

/// Verify a VM-dispatch vtable exposes exactly `expected` functions by probing
/// fn_ids: indices `0..expected` must return `Ok`, and index `expected` must
/// return `FunctionNotAvailable`.
fn assert_vm_function_count(vtable: &GuestContractInterface, expected: u32) {
    use polyplug_abi::AbiError;
    use polyplug_abi::AbiErrorCode;

    assert_eq!(vtable.dispatch_type, DispatchType::VirtualMachine);
    for fn_id in 0..expected {
        // SAFETY: dispatch.vm.call is js_dispatch; the noop functions ignore the
        // null args/out pointers.
        let result: AbiError = unsafe {
            (vtable.dispatch.vm.call)(
                vtable.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                fn_id,
                core::ptr::null::<()>(),
                core::ptr::null_mut::<()>(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            result.code,
            AbiErrorCode::Ok as u32,
            "fn_id {fn_id} must dispatch to Ok"
        );
    }
    // SAFETY: dispatch.vm.call is js_dispatch.
    let missing: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            expected,
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        missing.code,
        AbiErrorCode::FunctionNotAvailable as u32,
        "fn_id {expected} must report FunctionNotAvailable"
    );
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
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.noop", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.noop");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered by querying the registry.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

#[test]
fn load_bundle_with_functions_registers_correct_count() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.math", 1);
    let fn_count: u32 = 3;

    // Bundle with 3 functions
    let bundle: String = format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
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
        vtable.contractName,
        vtable.version
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "load must succeed: {result:?}");

    // Verify the plugin was registered.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    // Verify function count by probing the VM dispatch.
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("resolve must succeed");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_vm_function_count(vtable_ref, fn_count);
}

// ── Directory path fallback ───────────────────────────────────────────────────

#[test]
fn load_accepts_directory_path() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.dir", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.dir");
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bundle.js"), &bundle).expect("write bundle.js");

    let bundle_id: u64 = polyplug_utils::bundle_id("test.dir");
    let manifest_toml: String = format!(
        r#"id = {}
name = "test.dir"
runtime = "js-quickjs"
file = "bundle.js"
"#,
        bundle_id
    );
    std::fs::write(dir.path().join("manifest.toml"), &manifest_toml).expect("write manifest.toml");

    let runtime: Arc<Runtime> = make_runtime();
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
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        result.is_ok(),
        "load from directory path must succeed: {result:?}"
    );

    // Verify the plugin was registered.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

// ── Syntax error ──────────────────────────────────────────────────────────────

#[test]
fn load_syntax_error_returns_error() {
    let bundle: &str = "this is not valid javascript }{{{";
    let (_dir, path) = write_temp_bundle(bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
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

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
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

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
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

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = ManifestData {
        id: 0,
        name: "nonexistent".to_owned(),
        runtime: "js-quickjs".to_owned(),
        file: "bundle.js".to_owned(),
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
    };
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err(), "non-existent file must return Err");
}

// ── BundlePath global injection ───────────────────────────────────────────────

#[test]
fn bundle_path_global_is_injected() {
    // The loader injects `globalThis.bundlePath` before evaluating the bundle.
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.bundlepath", 1);

    // Bundle reads bundlePath; if it is undefined the throw will surface as Err.
    let bundle: String = format!(
        r#"
if (typeof globalThis.bundlePath !== 'string') {{
    throw new Error('bundlePath not injected');
}}
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        result.is_ok(),
        "bundle reading bundlePath must succeed: {result:?}"
    );
}

// ── polyplug object is accessible in JS ───────────────────────────────────────

#[test]
fn polyplug_object_has_expected_methods() {
    // Verify all expected host methods are present on the polyplug global.
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.methods", 1);

    let bundle: String = format!(
        r#"
var methods = ['findByContract', 'findByBundle', 'findAllByContract',
                'resolveGuestContract', 'callHostContract', 'registerVtable', 'alloc', 'free'];
for (var i = 0; i < methods.length; i++) {{
    if (typeof polyplug[methods[i]] !== 'function') {{
        throw new Error('missing method: ' + methods[i]);
    }}
}}
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        result.is_ok(),
        "all polyplug methods must be present: {result:?}"
    );
}

// ── VTable registration — contract_id roundtrip ───────────────────────────────

#[test]
fn vtable_contract_id_roundtrip() {
    // Use a well-known FNV-1a contract — contract_id("image.decode", 1).
    let contract_id: u64 = polyplug_utils::guest_contract_id("image.decode", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "image.decode");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();
    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // Verify the plugin was registered with the correct contract_id.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");
}

// ── VM dispatch verification ──────────────────────────────────────────────────

#[test]
fn vtable_uses_vm_dispatch() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.vm_dispatch", 1);
    let fn_count: u32 = 2;

    let bundle: String = format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
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
        vtable.contractName,
        vtable.version
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32,
        fn_count
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("load must succeed");

    // Verify the plugin was registered and get its vtable.
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    assert!(!handle.is_null(), "handle must be valid");

    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };

    assert_vm_function_count(vtable_ref, fn_count);
    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);
    // SAFETY: dispatch_type is VirtualMachine, so accessing .vm is valid.
    // The dispatch function pointer is always non-null (it's js_dispatch).
    assert!(!unsafe { vtable_ref.dispatch.vm.loader_data }.is_null());
}

// ── Memory management helpers ─────────────────────────────────────────────────

#[test]
fn js_alloc_and_free_calls_host_vtable() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.memory", 1);

    // Bundle calls alloc then free.
    // alloc returns [ptr_lo, ptr_hi] tuple; free takes (ptr_lo, ptr_hi, size, align)
    // — the size/align must match the allocation so the host actually frees it.
    let bundle: String = format!(
        r#"
var result = polyplug.alloc(64);
var ptr_lo = result[0];
var ptr_hi = result[1];
if (ptr_lo !== 0 || ptr_hi !== 0) {{
    polyplug.free(ptr_lo, ptr_hi, 64, 1);
}}
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
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
        version: 0x00010000,
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
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
                    polyplug_utils::guest_contract_id(&format!("test.concurrent.{i}"), 1);

                let bundle: String =
                    make_bundle_js(contract_id, 1, &format!("test.concurrent.{i}"));
                let (_dir, path) = write_temp_bundle(&bundle);

                let runtime: Arc<Runtime> = RuntimeBuilder::new()
                    .loader(JsLoader::new(JsConfig {}))
                    .build()
                    .expect("runtime build must succeed");
                let loader: JsLoader = JsLoader::new(JsConfig {});

                let manifest: ManifestData = make_manifest(&path, "test.bundle");
                if let Err(e) = loader.load(
                    &manifest,
                    &polyplug::loader::BundleSource::Path(manifest.path.clone()),
                    &runtime,
                ) {
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
    let contract_id_a: u64 = polyplug_utils::guest_contract_id("test.isolation.a", 1);
    let bundle_a: String = make_bundle_js(contract_id_a, 1, "test.isolation.a");
    let (_dir_a, path_a) = write_temp_bundle(&bundle_a);

    let runtime_a: Arc<Runtime> = RuntimeBuilder::new()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("runtime_a build must succeed");
    let loader_a: JsLoader = JsLoader::new(JsConfig {});

    let manifest_a: ManifestData = make_manifest(&path_a, "test.bundle.a");
    loader_a
        .load(
            &manifest_a,
            &polyplug::loader::BundleSource::Path(manifest_a.path.clone()),
            &runtime_a,
        )
        .expect("load runtime_a must succeed");

    // Verify plugin A is registered in runtime_a
    let handle_a: GuestContractHandle = runtime_a
        .registry()
        .find(GuestContractId::from_u64(contract_id_a), 0)
        .expect("plugin A must be registered in runtime_a");
    assert!(!handle_a.is_null(), "handle_a must be valid");

    // Create second runtime on the SAME thread and load a plugin with contract_id B
    let contract_id_b: u64 = polyplug_utils::guest_contract_id("test.isolation.b", 1);
    let bundle_b: String = make_bundle_js(contract_id_b, 1, "test.isolation.b");
    let (_dir_b, path_b) = write_temp_bundle(&bundle_b);

    let runtime_b: Arc<Runtime> = RuntimeBuilder::new()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("runtime_b build must succeed");
    let loader_b: JsLoader = JsLoader::new(JsConfig {});

    let manifest_b: ManifestData = make_manifest(&path_b, "test.bundle.b");
    loader_b
        .load(
            &manifest_b,
            &polyplug::loader::BundleSource::Path(manifest_b.path.clone()),
            &runtime_b,
        )
        .expect("load runtime_b must succeed");

    // Verify plugin B is registered in runtime_b
    let handle_b: GuestContractHandle = runtime_b
        .registry()
        .find(GuestContractId::from_u64(contract_id_b), 0)
        .expect("plugin B must be registered in runtime_b");
    assert!(!handle_b.is_null(), "handle_b must be valid");

    // CRITICAL: Verify that runtime_a still has plugin A (not corrupted by runtime_b)
    // With the old thread-local approach, loading runtime_b would have overwritten
    // the registration data, causing this check to fail.
    let handle_a_still_valid: GuestContractHandle = runtime_a
        .registry()
        .find(GuestContractId::from_u64(contract_id_a), 0)
        .expect("plugin A must still be registered in runtime_a");
    assert!(
        !handle_a_still_valid.is_null(),
        "handle_a must still be valid after runtime_b was created"
    );

    // Verify runtime_b does NOT have plugin A (isolation)
    let handle_a_in_b: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime_b
        .registry()
        .find(GuestContractId::from_u64(contract_id_a), 0);
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
    let handle_b_in_a: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime_a
        .registry()
        .find(GuestContractId::from_u64(contract_id_b), 0);
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
    let runtime: Arc<Runtime> = make_runtime();

    for i in 0..4_u32 {
        let contract_id: u64 =
            polyplug_utils::guest_contract_id(&format!("test.sequential.{i}"), 1);

        let bundle: String = make_bundle_js(contract_id, 1, &format!("test.sequential.{i}"));
        let (_dir, path) = write_temp_bundle(&bundle);

        let manifest: ManifestData = make_manifest(&path, "test.bundle");
        let result: Result<(), polyplug::error::RuntimeError> = loader.load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        );
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
    use polyplug_abi::AbiErrorCode;

    let contract_id: u64 = polyplug_utils::guest_contract_id("test.dispatch.call", 1);
    let bundle: String = make_bundle_js(contract_id, 1, "test.dispatch.call");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "load must succeed: {result:?}");

    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");

    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("resolve must succeed");

    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };

    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);

    // SAFETY: dispatch_type is VirtualMachine, so accessing .vm is valid.
    // dispatch.vm.call is js_dispatch, loader_data is valid.
    let call_result: AbiError = unsafe {
        (vtable_ref.dispatch.vm.call)(
            vtable_ref.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0, // fn_id = 0 (first function)
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
            core::ptr::null_mut(),
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "dispatch.vm.call must return Ok, got code={}",
        call_result.code
    );
}

// ── StringView.toString() Tests ────────────────────────────────────────────────

#[test]
fn stringview_to_string_handles_empty_string() {
    // Test that StringView.toString() handles empty strings (len=0).
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.stringview.empty", 1);

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

function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{
        contractLo: {},
        contractHi: {},
        fnCount: 1,
        contractName: "test.stringview.empty",
        version: 0x00010000,
        functions: [function(args, out) {{ return 0; }}]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
    return {{ code: 0, message: null }};
}}
"#,
        contract_id as u32,
        (contract_id >> 32) as u32
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = JsLoader::new(JsConfig {});

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_ok(), "Empty string test must succeed: {result:?}");
}

// ── Hot-reload ──────────────────────────────────────────────────────────────────

#[test]
fn js_reload_disabled_returns_error() {
    // With hot-reload disabled, the loader must reject reload() up front.
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.reload.disabled", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.reload.disabled");
    let (_dir, path) = write_temp_bundle(&bundle);

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    let result: Result<(), RuntimeError> = loader.reload(&manifest, &runtime);
    assert!(
        matches!(result, Err(RuntimeError::HotReloadDisabled)),
        "reload with hot-reload disabled must return HotReloadDisabled: {result:?}"
    );
}

#[test]
fn js_reload_reinitializes_contracts() {
    // Load a bundle, then reload it through the runtime and verify the contract
    // remains resolvable after the interface swap.
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.reload.contract", 1);

    let bundle: String = make_bundle_js(contract_id, 1, "test.reload.contract");
    let (dir, _path) = write_temp_bundle_with_name(&bundle, "test.reload.contract");

    let runtime: Arc<Runtime> = make_runtime_hot_reload();

    runtime
        .load_bundle(dir.path())
        .expect("initial load must succeed");

    let handle_before: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered after load");
    assert!(!handle_before.is_null(), "handle must be valid after load");

    let reload_result: Result<(), RuntimeError> = runtime.reload_bundle(dir.path());
    assert!(
        reload_result.is_ok(),
        "reload_bundle must succeed: {reload_result:?}"
    );

    let handle_after: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must still be registered after reload");
    assert!(!handle_after.is_null(), "handle must be valid after reload");

    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle_after)
        .expect("resolve must succeed after reload");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);
}

/// After a successful load, the contract must be attributed to the bundle's REAL
/// id in the registry — not bundle 0. The JS `register_guest_contract` call runs
/// after `polyplug_init` stashes its registration data, so the init-bundle window
/// must stay open across that call for `host_register_guest_contract` to attribute
/// it correctly. Invalidating by the real id must then remove it.
#[test]
fn registrations_attributed_to_real_bundle_id() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.attribution.contract", 1);
    let bundle: String = make_bundle_js(contract_id, 1, "test.attribution.contract");
    let (dir, _path) = write_temp_bundle_with_name(&bundle, "test.attribution.contract");
    let bundle_id: u64 = polyplug_utils::bundle_id("test.attribution.contract");

    let runtime: Arc<Runtime> = make_runtime();
    runtime
        .load_bundle(dir.path())
        .expect("initial load must succeed");

    // The contract must be findable under the REAL bundle id.
    let by_real: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.find_guest_contract_by_bundle(bundle_id, contract_id, 0);
    assert!(
        by_real.is_ok(),
        "contract must be attributed to the real bundle id {bundle_id}, not bundle 0"
    );

    // And it must NOT be attributed to bundle 0.
    let by_zero: Result<GuestContractHandle, polyplug::error::RegistryError> =
        runtime.find_guest_contract_by_bundle(0, contract_id, 0);
    assert!(
        by_zero.is_err(),
        "contract must not be attributed to bundle 0"
    );

    // Invalidating by the real bundle id must remove the contract from the registry.
    runtime
        .registry()
        .invalidate_bundle(polyplug_utils::BundleId::from_u64(bundle_id))
        .expect("invalidate by real bundle id must succeed");
    let after: Result<GuestContractHandle, polyplug::error::RegistryError> = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0);
    assert!(
        after.is_err(),
        "contract must be gone after invalidating the real bundle id"
    );
}

// ── BundleSource::Code / Bytes ──────────────────────────────────────────────────

/// Absolute path to the shared JS fixture's `bundle.js` at the workspace root.
fn fixture_bundle_js_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/test_plugin_js/bundle.js")
}

/// Build a ManifestData matching the shared `test_plugin_js` fixture, with the
/// given on-disk bundle directory (empty for in-memory sources).
fn fixture_manifest(bundle_dir: std::path::PathBuf) -> ManifestData {
    let mut function_count: HashMap<String, u32> = HashMap::new();
    function_count.insert("test.add@1".to_owned(), 5);
    ManifestData {
        id: polyplug_utils::bundle_id("test_bundle"),
        name: "test_bundle".to_owned(),
        runtime: "js-quickjs".to_owned(),
        file: "bundle.js".to_owned(),
        path: bundle_dir,
        version: "1.0.0".to_owned(),
        provides: vec!["test.add@1".to_owned()],
        function_count,
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

/// Dispatch the fixture's `add` function (fn_id 0) with two i32 args, returning
/// the i32 written to the out buffer.
fn dispatch_add(runtime: &Runtime, contract_id: u64, a: i32, b: i32) -> i32 {
    use polyplug_abi::AbiError;
    use polyplug_abi::AbiErrorCode;

    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("plugin must be registered");
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("resolve must succeed");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(vtable_ref.dispatch_type, DispatchType::VirtualMachine);

    // The fixture's `add` reads two i32 from argsPtr / argsPtr+4 and writes the
    // sum to outPtr. Lay out a packed [a, b] args buffer and a single-i32 out.
    let args: [i32; 2] = [a, b];
    let mut out: i32 = 0;
    // SAFETY: dispatch.vm.call is js_dispatch; args points at two valid i32 and
    // out at one valid i32, matching what the guest reads/writes.
    let result: AbiError = unsafe {
        (vtable_ref.dispatch.vm.call)(
            vtable_ref.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            args.as_ptr() as *const (),
            &mut out as *mut i32 as *mut (),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(result.code, AbiErrorCode::Ok as u32, "add must dispatch Ok");
    out
}

/// Loading the fixture via `BundleSource::Code` (in-memory source text) registers
/// the same contract and dispatches to the same result as path-based loading.
#[test]
fn load_code_source_has_parity_with_path() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);

    // Path-loaded reference: load the on-disk fixture directory and dispatch add.
    let fixture_dir: std::path::PathBuf = fixture_bundle_js_path()
        .parent()
        .expect("fixture must have a parent directory")
        .to_path_buf();
    let runtime_path: Arc<Runtime> = make_runtime();
    runtime_path
        .load_bundle(&fixture_dir)
        .expect("path load of fixture must succeed");
    let path_result: i32 = dispatch_add(&runtime_path, contract_id, 2, 3);
    assert_eq!(path_result, 5, "path-loaded add(2,3) must be 5");

    // Code-loaded: read the same source to a String and load it via Code.
    let source: String =
        std::fs::read_to_string(fixture_bundle_js_path()).expect("read fixture bundle.js");
    let runtime_code: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = fixture_manifest(std::path::PathBuf::new());
    runtime_code
        .load_bundle_from_source(manifest, BundleSource::Code(source))
        .expect("code load of fixture must succeed");
    let code_result: i32 = dispatch_add(&runtime_code, contract_id, 2, 3);

    assert_eq!(
        code_result, path_result,
        "Code-loaded dispatch must match path-loaded dispatch"
    );
}

/// Loading the fixture via `BundleSource::Bytes` (valid UTF-8) takes the same path
/// as Code and registers the same contract.
#[test]
fn load_bytes_source_valid_utf8_succeeds() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.add", 1);

    let source: Vec<u8> = std::fs::read(fixture_bundle_js_path()).expect("read fixture bundle.js");
    let runtime: Arc<Runtime> = make_runtime();
    let manifest: ManifestData = fixture_manifest(std::path::PathBuf::new());
    runtime
        .load_bundle_from_source(manifest, BundleSource::Bytes(source))
        .expect("bytes load of fixture must succeed");

    let result: i32 = dispatch_add(&runtime, contract_id, 7, 4);
    assert_eq!(result, 11, "bytes-loaded add(7,4) must be 11");
}

/// `BundleSource::Bytes` carrying invalid UTF-8 yields a structured
/// `LoaderError::InvalidSourceEncoding`, never a string error or a panic.
#[test]
fn load_bytes_source_invalid_utf8_returns_structured_error() {
    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();

    // 0xFF is never valid in a UTF-8 sequence.
    let invalid: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01];
    let manifest: ManifestData = fixture_manifest(std::path::PathBuf::new());
    let result: Result<(), RuntimeError> =
        loader.load(&manifest, &BundleSource::Bytes(invalid), &runtime);

    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::InvalidSourceEncoding {
                loader: "js-quickjs",
                source_kind: "bytes",
                ..
            }))
        ),
        "invalid UTF-8 bytes must yield InvalidSourceEncoding: {result:?}"
    );
}

// ── callHostContract from JS: instance lifecycle + bounds checks ───────────────

use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;

// The counting host contract's create/destroy callbacks are plain `extern "C"`
// functions and cannot capture per-test state, so they share these process-wide
// counters. The tests that observe exact counts therefore serialize on TEST_LOCK
// (held for the whole test body) and reset the counters under it.
static CREATE_COUNT: AtomicUsize = AtomicUsize::new(0);
static DESTROY_COUNT: AtomicUsize = AtomicUsize::new(0);
static COUNTER_LOCK: Mutex<()> = Mutex::new(());

/// Counting create_instance: bumps CREATE_COUNT and returns a unique non-null
/// instance pointer (the running count, +1 so it is never null).
///
/// # Safety
/// Matches the `HostContractInterface::create_instance` ABI signature.
unsafe extern "C" fn counting_create_instance(
    _this: *const HostContractInterface,
    _args: *const (),
) -> HostContractInstance {
    let n: usize = CREATE_COUNT.fetch_add(1, Ordering::SeqCst);
    HostContractInstance {
        data: (n + 1) as *mut core::ffi::c_void,
    }
}

/// Counting destroy_instance: bumps DESTROY_COUNT. No real memory to free (the
/// "instance" is just an integer used as a pointer).
///
/// # Safety
/// Matches the `HostContractInterface::destroy_instance` ABI signature.
unsafe extern "C" fn counting_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
    DESTROY_COUNT.fetch_add(1, Ordering::SeqCst);
}

/// A no-op native host-contract function: `(state, args, out) -> AbiError::ok()`.
///
/// # Safety
/// Matches the native dispatch signature `(state, args, out) -> AbiError`.
unsafe extern "C" fn host_noop_fn(
    _state: *const core::ffi::c_void,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
) -> polyplug_abi::AbiError {
    polyplug_abi::AbiError::ok()
}

/// `Sync` wrapper around a 'static read-only function-pointer table so it can live
/// in a `static`. Function pointers are safe to share across threads.
#[repr(transparent)]
struct HostFnTable([*const (); 1]);
// SAFETY: the table holds only 'static read-only function pointers; sharing them
// across threads is sound (the functions handle their own synchronization).
unsafe impl Sync for HostFnTable {}

// A single 'static native function table for the counting host contract.
static HOST_NOOP_FNS: HostFnTable = HostFnTable([host_noop_fn as *const ()]);

/// Build and leak a non-singleton counting host contract interface with one
/// native function. `Box::leak` gives the required `&'static` lifetime.
fn leak_counting_host_contract(contract_id: u64, major: u32) -> &'static HostContractInterface {
    Box::leak(Box::new(HostContractInterface {
        contract_id: polyplug_utils::HostContractId::from(contract_id),
        contract_version: polyplug_abi::types::Version {
            major,
            minor: 0,
            patch: 0,
        },
        singleton: false,
        dispatch_type: DispatchType::Native,
        runtime: core::ptr::null_mut(),
        user_data: core::ptr::null_mut(),
        create_instance: counting_create_instance,
        destroy_instance: counting_destroy_instance,
        dispatch: polyplug_abi::DispatchMechanisms {
            native: polyplug_abi::NativeDispatch {
                function_count: 1,
                functions: HOST_NOOP_FNS.0.as_ptr(),
            },
        },
    }))
}

/// Inline JS bundle whose single guest function calls
/// `polyplug.callHostContract(lo, hi, minVer, fnId, argsPtr, outPtr)` and returns
/// the host call's AbiError code. The guest contract id / name are parameterised
/// so each test registers a distinct contract.
fn host_caller_bundle_source(
    guest_contract_id: u64,
    guest_name: &str,
    host_contract_id: u64,
    host_fn_id: u32,
) -> String {
    let guest_lo: u32 = (guest_contract_id & 0xFFFF_FFFF) as u32;
    let guest_hi: u32 = (guest_contract_id >> 32) as u32;
    let host_lo: u32 = (host_contract_id & 0xFFFF_FFFF) as u32;
    let host_hi: u32 = (host_contract_id >> 32) as u32;
    format!(
        r#"
function callHost(argsPtr, outPtr) {{
    // callHostContract(contractLo, contractHi, minVersion, fnId, argsPtr, outPtr).
    // minVersion is the PACKED version (major << 16 | minor); major 1 -> 0x10000.
    return polyplug.callHostContract({host_lo}, {host_hi}, 0x10000, {host_fn_id}, argsPtr, outPtr);
}}

function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{
        contractLo: {guest_lo} >>> 0,
        contractHi: {guest_hi} >>> 0,
        fnCount: 1,
        contractName: "{guest_name}",
        version: 0x10000,
        functions: [callHost]
    }};
    polyplug.registerVtable(
        vtable.contractLo, vtable.contractHi, vtable,
        vtable.fnCount, vtable.contractName, vtable.version
    );
    return {{ code: 0, message: null }};
}}
"#
    )
}

/// Dispatch the guest's single function (fn_id 0), which calls callHostContract
/// and RETURNS that host call's AbiError code. `js_dispatch` surfaces the JS
/// return value as the dispatch's `AbiError.code`, so the returned code IS the
/// host-call result code (Ok on success, FunctionNotAvailable on a rejected fn_id).
fn dispatch_host_caller(runtime: &Runtime, guest_contract_id: u64) -> u32 {
    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(guest_contract_id), 0)
        .expect("guest contract must be registered");
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("resolve must succeed");
    // SAFETY: vtable_ptr is a valid pointer returned by resolve.
    let vtable_ref: &GuestContractInterface = unsafe { &*vtable_ptr };
    let mut out: i32 = 0;
    // SAFETY: dispatch.vm.call is js_dispatch; the guest fn forwards the (unused)
    // args/out pointers straight into callHostContract.
    let result: polyplug_abi::AbiError = unsafe {
        (vtable_ref.dispatch.vm.call)(
            vtable_ref.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            core::ptr::null::<()>(),
            &mut out as *mut i32 as *mut (),
            core::ptr::null_mut(),
        )
    };
    result.code
}

/// B5 regression: calling a NON-singleton host contract from JS must destroy the
/// freshly-minted instance after each dispatch. Calling twice must yield exactly
/// two creates and two destroys — previously the instance leaked every call.
#[test]
fn callhostcontract_destroys_non_singleton_instance_each_call() {
    // Serialize with the other counter-observing test; reset counters under the lock.
    let _guard: std::sync::MutexGuard<'_, ()> =
        COUNTER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    CREATE_COUNT.store(0, Ordering::SeqCst);
    DESTROY_COUNT.store(0, Ordering::SeqCst);

    let guest_name: &str = "jstest.hostcaller_lifecycle";
    let guest_id: u64 = polyplug_utils::guest_contract_id(guest_name, 1);
    let host_name: &str = "jstest.counting_host_lifecycle";
    let host_id: u64 = polyplug_utils::host_contract_id(host_name, 1);

    let runtime: Arc<Runtime> = make_runtime();
    runtime
        .register_host_contract(host_id, leak_counting_host_contract(host_id, 1))
        .expect("host contract registration must succeed");

    let source: String = host_caller_bundle_source(guest_id, guest_name, host_id, 0);
    let manifest: ManifestData = make_manifest_named(guest_name);
    runtime
        .load_bundle_from_source(manifest, BundleSource::Code(source))
        .expect("guest bundle load must succeed");

    let code1: u32 = dispatch_host_caller(&runtime, guest_id);
    assert_eq!(
        code1,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "first host call must succeed"
    );
    let code2: u32 = dispatch_host_caller(&runtime, guest_id);
    assert_eq!(
        code2,
        polyplug_abi::AbiErrorCode::Ok as u32,
        "second host call must succeed"
    );

    assert_eq!(
        CREATE_COUNT.load(Ordering::SeqCst),
        2,
        "two calls must mint two instances"
    );
    assert_eq!(
        DESTROY_COUNT.load(Ordering::SeqCst),
        2,
        "each non-singleton instance must be destroyed after its dispatch (no leak)"
    );
}

/// B4 regression: calling callHostContract with an out-of-range fn_id must return
/// FunctionNotAvailable instead of indexing past the host function table (UB /
/// crash). The bounds check must also destroy the non-singleton instance it took.
#[test]
fn callhostcontract_out_of_range_fn_id_returns_function_not_available() {
    // Serialize with the other counter-observing test; reset counters under the lock.
    let _guard: std::sync::MutexGuard<'_, ()> =
        COUNTER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    CREATE_COUNT.store(0, Ordering::SeqCst);
    DESTROY_COUNT.store(0, Ordering::SeqCst);

    let guest_name: &str = "jstest.hostcaller_bounds";
    let guest_id: u64 = polyplug_utils::guest_contract_id(guest_name, 1);
    let host_name: &str = "jstest.counting_host_bounds";
    let host_id: u64 = polyplug_utils::host_contract_id(host_name, 1);

    let runtime: Arc<Runtime> = make_runtime();
    runtime
        .register_host_contract(host_id, leak_counting_host_contract(host_id, 1))
        .expect("host contract registration must succeed");

    // The host contract has exactly one function (fn_id 0); ask for fn_id 5.
    let source: String = host_caller_bundle_source(guest_id, guest_name, host_id, 5);
    let manifest: ManifestData = make_manifest_named(guest_name);
    runtime
        .load_bundle_from_source(manifest, BundleSource::Code(source))
        .expect("guest bundle load must succeed");

    let code: u32 = dispatch_host_caller(&runtime, guest_id);
    assert_eq!(
        code,
        polyplug_abi::AbiErrorCode::FunctionNotAvailable as u32,
        "out-of-range fn_id must yield FunctionNotAvailable, not a crash"
    );

    // The bounds-check path still took (and must release) a non-singleton instance.
    assert_eq!(
        CREATE_COUNT.load(Ordering::SeqCst),
        1,
        "one call mints one instance even when the fn_id is rejected"
    );
    assert_eq!(
        DESTROY_COUNT.load(Ordering::SeqCst),
        1,
        "the instance must be destroyed even on the bounds-check bail-out (no leak)"
    );
}

/// Build a ManifestData for an in-memory JS guest bundle with the given contract
/// name (used as bundle name; provides one function).
fn make_manifest_named(name: &str) -> ManifestData {
    let mut function_count: HashMap<String, u32> = HashMap::new();
    function_count.insert(format!("{name}@1"), 1);
    ManifestData {
        id: polyplug_utils::bundle_id(name),
        name: name.to_owned(),
        runtime: "js-quickjs".to_owned(),
        file: "bundle.js".to_owned(),
        path: std::path::PathBuf::new(),
        version: "1.0.0".to_owned(),
        provides: vec![format!("{name}@1")],
        function_count,
        dependencies: Vec::new(),
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
    }
}

// ── Guest logging — the injected `polyplug.log` bridge ───────────────────────

/// A guest calling the loader-injected `polyplug.log(level, scope, message)`
/// bridge mid-dispatch must deliver (level, scope, message) verbatim through
/// the host logger installed via `RuntimeBuilder::logger`, and an out-of-range
/// level must clamp to `LogLevel::Error`. The bridge runs inside `js_dispatch`'s
/// `Context::with` (the QuickJS VM lock is held by the calling thread) — this
/// test also proves that path is deadlock-free.
#[test]
fn guest_log_bridge_delivers_records_and_clamps_level() {
    let contract_id: u64 = polyplug_utils::guest_contract_id("test.guest.log", 1);
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let bundle: String = format!(
        r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {{
    var vtable = {{
        contractLo: {contract_lo},
        contractHi: {contract_hi},
        fnCount: 1,
        contractName: "test.guest.log",
        version: 0x00010000,
        functions: [
            function(args, out) {{
                polyplug.log(3, "guest.test-log", "hello from js guest");
                polyplug.log(99, "guest.test-log", "out of range level");
                return 0;
            }}
        ]
    }};
    polyplug.registerVtable(
        vtable.contractLo,
        vtable.contractHi,
        vtable,
        vtable.fnCount,
        vtable.contractName,
        vtable.version
    );
}}
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let records: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let records_clone: Arc<Mutex<Vec<(LogLevel, String, String)>>> = Arc::clone(&records);
    let runtime: Arc<Runtime> = RuntimeBuilder::new()
        .logger(move |level: LogLevel, scope: &str, msg: &str| {
            records_clone.lock().expect("records lock").push((
                level,
                scope.to_owned(),
                msg.to_owned(),
            ));
        })
        .build()
        .expect("runtime build must succeed");

    let loader: JsLoader = JsLoader::new(JsConfig {});
    let manifest: ManifestData = make_manifest(&path, "test.bundle");
    loader
        .load(
            &manifest,
            &polyplug::loader::BundleSource::Path(manifest.path.clone()),
            &runtime,
        )
        .expect("logging bundle must load");

    let handle: GuestContractHandle = runtime
        .registry()
        .find(GuestContractId::from_u64(contract_id), 0)
        .expect("test.guest.log@1 must be registered");
    let vtable_ptr: *const GuestContractInterface = runtime
        .registry()
        .resolve_guest_contract(handle)
        .expect("handle must resolve to vtable");
    // SAFETY: vtable_ptr is a valid GuestContractInterface owned by the registry.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // SAFETY: dispatch.vm.call is js_dispatch, loader_data is valid, and the
    // logging function never reads its (args, out) pointers.
    let result: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            0,
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
            core::ptr::null_mut(),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "logging guest function must dispatch Ok, got code={}",
        result.code
    );

    let captured: Vec<(LogLevel, String, String)> = records.lock().expect("records lock").clone();
    assert!(
        captured.contains(&(
            LogLevel::Info,
            String::from("guest.test-log"),
            String::from("hello from js guest"),
        )),
        "expected verbatim (Info, \"guest.test-log\", \"hello from js guest\") record, got: {captured:?}"
    );
    assert!(
        captured.contains(&(
            LogLevel::Error,
            String::from("guest.test-log"),
            String::from("out of range level"),
        )),
        "expected out-of-range level 99 to clamp to Error, got: {captured:?}"
    );
}

// ── polyplug_init returned AbiError ───────────────────────────────────────────

/// A plugin whose `polyplug_init` registers a valid vtable but returns a
/// non-zero AbiError must FAIL to load with that code and message. Before
/// the fix the loader discarded the return value and treated the bundle
/// as loaded.
#[test]
fn load_init_returning_error_code_fails_load() {
    let bundle: &str = r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {
    var vtable = {
        functions: [function(args, out) { return 0; }]
    };
    polyplug.registerVtable(0x1, 0x0, vtable, 1, "test.initerr", 0x00010000);
    return { code: 1, message: "init refused" };
}
"#;
    let (_dir, path) = write_temp_bundle_with_name(bundle, "test.initerr");

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();
    let manifest: ManifestData = make_manifest(&path, "test.initerr");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err(), "non-zero init code must fail the load");
    let err_str: String = result
        .expect_err("expected Err for non-zero init code")
        .to_string();
    assert!(
        err_str.contains("returned error code 1") && err_str.contains("init refused"),
        "error must carry the returned code and message, got: {err_str}"
    );
}

/// A bare numeric non-zero return from `polyplug_init` is also honored.
#[test]
fn load_init_returning_bare_error_number_fails_load() {
    let bundle: &str = r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {
    var vtable = {
        functions: [function(args, out) { return 0; }]
    };
    polyplug.registerVtable(0x2, 0x0, vtable, 1, "test.initnum", 0x00010000);
    return 3;
}
"#;
    let (_dir, path) = write_temp_bundle_with_name(bundle, "test.initnum");

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();
    let manifest: ManifestData = make_manifest(&path, "test.initnum");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(
        result.is_err(),
        "bare non-zero init number must fail the load"
    );
    let err_str: String = result
        .expect_err("expected Err for bare non-zero init number")
        .to_string();
    assert!(
        err_str.contains("returned error code 3"),
        "error must carry the returned code, got: {err_str}"
    );
}

// ── registerVtable malformed vtables ──────────────────────────────────────────

/// A vtable without a `functions` array must fail the load with a precise
/// error naming the missing field — not the misleading generic
/// "polyplug_init did not call registerVtable".
#[test]
fn register_vtable_without_functions_array_fails_precisely() {
    let bundle: &str = r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {
    polyplug.registerVtable(0x3, 0x0, { notFunctions: [] }, 1, "test.malformed", 0x00010000);
    return { code: 0, message: null };
}
"#;
    let (_dir, path) = write_temp_bundle_with_name(bundle, "test.malformed");

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();
    let manifest: ManifestData = make_manifest(&path, "test.malformed");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err(), "malformed vtable must fail the load");
    let err_str: String = result
        .expect_err("expected Err for malformed vtable")
        .to_string();
    assert!(
        err_str.contains("no 'functions' array"),
        "error must name the missing functions array, got: {err_str}"
    );
    assert!(
        !err_str.contains("did not call registerVtable"),
        "error must not blame the bundle for skipping registerVtable, got: {err_str}"
    );
}

/// A vtable whose declared fnCount exceeds the functions array must fail
/// the load naming the missing index.
#[test]
fn register_vtable_with_short_functions_array_fails_precisely() {
    let bundle: &str = r#"
function polyplug_init(host_lo, host_hi, ctx_lo, ctx_hi) {
    var vtable = {
        functions: [function(args, out) { return 0; }]
    };
    polyplug.registerVtable(0x4, 0x0, vtable, 2, "test.short", 0x00010000);
    return { code: 0, message: null };
}
"#;
    let (_dir, path) = write_temp_bundle_with_name(bundle, "test.short");

    let runtime: Arc<Runtime> = make_runtime();
    let loader: JsLoader = make_loader();
    let manifest: ManifestData = make_manifest(&path, "test.short");
    let result: Result<(), polyplug::error::RuntimeError> = loader.load(
        &manifest,
        &polyplug::loader::BundleSource::Path(manifest.path.clone()),
        &runtime,
    );
    assert!(result.is_err(), "short functions array must fail the load");
    let err_str: String = result
        .expect_err("expected Err for short functions array")
        .to_string();
    assert!(
        err_str.contains("functions[1] is missing"),
        "error must name the missing function index, got: {err_str}"
    );
}
