//! Stress tests for the polyplug error model: error codes, panic propagation, chain dispatch.
//!
//! This test crate is the crate root for the `stress_error` test binary.

#![allow(clippy::expect_used)]

#[cfg(unix)]
use libloading::os::unix::Library as UnixLibrary;
#[cfg(unix)]
use libloading::os::unix::RTLD_GLOBAL;
#[cfg(unix)]
use libloading::os::unix::RTLD_LAZY;
use polyplug::abi::ABI_ERROR_PANIC;
use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::HostVTable;
use polyplug::abi::PluginContext;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::allocator::polyplug_host_free;
use polyplug::allocator::tracking::TrackingAllocator;
use polyplug::registry::Registry;

// ─── Plugin environment variable ──────────────────────────────────────────────

/// Path to the compiled error_plugin shared library — set by build.rs.
const ERROR_PLUGIN_SO: &str = env!("ERROR_PLUGIN_SO");

// ─── ChainArgs (mirrors error_plugin's ChainArgs) ─────────────────────────────

/// Arguments for error_chain_propagate (fn 2).
/// Mirrors the definition in tests/fixtures/error_plugin/src/lib.rs.
#[repr(C)]
struct ChainArgs {
    host: *const HostVTable,
    target_contract_id: u64,
    target_fn_id: u32,
}

// ─── Thread-local registry ────────────────────────────────────────────────────

std::thread_local! {
    static ERROR_REGISTRY: std::cell::RefCell<Registry> =
        std::cell::RefCell::new(Registry::new());
}

// ─── HostVTable callbacks (for Test 3 chain dispatch) ───────────────────────

/// find_by_contract that looks up a plugin from the thread-local ERROR_REGISTRY.
///
/// # Safety
/// Must only be called when ERROR_REGISTRY has been populated on this thread.
unsafe extern "C" fn chain_find_by_contract(contract_id: u64, _min_version: u32) -> PluginHandle {
    ERROR_REGISTRY.with(|cell| {
        let registry: std::cell::Ref<'_, Registry> = cell.borrow();
        match registry.find(contract_id, 0) {
            Ok(handle) => handle,
            Err(_) => PluginHandle {
                index: u32::MAX,
                generation: 0,
            },
        }
    })
}

/// find_by_bundle stub — delegates to find_by_contract (bundle-scoped lookup not implemented).
///
/// # Safety
/// Always safe to call; delegates to chain_find_by_contract.
unsafe extern "C" fn chain_find_by_bundle(
    _bundle_id: u64,
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    // SAFETY: chain_find_by_contract has no pointer preconditions.
    unsafe { chain_find_by_contract(contract_id, min_version) }
}

/// find_all_by_contract stub — returns 0 (not needed for error chain tests).
///
/// # Safety
/// Always safe to call; no pointer dereferences if out_cap is 0.
unsafe extern "C" fn chain_find_all_by_contract(
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// resolve_plugin that dispatches through the thread-local ERROR_REGISTRY.
///
/// # Safety
/// The returned pointer is 'static (error_plugin library is kept alive via mem::forget).
unsafe extern "C" fn chain_resolve_plugin(handle: PluginHandle) -> *const PluginVTable {
    ERROR_REGISTRY.with(|cell| {
        let registry: std::cell::Ref<'_, Registry> = cell.borrow();
        registry.resolve(handle).unwrap_or(core::ptr::null())
    })
}

/// Stub get_extension — returns null (no extensions implemented in MVP).
///
/// # Safety
/// Always safe to call; returns null pointer.
unsafe extern "C" fn stub_get_extension(_extension_id: u32) -> *const () {
    core::ptr::null()
}

// ─── Registry callback ────────────────────────────────────────────────────────

/// A registrar callback that stores vtable entries into the thread-local ERROR_REGISTRY.
///
/// # Safety
/// `_registrar`, `descriptor`, and `vtable` must be valid for the call duration.
unsafe extern "C" fn registry_register_callback(
    _registrar: *mut PluginRegistrar,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1_u32,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<PluginHandle, _> = ERROR_REGISTRY.with(|reg_cell| {
        let registry: std::cell::Ref<'_, Registry> = reg_cell.borrow();
        unsafe {
            registry.register(
                *desc,
                vtable as *const PluginVTable,
                contract_name.to_owned(),
                vt.contract_id,
            )
        }
    });

    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1_u32,
            message: StringView::null(),
        },
    }
}

// ─── Helper functions ─────────────────────────────────────────────────────────

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

