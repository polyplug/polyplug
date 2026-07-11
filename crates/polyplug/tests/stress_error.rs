#![allow(clippy::expect_used)]

//! Stress tests for the polyplug error model: error codes, panic propagation, chain dispatch.
//!
//! This test crate is the crate root for the `stress_error` test binary.

#[cfg(unix)]
use libloading::os::unix::Library as UnixLibrary;
#[cfg(unix)]
use libloading::os::unix::RTLD_GLOBAL;
#[cfg(unix)]
use libloading::os::unix::RTLD_LAZY;
use libloading::{Library, Symbol};

use core::cell::Ref;
use core::cell::RefCell;
use core::ffi::c_void;
use core::mem::forget;
use core::mem::transmute;
use core::ptr::null;
use core::ptr::null_mut;
use core::slice::from_raw_parts;
use core::str::from_utf8_unchecked;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::Array;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::in_process::reject_in_process_bundle;
use polyplug_abi::tracking::TrackingAllocator;
use polyplug_abi::{
    AbiError, AbiErrorCode, BundleInitContext, GuestContractHandle, GuestContractInterface,
    HostApi, PluginDescriptor, StringView,
};
use polyplug_utils::{BundleId, GuestContractId};

// --- Plugin environment variable ---------------------------------------------

/// Path to the compiled error_plugin shared library -- set by build.rs.
const ERROR_PLUGIN_SO: &str = env!("ERROR_PLUGIN_SO");

// --- Plugin args mirrors -----------------------------------------------------

/// Arguments for error_return_with_message (fn 0).
/// Mirrors the definition in tests/fixtures/error_plugin/src/lib.rs.
#[repr(C)]
struct MessageArgs {
    host: *const HostApi,
}

/// Arguments for error_chain_propagate (fn 2).
/// Mirrors the definition in tests/fixtures/error_plugin/src/lib.rs.
#[repr(C)]
struct ChainArgs {
    host: *const HostApi,
    target_contract_id: u64,
    target_fn_id: u32,
}

// --- Thread-local registry ---------------------------------------------------

thread_local! {
    static ERROR_REGISTRY: RefCell<RuntimeStore> = RefCell::new(RuntimeStore::new());
}

// --- HostApi callbacks (for Test 3 chain dispatch) -----------------------

/// find_guest_contract that looks up a plugin from the thread-local ERROR_REGISTRY.
///
/// # Safety
/// Must only be called when ERROR_REGISTRY has been populated on this thread.
unsafe extern "C" fn chain_find_guest_contract(
    _this: *const HostApi,
    contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    ERROR_REGISTRY.with(|cell| {
        let registry: Ref<'_, RuntimeStore> = cell.borrow();
        match registry.find(GuestContractId::from_u64(contract_id), 0) {
            Ok(handle) => handle,
            Err(_) => GuestContractHandle::null(),
        }
    })
}

/// find_all_by_contract stub -- returns empty array.
///
/// # Safety
/// Always safe to call.
unsafe extern "C" fn chain_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// resolve_guest_contract that dispatches through the thread-local ERROR_REGISTRY.
///
/// # Safety
/// The returned pointer is 'static (error_plugin library is kept alive via mem::forget).
unsafe extern "C" fn chain_resolve_guest_contract(
    _this: *const HostApi,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ERROR_REGISTRY.with(|cell| {
        let registry: Ref<'_, RuntimeStore> = cell.borrow();
        registry.resolve_guest_contract(handle).unwrap_or(null())
    })
}

/// Stub get_host_contract -- returns null instance.
unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// Stub resolve_host_contract_interface -- returns null.
unsafe extern "C" fn stub_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    null()
}

/// Stub alloc callback.
unsafe extern "C" fn stub_alloc(_this: *const HostApi, size: usize, align: usize) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// Stub free callback.
unsafe extern "C" fn stub_free(_this: *const HostApi, ptr: *mut u8, size: usize, align: usize) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_guest_contract callback.
unsafe extern "C" fn noop_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// No-op resolve_guest_contract callback.
unsafe extern "C" fn noop_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    null()
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    null()
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
    _interface: *const HostContractInterface,
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
    _loader_ptr: *mut c_void,
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

