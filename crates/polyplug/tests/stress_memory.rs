#![allow(clippy::expect_used)]

//! Stress tests for the polyplug memory model: Buffer, StringView, allocator.
//!
//! This test crate is the crate root for the `stress_memory` test binary.

use core::cell::Ref;
use core::cell::RefCell;
use core::ffi::c_void;
use core::mem::forget;
use core::mem::transmute;
use core::ptr::null;
use core::ptr::null_mut;
use core::slice::from_raw_parts;
use core::str::from_utf8;
use core::str::from_utf8_unchecked;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::sync::Arc;
use std::thread;

use libloading::{Library, Symbol};
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::DependencyInfo;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::HostContractInstance;
use polyplug_abi::HostContractInterface;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::tracking::TrackingAllocator;
use polyplug_abi::{
    AbiError, AbiErrorCode, Array, Buffer, BundleInitContext, GuestContractHandle,
    GuestContractInterface, HostApi, PluginDescriptor, StringView,
};
use polyplug_utils::{BundleId, GuestContractId};

// --- Plugin environment variable ---------------------------------------------

/// Path to the compiled memory_plugin shared library -- set by build.rs.
const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

// --- Thread-local registry ---------------------------------------------------

thread_local! {
    static STRESS_REGISTRY: RefCell<RuntimeStore> = RefCell::new(RuntimeStore::new());
    static TLS_TRACKING_ALLOC: RefCell<unsafe extern "C" fn(usize, usize) -> *mut u8> =
        RefCell::new(polyplug_host_alloc);
    static TLS_TRACKING_FREE: RefCell<unsafe extern "C" fn(*mut u8, usize, usize)> =
        RefCell::new(polyplug_host_free);
}

// --- ABI argument/result types (mirror memory_plugin's definitions) ----------

/// Arguments to `memory_fill_preallocated_buffer` (fn 0).
#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

/// Arguments to `memory_alloc_buffer_via_host` (fn 1).
#[repr(C)]
struct AllocArgs {
    host: *const HostApi,
    size: u64,
    fill_byte: u8,
}

/// Arguments to `memory_zero_length_roundtrip` (fn 3).
#[repr(C)]
struct ZeroArgs {
    buf: Buffer,
    sv: StringView,
}

/// Result of `memory_zero_length_roundtrip` (fn 3).
#[repr(C)]
struct ZeroResult {
    buf_len: u64,
    sv_len: u64,
}

// --- HostApi stub functions ---------------------------------------------

/// Stub find_guest_contract -- returns a null handle (not needed for memory stress tests).
///
/// # Safety
/// Always safe to call; returns a sentinel null handle.
unsafe extern "C" fn stub_find_guest_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> GuestContractHandle {
    GuestContractHandle::null()
}

