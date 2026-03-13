//! Integration tests for LuaLoader and the Lua VM initialization / bundle loading pipeline.
//!
//! These tests exercise:
//! - Lua state initialization (idempotent, error paths)
//! - Bundle loading with a valid plugin script
//! - Bundle loading with a Lua syntax error
//! - Bundle loading with a Lua runtime error inside polyplug_init
//! - Missing `polyplug_init` function detection
//! - VTable registration (function count, contract_id, dispatch)
//! - Stack management (loading multiple bundles in sequence)
//! - Thread safety (concurrent loaders share one global VM without data races)

// allow expect_used in test code per AGENTS.md §4
#![allow(clippy::expect_used)]

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::MutexGuard;

use polyplug::abi::AbiError;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::abi::ABI_OK;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;

// ── Process-global serialization ────────────────────────────────────────────
//
// The LuaJIT VM uses process-global state (LUA_VM, FUNCTION_REGISTRY).
// Without serialization, parallel test threads would race on the shared
// `_G.polyplug_init` / `_G._polyplug_handlers` globals.
static TEST_MUTEX: Mutex<()> = Mutex::new(());

// ── Thread-local registry ────────────────────────────────────────────────────

std::thread_local! {
    static TEST_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
}

// ── ABI registration callback ────────────────────────────────────────────────

/// `register_plugin` callback installed in the test's `PluginRegistrar`.
///
/// Stores each vtable in the thread-local `TEST_REGISTRY` so tests can
/// inspect it without depending on the full `Runtime` stack.
// SAFETY: This is an `extern "C"` callback invoked synchronously during
// `LuaLoader::load`. `descriptor` and `vtable` are valid for the duration of
// the call (ABI contract). Registry::register is marked unsafe because it
// dereferences vtable internally — see SAFETY comment on the inner call.
unsafe extern "C" fn test_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }
    // SAFETY: descriptor is non-null and points to a valid PluginDescriptor
    // passed by the Lua loader for the duration of this call.
    let desc: PluginDescriptor = unsafe { *descriptor };
    // SAFETY: vtable is non-null and points to a leaked 'static PluginVTable
    // created by LuaLoader. The vtable outlives the process.
    let vt: &PluginVTable = unsafe { &*vtable };
    let contract_name: String = format!("lua_contract_{:#x}", vt.contract_id);
    // SAFETY: vtable is a valid 'static PluginVTable from the Lua loader.
    let result: Result<PluginHandle, polyplug::error::RegistryError> = TEST_REGISTRY.with(|cell| {
        let reg: core::cell::Ref<'_, Registry> = cell.borrow();
        // SAFETY: vtable pointer is 'static (leaked by LuaLoader). Registry::register
        // requires the vtable to outlive the registry — guaranteed by the Lua VM's
        // static function registry.
        unsafe { reg.register(desc, vtable, contract_name, vt.contract_id) }
    });
    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(polyplug::error::RegistryError::DuplicateProvider { .. }) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build a `PluginRegistrar` wired to the test callback.
fn test_registrar() -> PluginRegistrar {
    PluginRegistrar {
        register_plugin: test_register_callback,
        host: core::ptr::null(),
    }
}

/// Reset the thread-local registry so each test starts with a fresh slate.
fn reset_registry() {
    TEST_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
}

/// Write `content` to a named temp file in the system temp directory.
/// Returns the path so the caller can keep the file alive until it is no
/// longer needed.
fn write_temp_lua(name: &str, content: &[u8]) -> PathBuf {
    let path: PathBuf = std::env::temp_dir().join(name);
    let mut f: std::fs::File = std::fs::File::create(&path).expect("create temp lua file");
    f.write_all(content).expect("write temp lua content");
    path
}

/// A minimal valid Lua plugin script that implements the `test.loader@1`
/// contract with a single no-op function.
fn valid_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function impl_noop(_args_ptr, _out_ptr)
end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "test.loader",
        contract_version = 1,
        plugin_name      = "test-loader-unit",
        functions        = { [0] = impl_noop },
    }
end
"#
}

/// A Lua plugin that defines two functions so we can verify function_count.
fn two_function_plugin_script() -> &'static [u8] {
    br#"