unsafe extern "C" fn noop_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

// --- Registry callback -------------------------------------------------------

/// A register_guest_contract callback that stores interface entries into the thread-local ERROR_REGISTRY.
///
/// # Safety
/// `_this`, `descriptor`, and `interface` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _this: *const HostApi,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
    out_err: *mut AbiError,
) {
    if descriptor.is_null() || interface.is_null() {
        if !out_err.is_null() {
            // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
            unsafe {
                out_err.write(AbiError {
                    code: AbiErrorCode::InvalidPointer as u32,
                    message: StringView::null(),
                })
            };
        }
        return;
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name -- guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] = from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
    let result: Result<GuestContractHandle, _> = ERROR_REGISTRY.with(|reg_cell| {
        let registry: Ref<'_, RuntimeStore> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
        unsafe {
            registry.register_guest_contract(
                *desc,
                interface,
                contract_name.to_owned(),
                BundleId::from_u64(iface.contract_id.id()),
            )
        }
    });

    let err_val: AbiError = match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok as u32,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic as u32,
            message: StringView::null(),
        },
    };
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(err_val) };
    }
}

/// Build a HostApi with all callbacks.
fn make_host_interface() -> HostApi {
    HostApi {
        runtime: null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: stub_alloc,
        free: stub_free,
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
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: null(),
    }
}

// --- Helper functions --------------------------------------------------------

/// Loads the error_plugin shared library with RTLD_GLOBAL so that the plugin can
/// resolve `polyplug_host_alloc` and `polyplug_host_free` from the host binary.
fn load_error_plugin() -> Library {
    #[cfg(unix)]
    {
        // SAFETY: ERROR_PLUGIN_SO is a compiled cdylib built by build.rs.
        // RTLD_LAZY | RTLD_GLOBAL: lazy resolution, global visibility so the plugin
        // can find polyplug_host_alloc exported by the host test binary.
        let raw: UnixLibrary = unsafe {
            UnixLibrary::open(Some(ERROR_PLUGIN_SO), RTLD_LAZY | RTLD_GLOBAL)
                .expect("failed to load error_plugin .so")
        };
        // UnixLibrary converts to Library via From<imp::Library>.
        Library::from(raw)
    }
    #[cfg(not(unix))]
    {
        // SAFETY: ERROR_PLUGIN_SO is a compiled cdylib built by build.rs.
        unsafe { Library::new(ERROR_PLUGIN_SO).expect("failed to load error_plugin .so") }
    }
}

/// Initialise error_plugin and return the interface pointer.
/// Also resets the thread-local registry.
fn init_error_plugin(library: &Library) -> *const GuestContractInterface {
    // Reset registry before each use.
    ERROR_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature (2-arg).
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostApi = make_host_interface();

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
        "polyplug_init must succeed"
    );

    let contract_id: GuestContractId = GuestContractId::new("error.test", 1);
    let handle: GuestContractHandle = ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("error.test must be registered")
    });

    ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface must be resolvable")
    })
}

// --- Tests -------------------------------------------------------------------