/// Stub find_all_by_contract -- returns empty array (not needed for memory stress tests).
///
/// # Safety
/// Always safe to call; returns empty array.
unsafe extern "C" fn stub_find_all_guest_contracts(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> Array<GuestContractHandle> {
    Array::empty()
}

/// Stub resolve_guest_contract -- returns null (not needed for memory stress tests).
///
/// # Safety
/// Always safe to call; returns null pointer.
unsafe extern "C" fn stub_resolve_guest_contract(
    _this: *const HostApi,
    _handle: GuestContractHandle,
) -> *const GuestContractInterface {
    null()
}

/// Stub get_host_contract -- returns null instance.
unsafe extern "C" fn stub_get_host_contract(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> HostContractInstance {
    HostContractInstance::null()
}

/// Stub list_bundles -- returns empty array.
unsafe extern "C" fn stub_list_bundles(_this: *const HostApi) -> Array<BundleId> {
    Array::empty()
}

/// Stub get_dependencies -- returns empty array.
unsafe extern "C" fn stub_get_dependencies(_this: *const HostApi) -> Array<DependencyInfo> {
    Array::empty()
}

/// Stub resolve_host_contract_interface -- returns null.
unsafe extern "C" fn stub_resolve_host_contract_interface(
    _this: *const HostApi,
    _contract_id: u64,
    _min_version: u32,
) -> *const HostContractInterface {
    null()
}

/// Stub load_bundle callback.
unsafe extern "C" fn stub_load_bundle(
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

/// Stub reload_bundle callback.
unsafe extern "C" fn stub_reload_bundle(
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

/// Stub register_host_contract callback.
unsafe extern "C" fn stub_register_host_contract(
    _this: *const HostApi,
    _interface: *const HostContractInterface,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Stub register_loader callback.
unsafe extern "C" fn stub_register_loader(
    _this: *const HostApi,
    _loader_ptr: *mut c_void,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

/// Stub get_last_error callback.
unsafe extern "C" fn stub_get_last_error(
    _this: *const HostApi,
    _buf: *mut u8,
    _buf_len: usize,
) -> usize {
    0
}

/// Stub get_error_len callback.
unsafe extern "C" fn stub_get_error_len(_this: *const HostApi) -> usize {
    0
}

unsafe extern "C" fn stub_unload_bundle(
    _this: *const HostApi,
    _bundle_id: BundleId,
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable per the ABI contract.
        unsafe { out_err.write(AbiError::ok()) };
    }
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

// --- Registry callback -------------------------------------------------------

/// A register_guest_contract callback that stores interface entries into the thread-local Registry.
///
/// # Safety
/// `this`, `descriptor`, and `interface` must be valid for the call duration.
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
    let result: Result<GuestContractHandle, _> = STRESS_REGISTRY.with(|reg_cell| {
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

// --- Helper functions --------------------------------------------------------

/// Returns the workspace root path (two levels up from crates/polyplug/).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of crates/polyplug")
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

/// Loads the memory_plugin shared library.
fn load_memory_plugin() -> Library {
    // SAFETY: MEMORY_PLUGIN_SO is a compiled cdylib built by build.rs.
    unsafe { Library::new(MEMORY_PLUGIN_SO).expect("failed to load memory_plugin .so") }
}

/// Initialise the memory_plugin and store interface into the thread-local registry.
/// Returns the interface pointer.
fn init_memory_plugin_interface(library: &Library) -> *const GuestContractInterface {
    // Reset registry before each use.
    STRESS_REGISTRY.with(|cell| {
        *cell.borrow_mut() = RuntimeStore::new();
    });

    // SAFETY: polyplug_init matches the expected 2-arg ABI signature.
    let init_fn: Symbol<
        '_,
        unsafe extern "C" fn(*const HostApi, *const BundleInitContext) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_interface: HostApi = HostApi {
        runtime: null_mut(),
        register_guest_contract: registry_register_callback,
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
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        revision_counter: stub_revision_counter,
        reserved: null(),
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
        "polyplug_init must succeed"
    );

    let contract_id: GuestContractId = GuestContractId::new("memory.test", 1);
    let handle: GuestContractHandle = STRESS_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("memory.test must be registered")
    });

    STRESS_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve_guest_contract(handle)
            .expect("interface must be resolvable")
    })
}

// --- Tests -------------------------------------------------------------------

#[test]
fn stress_large_buffer_fill_and_read() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Allocate 1 MiB buffer via the host allocator.
    const BUFFER_SIZE: usize = 1024 * 1024;
    // SAFETY: BUFFER_SIZE is non-zero and align=1 is valid.
    let ptr: *mut u8 = polyplug_host_alloc(BUFFER_SIZE, 1);
    assert!(
        !ptr.is_null(),
        "polyplug_host_alloc must return non-null for 1 MiB"
    );

    let args: FillArgs = FillArgs {
        buf: Buffer {
            ptr,
            len: 0,
            cap: BUFFER_SIZE,
        },
        fill_byte: 0xAB_u8,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are enforced
        // by the test (FillArgs matches what memory_plugin fn 0 expects).
        unsafe { transmute(fn_ptr) };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "memory_fill_preallocated_buffer must return Ok"
    );
    assert_eq!(
        out as usize, BUFFER_SIZE,
        "written byte count must equal buffer capacity"
    );

    // Verify all bytes are 0xAB.
    // SAFETY: ptr is valid for BUFFER_SIZE bytes, written by the plugin.
    let filled_slice: &[u8] = unsafe { from_raw_parts(ptr, BUFFER_SIZE) };
    assert!(
        filled_slice.iter().all(|&b| b == 0xAB_u8),
        "all bytes in 1 MiB buffer must be 0xAB"
    );

    // Free the buffer.
    // SAFETY: ptr was allocated by polyplug_host_alloc with BUFFER_SIZE and align=1.
    unsafe { polyplug_host_free(ptr, BUFFER_SIZE, 1) };

    // The allocations above go directly through polyplug_host_alloc/free, not through
    // a TrackingAllocator, so there is nothing for a tracker to observe here. (A fresh
    // tracker created at this point would only assert 0 == 0 — vacuously.)

    forget(library);
}

#[test]
fn stress_string_view_non_ascii_utf8() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Non-ASCII UTF-8: "café" encoded as bytes (é = 0xC3 0xA9, two bytes).
    let input_bytes: &[u8] = b"caf\xc3\xa9";
    let input_sv: StringView = StringView {
        ptr: input_bytes.as_ptr(),
        len: input_bytes.len(),
    };

    let mut out_sv: StringView = StringView::null();

    // SAFETY: fn_ptr is function 2 in the interface (memory_echo_string_view).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (StringView matches what memory_plugin fn 2 expects).
        unsafe { transmute(fn_ptr) };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: input_sv is a valid StringView with a valid ptr/len, out_sv is a valid location.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &input_sv as *const StringView as *const (),
            &mut out_sv as *mut StringView as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "memory_echo_string_view must return Ok"
    );
    assert_eq!(
        out_sv.ptr, input_sv.ptr,
        "echoed StringView must have same pointer"
    );
    assert_eq!(
        out_sv.len, input_sv.len,
        "echoed StringView must have same length"
    );

    // Validate the returned bytes are valid UTF-8.
    // SAFETY: out_sv.ptr is valid for out_sv.len bytes (same memory as input_bytes).
    let returned_bytes: &[u8] = unsafe { from_raw_parts(out_sv.ptr, out_sv.len) };
    let returned_str: &str =
        from_utf8(returned_bytes).expect("echoed StringView must be valid UTF-8");
    assert_eq!(returned_str, "café", "echoed string must equal input");

    // No TrackingAllocator is involved on this path (the echo returns a borrowed view),
    // so there is nothing for a tracker to observe — a fresh tracker would assert 0 == 0.

    forget(library);
}

#[test]
fn stress_zero_length_buffer_and_string_view() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Zero-length Buffer and StringView.
    let zero_buf: Buffer = Buffer {
        ptr: null_mut(),
        len: 0,
        cap: 0,
    };
    let zero_sv: StringView = StringView {
        ptr: null(),
        len: 0,
    };
    let args: ZeroArgs = ZeroArgs {
        buf: zero_buf,
        sv: zero_sv,
    };
    let mut out: ZeroResult = ZeroResult {
        buf_len: u64::MAX,
        sv_len: u64::MAX,
    };

    // SAFETY: fn_ptr is function 3 in the interface (memory_zero_length_roundtrip).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (ZeroArgs/ZeroResult match what memory_plugin fn 3 expects).
        unsafe { transmute(fn_ptr) };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid ZeroArgs, out is a valid ZeroResult location.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const ZeroArgs as *const (),
            &mut out as *mut ZeroResult as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "memory_zero_length_roundtrip must return Ok"
    );
    assert_eq!(
        out.buf_len, 0_u64,
        "zero-length Buffer.len must round-trip as 0"
    );
    assert_eq!(
        out.sv_len, 0_u64,
        "zero-length StringView.len must round-trip as 0"
    );

    // This path does not allocate through a TrackingAllocator, so a fresh tracker
    // here would only assert 0 == 0 (vacuous) — omitted.

    forget(library);
}

