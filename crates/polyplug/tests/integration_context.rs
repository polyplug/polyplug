#![allow(clippy::expect_used)]

//! Integration test: verify the `BundleInitContext.bundle_path` round-trip for the Rust fixture plugin.
//!
//! Loads the test_plugin shared library, calls `polyplug_init` with a crafted
//! `BundleInitContext` containing a known `bundle_path`, then calls
//! `polyplug_get_last_bundle_path()` and asserts the returned `StringView` matches.
//!
//! This test crate is the crate root for the `integration_context` test binary.

use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;

/// Path to the compiled test_plugin shared library -- set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── No-op host functions ─────────────────────────────────────────────────────

/// No-op register_guest_contract callback -- accepts the registration and returns Ok.
///
/// # Safety
/// `this`, `descriptor`, `interface`, and `out_err` must be valid non-null pointers for
/// the duration of this call (guaranteed by the ABI contract).
unsafe extern "C" fn noop_register(
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

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::GuestContractHandle {
    polyplug_abi::GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<polyplug_abi::GuestContractHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: polyplug_abi::GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostApi,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostApi,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

/// No-op load_bundle callback.
unsafe extern "C" fn noop_load_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut core::ffi::c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// No-op get_last_error callback.
unsafe extern "C" fn noop_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// No-op get_error_len callback.
unsafe extern "C" fn noop_get_error_len(_this: *const HostApi) -> usize {
    0
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostApi,
    _instance: polyplug_abi::GuestContractInstance,
    _fn_id: u32,
    _args: *const core::ffi::c_void,
    _out: *mut core::ffi::c_void,
    _arena: *mut polyplug_abi::CallArena,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: polyplug_utils::BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn rust_plugin_receives_bundle_path() {
    // SAFETY: TEST_PLUGIN_SO is an absolute path to a compiled cdylib.
    // libloading loads it with RTLD_NOW | RTLD_LOCAL semantics.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve the two-arg polyplug_init symbol.
    // SAFETY: polyplug_init is a C function with signature
    //   `unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError`.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Resolve polyplug_get_last_bundle_path symbol.
    // SAFETY: polyplug_get_last_bundle_path is a C function with signature
    //   `unsafe extern "C" fn() -> StringView`.
    let get_path_fn: libloading::Symbol<'_, unsafe extern "C" fn() -> StringView> = unsafe {
        library
            .get(b"polyplug_get_last_bundle_path\0")
            .expect("polyplug_get_last_bundle_path symbol not found")
    };

    // Build a HostApi with no-op callbacks.
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: noop_register,
        alloc: noop_alloc,
        free: noop_free,
        find_guest_contract: noop_find_guest_contract,
        find_all_guest_contracts: noop_find_all_guest_contracts,
        resolve_guest_contract: noop_resolve_guest_contract,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        call_guest_method: noop_call_guest_method,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        reserved: core::ptr::null(),
    };

    // Build a BundleInitContext with a known bundle_path string.
    let bundle_path_str: &str = "/tmp/test_bundle_dir";
    let ctx: BundleInitContext = BundleInitContext {
        bundle_path: StringView {
            ptr: bundle_path_str.as_ptr(),
            len: bundle_path_str.len(),
        },
        bundle_id: 0,
    };

    // Call polyplug_init with the crafted context.
    // SAFETY: init_fn is a valid function pointer. host_interface and ctx are valid
    // stack-allocated values whose lifetimes span this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };

    assert_eq!(
        init_result.code,
        AbiErrorCode::Ok as u32,
        "polyplug_init must return Ok"
    );

    // Call polyplug_get_last_bundle_path to retrieve the stored StringView.
    // SAFETY: get_path_fn is a valid function pointer. The bundle_path_str
    // memory is valid for the duration of this test (stack-allocated above).
    let returned_sv: StringView = unsafe { get_path_fn() };

    // Verify the returned StringView length matches.
    assert_eq!(
        returned_sv.len,
        bundle_path_str.len(),
        "returned StringView.len must match the original bundle_path length"
    );

    // Verify the returned StringView bytes match.
    // SAFETY: returned_sv.ptr points to the same bundle_path_str bytes that
    // were stored by polyplug_init. bundle_path_str is live for the duration
    // of this test. len was just verified to equal bundle_path_str.len().
    let returned_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(returned_sv.ptr, returned_sv.len) };
    assert_eq!(
        returned_bytes,
        bundle_path_str.as_bytes(),
        "returned StringView bytes must match the original bundle_path"
    );

    println!(
        "rust_plugin_receives_bundle_path: bundle_path round-trip verified for {:?}",
        bundle_path_str
    );

    // Leak the library -- keeping interface pointers valid until process exit.
    core::mem::forget(library);
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