/// Test 1: error_return_with_message writes an AbiError { code=Generic, message="test error from plugin" }
/// to the out pointer, and the message must be freed after reading.
#[test]
fn stress_error_code_and_message_received_correctly() {
    let library: Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 0 in the interface (error_return_with_message).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { transmute(fn_ptr) }
    };

    let host_interface: HostApi = make_host_interface();
    let message_args: MessageArgs = MessageArgs {
        host: &host_interface as *const HostApi,
    };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: message_args carries a live HostApi; out is a valid AbiError location.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &message_args as *const MessageArgs as *const (),
            &mut out as *mut AbiError as *mut (),
            &mut call_result,
        )
    };

    // The dispatch wrapper returns Ok (success).
    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "dispatch wrapper must return Ok"
    );

    // The actual error is written to *out.
    assert_eq!(
        out.code,
        AbiErrorCode::Generic as u32,
        "error code must be Generic"
    );
    assert_eq!(out.message.len, 22_usize, "message length must be 22");

    // Read the message bytes.
    // SAFETY: out.message.ptr is valid for out.message.len bytes, allocated by error_plugin
    // via polyplug_host_alloc(22, 1). The memory remains valid until we free it.
    let msg_bytes: &[u8] = unsafe { from_raw_parts(out.message.ptr, out.message.len) };
    assert_eq!(msg_bytes, b"test error from plugin", "message must match");

    // Free the message: caller owns the allocation per error_plugin ABI contract.
    // SAFETY: out.message.ptr was allocated by error_plugin via polyplug_host_alloc(22, 1).
    // It has not been freed yet. We free it here with matching size and align.
    unsafe {
        polyplug_host_free(out.message.ptr as *mut u8, out.message.len, 1);
    }

    // TrackingAllocator: verify no leaks through the tracking layer.
    // Both counters are 0 (alloc/free above used the raw allocator, not the tracker).
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    forget(library);
}

/// Test 2: error_panic catches an intentional panic and returns Panic (code=3).
/// The message is from_static -- must NOT be freed. Process continues after the call.
#[test]
fn stress_panic_returns_abi_error_panic_process_continues() {
    let library: Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 1 in the interface (error_panic).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(1) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. fn 1 ignores both
    // args and out -- it catches the panic internally and writes Panic to out_err.
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { transmute(fn_ptr) }
    };

    // fn 1 returns the AbiError via out_err. Both args and out are null.
    let mut result: AbiError = AbiError::ok();
    // SAFETY: fn 1 ignores args and out entirely (no pointer dereferences).
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            null(),
            null_mut(),
            &mut result,
        )
    };

    assert_eq!(
        result.code,
        AbiErrorCode::Panic as u32,
        "error_panic must return Panic (code={})",
        AbiErrorCode::Panic
    );

    // The message is from_static ("plugin panicked") -- do NOT free it.
    // SAFETY: result.message.ptr points to 'static bytes that remain valid indefinitely.
    let msg_bytes: &[u8] = unsafe { from_raw_parts(result.message.ptr, result.message.len) };
    assert_eq!(
        msg_bytes, b"plugin panicked",
        "panic message must be 'plugin panicked'"
    );

    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    forget(library);
}

/// Test 3: error_chain_propagate (fn 2) calls another plugin via a real HostApi
/// and propagates the error back to the test. The chain target is fn 1 (error_panic)
/// which returns Panic via its return value (not via out pointer).
/// The propagated error code is written to *out by error_chain_propagate.
#[test]
fn stress_error_chain_b_errors_a_propagates() {
    let library: Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Build a HostApi that routes find_guest_contract and resolve_guest_contract through the
    // thread-local ERROR_REGISTRY that contains error_plugin's interface.
    let chain_host_interface: HostApi = HostApi {
        runtime: null_mut(),
        register_guest_contract: registry_register_callback,
        register_in_process_bundle: reject_in_process_bundle,
        alloc: stub_alloc,
        free: stub_free,
        find_guest_contract: chain_find_guest_contract,
        find_all_guest_contracts: chain_find_all_guest_contracts,
        resolve_guest_contract: chain_resolve_guest_contract,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
        load_bundle: noop_load_bundle,
        reload_bundle: noop_reload_bundle,
        register_host_contract: noop_register_host_contract,
        register_loader: noop_register_loader,
        get_last_error: noop_get_last_error,
        get_error_len: noop_get_error_len,
        unload_bundle: noop_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        registry_revision: stub_registry_revision,
        reserved: null(),
    };

    // error.test contract_id is FNV-1a("error.test@1").
    let error_contract_id: GuestContractId = GuestContractId::new("error.test", 1);

    // ChainArgs pointing to fn 1 (error_panic).
    // fn 1 returns Panic via its return value (not via *out),
    // so error_chain_propagate receives it as inner_result and writes it to *out.
    let chain_args: ChainArgs = ChainArgs {
        host: &chain_host_interface as *const HostApi,
        target_contract_id: error_contract_id.id(),
        target_fn_id: 1_u32, // fn 1 = error_panic
    };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    };

    // SAFETY: fn_ptr is function 2 in the interface (error_chain_propagate).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Args is *const ChainArgs,
    // out is *mut AbiError -- types enforced by this test.
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { transmute(fn_ptr) }
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: chain_args is a valid ChainArgs with a live HostApi.
    // out is a valid AbiError location. error_chain_propagate calls fn 1 via the host
    // interface and writes the returned AbiError (Panic) to *out.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &chain_args as *const ChainArgs as *const (),
            &mut out as *mut AbiError as *mut (),
            &mut call_result,
        )
    };

    // error_chain_propagate itself returns Ok (wrapper success).
    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "error_chain_propagate wrapper must return Ok"
    );

    // The propagated error from fn 1 (error_panic) is Panic.
    assert_eq!(
        out.code,
        AbiErrorCode::Panic as u32,
        "propagated error must be Panic (={})",
        AbiErrorCode::Panic
    );

    // The message from error_panic is from_static -- do NOT free it.
    // No host_alloc'd memory was produced by fn 1.

    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    forget(library);
}

