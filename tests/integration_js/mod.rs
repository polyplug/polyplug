//! Integration tests for JsLoader (js-quickjs) and JsDenoLoader (js-deno).
//!
//! Tests that verify runtime name, RuntimeBuilder registration, and duplicate detection.
//! Tests requiring pre-built bundle fixtures are marked #[ignore].

#![allow(clippy::expect_used)]

use polyplug::error::LoaderError;
use polyplug::error::RuntimeError;
use polyplug::loader::BundleLoader;
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

#[test]
#[ignore = "requires pre-built bundle.js fixture"]
fn js_quickjs_load_bundle_and_call() {
    // TODO: Implement once a test bundle.js fixture exists
}

#[test]
#[ignore = "requires pre-built bundle.js or index.ts fixture"]
fn js_deno_load_bundle_and_call() {
    // TODO: Implement once a test bundle fixture exists
}
