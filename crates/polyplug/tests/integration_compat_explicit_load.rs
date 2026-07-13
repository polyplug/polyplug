//! Integration test: the runtime's configured `Compatibility` is honored on the
//! explicit programmatic load path (`Runtime::load_bundle_from_source`).
//!
//! Regression guard for the bug where `load_bundle` / `load_bundle_from_source`
//! hardcoded `Compatibility::default()` (Strict) in the `LoadOptions` they built,
//! ignoring `RuntimeConfig.compatibility`. With that bug, every Relaxed-gated
//! runtime warning (function_count mismatch in particular) was unreachable
//! through the FFI/programmatic load path.
//!
//! The bundle below declares `function_count = { "compat.test@1" = 2 }` but its
//! loader registers a native interface that exports only ONE function. That is a
//! function_count mismatch:
//! - a Relaxed-configured runtime must LOAD it and DELIVER a Warn to the logger,
//! - a Strict-configured runtime must REJECT it.

#![allow(clippy::expect_used)]

use core::ffi::c_void;
use core::ptr;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::Runtime;
use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, BundleSource};
use polyplug_abi::{AbiError, HostApi};

use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::dispatch::{DispatchMechanisms, DispatchType, NativeDispatch};
use polyplug_abi::guest::{GuestContractInstance, GuestContractInterface};
use polyplug_abi::plugin::PluginDescriptor;
use polyplug_abi::runtime::{Compatibility, RuntimeConfig};
use polyplug_abi::types::{LogLevel, StringView, Version};
use polyplug_common::ManifestData;
use polyplug_utils::bundle_id as compute_bundle_id;
use polyplug_utils::{BundleId, GuestContractId};

const CONTRACT_NAME: &str = "compat.test";
const BUNDLE_NAME: &str = "compat_test_bundle";

unsafe extern "C" fn noop_create_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _ctx: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null (just checked) and writable per the ABI contract.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn noop_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}
unsafe extern "C" fn noop_dispatch(
    _instance: GuestContractInstance,
    _args: *const (),
    _out: *mut (),
    out_err: *mut AbiError,
) {
    if !out_err.is_null() {
        // SAFETY: out_err is non-null (just checked) and writable by the caller.
        unsafe { out_err.write(AbiError::ok()) };
    }
}

struct NativeFunctionTable([*const (); 1]);

// SAFETY: the table contains one immutable `'static` function pointer.
unsafe impl Sync for NativeFunctionTable {}

static NATIVE_FUNCTIONS: NativeFunctionTable = NativeFunctionTable([noop_dispatch as *const ()]);

/// Leak a Native-dispatch interface that reports exactly `function_count`
/// exported functions. The leak is intentional: the ABI requires the interface
/// pointer to stay valid for the library lifetime, and this test holds it for the
/// process lifetime.
fn leak_native_interface(function_count: u32) -> &'static GuestContractInterface {
    Box::leak(Box::new(GuestContractInterface {
        contract_id: GuestContractId::new(CONTRACT_NAME, 1_u32),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        adapter_context: ptr::null_mut(),
        create_instance: noop_create_instance,
        destroy_instance: noop_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count,
                functions: NATIVE_FUNCTIONS.0.as_ptr(),
            },
        },
    }))
}

/// In-process loader that registers a single Native contract for the loaded
/// bundle, mirroring how a real plugin's `polyplug_init` registers its contract.
/// The interface reports `actual_function_count` exported functions.
struct CompatTestLoader {
    actual_function_count: u32,
}

impl BundleLoader for CompatTestLoader {
    fn loader_name(&self) -> &'static str {
        "compat-test"
    }

    fn supports_hot_reload(&self) -> bool {
        false
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(bundle_id.id());

        let interface: &'static GuestContractInterface =
            leak_native_interface(self.actual_function_count);
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(BUNDLE_NAME.as_bytes()),
            contract_name: StringView::from_static(CONTRACT_NAME.as_bytes()),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let host: *const HostApi = runtime.host_abi();
        let mut registration: AbiError = AbiError::ok();
        // SAFETY: the runtime owns `host`; the descriptor and leaked interface remain
        // valid for this synchronous registration callback.
        unsafe {
            ((*host).register_guest_contract)(host, &descriptor, interface, &mut registration);
        }
        runtime.pop_init_bundle_id();
        if registration.is_ok() {
            Ok(())
        } else {
            Err(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!(
                    "guest registration failed with ABI code {}",
                    registration.code
                ),
            })
        }
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), LoaderError> {
        Err(LoaderError::HotReloadUnsupported {
            loader_name: self.loader_name().to_owned(),
        })
    }
}

