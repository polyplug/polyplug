//! Integration tests for the Deno (V8 in-process) bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), vtable registration, trampoline dispatch, permission flags
//! (none required — V8 is in-process), and thread safety of concurrent loads.

#![allow(clippy::expect_used)]

use core::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug::loader::BundleLoader;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;

// ─── Minimal HostVTable (no-op implementations) ───────────────────────────────

/// No-op host allocator — returns null (sufficient for tests that do not
/// exercise host alloc/free from JS).
unsafe extern "C" fn noop_alloc(_size: usize, _align: usize) -> *mut u8 {
    core::ptr::null_mut()
}

/// No-op host free.
unsafe extern "C" fn noop_free(_ptr: *mut u8, _size: usize, _align: usize) {}

/// No-op find_by_contract — always returns null handle.
unsafe extern "C" fn noop_find_by_contract(_contract_id: u64, _min: u32) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_by_bundle — always returns null handle.
unsafe extern "C" fn noop_find_by_bundle(
    _bundle_id: u64,
    _contract_id: u64,
    _min: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract — always returns 0.
unsafe extern "C" fn noop_find_all_by_contract(
    _contract_id: u64,
    _min: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// No-op resolve_plugin — always returns null.
unsafe extern "C" fn noop_resolve_plugin(_handle: PluginHandle) -> *const PluginVTable {
    core::ptr::null()
}

/// No-op get_extension — always returns null.
unsafe extern "C" fn noop_get_extension(_extension_id: u32) -> *const () {
    core::ptr::null()
}

/// A static no-op HostVTable used for tests that do not exercise host
/// capabilities from JS.
static NOOP_HOST_VTABLE: HostVTable = HostVTable {
    alloc: noop_alloc,
    free: noop_free,
    find_by_contract: noop_find_by_contract,
    find_by_bundle: noop_find_by_bundle,
    find_all_by_contract: noop_find_all_by_contract,
    resolve_plugin: noop_resolve_plugin,
    get_extension: noop_get_extension,
};

// ─── Capture registrar ────────────────────────────────────────────────────────

/// Per-test captured registration result.
struct CapturedRegistration {
    contract_id: u64,
    contract_version: u32,
    function_count: u32,
}

thread_local! {
    static CAPTURED: RefCell<Option<CapturedRegistration>> = const { RefCell::new(None) };
}

/// Registrar callback that captures vtable data into CAPTURED for inspection.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid non-null pointers for
/// the duration of this call (guaranteed by the ABI contract).
unsafe extern "C" fn capture_register(
    registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if registrar.is_null() || descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }
    // SAFETY: vtable is valid for the duration of this call (ABI contract).
    let (contract_id, contract_version, function_count): (u64, u32, u32) = unsafe {
        (
            (*vtable).contract_id,
            (*vtable).contract_version,
            (*vtable).function_count,
        )
    };

    CAPTURED.with(|cell: &RefCell<Option<CapturedRegistration>>| {
        *cell.borrow_mut() = Some(CapturedRegistration {
            contract_id,
            contract_version,
            function_count,
        });
    });

    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a minimal Deno ES module bundle JS string that calls
/// `Deno.core.ops.op_register_vtable` with the given contract_id,
/// vtable_ptr, and fn_count.
///
/// The Deno op takes (contract_id: bigint, vtable_ptr: bigint, fn_count: u32).
fn make_deno_bundle(contract_id: u64, vtable_ptr: usize, fn_count: u32) -> String {
    format!("Deno.core.ops.op_register_vtable({contract_id}n, {vtable_ptr}n, {fn_count});\n")
}

/// Write `content` to a temp file named `bundle.js` and return the dir and path.
fn write_temp_bundle(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: std::path::PathBuf = dir.path().join("bundle.js");
    std::fs::write(&path, content).expect("write bundle.js");
    (dir, path)
}

/// Create a JsDenoLoader with default config.
fn make_loader() -> JsDenoLoader {
    JsDenoLoader::new(JsDenoConfig {})
}

/// Create a PluginRegistrar wired to the capture callback and the no-op HostVTable.
/// Resets CAPTURED to None before returning.
fn make_registrar() -> PluginRegistrar {
    CAPTURED.with(|cell: &RefCell<Option<CapturedRegistration>>| {
        *cell.borrow_mut() = None;
    });
    PluginRegistrar {
        register_plugin: capture_register,
        // SAFETY: NOOP_HOST_VTABLE is a static — valid for process lifetime.
        host: &NOOP_HOST_VTABLE as *const HostVTable,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

// ── Runtime name ──────────────────────────────────────────────────────────────

#[test]
fn runtime_name_is_js_deno() {
    let loader: JsDenoLoader = make_loader();
    assert_eq!(loader.runtime_name(), "js-deno");
}

// ── Deno runtime initialisation — loader construction does not panic ──────────

#[test]
fn loader_construction_does_not_panic() {
    // Constructing JsDenoLoader must not spin up V8 or Tokio eagerly.
    let _loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
}

// ── Valid bundle evaluation + vtable registration ─────────────────────────────

#[test]
fn load_valid_bundle_registers_vtable() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.noop", 1);

    // Build a static vtable pointer to pass through JS (non-null).
    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        // SAFETY: Box::into_raw produces a valid aligned pointer.
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_deno_bundle(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    let captured: CapturedRegistration = CAPTURED
        .with(|cell: &RefCell<Option<CapturedRegistration>>| cell.borrow_mut().take())
        .expect("vtable must have been registered");

    assert_eq!(
        captured.contract_id, contract_id,
        "registered contract_id must match"
    );
    assert_eq!(
        captured.contract_version, 0,
        "contract_version must be 0 (set by loader)"
    );
    assert_eq!(
        captured.function_count, 0,
        "function_count must match fn_count arg"
    );
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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "load must succeed: {result:?}");

    let captured: CapturedRegistration = CAPTURED
        .with(|cell: &RefCell<Option<CapturedRegistration>>| cell.borrow_mut().take())
        .expect("vtable must have been registered");

    assert_eq!(captured.contract_id, contract_id);
    assert_eq!(captured.function_count, fn_count);
}

// ── Directory path fallback ───────────────────────────────────────────────────

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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    // Pass the directory — loader must append "bundle.js" automatically.
    let result: Result<(), polyplug::error::PolyplugError> =
        loader.load(dir.path(), &mut registrar);
    assert!(
        result.is_ok(),
        "load from directory path must succeed: {result:?}"
    );

    let captured: CapturedRegistration = CAPTURED
        .with(|cell: &RefCell<Option<CapturedRegistration>>| cell.borrow_mut().take())
        .expect("vtable must have been registered");
    assert_eq!(captured.contract_id, contract_id);
}

// ── Syntax error ──────────────────────────────────────────────────────────────

#[test]
fn load_syntax_error_returns_error() {
    let bundle: &str = "this is not valid javascript }{{{";
    let (_dir, path) = write_temp_bundle(bundle);

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
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

// ── Runtime error ─────────────────────────────────────────────────────────────

#[test]
fn load_runtime_error_returns_error() {
    // Valid JS syntax but throws at runtime.
    let bundle: &str = "throw new Error('intentional runtime error');";
    let (_dir, path) = write_temp_bundle(bundle);

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
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

// ── Missing op_register_vtable call ───────────────────────────────────────────

#[test]
fn load_bundle_without_register_vtable_returns_error() {
    // Valid JS that does not call op_register_vtable.
    let bundle: &str = "var x = 1 + 2;";
    let (_dir, path) = write_temp_bundle(bundle);

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_err(),
        "bundle without op_register_vtable must return Err"
    );

    // The loader times out waiting for vtable registration.
    let err_str: String = result
        .expect_err("bundle without op_register_vtable must return Err")
        .to_string();
    assert!(
        err_str.contains("js-deno"),
        "error must mention runtime name: {err_str}"
    );
}

// ── Null vtable pointer ───────────────────────────────────────────────────────

#[test]
fn load_bundle_null_vtable_pointer_returns_error() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.null_vtable", 1);

    // Pass vtable_ptr=0n → null pointer.
    let bundle: String = format!("Deno.core.ops.op_register_vtable({contract_id}n, 0n, 1);\n");
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_err(), "null vtable pointer must return Err");

    let err_str: String = result
        .expect_err("null vtable pointer must return Err")
        .to_string();
    assert!(
        err_str.contains("null vtable"),
        "error must mention null vtable: {err_str}"
    );
}

// ── File not found ────────────────────────────────────────────────────────────

#[test]
fn load_nonexistent_file_returns_error() {
    let path: std::path::PathBuf =
        std::path::PathBuf::from("/tmp/polyplug_js_deno_test_nonexistent_bundle_xyz.js");

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_err(), "non-existent file must return Err");
}