#[test]
fn stress_concurrent_8_threads_no_shared_memory() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    // GuestContractInterface is Send+Sync per its unsafe impls in the plugin.
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    const THREAD_COUNT: usize = 8;
    const THREAD_BUFFER_SIZE: usize = 4096;

    let alloc_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let free_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    thread::scope(|s| {
        for thread_idx in 0_usize..THREAD_COUNT {
            let alloc_counter: Arc<AtomicUsize> = Arc::clone(&alloc_count);
            let free_counter: Arc<AtomicUsize> = Arc::clone(&free_count);
            let fill_byte: u8 = (0xA0_u8).wrapping_add(thread_idx as u8);

            s.spawn(move || {
                // Each thread independently allocates its own buffer.
                // SAFETY: polyplug_host_alloc is thread-safe per documentation.
                // THREAD_BUFFER_SIZE is non-zero and align=1 is valid.
                let ptr: *mut u8 = polyplug_host_alloc(THREAD_BUFFER_SIZE, 1);
                assert!(!ptr.is_null(), "thread {}: alloc must succeed", thread_idx);
                alloc_counter.fetch_add(1, Ordering::Relaxed);

                // Get function 0 (memory_fill_preallocated_buffer) from interface.
                // SAFETY: interface.functions is valid for function_count (4) entries.
                let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
                let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
                    // SAFETY: fn_ptr is the fill function with compatible signature.
                    unsafe { transmute(fn_ptr) };

                let args: FillArgs = FillArgs {
                    buf: Buffer {
                        ptr,
                        len: 0,
                        cap: THREAD_BUFFER_SIZE,
                    },
                    fill_byte,
                };
                let mut out: u32 = 0_u32;

                let mut result: AbiError = AbiError::ok();
                // SAFETY: args is a valid FillArgs, out is a valid u32 location.
                unsafe {
                    dispatch_fn(
                        GuestContractInstance::null(),
                        &args as *const FillArgs as *const (),
                        &mut out as *mut u32 as *mut (),
                        &mut result,
                    )
                };
                assert_eq!(
                    result.code,
                    AbiErrorCode::Ok as u32,
                    "thread {}: fill must return Ok",
                    thread_idx
                );
                assert_eq!(
                    out as usize, THREAD_BUFFER_SIZE,
                    "thread {}: written count must equal buffer size",
                    thread_idx
                );

                // Verify buffer contents.
                // SAFETY: ptr is valid for THREAD_BUFFER_SIZE bytes, written by the plugin.
                let slice: &[u8] = unsafe { from_raw_parts(ptr, THREAD_BUFFER_SIZE) };
                assert!(
                    slice.iter().all(|&b| b == fill_byte),
                    "thread {}: all bytes must equal fill_byte 0x{:02X}",
                    thread_idx,
                    fill_byte
                );

                // Free the thread-local buffer.
                // SAFETY: ptr was allocated by polyplug_host_alloc with THREAD_BUFFER_SIZE, align=1.
                unsafe { polyplug_host_free(ptr, THREAD_BUFFER_SIZE, 1) };
                free_counter.fetch_add(1, Ordering::Relaxed);
            });
        }
    });

    assert_eq!(
        alloc_count.load(Ordering::Relaxed),
        THREAD_COUNT,
        "all {} threads must have allocated",
        THREAD_COUNT
    );
    assert_eq!(
        free_count.load(Ordering::Relaxed),
        THREAD_COUNT,
        "all {} threads must have freed",
        THREAD_COUNT
    );

    // The per-thread allocations above went directly through polyplug_host_alloc/free,
    // not through a TrackingAllocator, and balance is verified by the alloc/free counter
    // assertions above. A fresh tracker on this coordinating thread observed nothing, so
    // asserting on it would be vacuous — omitted.

    forget(library);
}