/// Build a manifest whose declared `function_count` for `compat.test@1` is 2,
/// deliberately one more than the loader's interface will report (1).
fn mismatched_manifest() -> ManifestData {
    let mut function_count: HashMap<String, u32> = HashMap::new();
    function_count.insert(format!("{CONTRACT_NAME}@1"), 2_u32);

    ManifestData {
        loader: "compat-test".to_owned(),
        name: BUNDLE_NAME.to_owned(),
        dependencies: Vec::new(),
        id: compute_bundle_id(BUNDLE_NAME),
        version: "1.0".to_owned(),
        file: "inline.code".to_owned(),
        provides: vec![format!("{CONTRACT_NAME}@1")],
        function_count,
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
        path: PathBuf::new(),
    }
}

/// Raw extern "C" logger callback that appends `(level, scope, message)` to the
/// `Mutex<Vec<...>>` behind `user_data`. No statics — the sink outlives the runtime.
unsafe extern "C" fn capture_log(
    user_data: *mut c_void,
    level: u32,
    scope: StringView,
    message: StringView,
) {
    // SAFETY: each test passes a pointer to a Box<Mutex<Vec<...>>> sink that
    // outlives the runtime emitting the calls.
    let sink: &Mutex<Vec<(u32, String, String)>> =
        unsafe { &*(user_data as *const Mutex<Vec<(u32, String, String)>>) };
    // SAFETY: the runtime guarantees both views are valid UTF-8 for the duration
    // of the call; the bytes are copied immediately.
    let (scope_owned, message_owned): (String, String) =
        unsafe { (scope.as_str().to_owned(), message.as_str().to_owned()) };
    sink.lock()
        .expect("sink lock")
        .push((level, scope_owned, message_owned));
}

fn build_runtime(
    compatibility: Compatibility,
    sink: &Mutex<Vec<(u32, String, String)>>,
) -> Arc<Runtime> {
    let config: RuntimeConfig = RuntimeConfig {
        compatibility,
        log: Some(capture_log),
        log_user_data: sink as *const Mutex<Vec<(u32, String, String)>> as *mut c_void,
        log_max_level: LogLevel::Trace as u32,
        ..Default::default()
    };
    Runtime::builder()
        .config(config)
        .loader(CompatTestLoader {
            actual_function_count: 1,
        })
        .build()
        .expect("build runtime")
}

#[test]
fn relaxed_runtime_loads_mismatched_bundle_and_warns() {
    let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
    let runtime: Arc<Runtime> = build_runtime(Compatibility::Relaxed, &sink);

    let result: Result<(), RuntimeError> =
        runtime.load_bundle_from_source(mismatched_manifest(), BundleSource::Code(String::new()));

    assert!(
        result.is_ok(),
        "Relaxed runtime must LOAD the function_count-mismatched bundle, got: {:?}",
        result.err()
    );

    let captured: Vec<(u32, String, String)> = sink.lock().expect("sink lock").clone();
    assert!(
        captured.iter().any(|(level, scope, msg)| {
            *level == LogLevel::Warn as u32
                && scope == "runtime"
                && msg.contains("declared function_count 2 but interface exports 1")
        }),
        "Relaxed runtime must DELIVER the function_count-mismatch warning to the logger, got: {captured:?}"
    );
}

#[test]
fn strict_runtime_rejects_mismatched_bundle() {
    let sink: Box<Mutex<Vec<(u32, String, String)>>> = Box::new(Mutex::new(Vec::new()));
    let runtime: Arc<Runtime> = build_runtime(Compatibility::Strict, &sink);

    let result: Result<(), RuntimeError> =
        runtime.load_bundle_from_source(mismatched_manifest(), BundleSource::Code(String::new()));

    match result {
        Err(RuntimeError::Loader(_)) => {}
        other => panic!(
            "Strict runtime must REJECT the function_count-mismatched bundle with a Loader error, got: {other:?}"
        ),
    }
}

#[test]
fn external_source_load_validates_canonical_metadata_before_acquisition_fields() {
    let runtime: Arc<Runtime> = Runtime::builder().build().expect("build runtime");
    let mut manifest: ManifestData = mismatched_manifest();
    manifest.id = manifest.id.wrapping_add(1);
    manifest.file.clear();

    let result = runtime.load_bundle_from_source(manifest, BundleSource::Code(String::new()));
    assert!(
        matches!(
            result,
            Err(RuntimeError::Loader(LoaderError::BundleTampered { .. }))
        ),
        "external source loading must keep full manifest validation and report canonical id tampering before missing acquisition fields"
    );
}
