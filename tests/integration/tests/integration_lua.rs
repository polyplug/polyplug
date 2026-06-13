#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use core::sync::atomic::AtomicU64;
use core::sync::atomic::Ordering;
use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::Array;
use polyplug_abi::CallArena;
use polyplug_abi::DependencyInfo;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_utils::BundleId;
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
    let mut err: AbiError = AbiError::ok();
    unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            GuestContractInstance::null(),
            fn_id,
            args,
            out,
            core::ptr::null_mut(),
            &mut err as *mut AbiError,
        )
    };
    err
}

#[test]
fn integration_lua_loader_name() {
    let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
    assert_eq!(loader.loader_name(), "lua");
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
loader = "lua"
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

/// Loading the same bundle twice without an unload or reload in between must be
/// rejected: the second registration is a same-bundle duplicate of the same
/// contract (DuplicateProvider). The old "second load succeeds" behaviour left a
/// stale duplicate registration that `find` kept resolving to the first VM.
/// Legitimate re-loading goes through `unload_bundle` + `load_bundle` or
/// `reload_bundle`, both covered by their own tests.
#[test]
fn integration_lua_second_load_rejected_as_duplicate() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("first load must succeed");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(LUA_PLUGIN));
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::InitFailed { .. }))
        ),
        "second load of the same bundle must fail as a duplicate registration, got: {:?}",
        result.as_ref().err()
    );
}

// ─── Call-arena end-to-end (string-returning Lua guest) ────────────────────────

static ARENA_HOST_ALLOC_CALLS: AtomicU64 = AtomicU64::new(0);

unsafe extern "C" fn counting_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    ARENA_HOST_ALLOC_CALLS.fetch_add(1, Ordering::SeqCst);
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    unsafe { std::alloc::alloc(layout) }
}

unsafe extern "C" fn counting_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    let layout: core::alloc::Layout =
        core::alloc::Layout::from_size_align(size, align).expect("valid layout");
    unsafe { std::alloc::dealloc(ptr, layout) }
}

unsafe extern "C" fn arena_stub_register_guest(
    _this: *const HostApi,
    _descriptor: *const PluginDescriptor,
    _interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_find(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

unsafe extern "C" fn arena_stub_find_all(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_resolve_guest(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn arena_stub_get_host_contract(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

unsafe extern "C" fn arena_stub_resolve_host_interface(
    _this: *const HostApi,
    _id: u64,
    _ver: u32,
) -> *const HostContractInterface {
    core::ptr::null()
}

unsafe extern "C" fn arena_stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_get_deps(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

unsafe extern "C" fn arena_stub_load(
    _this: *const HostApi,
    _p: *const u8,
    _l: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_register_host(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_register_loader(
    _this: *const HostApi,
    _loader: *mut core::ffi::c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _len: usize,
) -> usize {
    0
}

unsafe extern "C" fn arena_stub_get_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn arena_stub_call_guest_method(
    _this: *const HostApi,
    _instance: GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut CallArena,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn arena_stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Build a HostApi whose allocator counts calls — only `alloc`/`free` are ever
/// exercised by the arena; the remaining fields are non-null stubs so the struct
/// is a valid HostApi.
fn counting_host() -> HostApi {
    HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: arena_stub_register_guest,
        alloc: counting_alloc,
        free: counting_free,
        find_guest_contract: arena_stub_find,
        find_all_guest_contracts: arena_stub_find_all,
        resolve_guest_contract: arena_stub_resolve_guest,
        get_host_contract: arena_stub_get_host_contract,
        resolve_host_contract_interface: arena_stub_resolve_host_interface,
        list_bundles: arena_stub_list_bundles,
        get_dependencies: arena_stub_get_deps,
        load_bundle: arena_stub_load,
        reload_bundle: arena_stub_load,
        register_host_contract: arena_stub_register_host,
        register_loader: arena_stub_register_loader,
        get_last_error: arena_stub_get_last_error,
        get_error_len: arena_stub_get_len,
        call_guest_method: arena_stub_call_guest_method,
        unload_bundle: arena_stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        reserved: core::ptr::null(),
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ArenaStringView {
    ptr: *const u8,
    len: usize,
}

impl ArenaStringView {
    fn read_string(self) -> String {
        if self.len == 0 || self.ptr.is_null() {
            return String::new();
        }
        // SAFETY: the view points into the call arena, valid until the next reset.
        let slice: &[u8] = unsafe { core::slice::from_raw_parts(self.ptr, self.len) };
        String::from_utf8_lossy(slice).into_owned()
    }
}

/// Repeatedly call the string-returning Lua `echo` function through a real
/// CallArena and assert: values are correct, a view from call N stays readable
/// until call N+1, and the host allocator is hit zero times per call after warmup
/// (alloc_string_arena serves small strings from the bump region).
#[test]
fn lua_echo_uses_call_arena() {
    let rt: Arc<Runtime> = create_runtime();
    load_fixture(&rt).expect("LuaLoader::load() failed");

    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };

    // The host buffer the guest reads its input from must outlive each call.
    let input: &[u8] = b"hello arena";
    let input_view: ArenaStringView = ArenaStringView {
        ptr: input.as_ptr(),
        len: input.len(),
    };

    let host: HostApi = counting_host();
    let mut arena_buf: [u8; 512] = [0; 512];
    let mut arena: CallArena = CallArena::new(&mut arena_buf, &host);

    const ECHO_FN_ID: u32 = 4;
    const ITERATIONS: usize = 10_000;
    const WARMUP: usize = 8;

    let mut prev_view: Option<ArenaStringView> = None;

    for i in 0..ITERATIONS {
        // Reset at call start: the previous call's view stays valid right up to
        // here, so verify it before reusing the arena region.
        if let Some(prev) = prev_view.take() {
            assert_eq!(
                prev.read_string(),
                "hello arena",
                "view from the previous call must read correctly until reset (iter {i})"
            );
        }
        arena.reset();

        let calls_before: u64 = ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst);

        let mut out: ArenaStringView = ArenaStringView {
            ptr: core::ptr::null(),
            len: 0,
        };
        let mut err: AbiError = AbiError::ok();
        unsafe {
            (vtable.dispatch.vm.call)(
                vtable.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                ECHO_FN_ID,
                &input_view as *const ArenaStringView as *const (),
                &mut out as *mut ArenaStringView as *mut (),
                &mut arena as *mut CallArena,
                &mut err as *mut AbiError,
            )
        };
        assert_eq!(
            err.code,
            AbiErrorCode::Ok as u32,
            "echo must return Ok (iter {i})"
        );
        assert_eq!(out.read_string(), "hello arena", "echo value (iter {i})");

        let calls_after: u64 = ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst);
        if i >= WARMUP {
            assert_eq!(
                calls_after, calls_before,
                "after warmup, a small-string echo must not hit the host allocator (iter {i})"
            );
        }

        prev_view = Some(out);
    }

    // Free everything the arena owns before it drops.
    arena.reset();

    assert_eq!(
        ARENA_HOST_ALLOC_CALLS.load(Ordering::SeqCst),
        0,
        "the 512-byte arena region serves every 11-byte echo with zero host allocations"
    );
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _args: *const core::ffi::c_void,
    out_instance: *mut polyplug_abi::GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(polyplug_abi::GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const polyplug_abi::HostApi,
    _interface: *const polyplug_abi::GuestContractInterface,
    _instance: polyplug_abi::GuestContractInstance,
) {
}
