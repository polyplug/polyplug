//! Integration tests for JsLoader (ts-node/js-node variant).
//!
//! These tests exercise JsLoader loading a compiled .node fixture.
//! Tests that require the fixture are skipped if TEST_JS_PLUGIN is unavailable.

#![allow(clippy::expect_used)]

use polyplug::abi::ABI_OK;
use polyplug::abi::AbiError;
use polyplug::abi::PluginDescriptor;
use polyplug::abi::PluginHandle;
use polyplug::abi::PluginRegistrar;
use polyplug::abi::PluginVTable;
use polyplug::abi::StringView;
use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::error::RegistryError;
use polyplug::loader::BundleLoader;
use polyplug::registry::Registry;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;

const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

/// Process-global mutex to serialize tests that share the LOADED_LIBRARIES static.
static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ─── Helper types and callbacks ───────────────────────────────────────

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

std::thread_local! {
    static JS_REGISTRY: core::cell::RefCell<Registry> =
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
    // SAFETY: descriptor and vtable are valid for this call (ABI contract).
    let desc: &PluginDescriptor = unsafe { &*descriptor };
    // SAFETY: vtable is valid for this call.
    let vt: &PluginVTable = unsafe { &*vtable };
    // SAFETY: contract_name.ptr points to valid UTF-8 bytes.
    let contract_name: &str = unsafe {
        let bytes: &[u8] =
            core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
        core::str::from_utf8_unchecked(bytes)
    };
    // SAFETY: vtable pointer is 'static — extracted from a library in LOADED_LIBRARIES.
    let result: Result<PluginHandle, RegistryError> = JS_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        // SAFETY: vtable pointer is 'static — extracted from a library in LOADED_LIBRARIES.
        // The ABI contract guarantees vtable_ptr remains valid for the library lifetime.
        unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
    });
    match result {
        Ok(_) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(RegistryError::DuplicateProvider { .. }) => AbiError {
            code: ABI_OK,
            message: StringView::null(),
        },
        Err(_) => AbiError {
            code: 1,
            message: StringView::null(),
        },
    }
}

// ─── Tests ──────────────────────────────────────────────────────────

#[test]
fn js_loader_runtime_name_ts_node() {
    let loader: JsLoader = JsLoader::new("ts-node", JsConfig::node_only());
    assert_eq!(loader.runtime_name(), "ts-node");
    assert_eq!(loader.runtime_names(), vec!["ts-node".to_owned()]);
}

#[test]
fn js_loader_runtime_name_js_node() {
    let loader: JsLoader = JsLoader::new("js-node", JsConfig::node_only());
    assert_eq!(loader.runtime_name(), "js-node");
    assert_eq!(loader.runtime_names(), vec!["js-node".to_owned()]);
}

#[test]
fn js_loader_bun_stub_returns_runtime_not_implemented() {
    let loader: JsLoader = JsLoader::new(
        "ts-bun",
        JsConfig {
            node: None,
            bun: Some(polyplug_js::config::BunConfig { bin: None }),
            deno: None,
        },
    );
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new("/nonexistent.node"), &mut registrar);
    match result {
        Err(PolyplugError::Loader(LoaderError::RuntimeNotImplemented { runtime_name })) => {
            assert!(
                runtime_name.contains("bun"),
                "Expected bun in error, got: {runtime_name}"
            );
        }
        other => panic!("Expected RuntimeNotImplemented, got: {other:?}"),
    }
}

#[test]
fn js_loader_deno_stub_returns_runtime_not_implemented() {
    let loader: JsLoader = JsLoader::new(
        "js-deno",
        JsConfig {
            node: None,
            bun: None,
            deno: Some(polyplug_js::config::DenoConfig { bin: None }),
        },
    );
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new("/nonexistent.node"), &mut registrar);
    assert!(matches!(
        result,
        Err(PolyplugError::Loader(
            LoaderError::RuntimeNotImplemented { .. }
        ))
    ));
}

#[test]
fn js_loader_node_config_none_returns_js_binary_not_configured() {
    let loader: JsLoader = JsLoader::new(
        "ts-node",
        JsConfig {
            node: None,
            bun: None,
            deno: None,
        },
    );
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new("/nonexistent.node"), &mut registrar);
    assert!(matches!(
        result,
        Err(PolyplugError::Loader(
            LoaderError::JsBinaryNotConfigured { .. }
        ))
    ));
}