// ── BundlePath global injection ───────────────────────────────────────────────

#[test]
fn bundle_path_global_is_injected() {
    // The loader injects `globalThis.bundlePath` before evaluating the module.
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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_ok(),
        "bundle reading bundlePath must succeed: {result:?}"
    );
}

// ── Deno ops are accessible from JS ──────────────────────────────────────────

#[test]
fn deno_core_ops_are_accessible() {
    // Verify that all polyplug Deno ops are accessible via Deno.core.ops.
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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_ok(), "all Deno ops must be present: {result:?}");
}

// ── VTable contract_id roundtrip ──────────────────────────────────────────────

#[test]
fn vtable_contract_id_roundtrip() {
    // Use a well-known FNV-1a contract.
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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();
    loader
        .load(&path, &mut registrar)
        .expect("load must succeed");

    let captured: CapturedRegistration = CAPTURED
        .with(|cell: &RefCell<Option<CapturedRegistration>>| cell.borrow_mut().take())
        .expect("vtable must have been registered");

    assert_eq!(
        captured.contract_id, contract_id,
        "contract_id must survive BigInt encoding → op → reconstruct"
    );
}

// ── Trampoline dispatch ───────────────────────────────────────────────────────

thread_local! {
    static TRAMPOLINE_VTABLE: RefCell<Option<*const PluginVTable>> =
        const { RefCell::new(None) };
}