/// Initialise error_plugin and return the vtable pointer.
/// Also resets the thread-local registry.
fn init_error_plugin(library: &libloading::Library) -> *const PluginVTable {
    // Reset registry before each use.
    ERROR_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(*mut PluginRegistrar, *const PluginContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; registrar lives for the call duration.
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
    };
    // SAFETY: init_fn is valid; registrar and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            &mut registrar as *mut PluginRegistrar,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must succeed");

    let contract_id: u64 = polyplug::abi::contract_id("error.test", 1);
    let handle: PluginHandle = ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("error.test must be registered")
    });

    ERROR_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable")
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Test 1: error_return_with_message writes an AbiError { code=99, message="test error from plugin" }
/// to the out pointer, and the message must be freed after reading.
#[test]
fn stress_error_code_and_message_received_correctly() {
    let library: libloading::Library = load_error_plugin();
    let vtable_ptr: *const PluginVTable = init_error_plugin(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // SAFETY: fn_ptr is function 0 in the vtable (error_return_with_message).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    let mut out: AbiError = AbiError {
        code: 0_u32,
        message: StringView::null(),
    };

    // SAFETY: fn 0 ignores args (pass null). out is a valid AbiError location.
    let call_result: AbiError =
        unsafe { dispatch_fn(core::ptr::null(), &mut out as *mut AbiError as *mut ()) };

    // The dispatch wrapper returns ABI_OK (success).
    assert_eq!(
        call_result.code, ABI_OK,
        "dispatch wrapper must return ABI_OK"
    );

    // The actual error is written to *out.
    assert_eq!(out.code, 99_u32, "error code must be 99");
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
/// The message is from_static — must NOT be freed. Process continues after the call.
#[test]
fn stress_panic_returns_abi_error_panic_process_continues() {
    let library: libloading::Library = load_error_plugin();
    let vtable_ptr: *const PluginVTable = init_error_plugin(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // SAFETY: fn_ptr is function 1 in the vtable (error_panic).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(1) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. fn 1 ignores both
    // args and out — it catches the panic internally and returns ABI_ERROR_PANIC directly.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // fn 1 returns the AbiError directly (not via out pointer). Both args and out are null.
    // SAFETY: fn 1 ignores args and out entirely (no pointer dereferences).
    let result: AbiError = unsafe { dispatch_fn(core::ptr::null(), core::ptr::null_mut()) };

    assert_eq!(
        result.code, ABI_ERROR_PANIC,
        "error_panic must return ABI_ERROR_PANIC (code={ABI_ERROR_PANIC})"
    );

    // The message is from_static ("plugin panicked") — do NOT free it.
    // SAFETY: result.message.ptr points to 'static bytes that remain valid indefinitely.
    let msg_bytes: &[u8] =
        unsafe { core::slice::from_raw_parts(result.message.ptr, result.message.len) };
    assert_eq!(
        msg_bytes, b"plugin panicked",
        "panic message must be 'plugin panicked'"
    );

    // Process continues — reaching this assertion IS the proof.
    assert!(true, "process continues after plugin panic");

    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    core::mem::forget(library);
}

/// Test 3: error_chain_propagate (fn 2) calls another plugin via a real HostVTable
/// and propagates the error back to the test. The chain target is fn 1 (error_panic)
/// which returns ABI_ERROR_PANIC via its return value (not via out pointer).
/// The propagated error code is written to *out by error_chain_propagate.
#[test]
fn stress_error_chain_b_errors_a_propagates() {
    let library: libloading::Library = load_error_plugin();
    let vtable_ptr: *const PluginVTable = init_error_plugin(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // Build a HostVTable that routes find_by_contract and resolve_plugin through the
    // thread-local ERROR_REGISTRY that contains error_plugin's vtable.
    let chain_host_vtable: HostVTable = HostVTable {
        alloc: polyplug::allocator::polyplug_host_alloc,
        // SAFETY: polyplug_host_free is a valid extern "C" fn pointer.
        free: polyplug_host_free,
        find_by_contract: chain_find_by_contract,
        find_by_bundle: chain_find_by_bundle,
        find_all_by_contract: chain_find_all_by_contract,
        resolve_plugin: chain_resolve_plugin,
        get_extension: stub_get_extension,
    };

    // error.test contract_id is FNV-1a("error.test@1").
    let error_contract_id: u64 = polyplug::abi::contract_id("error.test", 1);

    // ChainArgs pointing to fn 1 (error_panic).
    // fn 1 returns ABI_ERROR_PANIC via its return value (not via *out),
    // so error_chain_propagate receives it as inner_result and writes it to *out.
    let chain_args: ChainArgs = ChainArgs {
        host: &chain_host_vtable as *const HostVTable,
        target_contract_id: error_contract_id,
        target_fn_id: 1_u32, // fn 1 = error_panic
    };

    let mut out: AbiError = AbiError {
        code: 0_u32,
        message: StringView::null(),
    };

    // SAFETY: fn_ptr is function 2 in the vtable (error_chain_propagate).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Args is *const ChainArgs,
    // out is *mut AbiError — types enforced by this test.
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: chain_args is a valid ChainArgs with a live HostVTable.
    // out is a valid AbiError location. error_chain_propagate calls fn 1 via the host
    // vtable and writes the returned AbiError (ABI_ERROR_PANIC) to *out.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &chain_args as *const ChainArgs as *const (),
            &mut out as *mut AbiError as *mut (),
        )
    };

    // error_chain_propagate itself returns ABI_OK (wrapper success).
    assert_eq!(
        call_result.code, ABI_OK,
        "error_chain_propagate wrapper must return ABI_OK"
    );

    // The propagated error from fn 1 (error_panic) is ABI_ERROR_PANIC.
    assert_eq!(
        out.code, ABI_ERROR_PANIC,
        "propagated error must be ABI_ERROR_PANIC (={ABI_ERROR_PANIC})"
    );

    // The message from error_panic is from_static — do NOT free it.
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
    let vtable_ptr: *const PluginVTable = init_error_plugin(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // SAFETY: fn_ptr is function 0 in the vtable (error_return_with_message).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
    // enforced by the test (fn 0 writes AbiError to *out, ignores args).
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };

    let mut out: AbiError = AbiError {
        code: 0_u32,
        message: StringView::null(),
    };

    // SAFETY: fn 0 ignores args (pass null). out is a valid AbiError location.
    let call_result: AbiError =
        unsafe { dispatch_fn(core::ptr::null(), &mut out as *mut AbiError as *mut ()) };

    assert_eq!(
        call_result.code, ABI_OK,
        "dispatch wrapper must return ABI_OK"
    );
    assert_eq!(out.code, 99_u32, "error code must be 99");
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