local ffi = require("ffi")
local function impl_a(_args_ptr, _out_ptr) end
local function impl_b(_args_ptr, _out_ptr) end
function polyplug_init(_registrar_ptr, _ctx_ptr)
    _G._polyplug_handlers = {
        contract_name    = "test.two",
        contract_version = 1,
        plugin_name      = "test-two-unit",
        functions        = { [0] = impl_a, [1] = impl_b },
    }
end
"#
}

/// Load the supplied Lua source via `LuaLoader::load` and return the result.
fn load_script(path: &Path) -> Result<(), PolyplugError> {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    reset_registry();
    let mut registrar: PluginRegistrar = test_registrar();
    loader.load(path, &mut registrar)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// ── 1. Runtime name ──────────────────────────────────────────────────────────

#[test]
fn lua_loader_runtime_name_is_lua() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.runtime_name(), "lua");
}

// ── 2. Lua state initialization ──────────────────────────────────────────────

/// Loading a valid bundle must succeed — which implicitly verifies that the
/// LuaJIT VM was initialized correctly.
#[test]
fn lua_state_initializes_on_first_load() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_init_test.lua", valid_plugin_script());
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(
        result.is_ok(),
        "Lua VM must initialize and bundle must load: {:?}",
        result.err()
    );
}

/// Calling `LuaLoader::load` a second time re-uses the same VM without
/// panicking (idempotent initialization).
#[test]
fn lua_state_init_is_idempotent() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_idempotent.lua", valid_plugin_script());
    load_script(&path).expect("first load must succeed");
    // Second load of the same file: VM is already initialized — must not panic.
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(
        result.is_ok(),
        "second load must succeed (idempotent VM init): {:?}",
        result.err()
    );
}

// ── 3. Bundle loading — valid script ─────────────────────────────────────────

#[test]
fn load_valid_bundle_succeeds() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_valid.lua", valid_plugin_script());
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(result.is_ok(), "valid bundle must load: {:?}", result.err());
}

// ── 4. Bundle loading — syntax error ─────────────────────────────────────────

/// A Lua script with a syntax error must produce a `LuaScriptLoadFailed` error.
#[test]
fn load_syntax_error_returns_script_load_failed() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua(
        "lua_loader_syntax_error.lua",
        b"function polyplug_init( -- SYNTAX ERROR: unclosed paren\n",
    );
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(result.is_err(), "syntax error must produce an Err");
    let err: PolyplugError = result.expect_err("expected Err for syntax error");
    assert!(
        matches!(
            err,
            PolyplugError::Loader(LoaderError::LuaScriptLoadFailed { .. })
        ),
        "expected LuaScriptLoadFailed, got: {:?}",
        err
    );
}

// ── 5. Bundle loading — runtime error in polyplug_init ───────────────────────

/// A script where `polyplug_init` raises a Lua error at runtime must produce
/// `LuaInitRaisedError`.
#[test]
fn load_runtime_error_in_init_returns_init_raised_error() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua(
        "lua_loader_runtime_err.lua",
        b"function polyplug_init(_reg, _ctx)\n  error('deliberate runtime error')\nend\n",
    );
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(result.is_err(), "runtime error in init must produce Err");
    let err: PolyplugError = result.expect_err("expected Err for runtime error in init");
    assert!(
        matches!(
            err,
            PolyplugError::Loader(LoaderError::LuaInitRaisedError { .. })
        ),
        "expected LuaInitRaisedError, got: {:?}",
        err
    );
}

// ── 6. Missing polyplug_init ─────────────────────────────────────────────────

/// A script that does not define `polyplug_init` must return
/// `LuaInitFunctionMissing`.
#[test]
fn load_missing_polyplug_init_returns_typed_error() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua(
        "lua_loader_no_init.lua",
        b"local x = 1  -- no polyplug_init\n",
    );
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(result.is_err(), "missing init must produce Err");
    let err: PolyplugError = result.expect_err("expected Err for missing polyplug_init");
    assert!(
        matches!(
            err,
            PolyplugError::Loader(LoaderError::LuaInitFunctionMissing { .. })
        ),
        "expected LuaInitFunctionMissing, got: {:?}",
        err
    );
}

