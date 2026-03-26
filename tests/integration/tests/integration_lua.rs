#![allow(clippy::expect_used)]

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::ABI_OK;
use polyplug_abi::AbiError;
use polyplug_abi::DispatchType;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginInterface;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;

const LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");

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
    rt.resolve_plugin(handle)
        .expect("handle must be valid")
        .vtable()
}

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
    unsafe { (vtable.dispatch.vm.call)(vtable.dispatch.vm.loader_data, fn_id, args, out) }
}

#[test]
fn integration_lua_runtime_name() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.runtime_name(), "lua");
}

#[test]
fn integration_lua_bundle_loads() {
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 2,
        "test.add vtable must have at least 2 functions"
    );
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 3,
        "test.add vtable must have at least 3 functions"
    );
    let mut out_view: StringView = StringView::null();
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "version must return ABI_OK");
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let version_str: &str = core::str::from_utf8(version_bytes).expect("version must be UTF-8");
    assert_eq!(version_str, "1.0.0-lua", "unexpected version string");
}

#[test]
fn integration_lua_reset() {
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 4,
        "test.add vtable must have at least 4 functions"
    );
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
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("noinit_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

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

    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_lua_utf8_roundtrip() {
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const PluginInterface = get_vtable(&rt);
    let vtable: &PluginInterface = unsafe { &*vtable_ptr };
    let mut out_view: StringView = StringView::null();
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK);
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
    let rt: Runtime = create_runtime();
    load_fixture(&rt).expect("first load must succeed");
    let result: Result<(), PolyplugError> = rt.load_bundle(std::path::Path::new(LUA_PLUGIN));
    assert!(
        result.is_ok(),
        "second load should succeed (multi-impl allowed): {:?}",
        result.err()
    );
}
