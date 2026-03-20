//! Cross-language tests for js-deno guest plugin (separate from 36-combination matrix).
//!
//! These tests verify that js-deno index.ts files can be loaded by the JsDenoLoader
//! and that vtable dispatch works correctly.
//!
//! Note: js-deno tests are separate from the main cross_language matrix (user decision).

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::HostVTable;
use polyplug_abi::PluginContext;
use polyplug_abi::PluginDescriptor;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::StringView;
use polyplug_abi::ABI_OK;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;

#[allow(dead_code)]
const TEST_JS_DENO_PLUGIN: &str = env!("TEST_JS_DENO_PLUGIN");
/// Path used for the JsDenoLoader load call — uses pre-built bundle.js (same as integration_js).
const TEST_JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");
const TEST_PLUGIN_SO: &str = env!("TEST_PLUGIN_SO");

/// Mutex to serialise all deno tests — shared Deno VM state is not Send-safe across threads.
static DENO_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

// ─── Thread-local registry ────────────────────────────────────────────────────

std::thread_local! {
    static DENO_REGISTRY: core::cell::RefCell<Registry> =
        core::cell::RefCell::new(Registry::new());
}

unsafe extern "C" fn registry_register_callback(
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
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    let vt: &PluginVTable = unsafe { &*vtable };
    // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
    // &'static str contract name — guaranteed valid UTF-8 by construction.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes) // SAFETY: see comment above
    };
    let result: Result<PluginHandle, polyplug::error::RegistryError> =
        DENO_REGISTRY.with(|reg_cell| {
            let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
            unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
        });
    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(polyplug::error::RegistryError::DuplicateProvider { .. }) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

fn reset_registry() {
    DENO_REGISTRY.with(|cell| {
        *cell.borrow_mut() = Registry::new();
    });
}

fn get_vtable() -> *const PluginVTable {
    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = DENO_REGISTRY.with(|cell| {
        cell.borrow()
            .find(contract_id, 0)
            .expect("test.add must be registered after load")
    });
    DENO_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Load the native Rust test plugin via libloading — verifies the host side works correctly
/// when the host itself is operating in a "cross-language" context (e.g. alongside deno).
#[test]
fn test_jsdeno_host_rust_guest() {
    if TEST_PLUGIN_SO.is_empty() {
        eprintln!("skipping: TEST_PLUGIN_SO not set");
        return;
    }

    reset_registry();

    // SAFETY: TEST_PLUGIN_SO is an absolute path to a compiled cdylib with the polyplug ABI.
    let library: libloading::Library = unsafe {
        libloading::Library::new(TEST_PLUGIN_SO).expect("failed to load test_plugin shared library")
    };

    // SAFETY: polyplug_init matches the expected ABI signature.
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
        register_plugin: registry_register_callback,
        alloc: stub_alloc,
        free: stub_free,
        find_by_contract: stub_find_by_contract,
        find_by_bundle: stub_find_by_bundle,
        find_all_by_contract: stub_find_all_by_contract,
        resolve_plugin: stub_resolve_plugin,
        get_extension: stub_get_extension,
    };

    // SAFETY: init_fn is valid; host_vtable and ctx live for the call duration.
    let ctx: PluginContext = PluginContext {
        bundle_path: StringView::null(),
        host_abi_version: polyplug_abi::POLYPLUG_ABI_VERSION,
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
    assert_eq!(init_result.code, ABI_OK, "polyplug_init must return ABI_OK");

    let vtable_ptr: *const PluginVTable = get_vtable();
    // SAFETY: vtable_ptr is valid — plugin library is still loaded.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert!(
        vtable.function_count >= 1,
        "test.add vtable must have at least 1 function"
    );

    let args: AddArgs = AddArgs { a: 10, b: 20 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr is the first entry in the vtable, matching the add(AddArgs)->u32 signature.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add dispatch must return ABI_OK");
    assert_eq!(out, 30_u32, "add(10, 20) must equal 30");
    // SAFETY: keep library alive until after last call.
    core::mem::forget(library);
}

/// Load the js-deno plugin via JsDenoLoader and dispatch test.add.
#[test]
fn test_rust_host_jsdeno_guest() {
    if TEST_JS_PLUGIN.is_empty() {
        eprintln!("skipping: TEST_JS_PLUGIN not set");
        return;
    }

    let _guard: std::sync::MutexGuard<'_, ()> =
        DENO_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    reset_registry();

    let rt: Runtime = Runtime::builder()
        .loader(JsDenoLoader::new(JsDenoConfig {}))
        .build()
        .expect("failed to build runtime");

    let result: Result<(), polyplug::error::PolyplugError> =
        rt.load_bundle(std::path::Path::new(TEST_JS_PLUGIN));
    assert!(
        result.is_ok(),
        "JsDenoLoader::load() failed: {:?}",
        result.err()
    );

    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = rt
        .find_by_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const PluginVTable = rt.resolve_plugin(handle).expect("handle must be valid");
    // SAFETY: vtable_ptr is valid — deno runtime keeps the plugin alive for the test duration.
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.function_count, 4,
        "test.add must register 4 functions"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    // SAFETY: fn_ptr at offset 0 is the add function; argument layout matches AddArgs.
    let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
    let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        unsafe { core::mem::transmute(fn_ptr) };
    let result: AbiError = unsafe {
        dispatch_fn(
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
}

// ─── HostVTable stub functions for native .so tests ─────────────────────────────

/// Stub alloc callback using the global allocator.
unsafe extern "C" fn stub_alloc(
    _rt_ctx: *mut core::ffi::c_void,
    size: usize,
    align: usize,
) -> *mut u8 {
    polyplug_abi::ffi::polyplug_host_alloc(size, align)
}

/// Stub free callback using the global allocator.
unsafe extern "C" fn stub_free(
    _rt_ctx: *mut core::ffi::c_void,
    ptr: *mut u8,
    size: usize,
    align: usize,
) {
    unsafe { polyplug_abi::ffi::polyplug_host_free(ptr, size, align) }
}

/// Stub find_by_contract — returns a null handle.
unsafe extern "C" fn stub_find_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

/// Stub find_by_bundle — returns a null handle.
unsafe extern "C" fn stub_find_by_bundle(
    _rt_ctx: *mut core::ffi::c_void,
    _bundle_id: u64,
    _contract_id: u64,
    _min_version: u32,
) -> PluginHandle {
    PluginHandle {
        index: u32::MAX,
        generation: 0,
    }
}

/// Stub find_all_by_contract — returns 0.
unsafe extern "C" fn stub_find_all_by_contract(
    _rt_ctx: *mut core::ffi::c_void,
    _contract_id: u64,
    _min_version: u32,
    _out: *mut PluginHandle,
    _out_cap: usize,
) -> usize {
    0
}

/// Stub resolve_plugin — returns null.
unsafe extern "C" fn stub_resolve_plugin(
    _rt_ctx: *mut core::ffi::c_void,
    _handle: PluginHandle,
) -> *const PluginVTable {
    core::ptr::null()
}

/// Stub get_extension — returns null.
unsafe extern "C" fn stub_get_extension(
    _rt_ctx: *mut core::ffi::c_void,
    _extension_id: u32,
) -> *const () {
    core::ptr::null()
}
