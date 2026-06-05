#![allow(clippy::expect_used)]

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::guest_contract_id;
use std::sync::Arc;

const LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

fn make_loader() -> LuaLoader {
    LuaLoader::new(LuaConfig::default())
}

fn create_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .loader(make_loader())
        .build()
        .expect("failed to build runtime")
}

fn load_fixture(rt: &Runtime) -> Result<(), RuntimeError> {
    rt.load_bundle(std::path::Path::new(LUA_PLUGIN))
}

fn get_vtable(rt: &Runtime) -> *const GuestContractInterface {
    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load_fixture()");
    rt.resolve_guest_contract(handle)
        .expect("handle must be valid")
}

unsafe fn call_vm_function(
    vtable: &GuestContractInterface,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "expected VM dispatch type"
    );
    // SAFETY: the dispatch_type assertion above proves the active union variant is `vm`, so
    // reading `dispatch.vm.{call,loader_data}` is the correct field of the union. The runtime
    // populates `vm.call` with a non-null loader-provided dispatcher and `vm.loader_data` with
    // the matching loader context during registration; `vtable` is a live borrow held by the
    // caller for the duration of this call, so both remain valid. The `fn_id`, `args`, and `out`
    // pointers are forwarded verbatim under the caller's invariants (see the call sites).
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            core::ptr::null_mut(),
        )
    }
}

#[test]
fn integration_lua_runtime_name() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.runtime_name(), "lua");
}

#[test]
fn integration_lua_bundle_loads() {
    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = load_fixture(&rt);
    assert!(
        result.is_ok(),
        "LuaLoader::load() must succeed for fixture: {:?}",
        result.err()
    );
}

#[test]
fn integration_lua_add() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: `vtable_ptr` comes from `resolve_guest_contract`, which returns a pointer into the
    // registry-owned `GuestContractInterface` for a live handle. The runtime (`rt`) is kept alive
    // for the whole test, so the registry storage backing this pointer outlives the borrow; the
    // pointer is non-null and properly aligned as guaranteed by the registration contract.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "lua loader must use VM dispatch"
    );
    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: `fn_id` 0 is the `add` slot declared by the fixture's `function_count`. `args` points
    // to a live `AddArgs` (`#[repr(C)]`, matching the guest's `add` parameter layout) and `out`
    // points to a live `u32` matching the declared return; both outlive the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            0,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

#[test]
fn integration_lua_add_primitive() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: `vtable_ptr` comes from `resolve_guest_contract`, which returns a pointer into the
    // registry-owned `GuestContractInterface` for a live handle. The runtime (`rt`) is kept alive
    // for the whole test, so the registry storage backing this pointer outlives the borrow; the
    // pointer is non-null and properly aligned as guaranteed by the registration contract.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "lua loader must use VM dispatch"
    );
    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: `fn_id` 1 is the `add_primitive` slot declared by the fixture. `args` points to a
    // live `AddArgs` (`#[repr(C)]`, matching the guest's parameter layout) and `out` points to a
    // live `u32` matching the declared return; both outlive the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            1,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add_primitive must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
}

#[test]
fn integration_lua_version_string() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: `vtable_ptr` comes from `resolve_guest_contract`, which returns a pointer into the
    // registry-owned `GuestContractInterface` for a live handle. The runtime (`rt`) is kept alive
    // for the whole test, so the registry storage backing this pointer outlives the borrow; the
    // pointer is non-null and properly aligned as guaranteed by the registration contract.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "lua loader must use VM dispatch"
    );
    let mut out_view: StringView = StringView::null();
    // SAFETY: `fn_id` 2 is the `version` slot declared by the fixture; it takes no arguments, so a
    // null `args` is the contract-correct value. `out` points to a live `StringView` that the
    // guest fills with a host-allocated UTF-8 view; the binding outlives the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "version must return AbiErrorCode::Ok"
    );
    // SAFETY: the call returned `Ok`, so `out_view` holds a valid (ptr, len) pair into a
    // host-allocated UTF-8 buffer that the StringView ABI guarantees stays alive while the
    // owning runtime is alive. `ptr` is non-null and `len` bytes are initialized and contiguous.
    let version_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
    let version_str: &str = core::str::from_utf8(version_bytes).expect("version must be UTF-8");
    assert_eq!(version_str, "1.0.0-lua", "unexpected version string");
}