#[test]
fn js_loader_registered_in_runtime_builder() {
    let result: Result<polyplug::runtime::Runtime, polyplug::error::RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new("ts-node", JsConfig::node_only()))
            .loader(JsLoader::new("js-node", JsConfig::node_only()))
            .loader(JsLoader::new("ts-bun", JsConfig::node_only()))
            .loader(JsLoader::new("js-bun", JsConfig::node_only()))
            .loader(JsLoader::new("ts-deno", JsConfig::node_only()))
            .loader(JsLoader::new("js-deno", JsConfig::node_only()))
            .build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder with 6 JsLoaders must succeed: {:?}",
        result.err()
    );
}

#[test]
fn js_loader_duplicate_runtime_name_is_rejected() {
    let result: Result<polyplug::runtime::Runtime, polyplug::error::RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new("ts-node", JsConfig::node_only()))
            .loader(JsLoader::new("ts-node", JsConfig::node_only()))
            .build();
    assert!(
        matches!(
            result,
            Err(polyplug::error::RuntimeError::Loader(
                LoaderError::DuplicateLoader { .. }
            ))
        ),
        "Duplicate ts-node registration must return DuplicateLoader"
    );
}

#[test]
fn js_node_loads_test_plugin_and_registers_vtable() {
    if JS_PLUGIN.is_empty() || JS_PLUGIN == "JS_NOT_AVAILABLE" {
        println!("SKIP: TEST_JS_PLUGIN not available");
        return;
    }
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    let loader: JsLoader = JsLoader::new("ts-node", JsConfig::node_only());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    let result: Result<(), PolyplugError> =
        loader.load(std::path::Path::new(JS_PLUGIN), &mut registrar);
    assert!(
        result.is_ok(),
        "JsLoader must load test_plugin_ts_node.node without error: {:?}",
        result.err()
    );

    // Verify vtable was registered in the thread-local registry.
    let handle: PluginHandle = JS_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry
            .find_by_contract(0xCC4232FAB0410D2B_u64, 0)
            .expect("test.add@1 must be registered after successful load")
    });
    assert_ne!(handle.index, u32::MAX, "handle must be valid");
}

#[test]
fn js_node_plugin_add_function_returns_correct_result() {
    if JS_PLUGIN.is_empty() || JS_PLUGIN == "JS_NOT_AVAILABLE" {
        println!("SKIP: TEST_JS_PLUGIN not available");
        return;
    }
    let _guard: std::sync::MutexGuard<'_, ()> =
        TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());

    // Load the plugin and get the vtable.
    let loader: JsLoader = JsLoader::new("ts-node", JsConfig::node_only());
    let mut registrar: PluginRegistrar = PluginRegistrar {
        register_plugin: registry_register_callback,
        host: core::ptr::null(),
    };
    loader
        .load(std::path::Path::new(JS_PLUGIN), &mut registrar)
        .expect("load must succeed");

    let handle: PluginHandle = JS_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry
            .find_by_contract(0xCC4232FAB0410D2B_u64, 0)
            .expect("test.add@1 contract must be registered")
    });

    // Resolve vtable and call add(3, 4).
    let vtable_ptr: *const PluginVTable = JS_REGISTRY.with(|reg_cell| {
        let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
        registry.resolve(handle).expect("handle must resolve")
    });
    // SAFETY: vtable_ptr is 'static from the loaded .node library (never unloaded).
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    // SAFETY: functions[0] is js_test_add with signature fn(*const (), *mut ()) -> AbiError.
    let add_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
        // SAFETY: cast to generic dispatch signature; arg types enforced by test (AddArgs matches).
        unsafe { core::mem::transmute(*vtable.functions) };
    let args: AddArgs = AddArgs { a: 3, b: 4 };
    let mut result: u32 = 0_u32;
    // SAFETY: args and result have the correct types for the test.add@1 ABI.
    let error: AbiError = unsafe {
        add_fn(
            &args as *const AddArgs as *const (),
            &mut result as *mut u32 as *mut (),
        )
    };
    assert_eq!(error.code, ABI_OK, "add function must return ABI_OK");
    assert_eq!(result, 7_u32, "add(3, 4) must return 7");
}
