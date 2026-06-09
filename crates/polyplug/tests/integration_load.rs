#![allow(clippy::expect_used)]

//! Integration test: load the test_plugin .so, verify ABI version, verify interface registration.
//!
//! This test crate is the crate root for the `integration_load` test binary.

use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::BundleInitContext;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractId;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::StringView;

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Host functions for integration tests ─────────────────────────────────────

/// register_guest_contract callback that captures the registered interface pointer for inspection.
///
/// # Safety
/// `this`, `descriptor`, and `interface` must be valid non-null pointers for
/// the duration of this call (guaranteed by the ABI contract).
unsafe extern "C" fn capture_register(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: polyplug_abi::AbiErrorCode::Generic as u32,
            message: polyplug_abi::StringView::null(),
        };
    }
    // SAFETY: interface is valid for the call duration. We store the contract_id for
    // later verification. The interface itself lives in the plugin's static memory.
    let contract_id: u64 = unsafe { (*interface).contract_id.id() };
    // SAFETY: interface is valid for this call (ABI contract); reading the function_count.
    let function_count: u32 = unsafe { (*interface).dispatch.native.function_count };

    // Store results in thread-local for the test to read back.
    CAPTURED_CONTRACT_ID.with(|cell| {
        *cell.borrow_mut() = Some(contract_id);
    });
    CAPTURED_FUNCTION_COUNT.with(|cell| {
        *cell.borrow_mut() = Some(function_count);
    });

    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: polyplug_abi::StringView::null(),
    }
}

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    let _ = this;
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    let _ = this;
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    let _ = this;
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback (not used in this test).
unsafe extern "C" fn noop_find_all_guest_contracts(
    this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    let _ = this;
    polyplug_abi::Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    let _ = this;
    core::ptr::null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    let _ = this;
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    this: *const HostApi,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    let _ = this;
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    this: *const HostApi,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    let _ = this;
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
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op reload_bundle callback.
unsafe extern "C" fn noop_reload_bundle(
    _this: *const HostApi,
    _path: *const u8,
    _path_len: usize,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op register_host_contract callback.
unsafe extern "C" fn noop_register_host_contract(
    _this: *const HostApi,
    _interface: *const polyplug_abi::HostContractInterface,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

/// No-op register_loader callback.
unsafe extern "C" fn noop_register_loader(
    _this: *const HostApi,
    _runtime_name: StringView,
    _loader_ptr: *mut core::ffi::c_void,
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
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
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    }
}

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: polyplug_utils::BundleId,
) -> AbiError {
    AbiError::ok()
}

std::thread_local! {
    static CAPTURED_CONTRACT_ID: core::cell::RefCell<Option<u64>> =
        const { core::cell::RefCell::new(None) };
    static CAPTURED_FUNCTION_COUNT: core::cell::RefCell<Option<u32>> =
        const { core::cell::RefCell::new(None) };
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_load_and_abi_version() {
    // SAFETY: TEST_PLUGIN_SO is an absolute path to a compiled cdylib.
    // libloading loads it with RTLD_NOW | RTLD_LOCAL semantics.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve and call polyplug_abi_version
    // SAFETY: polyplug_abi_version is a C function with signature `extern "C" fn() -> u32`.
    let abi_version_fn: libloading::Symbol<'_, unsafe extern "C" fn() -> u32> = unsafe {
        library
            .get(b"polyplug_abi_version\0")
            .expect("polyplug_abi_version symbol not found")
    };

    // SAFETY: symbol was just resolved and is a valid C function pointer.
    let version: u32 = unsafe { abi_version_fn() };
    assert_eq!(version, 1, "polyplug_abi_version() must return 1");

    // Leak the library — interface pointers must remain valid.
    core::mem::forget(library);
}

#[test]
fn test_init_registers_interface() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // Resolve polyplug_init symbol.
    // SAFETY: polyplug_init is a C function with the HostApi ABI (2-arg signature).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    // Build a HostApi that captures registration data.
    let host_interface: HostApi = HostApi {
        runtime: core::ptr::null_mut(),
        register_guest_contract: capture_register,
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
        reserved: core::ptr::null(),
        unload_bundle: noop_unload_bundle,
    };

    // Clear thread-locals before calling init.
    CAPTURED_CONTRACT_ID.with(|cell| *cell.borrow_mut() = None);
    CAPTURED_FUNCTION_COUNT.with(|cell| *cell.borrow_mut() = None);

    let ctx: BundleInitContext = BundleInitContext {
        bundle_id: 0,
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostApi,
            &ctx as *const BundleInitContext,
        )
    };

    assert!(
        result.code == AbiErrorCode::Ok as u32,
        "polyplug_init must return Ok"
    );

    // Verify the interface was registered with correct data.
    let captured_id: u64 = CAPTURED_CONTRACT_ID
        .with(|cell| *cell.borrow())
        .expect("interface was not registered during init");

    let captured_count: u32 = CAPTURED_FUNCTION_COUNT
        .with(|cell| *cell.borrow())
        .expect("function_count was not captured");

    // FNV-1a("test.add@1") = 0xCC4232FAB0410D2B (verified at compile time)
    let expected_contract_id: u64 = GuestContractId::new("test.add", 1).id();
    assert_eq!(
        captured_id, expected_contract_id,
        "contract_id must match FNV-1a(\"test.add@1\")"
    );
    assert_eq!(
        captured_count, 1,
        "test.add interface must have function_count = 1"
    );

    // Leak the library.
    core::mem::forget(library);
}

#[test]
fn test_missing_symbol_returns_error() {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // A non-existent symbol should return Err.
    // SAFETY: library is loaded; querying a symbol name is safe even if it doesn't exist.
    let result: Result<libloading::Symbol<'_, unsafe extern "C" fn()>, _> =
        unsafe { library.get(b"nonexistent_symbol_xyz\0") };
    assert!(result.is_err(), "non-existent symbol must return Err");

    core::mem::forget(library);
}
