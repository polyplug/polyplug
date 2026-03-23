#![allow(clippy::expect_used)]

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::DispatchType;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_abi::ABI_OK;
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

fn make_loader() -> LuaLoader {
    LuaLoader::new(LuaConfig::default())
}

fn create_runtime() -> Runtime {
    Runtime::builder()
        .loader(make_loader())
        .build()
        .expect("failed to build runtime")
}

fn load_fixture(rt: &Runtime) -> Result<(), PolyplugError> {
    rt.load_bundle(std::path::Path::new(LUA_PLUGIN))
}

fn get_vtable(rt: &Runtime) -> *const PluginInterface {
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = rt
        .find_by_contract(contract_id, 0)
        .expect("test.add must be registered after load_fixture()");
    rt.resolve_plugin(handle).expect("handle must be valid")
}

/// Call a VM dispatch function by fn_id.
/// SAFETY: vtable must be valid and have VM dispatch type.
unsafe fn call_vm_function(
    vtable: &PluginInterface,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "expected VM dispatch type"
    );
    (vtable.dispatch.vm.call)(vtable.dispatch.vm.loader_data, fn_id, args, out)
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
    let rt: Runtime = create_runtime();
    let result: Result<(), PolyplugError> = load_fixture(&rt);
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Lua VM stays alive for process lifetime.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable is valid with VM dispatch; args/out are correctly typed.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            0,
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 2,
        "test.add vtable must have at least 2 functions"
    );
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: vtable is valid with VM dispatch; args/out are correctly typed.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            1,
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
    // SAFETY: vtable is valid with VM dispatch; out_view is a valid StringView.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr valid.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 4,
        "test.add vtable must have at least 4 functions"
    );
    // SAFETY: vtable is valid with VM dispatch; reset takes no args and returns nothing.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            3,
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
        )
    };
    assert_eq!(result.code, ABI_OK, "reset must return ABI_OK");
}

#[test]
fn integration_lua_init_function_missing_returns_typed_error() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // Create a temp bundle directory with manifest.toml but no polyplug_init in the script.
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("noinit_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    // Write manifest.toml
    let manifest_content = r#"
name = "noinit_test"
id = 9999999999999
version = "1.0.0"
runtime = "lua"
file = "plugin.lua"
provides = ["test.noinit@1"]

[function_count]
"test.noinit@1" = 1
"#;
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");

    // Write Lua script without polyplug_init
    std::fs::write(tmp_dir.join("plugin.lua"), b"local x = 1\n").expect("write plugin.lua");

    let rt: Runtime = create_runtime();
    let result: Result<(), PolyplugError> = rt.load_bundle(&tmp_dir);
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

    // Cleanup
    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_lua_utf8_roundtrip() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    // SAFETY: vtable_ptr is valid; the Lua VM stays alive for process lifetime.
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    let mut out_view: StringView = StringView::null();
    // SAFETY: vtable is valid with VM dispatch; out_view is valid stack allocation.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
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
fn integration_lua_second_load_succeeds() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    // Loading the same plugin twice should succeed (multi-impl support)
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("first load must succeed");
    let result: Result<(), PolyplugError> = rt.load_bundle(std::path::Path::new(LUA_PLUGIN));
    // Second load should succeed (multi-impl allowed)
    assert!(
        result.is_ok(),
        "second load should succeed (multi-impl allowed): {:?}",
        result.err()
    );
}
