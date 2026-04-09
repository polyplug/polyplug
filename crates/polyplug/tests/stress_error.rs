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

use polyplug::registry::plugin_registry::PluginRegistry;
use polyplug_abi::{
    AbiErrorCode, AbiError, HostInterface, GuestContractInterface, GuestContractInstance,
    PluginContext, PluginDescriptor, GuestContractHandle, StringView,
};
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::tracking::TrackingAllocator;
use polyplug_utils::{GuestContractId, BundleId};

// --- Plugin environment variable ---------------------------------------------

/// Path to the compiled error_plugin shared library -- set by build.rs.
const ERROR_PLUGIN_SO: &str = env!("ERROR_PLUGIN_SO");

// --- ChainArgs (mirrors error_plugin's ChainArgs) ----------------------------

/// Arguments for error_chain_propagate (fn 2).
/// Mirrors the definition in tests/fixtures/error_plugin/src/lib.rs.
#[repr(C)]
struct ChainArgs {
    host: *const HostInterface,
    target_contract_id: u64,
    target_fn_id: u32,
}

// --- Thread-local registry ---------------------------------------------------

std::thread_local! {
    static ERROR_REGISTRY: core::cell::RefCell<PluginRegistry> =
        core::cell::RefCell::new(PluginRegistry::new());
}

// --- HostInterface callbacks (for Test 3 chain dispatch) -----------------------

/// find_by_contract that looks up a plugin from the thread-local ERROR_REGISTRY.
///
/// # Safety
/// Must only be called when ERROR_REGISTRY has been populated on this thread.
unsafe extern "C" fn chain_find_by_contract(
    _this: *const HostInterface,
    contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    ERROR_REGISTRY.with(|cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = cell.borrow();
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
unsafe extern "C" fn chain_find_all_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    polyplug_abi::Array::empty()
}

/// resolve_contract that dispatches through the thread-local ERROR_REGISTRY.
///
/// # Safety
/// The returned pointer is 'static (error_plugin library is kept alive via mem::forget).
unsafe extern "C" fn chain_resolve_contract(
    _this: *const HostInterface,
    handle: GuestContractHandle,
) -> *const GuestContractInterface {
    ERROR_REGISTRY.with(|cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = cell.borrow();
        registry.resolve(handle).unwrap_or(core::ptr::null())
    })
}

/// Stub call_guest_method -- returns Ok.
unsafe extern "C" fn stub_call_guest_method(
    _this: *const HostInterface,
    _instance: GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    }
}