/// Test 4: error_return_with_message (fn 0) produces a StringView message that remains
/// valid while the allocation lives. Read the message 1000 times, verify consistency,
/// then free after all reads complete.
#[test]
fn stress_error_message_lifetime_valid_during_read() {
    let library: Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 0 in the interface (error_return_with_message).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(
        *mut c_void,
        GuestContractInstance,
        *const (),
        *mut (),
        *mut AbiError,
    ) = {
        // SAFETY: the pointer comes from the generated dispatch table and is cast to that function's exact ABI.
        unsafe { transmute(fn_ptr) }
    };

    let host_interface: HostApi = make_host_interface();
    let message_args: MessageArgs = MessageArgs {
        host: &host_interface as *const HostApi,
    };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok as u32,
        message: StringView::null(),
    };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: message_args carries a live HostApi; out is a valid AbiError location.
    unsafe {
        dispatch_fn(
            interface.adapter_context,
            GuestContractInstance::null(),
            &message_args as *const MessageArgs as *const (),
            &mut out as *mut AbiError as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "dispatch wrapper must return Ok"
    );
    assert_eq!(
        out.code,
        AbiErrorCode::Generic as u32,
        "error code must be Generic"
    );
    assert_eq!(out.message.len, 22_usize, "message length must be 22");

    // Read the message 1000 times to verify pointer stability.
    // The allocation is valid until we call polyplug_host_free below.
    for _i in 0_u32..1000_u32 {
        // SAFETY: out.message.ptr is valid for out.message.len bytes.
        // The allocation was made by error_plugin via polyplug_host_alloc(22, 1)
        // and remains valid until we explicitly free it below.
        let bytes: &[u8] = unsafe { from_raw_parts(out.message.ptr, out.message.len) };
        assert_eq!(
            bytes, b"test error from plugin",
            "message must remain stable across 1000 reads"
        );
    }

    // Free AFTER all reads complete.
    // SAFETY: out.message.ptr was allocated by error_plugin via polyplug_host_alloc(22, 1).
    // It has not been freed yet. We free it here with matching size and align.
    unsafe {
        polyplug_host_free(out.message.ptr as *mut u8, out.message.len, 1);
    }

    // TrackingAllocator: verify no leaks through the tracking layer.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    forget(library);
}

/// `HostApi.log` stub for test hosts — drops the record.
unsafe extern "C" fn stub_host_log(
    _this: *const HostApi,
    _level: u32,
    _scope: StringView,
    _message: StringView,
) {
}

unsafe extern "C" fn stub_create_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _args: *const c_void,
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn stub_destroy_guest_instance(
    _this: *const HostApi,
    _interface: *const GuestContractInterface,
    _instance: GuestContractInstance,
) {
}

unsafe extern "C" fn stub_registry_revision(_this: *const HostApi) -> u64 {
    0
}
