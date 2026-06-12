//! Cross-language test matrix: 6 host languages × 6 guest languages = 36 tests.
//!
//! All tests dispatch `add(3, 5)` and assert the result equals 8.
//! Tests skip gracefully when a required toolchain is unavailable.
//!
//! Host/guest language mapping:
//!   Hosts: Rust, C++, C#, Python, Lua, js-quickjs
//!   Guests: Rust, C++, C#, Python, Lua, js-quickjs
//!
//! Since this is a Rust test harness, ALL 36 tests use the same underlying
//! Rust interface dispatch. The "host" label indicates which host language would
//! typically call this guest in production. The difference between tests for
//! the same guest is only the label — all share the same load + dispatch logic.

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::runtime::Runtime;
use polyplug_abi::Array;
use polyplug_abi::BundleInitContext;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;
use polyplug_abi::{AbiError, AbiErrorCode};
use polyplug_dotnet::DotnetConfig;
use polyplug_dotnet::DotnetLoader;
use polyplug_dotnet::HostfxrLocation;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_lua::LuaConfig;
use polyplug_lua::LuaLoader;
use polyplug_python::PythonConfig;
use polyplug_python::PythonLoader;
use polyplug_utils::BundleId;
use polyplug_utils::guest_contract_id;
use std::path::Path;
use std::sync::Arc;

// ─── Test fixture paths ───────────────────────────────────────────────────────
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");
const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");
const TEST_CSHARP_PLUGIN_DLL: &str = env!("TEST_CSHARP_PLUGIN_DLL");
const TEST_PYTHON_PLUGIN: &str = env!("TEST_PYTHON_PLUGIN");
const TEST_LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");
const TEST_JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

// ─── Compile-time availability flags ─────────────────────────────────────────

const SKIP_DOTNET: bool = {
    let a: &[u8] = TEST_CSHARP_PLUGIN_DLL.as_bytes();
    let b: &[u8] = b"DOTNET_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

const SKIP_PYTHON: bool = {
    let a: &[u8] = TEST_PYTHON_PLUGIN.as_bytes();
    let b: &[u8] = b"PYTHON_NOT_AVAILABLE";
    if a.len() != b.len() {
        false
    } else {
        let mut i: usize = 0;
        let mut eq: bool = true;
        while i < a.len() {
            if a[i] != b[i] {
                eq = false;
            }
            i += 1;
        }
        eq
    }
};

// ─── Shared struct ────────────────────────────────────────────────────────────

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Thread-local for native .so tests ────────────────────────────────────────

std::thread_local! {
    static CAPTURED_INTERFACE: core::cell::Cell<*const GuestContractInterface> =
        const { core::cell::Cell::new(core::ptr::null()) };
}

// ─── HostApi callbacks ──────────────────────────────────────────────────────

/// Registration callback for native .so guests (Rust, C++) via libloading.
/// Captures the interface pointer into a thread-local cell for later dispatch.
///
/// # Safety
/// `interface` must be valid for the call duration and remain valid as long as the
/// loaded library is live (caller must use `core::mem::forget` on the Library).
unsafe extern "C" fn capture_interface_cb(
    _this: *const HostApi,
    _desc: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    CAPTURED_INTERFACE.with(|cell| cell.set(interface));
    AbiError::ok()
}

// ─── HostApi stub functions for native .so tests ─────────────────────────────

/// Stub alloc callback using the global allocator.
unsafe extern "C" fn stub_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// Stub free callback using the global allocator.
unsafe extern "C" fn stub_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// Stub find_guest_contract — returns a null handle.
unsafe extern "C" fn stub_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// Stub find_all_guest_contracts — returns empty array.
unsafe extern "C" fn stub_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// Stub resolve_guest_contract — returns null.
unsafe extern "C" fn stub_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// Stub get_host_contract — returns null instance.
unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// Stub resolve_host_contract_interface — returns null.
unsafe extern "C" fn stub_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    core::ptr::null()
}

/// Stub list_bundles — returns empty array.
unsafe extern "C" fn stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// Stub get_dependencies — returns empty array.
unsafe extern "C" fn stub_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// Stub load_bundle — returns success.
unsafe extern "C" fn stub_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError::ok()
}

