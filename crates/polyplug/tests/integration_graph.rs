#![allow(clippy::expect_used)]

//! Integration test: multi-contract registration and lookup.
//!
//! This test crate is the crate root for the `integration_graph` test binary.
//!
//! Tests that:
//! - Multiple registrations in a registry work correctly
//! - contract_id lookup returns correct handles
//! - Stale handles are detected after replacement

use polyplug::registry::Registry;
use polyplug_abi::contract_id;
use polyplug_abi::ffi::polyplug_host_alloc;
use polyplug_abi::ffi::polyplug_host_free;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug_abi::ABI_OK;
use polyplug_abi::POLYPLUG_ABI_VERSION;

/// Path to the compiled test_plugin shared library — set by build.rs.
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

// ─── Host functions for integration tests ─────────────────────────────────────

/// A register_plugin callback that stores vtable entries into a Registry
/// via thread-local state (avoids threading through the opaque host pointer).
///
/// # Safety
/// `rt_ctx`, `descriptor`, and `vtable` must be valid non-null pointers for the call duration.
unsafe extern "C" fn graph_register_callback(
    _rt_ctx: *mut core::ffi::c_void,
    descriptor: *const PluginDescriptor,
    vtable: *const PluginVTable,
) -> AbiError {
    if descriptor.is_null() || vtable.is_null() {
        return AbiError {
            code: 1,
            message: StringView::null(),
        };
    }

    // SAFETY: descriptor and vtable are valid for this call.
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for this call.
    let vt: &PluginVTable = unsafe { &*vtable };

    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name_str: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };

    // SAFETY: vtable pointer is 'static — extracted from a loaded library that outlives registry.
    let result: Result<PluginHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow()
            .register(*desc, vtable, contract_name_str.to_owned(), vt.contract_id)
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

/// No-op alloc callback.
unsafe extern "C" fn noop_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_host_alloc(size, align)
}

/// No-op free callback.
unsafe extern "C" fn noop_free(
    _rt_ctx: *mut core::ffi::c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    // SAFETY: polyplug_host_free is a safe wrapper around the system allocator.
    unsafe { polyplug_host_free(ptr, size, align) }
}

/// No-op find_by_contract callback.
unsafe extern "C" fn noop_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_by_bundle callback.
unsafe extern "C" fn noop_find_by_bundle(
    _rt_ctx: *mut core::ffi::c_void,
    _bundle_id: u64,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle::null()
}

/// No-op find_all_by_contract callback.
unsafe extern "C" fn noop_find_all_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// No-op resolve_plugin callback.
unsafe extern "C" fn noop_resolve_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _handle: PluginHandle,
) -> *const PluginVTable {
    core::ptr::null()
}

/// No-op get_extension callback.
unsafe extern "C" fn noop_get_extension(
    _rt_ctx: *mut core::ffi::c_void,
    _extension_id: u32,
) -> *const () {
    core::ptr::null()
}

std::thread_local! {
    static GRAPH_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
}

/// Load the test_plugin and call polyplug_init, storing results in GRAPH_REGISTRY.
/// Returns the loaded Library (caller must `std::mem::forget` it to prevent unload).
fn load_and_init_plugin() -> libloading::Library {
    // SAFETY: TEST_PLUGIN_SO is a compiled cdylib with correct ABI.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init signature is `extern "C" fn(*mut c_void, *const HostVTable, *const PluginContext) -> AbiError`.
    let init_fn: libloading::Symbol<
        '_,
        unsafe extern "C" fn(
            *mut core::ffi::c_void,
            *const HostVTable,
            *const PluginContext,
        ) -> AbiError,
    > = unsafe {
        library
            .get(b"polyplug_init\0")
            .expect("polyplug_init symbol not found")
    };

    let host_vtable: HostVTable = HostVTable {
        register_plugin: graph_register_callback,
        alloc: noop_alloc,
        free: noop_free,
        find_by_contract: noop_find_by_contract,
        find_by_bundle: noop_find_by_bundle,
        find_all_by_contract: noop_find_all_by_contract,
        resolve_plugin: noop_resolve_plugin,
        get_extension: noop_get_extension,
    };

    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: POLYPLUG_ABI_VERSION,
        bundle_id: 0,
    };
    // SAFETY: init_fn is valid; host_vtable and ctx live for the duration of this call.
    let init_result: AbiError = unsafe {
        init_fn(
            core::ptr::null_mut(),
            &host_vtable as *const HostVTable,
            &ctx as *const PluginContext,
        )
    };
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must succeed");

    library
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_single_contract_registration_and_lookup() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = Registry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: u64 = contract_id("test.add", 1);

    // Find the test.add contract.
    let handle: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("test.add must be found")
    });

    assert!(!handle.is_null(), "handle must not be null");

    // Resolve the vtable.
    let vtable_ptr: *const PluginVTable = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .resolve(handle)
            .expect("handle must resolve to vtable")
    });

    // SAFETY: vtable_ptr is valid — library is alive (not yet forgotten).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.contract_id, test_add_id,
        "vtable contract_id must match"
    );
    assert_eq!(vtable.function_count, 1, "test.add must have 1 function");

    core::mem::forget(lib);
}

