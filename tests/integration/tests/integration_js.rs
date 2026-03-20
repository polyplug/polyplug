//! Integration tests for JsLoader (js-quickjs) and JsDenoLoader (js-deno).
//!
//! Tests that verify runtime name, RuntimeBuilder registration, and duplicate detection.
//! Tests requiring pre-built bundle fixtures are marked #[ignore].

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::LoaderError;
use polyplug::error::PolyplugError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::PluginHandle;
use polyplug_abi::PluginVTable;
use polyplug_abi::ABI_OK;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;
use polyplug_js_deno::JsDenoConfig;
use polyplug_js_deno::JsDenoLoader;

#[test]
fn js_quickjs_loader_runtime_name() {
    let loader: JsLoader = JsLoader::new(JsConfig {});
    assert_eq!(loader.runtime_name(), "js-quickjs");
    assert_eq!(loader.runtime_names(), vec!["js-quickjs".to_owned()]);
}

#[test]
fn js_deno_loader_runtime_name() {
    let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
    assert_eq!(loader.runtime_name(), "js-deno");
    assert_eq!(loader.runtime_names(), vec!["js-deno".to_owned()]);
}

#[test]
fn js_quickjs_registered_in_runtime_builder() {
    let result: Result<polyplug::runtime::Runtime, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new(JsConfig {}))
            .build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder with JsLoader must succeed: {:?}",
        result.err()
    );
}

#[test]
fn js_deno_registered_in_runtime_builder() {
    let result: Result<polyplug::runtime::Runtime, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsDenoLoader::new(JsDenoConfig {}))
            .build();
    assert!(
        result.is_ok(),
        "RuntimeBuilder with JsDenoLoader must succeed: {:?}",
        result.err()
    );
}

#[test]
fn js_quickjs_duplicate_runtime_name_is_rejected() {
    let result: Result<polyplug::runtime::Runtime, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsLoader::new(JsConfig {}))
            .loader(JsLoader::new(JsConfig {}))
            .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))
        ),
        "Duplicate js-quickjs registration must return DuplicateLoader"
    );
}

#[test]
fn js_deno_duplicate_runtime_name_is_rejected() {
    let result: Result<polyplug::runtime::Runtime, RuntimeError> =
        polyplug::runtime::Runtime::builder()
            .loader(JsDenoLoader::new(JsDenoConfig {}))
            .loader(JsDenoLoader::new(JsDenoConfig {}))
            .build();
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))
        ),
        "Duplicate js-deno registration must return DuplicateLoader"
    );
}

static JS_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

#[test]
fn js_quickjs_load_bundle_and_call() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        JS_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let rt: Runtime = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), PolyplugError> = rt.load_bundle(std::path::Path::new(JS_PLUGIN));
    assert!(
        result.is_ok(),
        "JsLoader::load() failed: {:?}",
        result.err()
    );

    let contract_id: u64 = polyplug_abi::contract_id("test.add", 1);
    let handle: PluginHandle = rt
        .find_by_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const PluginVTable = rt.resolve_plugin(handle).expect("handle must be valid");
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.function_count, 4,
        "test.add must register 4 functions"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
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

#[test]
fn js_deno_load_bundle_and_call() {
    let _guard: std::sync::MutexGuard<'_, ()> =
        JS_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    let rt: Runtime = Runtime::builder()
        .loader(JsDenoLoader::new(JsDenoConfig {}))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), PolyplugError> = rt.load_bundle(std::path::Path::new(JS_PLUGIN));
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
    let vtable: &PluginVTable = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.function_count, 4,
        "test.add must register 4 functions"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
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
