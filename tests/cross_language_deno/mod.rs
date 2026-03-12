//! Cross-language tests for js-deno guest plugin (separate from 36-combination matrix).
//!
//! These tests verify that js-deno index.ts files can be loaded by the JsDenoLoader
//! and that vtable dispatch works correctly.
//!
//! Note: js-deno tests are separate from the main cross_language matrix (user decision).

#![allow(clippy::expect_used)]

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::PluginContext;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
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
    let contract_id: u64 = polyplug::abi::contract_id("test.add", 1);
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

    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };

    let result: Result<(), polyplug::error::PolyplugError> =
        loader.load(std::path::Path::new(TEST_JS_PLUGIN), &mut registrar);
    assert!(
        result.is_ok(),
        "JsDenoLoader::load() failed: {:?}",
        result.err()
    );

    let vtable_ptr: *const PluginVTable = get_vtable();
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