/// Stub reload_bundle — returns success.
unsafe extern "C" fn stub_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError::ok()
}

/// Stub register_host_contract — returns success.
unsafe extern "C" fn stub_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
) -> AbiError {
    AbiError::ok()
}

/// Stub register_loader — returns success.
unsafe extern "C" fn stub_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut core::ffi::c_void,
) -> AbiError {
    AbiError::ok()
}

/// Stub get_last_error — returns 0 bytes written.
unsafe extern "C" fn stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// Stub get_error_len — returns 0.
unsafe extern "C" fn stub_get_error_len(_this: *const HostApi) -> usize {
    0
}

/// Stub call_guest_method — returns success.
unsafe extern "C" fn stub_call_guest_method(
    _this: *const HostApi,
    _instance: GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut polyplug_abi::CallArena,
) -> AbiError {
    AbiError::ok()
}

unsafe extern "C" fn stub_unload_bundle(_this: *const HostApi, _bundle_id: BundleId) -> AbiError {
    AbiError::ok()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Retrieve interface for `test.add@1` from a Runtime instance.
fn get_interface_from_runtime(runtime: &Runtime) -> *const GuestContractInterface {
    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    runtime
        .resolve_guest_contract(handle)
        .expect("handle must be valid")
}

/// Dispatch add(3, 5) and verify the result equals 8.
fn dispatch_add_and_verify(interface_ptr: *const GuestContractInterface) {
    use polyplug_abi::DispatchType;
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr is valid for the call.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    let result: AbiError = if interface.dispatch_type == DispatchType::Native {
        // SAFETY: functions[0] is the add wrapper.
        let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
        // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
        let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
            unsafe { core::mem::transmute(fn_ptr) };
        // SAFETY: args valid AddArgs; out valid u32 location.
        unsafe {
            dispatch_fn(
                &args as *const AddArgs as *const (),
                &mut out as *mut u32 as *mut (),
            )
        }
    } else {
        // SAFETY: dispatch_type is VirtualMachine, so .vm is valid.
        unsafe {
            (interface.dispatch.vm.call)(
                interface.dispatch.vm.loader_data,
                GuestContractInstance::null(),
                0, // fn_id = 0 for add
                &args as *const AddArgs as *const (),
                &mut out as *mut u32 as *mut (),
                core::ptr::null_mut(),
            )
        }
    };
    assert_eq!(
        result.code,
        AbiErrorCode::Ok as u32,
        "add must return AbiErrorCode::Ok"
    );
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}

// ─────────────────────────────────────────────────────────────────────────────
// RUST GUEST (all 6 host labels)
// For Rust guests: load via libloading + polyplug_init, capture interface.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper with signature extern "C" fn(*const (), *mut ()) -> AbiError.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is transmuted to the frozen native dispatch signature; AddArgs matches the add function.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args is a valid AddArgs; out is a valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_cpp_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to the frozen native dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_csharp_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to the frozen native dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_python_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to the frozen native dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_lua_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to the frozen native dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_js_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_SO is empty (Rust plugin not built)");
        return;
    }
    if !Path::new(TEST_PLUGIN_SO).exists() {
        println!("skipping: TEST_PLUGIN_SO path does not exist: {TEST_PLUGIN_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load Rust test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in Rust plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the `add` wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to the frozen native dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location; null stateless instance.
    let result: AbiError = unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

// ─────────────────────────────────────────────────────────────────────────────
// C++ GUEST (all 6 host labels)
// For C++ guests: load via libloading + polyplug_init, capture interface.
// Skip if TEST_PLUGIN_CPP_SO is empty (g++ unavailable).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin built by build.rs.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches cpp_test_add.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_cpp_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_csharp_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_python_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_lua_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

#[test]
fn test_js_host_cpp_guest() {
    if TEST_PLUGIN_CPP_SO.is_empty() {
        println!("skipping: TEST_PLUGIN_CPP_SO is empty (g++ not available)");
        return;
    }
    if !Path::new(TEST_PLUGIN_CPP_SO).exists() {
        println!("skipping: TEST_PLUGIN_CPP_SO path does not exist: {TEST_PLUGIN_CPP_SO}");
        return;
    }
    CAPTURED_INTERFACE.with(|cell| cell.set(core::ptr::null()));
    // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib C++ test plugin.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("failed to load C++ test plugin .so")
    };
    // SAFETY: symbol matches expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found in C++ plugin")
    };
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_interface_cb,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: stub_find_guest_contract,
        find_all_guest_contracts: stub_find_all_guest_contracts,
        resolve_guest_contract: stub_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: stub_list_bundles,
        get_dependencies: stub_get_dependencies,
        load_bundle: stub_load_bundle,
        reload_bundle: stub_reload_bundle,
        register_host_contract: stub_register_host_contract,
        register_loader: stub_register_loader,
        get_last_error: stub_get_last_error,
        get_error_len: stub_get_error_len,
        call_guest_method: stub_call_guest_method,
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        reserved: core::ptr::null(),
    };
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };
    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return AbiErrorCode::Ok"
    );
    let interface_ptr: *const GuestContractInterface = CAPTURED_INTERFACE.with(|cell| cell.get());
    assert!(
        !interface_ptr.is_null(),
        "interface must be non-null after polyplug_init"
    );
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid — library kept alive via forget below.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the cpp_test_add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

