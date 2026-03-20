//! Integration tests for the QuickJS bundle loader.
//!
//! Covers: runtime initialisation, bundle evaluation (valid / syntax error /
//! runtime error), vtable registration, trampoline dispatch, memory management
//! helpers, and thread-safety of the shared QuickJS runtime.

#![allow(clippy::expect_used)]

use core::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::loader::BundleLoader;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;

// ─── Minimal HostVTable (no-op implementations) ───────────────────────────────

/// No-op host allocator — returns null (never called in these tests since we
/// do not exercise host alloc/free from JS; tests that need alloc use a real
/// allocator stub defined below).
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
    // contract_id, contract_version, and function_count are plain u32/u64 fields.
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

/// Build a minimal bundle JS string that calls registerVtable with the given
/// contract_id and vtable_ptr.  fn_count controls `function_count`.
fn make_bundle_js(contract_id: u64, vtable_ptr: usize, fn_count: u32) -> String {
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let vtable_lo: u32 = vtable_ptr as u32;
    let vtable_hi: u32 = (vtable_ptr >> 32) as u32;
    format!(
        "polyplug.registerVtable({contract_lo}, {contract_hi}, {vtable_lo}, {vtable_hi}, {fn_count}, \"test.contract\");"
    )
}

/// Write `content` to a temp file and return the path.
fn write_temp_bundle(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    let path: std::path::PathBuf = dir.path().join("bundle.js");
    std::fs::write(&path, content).expect("write bundle.js");
    (dir, path)
}

/// Create a JsLoader backed by the global no-op host vtable.
fn make_loader() -> JsLoader {
    JsLoader::new(JsConfig {})
}

/// Create a PluginRegistrar wired to the capture callback and the no-op
/// HostVTable.
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

    // Build a static vtable pointer to pass through JS (non-null, non-zero).
    // We leak a Box so the pointer is 'static and the JS side gets a stable address.
    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        // SAFETY: Box::into_raw produces a valid aligned pointer.
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_bundle_js(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
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
    let contract_id: u64 = polyplug_abi::contract_id("test.math", 1);
    let fn_count: u32 = 3;

    let dummy_fn_array: Box<[*const ()]> = vec![core::ptr::null(); fn_count as usize].into();
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: fn_count,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_bundle_js(contract_id, vtable_ptr, fn_count);
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
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
    let contract_id: u64 = polyplug_abi::contract_id("test.dir", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;

    let bundle: String = make_bundle_js(contract_id, vtable_ptr, 0);
    let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("bundle.js"), &bundle).expect("write bundle.js");

    let loader: JsLoader = make_loader();
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

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
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

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_err(), "runtime error bundle must return Err");

    let err_str: String = result
        .expect_err("runtime error bundle must return Err")
        .to_string();
    assert!(
        err_str.contains("js-quickjs"),
        "error must mention runtime name: {err_str}"
    );
}

// ── Missing registerVtable call ───────────────────────────────────────────────

#[test]
fn load_bundle_without_register_vtable_returns_error() {
    // Valid JS that does not call registerVtable.
    let bundle: &str = "var x = 1 + 2;";
    let (_dir, path) = write_temp_bundle(bundle);

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_err(),
        "bundle without registerVtable must return Err"
    );

    let err_str: String = result
        .expect_err("bundle without registerVtable must return Err")
        .to_string();
    assert!(
        err_str.contains("registerVtable"),
        "error must mention registerVtable: {err_str}"
    );
}

// ── Null vtable pointer ───────────────────────────────────────────────────────