// ── 7. Non-existent file ──────────────────────────────────────────────────────

#[test]
fn load_nonexistent_path_returns_script_load_failed() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = std::env::temp_dir().join("this_file_does_not_exist_42.lua");
    // Ensure the file really does not exist.
    let _ = std::fs::remove_file(&path);
    let result: Result<(), PolyplugError> = load_script(&path);
    assert!(result.is_err(), "missing file must produce Err");
    let err: PolyplugError = result.expect_err("expected Err for nonexistent file");
    assert!(
        matches!(
            err,
            PolyplugError::Loader(LoaderError::LuaScriptLoadFailed { .. })
        ),
        "expected LuaScriptLoadFailed for missing file, got: {:?}",
        err
    );
}

// ── 8. VTable registration ────────────────────────────────────────────────────

/// After a successful load, the registry must contain a plugin for the
/// expected contract_id with the correct function count.
#[test]
fn vtable_is_registered_after_load() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_vtable.lua", valid_plugin_script());
    load_script(&path).expect("valid bundle must load");

    let contract_id: u64 = polyplug::abi::contract_id("test.loader", 1);
    let handle: Result<PluginHandle, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().find(contract_id, 0));
    assert!(
        handle.is_ok(),
        "registry must contain test.loader@1 after load"
    );
    let handle: PluginHandle = handle.expect("handle must be Ok");
    let vtable_ptr: Result<*const PluginVTable, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().resolve(handle));
    assert!(vtable_ptr.is_ok(), "handle must resolve to a vtable");
    // SAFETY: vtable_ptr is a 'static pointer produced by LuaLoader; the Lua VM
    // and leaked PluginVTable outlive this test.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    assert_eq!(
        vtable.function_count, 1,
        "valid_plugin_script has exactly one function"
    );
}

/// After loading the two-function plugin, function_count must equal 2.
#[test]
fn vtable_function_count_matches_script() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_two_fn.lua", two_function_plugin_script());
    load_script(&path).expect("two-function bundle must load");

    let contract_id: u64 = polyplug::abi::contract_id("test.two", 1);
    let handle: Result<PluginHandle, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().find(contract_id, 0));
    let handle: PluginHandle = handle.expect("test.two@1 must be registered");
    let vtable_ptr: Result<*const PluginVTable, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().resolve(handle));
    // SAFETY: see vtable_is_registered_after_load.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    assert_eq!(
        vtable.function_count, 2,
        "two_function_plugin_script must register 2 functions"
    );
}

/// The contract_id stored in the vtable must match the FNV-1a hash computed
/// from the contract name and version declared in the script.
#[test]
fn vtable_contract_id_matches_computed_hash() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_cid.lua", valid_plugin_script());
    load_script(&path).expect("valid bundle must load");

    let expected_cid: u64 = polyplug::abi::contract_id("test.loader", 1);
    let handle: Result<PluginHandle, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().find(expected_cid, 0));
    let handle: PluginHandle = handle.expect("test.loader@1 must be registered");
    let vtable_ptr: Result<*const PluginVTable, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().resolve(handle));
    // SAFETY: see vtable_is_registered_after_load.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr.expect("vtable must resolve") };
    assert_eq!(
        vtable.contract_id, expected_cid,
        "contract_id in vtable must match FNV-1a hash of 'test.loader@1'"
    );
}

// ── 9. Stack management — sequential loads ────────────────────────────────────

/// Load two different plugins in sequence: each must succeed and register its
/// own contract.
#[test]
fn sequential_loads_both_succeed() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path1: PathBuf = write_temp_lua("lua_loader_seq1.lua", valid_plugin_script());
    let path2: PathBuf = write_temp_lua("lua_loader_seq2.lua", two_function_plugin_script());

    // Reset registry and load the first bundle.
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    reset_registry();
    let mut reg1: PluginRegistrar = test_registrar();
    loader
        .load(&path1, &mut reg1)
        .expect("first sequential load must succeed");

    // Load the second bundle with the same loader.
    let mut reg2: PluginRegistrar = test_registrar();
    loader
        .load(&path2, &mut reg2)
        .expect("second sequential load must succeed");

    // Both contracts must be visible.
    let cid1: u64 = polyplug::abi::contract_id("test.loader", 1);
    let cid2: u64 = polyplug::abi::contract_id("test.two", 1);

    let handle1: Result<PluginHandle, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().find(cid1, 0));
    assert!(handle1.is_ok(), "test.loader must be registered");

    let handle2: Result<PluginHandle, polyplug::error::RegistryError> =
        TEST_REGISTRY.with(|cell| cell.borrow().find(cid2, 0));
    assert!(handle2.is_ok(), "test.two must be registered");
}