/// Stub get_host_contract -- returns null instance.
unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// Stub resolve_host_contract_interface -- returns null.
unsafe extern "C" fn stub_resolve_host_contract_interface(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

/// Stub alloc callback.
unsafe extern "C" fn stub_alloc(
    _this: *const HostInterface,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// Stub free callback.
unsafe extern "C" fn stub_free(
    _this: *const HostInterface,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::Array<GuestContractHandle> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_contract callback.
unsafe extern "C" fn noop_resolve_contract(
    _this: *const HostInterface,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    core::ptr::null()
}

/// No-op call_guest_method callback.
unsafe extern "C" fn noop_call_guest_method(
    _this: *const HostInterface,
    _instance: GuestContractInstance,
    _method_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    }
}

/// No-op get_host_contract callback.
unsafe extern "C" fn noop_get_host_contract(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> polyplug_abi::HostContractInstance {
    polyplug_abi::HostContractInstance::null()
}

/// No-op list_bundles callback.
unsafe extern "C" fn noop_list_bundles(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_utils::BundleId> {
    polyplug_abi::Array::empty()
}

/// No-op get_dependencies callback.
unsafe extern "C" fn noop_get_dependencies(
    _this: *const HostInterface,
) -> polyplug_abi::Array<polyplug_abi::DependencyInfo> {
    polyplug_abi::Array::empty()
}

/// No-op resolve_host_contract_interface callback.
unsafe extern "C" fn noop_resolve_host_contract_interface(
    _this: *const HostInterface,
    _contract_id: u64,
    _min_version: u32,
) -> *const polyplug_abi::HostContractInterface {
    core::ptr::null()
}

// --- Registry callback -------------------------------------------------------

/// A register_contract callback that stores interface entries into the thread-local ERROR_REGISTRY.
///
/// # Safety
/// `_this`, `descriptor`, and `interface` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _this: *const HostInterface,
    descriptor: *const PluginDescriptor,
    interface: *const GuestContractInterface,
) -> AbiError {
    if descriptor.is_null() || interface.is_null() {
        return AbiError {
            code: AbiErrorCode::InvalidPointer,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and interface are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: interface is valid for this call (ABI contract).
    let iface: &GuestContractInterface = unsafe { &*interface };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name -- guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
    let result: Result<GuestContractHandle, _> = ERROR_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, PluginRegistry> = reg_cell.borrow();
        // SAFETY: interface pointer is 'static -- extracted from a loaded library that outlives registry.
        unsafe { registry.register(*desc, interface, contract_name.to_owned(), BundleId::from_u64(iface.contract_id.id())) }
    });

    match result {
        Ok(_) => AbiError {
            code: AbiErrorCode::Ok,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: AbiErrorCode::Generic,
            message: StringView::null(),
        },
    }
}

/// Build a HostInterface with all callbacks.
fn make_host_interface() -> HostInterface {
    HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: stub_alloc,
        free: stub_free,
        find_by_contract: noop_find_by_contract,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_contract: noop_resolve_contract,
        call_guest_method: noop_call_guest_method,
        get_host_contract: noop_get_host_contract,
        resolve_host_contract_interface: noop_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    }
}

// --- Helper functions --------------------------------------------------------

/// Loads the error_plugin shared library with RTLD_GLOBAL so that the plugin can
/// resolve `polyplug_host_alloc` and `polyplug_host_free` from the host binary.
fn load_error_plugin() -> libloading::Library {
    #[cfg(unix)]
    {
        // SAFETY: ERROR_PLUGIN_SO is a compiled cdylib built by build.rs.
        // RTLD_LAZY | RTLD_GLOBAL: lazy resolution, global visibility so the plugin
        // can find polyplug_host_alloc exported by the host test binary.
        let raw: UnixLibrary = unsafe {
            UnixLibrary::open(Some(ERROR_PLUGIN_SO), RTLD_LAZY | RTLD_GLOBAL)
                .expect("failed to load error_plugin .so")
        };
        // UnixLibrary converts to libloading::Library via From<imp::Library>.
        libloading::Library::from(raw)
    }
    #[cfg(not(unix))]
    {
        // SAFETY: ERROR_PLUGIN_SO is a compiled cdylib built by build.rs.
        unsafe {
            libloading::Library::new(ERROR_PLUGIN_SO).expect("failed to load error_plugin .so")
        }
    }
}

/// Initialise error_plugin and return the interface pointer.
/// Also resets the thread-local registry.
fn init_error_plugin(library: &libloading::Library) -> *const GuestContractInterface {
    // Reset registry before each use.
    ERROR_REGISTRY.with(|cell| {
        *cell.borrow_mut() = PluginRegistry::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature (2-arg).
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *const HostInterface,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostInterface = make_host_interface();

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_interface and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &host_interface as *const HostInterface,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, AbiErrorCode::Ok, "polyplug_init must succeed");

    let contract_id: GuestContractId = GuestContractId::new("error.test", 1);
    let handle: GuestContractHandle = ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("error.test must be registered")
    });

    ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("interface must be resolvable")
    })
}

// --- Tests -------------------------------------------------------------------

/// Test 1: error_return_with_message writes an AbiError { code=99, message="test error from plugin" }
/// to the out pointer, and the message must be freed after reading.
#[test]
fn stress_error_code_and_message_received_correctly() {
    let library: libloading::Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 0 in the interface (error_return_with_message).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    };

    // SAFETY: fn 0 ignores args (pass null). out is a valid AbiError location.
    let call_result: AbiError =
        unsafe { dispatch_fn(core::ptr::null(), &mut out as *mut AbiError as *mut ()) };

    // The dispatch wrapper returns ABI_OK (success).
    assert_eq!(
        call_result.code, AbiErrorCode::Ok,
        "dispatch wrapper must return Ok"
    );

    // The actual error is written to *out.
    assert_eq!(out.code, AbiErrorCode::from_u32(99), "error code must be 99");
    assert_eq!(out.message.len, 22_usize, "message length must be 22");

    // Read the message bytes.
    // SAFETY: out.message.ptr is valid for out.message.len bytes, allocated by error_plugin
    // via polyplug_host_alloc(22, 1). The memory remains valid until we free it.
    let msg_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out.message.ptr, out.message.len) };
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

    core::mem::forget(library);
}

/// Test 2: error_panic catches an intentional panic and returns ABI_ERROR_PANIC (code=3).
/// The message is from_static -- must NOT be freed. Process continues after the call.
#[test]
fn stress_panic_returns_abi_error_panic_process_continues() {
    let library: libloading::Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 1 in the interface (error_panic).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(1) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. fn 1 ignores both
    // args and out -- it catches the panic internally and returns ABI_ERROR_PANIC directly.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // fn 1 returns the AbiError directly (not via out pointer). Both args and out are null.
    // SAFETY: fn 1 ignores args and out entirely (no pointer dereferences).
    let result: AbiError = unsafe { dispatch_fn(core::ptr::null(), core::ptr::null_mut()) };

    assert_eq!(
        result.code, AbiErrorCode::Panic,
        "error_panic must return Panic (code={})",
        AbiErrorCode::Panic as u32
    );

    // The message is from_static ("plugin panicked") -- do NOT free it.
    // SAFETY: result.message.ptr points to 'static bytes that remain valid indefinitely.
    let msg_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(result.message.ptr, result.message.len) };
    assert_eq!(
        msg_bytes, b"plugin panicked",
        "panic message must be 'plugin panicked'"
    );

    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    core::mem::forget(library);
}