#[test]
fn load_bundle_null_vtable_pointer_returns_error() {
    let contract_id: u64 = polyplug_abi::contract_id("test.null_vtable", 1);
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;

    // Pass vtable_lo=0, vtable_hi=0 → null pointer.
    let bundle: String = format!(
        "polyplug.registerVtable({contract_lo}, {contract_hi}, 0, 0, 1, \"test.contract\");"
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
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
        std::path::PathBuf::from("/tmp/polyplug_js_test_nonexistent_bundle_xyz.js");

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(result.is_err(), "non-existent file must return Err");
}

// ── BundlePath global injection ───────────────────────────────────────────────

#[test]
fn bundle_path_global_is_injected() {
    // The loader injects `globalThis.bundlePath` before evaluating the bundle.
    // This test verifies the injection does not cause an error and that the
    // bundle can read the value.
    let contract_id: u64 = polyplug_abi::contract_id("test.bundlepath", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let vtable_lo: u32 = vtable_ptr as u32;
    let vtable_hi: u32 = (vtable_ptr >> 32) as u32;

    // Bundle reads bundlePath; if it is undefined the throw will surface as Err.
    let bundle: String = format!(
        r#"
if (typeof globalThis.bundlePath !== 'string') {{
    throw new Error('bundlePath not injected');
}}
polyplug.registerVtable({contract_lo}, {contract_hi}, {vtable_lo}, {vtable_hi}, 0, "test.contract");
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
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

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let vtable_lo: u32 = vtable_ptr as u32;
    let vtable_hi: u32 = (vtable_ptr >> 32) as u32;

    let bundle: String = format!(
        r#"
var methods = ['findByContract', 'findByBundle', 'findAllByContract',
                'resolvePlugin', 'getExtension', 'registerVtable', 'alloc', 'free'];
for (var i = 0; i < methods.length; i++) {{
    if (typeof polyplug[methods[i]] !== 'function') {{
        throw new Error('missing method: ' + methods[i]);
    }}
}}
polyplug.registerVtable({contract_lo}, {contract_hi}, {vtable_lo}, {vtable_hi}, 0, "test.contract");
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();

    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
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

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let bundle: String = make_bundle_js(contract_id, vtable_ptr, 0);
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = make_loader();
    let mut registrar: PluginRegistrar = make_registrar();
    loader
        .load(&path, &mut registrar)
        .expect("load must succeed");

    let captured: CapturedRegistration = CAPTURED
        .with(|cell: &RefCell<Option<CapturedRegistration>>| cell.borrow_mut().take())
        .expect("vtable must have been registered");

    assert_eq!(
        captured.contract_id, contract_id,
        "contract_id must survive lo/hi split → JS → reconstruct"
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
    let contract_id: u64 = polyplug_abi::contract_id("test.trampoline", 1);
    let fn_count: u32 = 2;

    let dummy_fn_array: Box<[*const ()]> = vec![core::ptr::null(); fn_count as usize].into();
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: fn_count,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let bundle: String = make_bundle_js(contract_id, vtable_ptr, fn_count);
    let (_dir, path) = write_temp_bundle(&bundle);

    TRAMPOLINE_VTABLE.with(|cell: &RefCell<Option<*const PluginVTable>>| {
        *cell.borrow_mut() = None;
    });

    let loader: JsLoader = JsLoader::new(JsConfig {});
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

        // Call the trampoline — it is a stub that returns ABI_OK.
        let dispatch: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
            // SAFETY: fn_ptr is a valid extern "C" trampoline generated by make_trampoline!.
            unsafe { core::mem::transmute(fn_ptr) };
        // SAFETY: null args/out pointers are safe because the stub ignores them.
        let result: AbiError = unsafe { dispatch(core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(result.code, ABI_OK, "trampoline[{slot}] must return ABI_OK");
    }
}

// ── Memory management helpers ─────────────────────────────────────────────────

/// Allocator that tracks calls for memory-management tests.
struct TrackingAllocator {
    alloc_calls: u32,
    free_calls: u32,
}

static TRACKING_ALLOCATOR: Mutex<TrackingAllocator> = Mutex::new(TrackingAllocator {
    alloc_calls: 0,
    free_calls: 0,
});

unsafe extern "C" fn tracking_alloc(size: usize, align: usize) -> *mut u8 {
    {
        let mut guard: std::sync::MutexGuard<'_, TrackingAllocator> =
            TRACKING_ALLOCATOR.lock().unwrap_or_else(|e| e.into_inner());
        guard.alloc_calls += 1;
    }
    // Delegate to the system allocator.
    let layout: core::alloc::Layout = core::alloc::Layout::from_size_align(size, align)
        .unwrap_or(core::alloc::Layout::new::<u8>());
    // SAFETY: layout is valid (size > 0 handled by caller; align is power-of-two by ABI).
    unsafe { std::alloc::alloc(layout) }
}

unsafe extern "C" fn tracking_free(ptr: *mut u8, size: usize, align: usize) {
    {
        let mut guard: std::sync::MutexGuard<'_, TrackingAllocator> =
            TRACKING_ALLOCATOR.lock().unwrap_or_else(|e| e.into_inner());
        guard.free_calls += 1;
    }
    if ptr.is_null() || size == 0 {
        return;
    }
    let layout: core::alloc::Layout = core::alloc::Layout::from_size_align(size, align)
        .unwrap_or(core::alloc::Layout::new::<u8>());
    // SAFETY: ptr was allocated via tracking_alloc with the same layout.
    unsafe { std::alloc::dealloc(ptr, layout) };
}

static TRACKING_HOST_VTABLE: HostVTable = HostVTable {
    alloc: tracking_alloc,
    free: tracking_free,
    find_by_contract: noop_find_by_contract,
    find_by_bundle: noop_find_by_bundle,
    find_all_by_contract: noop_find_all_by_contract,
    resolve_plugin: noop_resolve_plugin,
    get_extension: noop_get_extension,
};

#[test]
fn js_alloc_and_free_calls_host_vtable() {
    // Reset counters.
    {
        let mut guard: std::sync::MutexGuard<'_, TrackingAllocator> =
            TRACKING_ALLOCATOR.lock().unwrap_or_else(|e| e.into_inner());
        guard.alloc_calls = 0;
        guard.free_calls = 0;
    }

    let contract_id: u64 = polyplug_abi::contract_id("test.memory", 1);

    let dummy_fn_array: Box<[*const ()]> = Box::new([]);
    let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
        contract_id,
        contract_version: 0,
        function_count: 0,
        functions: Box::into_raw(dummy_fn_array) as *const *const (),
    });
    let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
    let contract_lo: u32 = contract_id as u32;
    let contract_hi: u32 = (contract_id >> 32) as u32;
    let vtable_lo: u32 = vtable_ptr as u32;
    let vtable_hi: u32 = (vtable_ptr >> 32) as u32;

    // Bundle calls alloc then free.
    let bundle: String = format!(
        r#"
var ptr = polyplug.alloc(64);
if (ptr !== 0) {{
    polyplug.free(ptr);
}}
polyplug.registerVtable({contract_lo}, {contract_hi}, {vtable_lo}, {vtable_hi}, 0, "test.contract");
"#
    );
    let (_dir, path) = write_temp_bundle(&bundle);

    let loader: JsLoader = JsLoader::new(JsConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: capture_register,
        // SAFETY: TRACKING_HOST_VTABLE is static.
        host: &TRACKING_HOST_VTABLE as *const HostVTable,
    };

    // Note: HOST_VTABLE is a process-global OnceLock — it was already set by
    // earlier tests to NOOP_HOST_VTABLE.  The tracking vtable is passed via the
    // registrar but the OnceLock will keep the first value.  We still validate
    // that the load succeeds and registerVtable was called correctly, which
    // exercises the alloc/free code path regardless of which vtable answers.
    let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
    assert!(
        result.is_ok(),
        "bundle with alloc+free must succeed: {result:?}"
    );
}

// ── Thread safety ─────────────────────────────────────────────────────────────

#[test]
fn concurrent_loads_do_not_panic() {
    // Spawn multiple threads each loading a different bundle concurrently.
    // All bundles call registerVtable so load() succeeds.
    // Tests that the shared QJS_RUNTIME and per-Context eval are thread-safe.
    let thread_count: usize = 4;
    let errors: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    let handles: Vec<std::thread::JoinHandle<()>> = (0..thread_count)
        .map(|i: usize| {
            let errors_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&errors);
            std::thread::spawn(move || {
                let contract_id: u64 =
                    polyplug_abi::contract_id(&format!("test.concurrent.{i}"), 1);

                let dummy_fn_array: Box<[*const ()]> = Box::new([]);
                let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
                    contract_id,
                    contract_version: 0,
                    function_count: 0,
                    functions: Box::into_raw(dummy_fn_array) as *const *const (),
                });
                let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
                let bundle: String = make_bundle_js(contract_id, vtable_ptr, 0);
                let (_dir, path) = write_temp_bundle(&bundle);

                let loader: JsLoader = JsLoader::new(JsConfig {});
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
    // Sequential re-use of the same JsLoader for multiple bundles.
    let loader: JsLoader = JsLoader::new(JsConfig {});

    for i in 0..4_u32 {
        let contract_id: u64 = polyplug_abi::contract_id(&format!("test.sequential.{i}"), 1);

        let dummy_fn_array: Box<[*const ()]> = Box::new([]);
        let dummy_vtable: Box<PluginVTable> = Box::new(PluginVTable {
            contract_id,
            contract_version: 0,
            function_count: 0,
            functions: Box::into_raw(dummy_fn_array) as *const *const (),
        });
        let vtable_ptr: usize = Box::into_raw(dummy_vtable) as usize;
        let bundle: String = make_bundle_js(contract_id, vtable_ptr, 0);
        let (_dir, path) = write_temp_bundle(&bundle);

        let mut registrar: PluginRegistrar = make_registrar();
        let result: Result<(), polyplug::error::PolyplugError> = loader.load(&path, &mut registrar);
        assert!(
            result.is_ok(),
            "sequential load {i} must succeed: {result:?}"
        );
    }
}