#[test]
fn stress_plugin_allocates_returns_to_host_then_host_frees() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Set up a tracking allocator and build a HostApi that uses wrapper functions.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    let alloc_fn: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();
    let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();

    // Wrapper functions that take HostApi and delegate to tracking functions
    unsafe extern "C" fn tracking_alloc_wrapper(
        _this: *const HostApi,
        size: usize,
        align: usize,
    ) -> *mut u8 {
        TLS_TRACKING_ALLOC.with(|cell| {
            let alloc_fn: unsafe extern "C" fn(usize, usize) -> *mut u8 = *cell.borrow();
            // SAFETY: alloc_fn is a valid function pointer from TrackingAllocator.
            unsafe { alloc_fn(size, align) }
        })
    }

    unsafe extern "C" fn tracking_free_wrapper(
        _this: *const HostApi,
        ptr: *mut u8,
        size: usize,
        align: usize,
    ) {
        TLS_TRACKING_FREE.with(|cell| {
            let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = *cell.borrow();
            // SAFETY: free_fn is a valid function pointer from TrackingAllocator.
            unsafe { free_fn(ptr, size, align) }
        })
    }

    // Store the function pointers in thread-local storage
    TLS_TRACKING_ALLOC.with(|cell| *cell.borrow_mut() = alloc_fn);
    TLS_TRACKING_FREE.with(|cell| *cell.borrow_mut() = free_fn);

    let host_interface: HostApi = HostApi {
        runtime: null_mut(),
        register_guest_contract: registry_register_callback,
        alloc: tracking_alloc_wrapper,
        free: tracking_free_wrapper,
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
        unload_bundle: stub_unload_bundle,
        log: stub_host_log,
        create_guest_instance: stub_create_guest_instance,
        destroy_guest_instance: stub_destroy_guest_instance,
        revision_counter: stub_revision_counter,
        reserved: null(),
    };

    let args: AllocArgs = AllocArgs {
        host: &host_interface as *const HostApi,
        size: 4096_u64,
        fill_byte: 0xCC_u8,
    };
    let mut out_buf: Buffer = Buffer {
        ptr: null_mut(),
        len: 0,
        cap: 0,
    };

    // SAFETY: fn_ptr is function 1 in the interface (memory_alloc_buffer_via_host).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(1) };
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (AllocArgs/Buffer match what memory_plugin fn 1 expects).
        unsafe { transmute(fn_ptr) };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid AllocArgs (host interface is live), out_buf is a valid Buffer location.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const AllocArgs as *const (),
            &mut out_buf as *mut Buffer as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "memory_alloc_buffer_via_host must return Ok"
    );
    assert!(
        !out_buf.ptr.is_null(),
        "plugin-allocated buffer pointer must be non-null"
    );
    assert!(
        out_buf.len > 0,
        "plugin-allocated buffer len must be non-zero"
    );

    // Plugin called host.alloc -- tracking counter should be 1.
    assert_eq!(
        tracker.alloc_count(),
        1,
        "alloc_count must be 1 after plugin allocated"
    );
    assert_eq!(
        tracker.free_count(),
        0,
        "free_count must be 0 before we free"
    );

    // Verify the buffer was filled with 0xCC.
    // SAFETY: out_buf.ptr is valid for out_buf.len bytes, filled by the plugin.
    let buf_slice: &[u8] = unsafe { from_raw_parts(out_buf.ptr, out_buf.len) };
    assert!(
        buf_slice.iter().all(|&b| b == 0xCC_u8),
        "all bytes in plugin-allocated buffer must be 0xCC"
    );

    // Free via the tracking free_fn to keep the counters balanced.
    let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
    // SAFETY: out_buf.ptr was allocated by tracking_alloc_wrapper (via host.alloc) with cap=4096, align=1.
    unsafe { free_fn(out_buf.ptr, out_buf.cap, 1) };

    assert_eq!(tracker.alloc_count(), 1, "alloc_count must still be 1");
    assert_eq!(tracker.free_count(), 1, "free_count must be 1 after free");
    tracker.assert_no_leaks();

    forget(library);
}

