#![allow(clippy::expect_used)]

use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginRegistrar;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::error::RegistryError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;

/// Process-global mutex to serialize integration tests.
/// The single LuaJIT VM uses shared globals (polyplug_init, _polyplug_handlers).
/// Without serialization, parallel tests race on those globals.
static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");

/// `AddArgs` is the repr(C) struct that maps to `fn add(a: u32, b: u32) -> u32`.
/// Fields must be in declaration order to match the Lua FFI cdef.
#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// Thread-local registry for test isolation.
std::thread_local! {
    static LUA_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
}

/// Registration callback passed to LuaLoader via PluginRegistrar.
/// Writes the registered plugin into the thread-local LUA_REGISTRY.
unsafe extern "C" fn registry_register_callback(
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
    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for this call (ABI contract).
    let vt: &PluginVTable = unsafe { &*vtable };
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };
    // SAFETY: vtable pointer is 'static — extracted from a Lua VM that outlives registry.
    let result: Result<PluginHandle, RegistryError> = LUA_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        // SAFETY: vtable pointer is 'static — extracted from a Lua VM that outlives registry.
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
    });
    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(RegistryError::DuplicateProvider { .. }) => {
            // Second load of the same plugin — already registered, treat as success.
            AbiError {
                code: ABI_OK,
                message: StringView::null(),
            }
        }
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

fn make_loader() -> LuaLoader {
    LuaLoader::new(LuaConfig::default())
}

fn load_fixture() -> Result<(), PolyplugError> {
    let loader: LuaLoader = make_loader();
    LUA_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    loader.load(std::path::Path::new(LUA_PLUGIN), &mut registrar)
}

fn get_vtable() -> *const PluginVTable {
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = LUA_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered after load_fixture()")
    });
    LUA_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"))
}

#[test]
fn integration_lua_runtime_name() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.runtime_name(), "lua");
}

#[test]
fn integration_lua_bundle_loads() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let result: Result<(), PolyplugError> = load_fixture();
    assert!(
        result.is_ok(),
        "LuaLoader::load() must succeed for fixture: {:?}",
        result.err()
    );
}

#[test]
fn integration_lua_add() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr is valid; the Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 0 (add). args/out are correctly typed for the add function.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
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
fn integration_lua_add_primitive() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr is valid.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 2,
        "test.add vtable must have at least 2 functions"
    );
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is function 1 (add_primitive). args/out are correctly typed.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(1) };
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
fn integration_lua_version_string() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 3,
        "test.add vtable must have at least 3 functions"
    );
    let mut out_view: StringView = StringView::null();
    // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; version takes no args (null input accepted by Lua side).
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
    assert_eq!(version_str, "1.0.0-lua", "unexpected version string");
}

#[test]
fn integration_lua_reset() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 4,
        "test.add vtable must have at least 4 functions"
    );
    // SAFETY: fn_ptr is function 3 (reset). vtable.functions is valid (non-null, in-bounds).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: same dispatch signature; reset has void args and void out.
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: both null — reset does not read args or write output.
    let result: AbiError =
        unsafe { dispatch_fn(core::ptr::null::<()>(), core::ptr::null_mut::<()>()) };
    assert_eq!(result.code, ABI_OK, "reset must return ABI_OK");
}

#[test]
fn integration_lua_init_function_missing_returns_typed_error() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // Write a temp Lua file without polyplug_init.
    let tmp_path: std::path::PathBuf = std::env::temp_dir().join("noinit_test.lua");
    std::fs::write(&tmp_path, b"local x = 1\n").expect("write temp file");

    let loader: LuaLoader = make_loader();
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> = loader.load(&tmp_path, &mut registrar);
    assert!(result.is_err());
    let err: PolyplugError = result.expect_err("expected Err(LuaInitFunctionMissing)");
    assert!(
        matches!(
            err,
            PolyplugError::Loader(LoaderError::LuaInitFunctionMissing { .. })
        ),
        "expected LuaInitFunctionMissing, got: {:?}",
        err
    );
}

#[test]
fn integration_lua_utf8_roundtrip() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_fixture().expect("fixture must load");
    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: fn_ptr is function 2 (version). vtable.functions is valid (non-null, in-bounds).
    // SAFETY: vtable_ptr is valid; the Lua VM stays alive for process lifetime.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: fn_ptr is function 2 (version). vtable.functions is valid (non-null, in-bounds).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: cast to generic dispatch signature; arg types enforced by test.
        unsafe { core::mem::transmute(fn_ptr) };
    let mut out_view: StringView = StringView::null();
    // SAFETY: out_view is valid stack allocation.
    let result: AbiError = unsafe {
        dispatch_fn(
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK);
    // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes.
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let version_str: &str = core::str::from_utf8(version_bytes).expect("version must be UTF-8");
    assert!(
        version_str.is_ascii(),
        "version string is not ASCII: {}",
        version_str
    );
    assert_eq!(version_str.as_bytes(), b"1.0.0-lua");
}

#[test]
fn integration_lua_second_load_does_not_panic() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // Loading the same plugin twice must not panic (ffi.cdef pcall guard).
    load_fixture().expect("first load");
    let loader: LuaLoader = make_loader();
    let mut registrar2: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(LUA_PLUGIN), &mut registrar2);
    assert!(result.is_ok(), "second load failed: {:?}", result.err());
}