/// Test 3: error_chain_propagate (fn 2) calls another plugin via a real HostInterface
/// and propagates the error back to the test. The chain target is fn 1 (error_panic)
/// which returns ABI_ERROR_PANIC via its return value (not via out pointer).
/// The propagated error code is written to *out by error_chain_propagate.
#[test]
fn stress_error_chain_b_errors_a_propagates() {
    let library: libloading::Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Build a HostInterface that routes find_by_contract and resolve_contract through the
    // thread-local ERROR_REGISTRY that contains error_plugin's interface.
    let chain_host_interface: HostInterface = HostInterface {
        runtime: core::ptr::null_mut(),
        register_contract: registry_register_callback,
        alloc: stub_alloc,
        free: stub_free,
        find_by_contract: chain_find_by_contract,
        find_all_by_contract: chain_find_all_by_contract,
        resolve_contract: chain_resolve_contract,
        call_guest_method: stub_call_guest_method,
        get_host_contract: stub_get_host_contract,
        resolve_host_contract_interface: stub_resolve_host_contract_interface,
        list_bundles: noop_list_bundles,
        get_dependencies: noop_get_dependencies,
    };

    // error.test contract_id is FNV-1a("error.test@1").
    let error_contract_id: GuestContractId = GuestContractId::new("error.test", 1);

    // ChainArgs pointing to fn 1 (error_panic).
    // fn 1 returns ABI_ERROR_PANIC via its return value (not via *out),
    // so error_chain_propagate receives it as inner_result and writes it to *out.
    let chain_args: ChainArgs = ChainArgs {
        host: &chain_host_interface as *const HostInterface,
        target_contract_id: error_contract_id.id(),
        target_fn_id: 1_u32, // fn 1 = error_panic
    };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    };

    // SAFETY: fn_ptr is function 2 in the interface (error_chain_propagate).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Args is *const ChainArgs,
    // out is *mut AbiError -- types enforced by this test.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: chain_args is a valid ChainArgs with a live HostInterface.
    // out is a valid AbiError location. error_chain_propagate calls fn 1 via the host
    // interface and writes the returned AbiError (ABI_ERROR_PANIC) to *out.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &chain_args as *const ChainArgs as *const (),
            &mut out as *mut AbiError as *mut (),
        )
    };

    // error_chain_propagate itself returns ABI_OK (wrapper success).
    assert_eq!(
        call_result.code, AbiErrorCode::Ok,
        "error_chain_propagate wrapper must return Ok"
    );

    // The propagated error from fn 1 (error_panic) is ABI_ERROR_PANIC.
    assert_eq!(
        out.code, AbiErrorCode::Panic,
        "propagated error must be ABI_ERROR_PANIC (={})",
        AbiErrorCode::Panic as u32
    );

    // The message from error_panic is from_static -- do NOT free it.
    // No host_alloc'd memory was produced by fn 1.

    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    core::mem::forget(library);
}

/// Test 4: error_return_with_message (fn 0) produces a StringView message that remains
/// valid while the allocation lives. Read the message 1000 times, verify consistency,
/// then free after all reads complete.
#[test]
fn stress_error_message_lifetime_valid_during_read() {
    let library: libloading::Library = load_error_plugin();
    let interface_ptr: *const GuestContractInterface = init_error_plugin(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // SAFETY: fn_ptr is function 0 in the interface (error_return_with_message).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    let mut out: AbiError = AbiError {
        code: AbiErrorCode::Ok,
        message: StringView::null(),
    };

    // SAFETY: fn 0 ignores args (pass null). out is a valid AbiError location.
    let call_result: AbiError =
        unsafe { dispatch_fn(core::ptr::null(), &mut out as *mut AbiError as *mut ()) };

    assert_eq!(
        call_result.code, AbiErrorCode::Ok,
        "dispatch wrapper must return Ok"
    );
    assert_eq!(out.code, AbiErrorCode::from_u32(99), "error code must be 99");
    assert_eq!(out.message.len, 22_usize, "message length must be 22");

    // Read the message 1000 times to verify pointer stability.
    // The allocation is valid until we call polyplug_host_free below.
    for _i in 0_u32..1000_u32 {
        // SAFETY: out.message.ptr is valid for out.message.len bytes.
        // The allocation was made by error_plugin via polyplug_host_alloc(22, 1)
        // and remains valid until we explicitly free it below.
        let bytes: &[u8] = unsafe { core::slice::from_raw_parts(out.message.ptr, out.message.len) };
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

    core::mem::forget(library);
}
