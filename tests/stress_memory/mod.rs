//! Stress tests for the polyplug memory model: Buffer, StringView, allocator.
//!
//! This test crate is the crate root for the `stress_memory` test binary.
//! (AGENTS.md Rule 1: module roots use dirname/mod.rs)

#![allow(clippy::expect_used)]

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::Buffer;
use polyplug::abi::HostVTable;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::allocator::polyplug_host_alloc;
use polyplug::allocator::polyplug_host_free;
use polyplug::allocator::tracking::TrackingAllocator;
use polyplug::registry::Registry;
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

// ─── Plugin environment variable ──────────────────────────────────────────────

/// Path to the compiled memory_plugin shared library — set by build.rs.
const MEMORY_PLUGIN_SO: &str = env!("MEMORY_PLUGIN_SO");

// ─── Thread-local registry ────────────────────────────────────────────────────

std::thread_local! {
    static STRESS_REGISTRY: RefCell<Registry> = RefCell::new(Registry::new());
}

// ─── ABI argument/result types (mirror memory_plugin's definitions) ───────────

/// Arguments to `memory_fill_preallocated_buffer` (fn 0).
#[repr(C)]
struct FillArgs {
    buf: Buffer,
    fill_byte: u8,
}

/// Arguments to `memory_alloc_buffer_via_host` (fn 1).
#[repr(C)]
struct AllocArgs {
    host: *const HostVTable,
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

// ─── HostVTable stub functions ────────────────────────────────────────────────

/// Stub find_plugin — returns a null handle (not needed for memory stress tests).
///
/// # Safety
/// Always safe to call; returns a sentinel null handle.
unsafe extern "C" fn stub_find_plugin(_contract_id: u64, _min_version: u32) -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

/// Stub call_plugin — returns ABI_OK without doing anything.
///
/// # Safety
/// Always safe to call; no pointer dereferences.
unsafe extern "C" fn stub_call_plugin(
    _plugin: PluginHandle,
    _fn_id: u32,
    _args: *const (),
    _out: *mut (),
) -> AbiError {
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}

/// Stub get_extension — returns null.
///
/// # Safety
/// Always safe to call; returns null pointer.
unsafe extern "C" fn stub_get_extension(_extension_id: u32) -> *const () {
    core::ptr::null()
}

// ─── Registry callback ────────────────────────────────────────────────────────

/// A registrar callback that stores vtable entries into the thread-local Registry.
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
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };

    // Extract contract name from StringView.
    // SAFETY: desc.contract_name.ptr points to valid UTF-8 bytes for desc.contract_name.len bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };

    let result: Result<PluginHandle, _> = STRESS_REGISTRY.with(|reg_cell| {
        let registry: std::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry.register(
            *desc,
            vtable as *const PluginVTable,
            contract_name.to_owned(),
            vt.contract_id,
        )
    });

    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

// ─── Helper functions ─────────────────────────────────────────────────────────

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
fn load_memory_plugin() -> libloading::Library {
    // SAFETY: MEMORY_PLUGIN_SO is a compiled cdylib built by build.rs.
    unsafe { libloading::Library::new(MEMORY_PLUGIN_SO).expect("failed to load memory_plugin .so") }
}

/// Initialise the memory_plugin and store vtable into the thread-local registry.
/// Returns the vtable pointer.
fn init_memory_plugin_vtable(library: &libloading::Library) -> *const PluginVTable {
    // Reset registry before each use.
    STRESS_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });

    // SAFETY: polyplug_init matches the expected ABI signature.
    let init_fn: libloading::Symbol<'_, unsafe extern "C" fn(*mut PluginRegistrar) -> AbiError> = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    // SAFETY: init_fn is valid; registrar lives for the call duration.
    let init_result: AbiError = unsafe { init_fn(&mut registrar as *mut PluginRegistrar) };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must succeed");

    let contract_id: u64 = polyplug::abi::contract_id("memory.test", 1);
    let handle: PluginHandle = STRESS_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("memory.test must be registered")
    });

    STRESS_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("vtable must be resolvable")
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn stress_large_buffer_fill_and_read() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

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

    // SAFETY: fn_ptr is function 0 in the vtable (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are enforced
        // by the test (FillArgs matches what memory_plugin fn 0 expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "memory_fill_preallocated_buffer must return ABI_OK"
    );
    assert_eq!(
        out as usize, BUFFER_SIZE,
        "written byte count must equal buffer capacity"
    );

    // Verify all bytes are 0xAB.
    // SAFETY: ptr is valid for BUFFER_SIZE bytes, written by the plugin.
    let filled_slice: &[u8] = unsafe { core::slice::from_raw_parts(ptr, BUFFER_SIZE) };
    assert!(
        filled_slice.iter().all(|&b| b == 0xAB_u8),
        "all bytes in 1 MiB buffer must be 0xAB"
    );

    // Free the buffer.
    // SAFETY: ptr was allocated by polyplug_host_alloc with BUFFER_SIZE and align=1.
    unsafe { polyplug_host_free(ptr, BUFFER_SIZE, 1) };

    // TrackingAllocator verifies the tracking layer is balanced (0 allocs, 0 frees through it).
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    std::mem::forget(library);
}

