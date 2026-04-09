//! Integration tests for JsLoader (js-quickjs).

#![allow(clippy::expect_used)]
#![allow(clippy::undocumented_unsafe_blocks)]

use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
use polyplug::runtime::Runtime;
use polyplug_abi::AbiError;
use polyplug_abi::AbiErrorCode;
use polyplug_abi::DispatchType;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::GuestContractHandle;
use polyplug_utils::guest_contract_id;
use polyplug_js::JsConfig;
use polyplug_js::JsLoader;

#[test]
fn js_quickjs_loader_runtime_name() {
    let loader: JsLoader = JsLoader::new(JsConfig {});
    assert_eq!(loader.runtime_name(), "js-quickjs");
    assert_eq!(loader.runtime_names(), vec!["js-quickjs".to_owned()]);
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

const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN");

#[repr(C)]
struct AddArgs {
    a: u32,
    b: u32,
}

#[test]
fn js_quickjs_load_bundle_and_call() {
    let rt: Runtime = Runtime::builder()
        .loader(JsLoader::new(JsConfig {}))
        .build()
        .expect("failed to build runtime");
    let result: Result<(), RuntimeError> = rt.load_bundle(std::path::Path::new(JS_PLUGIN));
    assert!(
        result.is_ok(),
        "JsLoader::load() failed: {:?}",
        result.err()
    );

    let contract_id: u64 = guest_contract_id("test.add", 1);
    let handle: GuestContractHandle = rt
        .find_by_contract(contract_id, 0)
        .expect("test.add must be registered after load");
    let vtable_ptr: *const GuestContractInterface = rt
        .resolve_plugin(handle)
        .expect("handle must be valid")
        .vtable();
    let vtable: &GuestContractInterface = unsafe { &*vtable_ptr };
    assert_eq!(
        vtable.function_count, 4,
        "test.add must register 4 functions"
    );
    assert_eq!(
        vtable.dispatch_type,
        DispatchType::VirtualMachine,
        "JS loader must use VM dispatch"
    );

    let args: AddArgs = AddArgs { a: 3, b: 5 };
    let mut out: u32 = 0_u32;
    let result: AbiError = unsafe {
        (vtable.dispatch.vm.call)(
            vtable.dispatch.vm.loader_data,
            0,
            &args as *const AddArgs as *const (),
            &mut out as *mut u32 as *mut (),
        )
    };
    assert_eq!(result.code, AbiErrorCode::Ok, "add must return ABI_OK");
    assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
}