#[test]
fn stress_caller_alloc_plugin_fills_freed_after_use() {
    let library: Library = load_memory_plugin();
    let interface_ptr: *const GuestContractInterface = init_memory_plugin_interface(&library);

    // SAFETY: interface_ptr is valid (plugin is loaded, library not yet dropped).
    let interface: &GuestContractInterface = unsafe { &*interface_ptr };

    // Use the tracking allocator for the caller-side allocation.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    let alloc_fn: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();

    // Allocate 64 bytes via the tracking allocator (increments alloc_count to 1).
    // SAFETY: size=64 is non-zero and align=1 is valid.
    let ptr: *mut u8 = unsafe { alloc_fn(64, 1) };
    assert!(!ptr.is_null(), "tracker alloc must return non-null");
    assert_eq!(
        tracker.alloc_count(),
        1,
        "alloc_count must be 1 after caller allocation"
    );

    let args: FillArgs = FillArgs {
        buf: Buffer {
            ptr,
            len: 0,
            cap: 64,
        },
        fill_byte: 0xDE_u8,
    };
    let mut out: u32 = 0_u32;

    // SAFETY: fn_ptr is function 0 in the interface (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *interface.dispatch.native.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(GuestContractInstance, *const (), *mut (), *mut AbiError) =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (FillArgs matches what memory_plugin fn 0 expects).
        unsafe { transmute(fn_ptr) };

    let mut call_result: AbiError = AbiError::ok();
    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    unsafe {
        dispatch_fn(
            GuestContractInstance::null(),
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
            &mut call_result,
        )
    };

    assert_eq!(
        call_result.code,
        AbiErrorCode::Ok as u32,
        "memory_fill_preallocated_buffer must return Ok"
    );
    assert_eq!(out, 64_u32, "written byte count must be 64");

    // Verify buffer was filled with 0xDE.
    // SAFETY: ptr is valid for 64 bytes, written by the plugin.
    let filled_slice: &[u8] = unsafe { from_raw_parts(ptr, 64) };
    assert!(
        filled_slice.iter().all(|&b| b == 0xDE_u8),
        "all 64 bytes must be 0xDE"
    );

    // Free via the tracking free_fn (increments free_count to 1).
    let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
    // SAFETY: ptr was allocated by tracking_alloc with size=64, align=1. Not yet freed.
    unsafe { free_fn(ptr, 64, 1) };

    assert_eq!(tracker.alloc_count(), 1, "alloc_count must be 1");
    assert_eq!(tracker.free_count(), 1, "free_count must be 1 after free");
    tracker.assert_no_leaks();

    // _workspace_root is unused here but workspace_root() is defined per task spec.
    let _workspace_root: PathBuf = workspace_root();

    forget(library);
}