/// Registrar callback that stores the PluginVTable pointer for trampoline testing.
///
/// # Safety
/// `registrar`, `descriptor`, and `vtable` must be valid non-null pointers for
/// the duration of this call.
unsafe extern "C" fn trampoline_capture_register(
    _registrar: *mut PluginRegistrar,
    _descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }
    TRAMPOLINE_VTABLE.with(|cell: &RefCell<Option<*const PluginVTable>>| {
        *cell.borrow_mut() = Some(vtable);
    });
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

#[test]
fn trampoline_fn_pointers_are_non_null_and_callable() {
    let contract_id: u64 = polyplug_abi::contract_id("deno.test.trampoline", 1);
    let fn_count: u32 = 2;

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

    TRAMPOLINE_VTABLE.with(|cell: &RefCell<Option<*const PluginVTable>>| {
        *cell.borrow_mut() = None;
    });

    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: trampoline_capture_register,
        // SAFETY: NOOP_HOST_VTABLE is static.
        host: &NOOP_HOST_VTABLE as *const HostVTable,
    };

    loader
        .load(&path, &mut registrar)
        .expect("load must succeed");

    let stored_vtable: *const PluginVTable = TRAMPOLINE_VTABLE
        .with(|cell: &RefCell<Option<*const PluginVTable>>| *cell.borrow())
        .expect("vtable must have been captured");

    // SAFETY: stored_vtable was set by the loader — it is the Box::into_raw
    // vtable allocated in load(). It is valid for the process lifetime.
    let vtable_ref: &PluginVTable = unsafe { &*stored_vtable };

    assert_eq!(vtable_ref.function_count, fn_count);
    assert!(!vtable_ref.functions.is_null());

    for slot in 0..fn_count as usize {
        // SAFETY: functions is a valid pointer to function_count entries.
        let fn_ptr: *const () = unsafe { *vtable_ref.functions.add(slot) };
        assert!(!fn_ptr.is_null(), "trampoline[{slot}] must be non-null");

        // Call the trampoline — Deno trampolines dispatch via DENO_FUNCTION_REGISTRY.
        // The slot's call_tx will be dropped (bundle thread finished), so the stub
        // returns an error code rather than ABI_OK, which is acceptable here — we
        // only verify non-null and callable (no panic/segfault).
        let dispatch: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
            // SAFETY: fn_ptr is a valid extern "C" trampoline generated by make_trampoline!.
            unsafe { core::mem::transmute(fn_ptr) };
        // SAFETY: null args/out pointers are safe because the dispatch stub
        // either routes to the call channel or returns an error immediately.
        let _result: AbiError = unsafe { dispatch(core::ptr::null(), core::ptr::null_mut()) };
    }
}

// ── No subprocess / no permission flags required ──────────────────────────────

#[test]
fn no_subprocess_permissions_required() {
    // V8 is embedded in-process — no Deno CLI --allow-* flags are needed.
    // This test confirms a basic bundle load succeeds without any special
    // environment setup (simulating a zero-permission environment).
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

    let loader: JsDenoLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    // No environment manipulation, no special flags — load must succeed.
    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_ok(),
        "in-process V8 load requires no external permissions: {result:?}"
    );
}

// ── Thread safety ─────────────────────────────────────────────────────────────

#[test]
fn concurrent_loads_do_not_panic() {
    // Spawn multiple threads each loading a different bundle concurrently.
    // Each JsDenoLoader pins its V8 isolate to a dedicated inner thread.
    // This tests that the outer concurrent invocations don't interfere.
    let thread_count: usize = 4;
    let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<std::thread::JoinHandle<()>> = (0..thread_count)
        .map(|i: usize| {
            let errors_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&errors);
            std::thread::spawn(move || {
                let contract_id: u64 =
                    polyplug_abi::contract_id(&format!("deno.test.concurrent.{i}"), 1);

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

                let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
                let mut registrar: PluginRegistrar = PluginRegistrar {
                    register_plugin: capture_register,
                    // SAFETY: NOOP_HOST_VTABLE is static.
                    host: &NOOP_HOST_VTABLE as *const HostVTable,
                };

                if let Err(e) = loader.load(&path, &mut registrar) {
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
    // Sequential re-use of the same JsDenoLoader for multiple bundles.
    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});

    for i in 0..4_u32 {
        let contract_id: u64 = polyplug_abi::contract_id(&format!("deno.test.sequential.{i}"), 1);

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

        let mut registrar: PluginRegistrar = make_registrar();
        let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
        assert!(
            result.is_ok(),
            "sequential load {i} must succeed: {result:?}"
        );
    }
}