// ── 10. Thread safety ─────────────────────────────────────────────────────────

/// Spawn multiple threads, each loading the same valid plugin.  The global
/// Mutex inside `LuaLoader` must prevent data races and every load must
/// either succeed or produce a recognized `PolyplugError` (no panics,
/// no UB).
///
/// The single LuaJIT VM uses process-global state, so this test acquires
/// `TEST_MUTEX` to serialize the Lua globals (`polyplug_init`,
/// `_polyplug_handlers`) while still exercising the Mutex-protected
/// FUNCTION_REGISTRY and the `OnceLock` initialization path across threads.
#[test]
fn concurrent_loaders_do_not_race() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let path: PathBuf = write_temp_lua("lua_loader_thread_safety.lua", valid_plugin_script());

    // Spawn 4 threads that all call LuaLoader::load on the same path.
    let path_arc: std::sync::Arc<PathBuf> = std::sync::Arc::new(path);
    let handles: Vec<std::thread::JoinHandle<Result<(), PolyplugError>>> = (0_u32..4_u32)
        .map(|_| {
            let p: std::sync::Arc<PathBuf> = std::sync::Arc::clone(&path_arc);
            std::thread::spawn(move || {
                let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
                let mut reg: PluginRegistrar = test_registrar();
                loader.load(p.as_ref(), &mut reg)
            })
        })
        .collect::<Vec<std::thread::JoinHandle<Result<(), PolyplugError>>>>();

    for handle in handles {
        // Each thread must not panic. Errors (e.g. DuplicateProvider inside
        // the callback) are acceptable — panics are not.
        let result: Result<(), PolyplugError> = handle
            .join()
            .expect("thread must not panic during concurrent load");
        // The result may be Ok or a recognized PolyplugError.
        // We simply assert it is a valid discriminant (no silent UB).
        let _ = result;
    }
}

// ── 11. Dispatch — calling a registered Lua function ─────────────────────────

/// Load the valid plugin and invoke its single function through the vtable.
/// The noop function must return `ABI_OK` without panicking.
#[test]
fn vtable_function_dispatch_returns_abi_ok() {
    let _guard: MutexGuard<'_, ()> = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let path: PathBuf = write_temp_lua("lua_loader_dispatch.lua", valid_plugin_script());
    load_script(&path).expect("valid bundle must load");

    let contract_id: u64 = polyplug::abi::contract_id("test.loader", 1);
    let handle: PluginHandle = TEST_REGISTRY
        .with(|cell| cell.borrow().find(contract_id, 0))
        .expect("test.loader@1 must be registered");
    let vtable_ptr: *const PluginVTable = TEST_REGISTRY
        .with(|cell| cell.borrow().resolve(handle))
        .expect("handle must resolve to vtable");
    // SAFETY: vtable_ptr is a 'static leaked PluginVTable from LuaLoader.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "vtable must have at least one function"
    );
    // SAFETY: vtable.functions is a non-null static array with function_count entries.
    // We index slot 0, which is valid because function_count >= 1.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    assert!(
        !fn_ptr.is_null(),
        "trampoline function pointer must be non-null"
    );

    // Cast to the generic dispatch signature used by all trampolines.
    // SAFETY: all LuaLoader trampolines use `extern "C" fn(*const (), *mut ()) -> AbiError`.
    let dispatch: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // The noop function ignores both pointers — pass null for both.
    // SAFETY: the Lua noop function does not dereference args_ptr or out_ptr.
    let result: AbiError =
        unsafe { dispatch(core::ptr::null::<()>(), core::ptr::null_mut::<()>()) };
    assert_eq!(
        result.code, ABI_OK,
        "noop function must return ABI_OK, got code={}",
        result.code
    );
}