// ─────────────────────────────────────────────────────────────────────────────
// C# GUEST (all 6 host labels)
// Skip if DOTNET_NOT_AVAILABLE. Use DotnetLoader with Runtime.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_cpp_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_csharp_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = runtime
        .find_guest_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let interface_ptr: *const GuestContractInterface = runtime
        .resolve_guest_contract(handle)
        .expect("handle must be valid");
    assert!(!interface_ptr.is_null(), "interface must be non-null");
    let args: AddArgs = AddArgs { a: 3_u32, b: 5_u32 };
    let mut out: u32 = 0_u32;
    // SAFETY: interface_ptr valid; CLR keeps assembly loaded for process lifetime.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };
    // SAFETY: functions[0] is the add wrapper.
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr transmuted to generic dispatch signature; AddArgs matches.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    // SAFETY: args valid AddArgs; out valid u32 location.
    let result: AbiError = unsafe {
        dispatch_fn(
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
fn test_python_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_lua_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_js_host_csharp_guest() {
    if SKIP_DOTNET {
        println!("skipping: dotnet not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(DotnetLoader::new(DotnetConfig {
            min_framework: String::from("net10.0"),
            hostfxr: HostfxrLocation::Auto,
        }))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> = runtime.load_bundle(
        Path::new(TEST_CSHARP_PLUGIN_DLL)
            .parent()
            .unwrap_or(Path::new(".")),
    );
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

// ─────────────────────────────────────────────────────────────────────────────
// PYTHON GUEST (all 6 host labels)
// Skip if PYTHON_NOT_AVAILABLE. Use PythonLoader with Runtime.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_cpp_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_csharp_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_python_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_lua_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_js_host_python_guest() {
    if SKIP_PYTHON {
        println!("skipping: python not available");
        return;
    }
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(PythonLoader::new(PythonConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_PYTHON_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

// ─────────────────────────────────────────────────────────────────────────────
// LUA GUEST (all 6 host labels)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_cpp_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_csharp_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_python_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_lua_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_js_host_lua_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(LuaLoader::new(LuaConfig::default()))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_LUA_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

// ─────────────────────────────────────────────────────────────────────────────
// JS GUEST (all 6 host labels)
// Use JsLoader with Runtime + process mutex (single-threaded JS VM).
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_rust_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_cpp_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_csharp_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_python_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_lua_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

#[test]
fn test_js_host_js_guest() {
    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let load_result: Result<(), polyplug::error::RuntimeError> =
        runtime.load_bundle(Path::new(TEST_JS_PLUGIN));
    assert!(
        load_result.is_ok(),
        "Runtime::load_bundle failed: {:?}",
        load_result.err()
    );
    let interface_ptr: *const GuestContractInterface = get_interface_from_runtime(&runtime);
    dispatch_add_and_verify(interface_ptr);
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const polyplug_abi::HostApi,
    _level: u32,
    _scope: polyplug_abi::StringView,
    _message: polyplug_abi::StringView,
) {
}