#[test]
fn integration_lua_reset() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: `vtable_ptr` comes from `resolve_guest_contract`, which returns a pointer into the
    // registry-owned `GuestContractInterface` for a live handle. The runtime (`rt`) is kept alive
    // for the whole test, so the registry storage backing this pointer outlives the borrow; the
    // pointer is non-null and properly aligned as guaranteed by the registration contract.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "lua loader must use VM dispatch"
    );
    // SAFETY: `fn_id` 3 is the `reset` slot declared by the fixture; it takes no arguments and
    // returns nothing, so null `args` and null `out` are the contract-correct pointers.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            3,
            core::ptr::null::<()>(),
            core::ptr::null_mut::<()>(),
        )
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "reset must return AbiErrorCode::Ok"
    );
}

#[test]
fn integration_lua_init_function_missing_returns_typed_error() {
    let tmp_dir: std::path::PathBuf = std::env::temp_dir().join("noinit_test_bundle");
    std::fs::create_dir_all(&tmp_dir).expect("create temp dir");

    let manifest_content: String = format!(
        r#"
name = "noinit_test"
id = {}
version = "1.0.0"
runtime = "lua"
file = "plugin.lua"
provides = ["test.noinit@1"]

[function_count]
"test.noinit@1" = 1
"#,
        polyplug_utils::bundle_id("noinit_test")
    );
    std::fs::write(tmp_dir.join("manifest.toml"), manifest_content).expect("write manifest");
    std::fs::write(tmp_dir.join("plugin.lua"), b"local x = 1\n").expect("write plugin.lua");

    let rt: Arc<Runtime> = create_runtime();
    let result: Result<(), RuntimeError> = rt.load_bundle(&tmp_dir);
    assert!(result.is_err());
    let err: RuntimeError = result.expect_err("expected Err(InitFailed)");
    assert!(
        matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
        "expected InitFailed for missing polyplug_init, got: {:?}",
        err
    );

    std::fs::remove_dir_all(&tmp_dir).ok();
}

#[test]
fn integration_lua_utf8_roundtrip() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("fixture must load");
    let vtable_ptr: *const GuestContractInterface = get_vtable(&rt);
    // SAFETY: `vtable_ptr` comes from `resolve_guest_contract`, which returns a pointer into the
    // registry-owned `GuestContractInterface` for a live handle. The runtime (`rt`) is kept alive
    // for the whole test, so the registry storage backing this pointer outlives the borrow; the
    // pointer is non-null and properly aligned as guaranteed by the registration contract.
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    let mut out_view: StringView = StringView::null();
    // SAFETY: `fn_id` 2 is the `version` slot declared by the fixture; it takes no arguments, so a
    // null `args` is the contract-correct value. `out` points to a live `StringView` that the
    // guest fills with a host-allocated UTF-8 view; the binding outlives the synchronous call.
    let result: AbiError = unsafe {
        call_vm_function(
            vtable,
            2,
            core::ptr::null::<()>(),
            &mut out_view as *mut StringView as *mut (),
        )
    };
    assert_eq!(result.code, AbiErrorCode::Ok as u32);
    // SAFETY: the call returned `Ok`, so `out_view` holds a valid (ptr, len) pair into a
    // host-allocated UTF-8 buffer that the StringView ABI guarantees stays alive while the
    // owning runtime is alive. `ptr` is non-null and `len` bytes are initialized and contiguous.
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
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("first load must succeed");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(LUA_PLUGIN));
    assert!(
        result.is_ok(),
        "second load should succeed (multi-impl allowed): {:?}",
        result.err()
    );
}