#[test]
fn stress_string_view_non_ascii_utf8() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // Non-ASCII UTF-8: "café" encoded as bytes.
    let input_bytes: &[u8] = b"caf\xc3\xa9";
    let input_sv: StringView = StringView {
        ptr: input_bytes.as_ptr(),
        len: input_bytes.len(),
    };

    let mut out_sv: StringView = StringView::null();

    // SAFETY: fn_ptr is function 2 in the vtable (memory_echo_string_view).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (StringView matches what memory_plugin fn 2 expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: input_sv is a valid StringView with a valid ptr/len, out_sv is a valid location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &input_sv as *const StringView as *const (),
            &mut out_sv as *mut StringView as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "memory_echo_string_view must return ABI_OK"
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
    let returned_bytes: &[u8] = unsafe { core::slice::from_raw_parts(out_sv.ptr, out_sv.len) };
    let returned_str: &str =
        core::str::from_utf8(returned_bytes).expect("echoed StringView must be valid UTF-8");
    assert_eq!(returned_str, "café", "echoed string must equal input");

    // TrackingAllocator verifies the tracking layer is balanced (no allocs/frees through it).
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    std::mem::forget(library);
}

#[test]
fn stress_zero_length_buffer_and_string_view() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // Zero-length Buffer and StringView.
    let zero_buf: Buffer = Buffer {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };
    let zero_sv: StringView = StringView {
        ptr: core::ptr::null(),
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

    // SAFETY: fn_ptr is function 3 in the vtable (memory_zero_length_roundtrip).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(3) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (ZeroArgs/ZeroResult match what memory_plugin fn 3 expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid ZeroArgs, out is a valid ZeroResult location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const ZeroArgs as *const (),
            &mut out as *mut ZeroResult as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "memory_zero_length_roundtrip must return ABI_OK"
    );
    assert_eq!(
        out.buf_len, 0_u64,
        "zero-length Buffer.len must round-trip as 0"
    );
    assert_eq!(
        out.sv_len, 0_u64,
        "zero-length StringView.len must round-trip as 0"
    );

    // TrackingAllocator verifies the tracking layer is balanced.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    std::mem::forget(library);
}