/// Called when the test binary is re-invoked with the `POLYPLUG_DOUBLE_FREE_SUBPROCESS`
/// environment variable set. Performs a real double-free to trigger `abort()`.
#[cfg(debug_assertions)]
fn run_double_free_subprocess() -> ! {
    let tracker: TrackingAllocator = TrackingAllocator::new();
    let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();
    let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
    // SAFETY: size=64, align=1 is a valid layout.
    let ptr: *mut u8 = unsafe { alloc(64_usize, 1_usize) };
    // SAFETY: ptr was just allocated with size=64, align=1.
    unsafe { free_fn(ptr, 64_usize, 1_usize) };
    // Second free -- triggers abort() in tracking_free.
    // SAFETY: This is intentionally invalid -- the abort fires before UB occurs.
    unsafe { free_fn(ptr, 64_usize, 1_usize) };
    // unreachable -- abort() terminates the process.
    process::exit(0)
}

#[test]
#[cfg(debug_assertions)]
fn test_double_free_detected() {
    // Use an env var (not a CLI arg) as the subprocess sentinel -- CLI args are intercepted
    // by the cargo test harness and cause "Unrecognized option" errors.
    const SENTINEL: &str = "POLYPLUG_DOUBLE_FREE_SUBPROCESS";
    // If this invocation IS the subprocess, perform the double-free and let abort() fire.
    if env::var(SENTINEL).is_ok() {
        run_double_free_subprocess();
    }
    // Otherwise spawn ourselves as a subprocess with the sentinel env var set.
    let exe: PathBuf = env::current_exe().expect("current_exe");
    let status: process::ExitStatus = process::Command::new(&exe)
        .env(SENTINEL, "1")
        .status()
        .expect("failed to spawn subprocess");
    assert!(
        !status.success(),
        "double-free subprocess must exit non-zero (aborted)"
    );
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

unsafe extern "C" fn stub_revision_counter(_this: *const HostApi) -> *const u64 {
    null()
}