#[test]
fn test_unknown_contract_returns_not_found() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = Registry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let unknown_id: u64 = contract_id("unknown.contract", 1);
    let result: Result<PluginHandle, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().find(unknown_id, 0));

    assert!(
        result.is_err(),
        "lookup of unregistered contract must return Err"
    );

    core::mem::forget(lib);
}

#[test]
fn test_duplicate_registration_is_rejected() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = Registry::new());

    let lib: libloading::Library = load_and_init_plugin();

    // Try to manually register the same contract again — must fail.
    let test_add_id: u64 = contract_id("test.add", 1);

    // Build a fake vtable for the duplicate attempt.
    // function_count=0, so the functions pointer is never dereferenced.
    let fake_vtable: PluginVTable = PluginVTable {
        contract_id: 0xCC4232FAB0410D2B, // test.add@1
        contract_version: 1_u32 << 16,
        function_count: 0,
        functions: core::ptr::null::<*const ()>(),
    };
    let fake_descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"duplicate_adder"),
        contract_name: StringView::from_static(b"test.add"),
        version_major: 1,
        version_minor: 0,
        version_patch: 0,
    };

    // SAFETY: fake_vtable is a local static with 'static lifetime.
    let result: Result<PluginHandle, _> = GRAPH_REGISTRY.with(|cell| unsafe {
        cell.borrow().register(
            fake_descriptor,
            &fake_vtable as *const PluginVTable,
            "test.add".to_owned(),
            test_add_id,
        )
    });

    assert!(
        result.is_err(),
        "second registration of same contract must return DuplicateProvider error"
    );

    core::mem::forget(lib);
}

#[test]
fn test_stale_handle_detected_after_explicit_construction() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = Registry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: u64 = contract_id("test.add", 1);
    let handle: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("must find test.add")
    });

    // Construct a stale handle with wrong generation.
    let stale: PluginHandle = PluginHandle {
        index: handle.index,
        generation: handle.generation + 1,
    };

    let result: Result<*const PluginVTable, _> =
        GRAPH_REGISTRY.with(|cell| cell.borrow().resolve(stale));

    assert!(result.is_err(), "stale handle must return Err");

    core::mem::forget(lib);
}

#[test]
fn test_multi_lookup_consistent() {
    GRAPH_REGISTRY.with(|cell| *cell.borrow_mut() = Registry::new());

    let lib: libloading::Library = load_and_init_plugin();

    let test_add_id: u64 = contract_id("test.add", 1);

    // Repeated lookups must return consistent results.
    let handle_a: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("first find must succeed")
    });
    let handle_b: PluginHandle = GRAPH_REGISTRY.with(|cell| {
        cell.borrow()
            .find(test_add_id, 0)
            .expect("second find must succeed")
    });

    assert_eq!(
        handle_a.index, handle_b.index,
        "repeated lookups must return same slot index"
    );
    assert_eq!(
        handle_a.generation, handle_b.generation,
        "repeated lookups must return same generation"
    );

    core::mem::forget(lib);
}