#[test]
fn stress_concurrent_8_threads_no_shared_memory() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    // PluginVTable is Send+Sync per its unsafe impls in the plugin.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    const THREAD_COUNT: usize = 8;
    const THREAD_BUFFER_SIZE: usize = 4096;

    let alloc_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let free_count: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));

    std::thread::scope(|s| {
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

                // Get function 0 (memory_fill_preallocated_buffer) from vtable.
                // SAFETY: vtable.functions is valid for function_count (4) entries.
                let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
                let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                    // SAFETY: fn_ptr is the fill function with compatible signature.
                    unsafe { core::mem::transmute(fn_ptr) };

                let args: FillArgs = FillArgs {
                    buf: Buffer {
                        ptr,
                        len: 0,
                        cap: THREAD_BUFFER_SIZE,
                    },
                    fill_byte,
                };
                let mut out: u32 = 0_u32;

                // SAFETY: args is a valid FillArgs, out is a valid u32 location.
                let result: AbiError = unsafe {
                    dispatch_fn(
                        &args as *const FillArgs as *const (),
                        &mut out as *mut u32 as *mut (),
                    )
                };
                assert_eq!(
                    result.code, ABI_OK,
                    "thread {}: fill must return ABI_OK",
                    thread_idx
                );
                assert_eq!(
                    out as usize, THREAD_BUFFER_SIZE,
                    "thread {}: written count must equal buffer size",
                    thread_idx
                );

                // Verify buffer contents.
                // SAFETY: ptr is valid for THREAD_BUFFER_SIZE bytes, written by the plugin.
                let slice: &[u8] = unsafe { core::slice::from_raw_parts(ptr, THREAD_BUFFER_SIZE) };
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

    // TrackingAllocator is thread-local; verify the tracking layer is balanced on this thread.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    tracker.assert_no_leaks();

    std::mem::forget(library);
}

#[test]
fn stress_plugin_allocates_returns_to_host_then_host_frees() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

    // Set up a tracking allocator and build a HostVTable that uses its fn pointers.
    let tracker: TrackingAllocator = TrackingAllocator::new();
    let host_vtable: HostVTable = HostVTable {
        alloc: tracker.alloc_fn(),
        free: tracker.free_fn(),
        find_plugin: stub_find_plugin,
        call_plugin: stub_call_plugin,
        get_extension: stub_get_extension,
    };

    let args: AllocArgs = AllocArgs {
        host: &host_vtable as *const HostVTable,
        size: 4096_u64,
        fill_byte: 0xCC_u8,
    };
    let mut out_buf: Buffer = Buffer {
        ptr: core::ptr::null_mut(),
        len: 0,
        cap: 0,
    };

    // SAFETY: fn_ptr is function 1 in the vtable (memory_alloc_buffer_via_host).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(1) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (AllocArgs/Buffer match what memory_plugin fn 1 expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid AllocArgs (host vtable is live), out_buf is a valid Buffer location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AllocArgs as *const (),
            &mut out_buf as *mut Buffer as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "memory_alloc_buffer_via_host must return ABI_OK"
    );
    assert!(
        !out_buf.ptr.is_null(),
        "plugin-allocated buffer pointer must be non-null"
    );
    assert!(
        out_buf.len > 0,
        "plugin-allocated buffer len must be non-zero"
    );

    // Plugin called host.alloc — tracking counter should be 1.
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
    let buf_slice: &[u8] = unsafe { core::slice::from_raw_parts(out_buf.ptr, out_buf.len) };
    assert!(
        buf_slice.iter().all(|&b| b == 0xCC_u8),
        "all bytes in plugin-allocated buffer must be 0xCC"
    );

    // Free via the tracking free_fn to keep the counters balanced.
    let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
    // SAFETY: out_buf.ptr was allocated by tracker.alloc_fn() (via host.alloc) with cap=4096, align=1.
    unsafe { free_fn(out_buf.ptr, out_buf.cap, 1) };

    assert_eq!(tracker.alloc_count(), 1, "alloc_count must still be 1");
    assert_eq!(tracker.free_count(), 1, "free_count must be 1 after free");
    tracker.assert_no_leaks();

    std::mem::forget(library);
}

#[test]
fn stress_caller_alloc_plugin_fills_freed_after_use() {
    let library: libloading::Library = load_memory_plugin();
    let vtable_ptr: *const PluginVTable = init_memory_plugin_vtable(&library);

    // SAFETY: vtable_ptr is valid (plugin is loaded, library not yet dropped).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };

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

    // SAFETY: fn_ptr is function 0 in the vtable (memory_fill_preallocated_buffer).
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: fn_ptr is cast to the generic dispatch signature. Arg types are
        // enforced by the test (FillArgs matches what memory_plugin fn 0 expects).
        unsafe { core::mem::transmute(fn_ptr) };

    // SAFETY: args is a valid FillArgs, out is a valid u32 location.
    let call_result: AbiError = unsafe {
        dispatch_fn(
            &args as *const FillArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };

    assert_eq!(
        call_result.code, ABI_OK,
        "memory_fill_preallocated_buffer must return ABI_OK"
    );
    assert_eq!(out, 64_u32, "written byte count must be 64");

    // Verify buffer was filled with 0xDE.
    // SAFETY: ptr is valid for 64 bytes, written by the plugin.
    let filled_slice: &[u8] = unsafe { core::slice::from_raw_parts(ptr, 64) };
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

    std::mem::forget(library);
}
