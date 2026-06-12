#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! End-to-end proof that the runtime's live-instance UB warning fires when a
//! host-mediated guest instance outlives its bundle.
//!
//! Slice 49.2b-ii routes the generated host/peer callers' instance lifecycle
//! through `HostApi::create_guest_instance` / `HostApi::destroy_guest_instance`
//! so the runtime's live-instance counter observes every instance. This test
//! exercises that exact host-mediated path: it loads a STATEFUL contract
//! (`cross.target@1`, whose `create_instance` returns non-null `data`), creates
//! an instance through the runtime's own `HostApi.create_guest_instance` field,
//! deliberately does NOT destroy it, then unloads the bundle and asserts the
//! runtime logged the "live guest instance(s)" warning via the captured logger.
//!
//! Run with:
//!   cargo test -p integration --test integration_live_instance_warning

use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::runtime::Runtime;
use polyplug_abi::GuestContractHandle;
use polyplug_abi::GuestContractInstance;
use polyplug_abi::GuestContractInterface;
use polyplug_abi::HostApi;
use polyplug_abi::types::LogLevel;
use polyplug_native::{NativeConfig, NativeLoader};
use polyplug_utils::{BundleId, guest_contract_id};

const CROSS_TARGET_PLUGIN_DIR: &str = env!("CROSS_TARGET_PLUGIN_DIR");

/// The runtime warns (never blocks) when a bundle is unloaded while one of its
/// contracts still has a live, host-tracked instance. Creating an instance
/// through `HostApi.create_guest_instance` and leaking it must make that warning
/// fire on unload — proving the host-mediated counter sees the instance.
#[test]
fn live_instance_warning_fires_on_unload_after_host_mediated_create() {
    let warnings: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let warnings_clone: Arc<Mutex<Vec<String>>> = Arc::clone(&warnings);

    let rt: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig::default()))
        .logger(move |level: LogLevel, _scope: &str, msg: &str| {
            if level == LogLevel::Warn {
                warnings_clone.lock().unwrap().push(msg.to_owned());
            }
        })
        .build()
        .expect("build runtime");

    rt.load_bundle(Path::new(CROSS_TARGET_PLUGIN_DIR))
        .expect("load cross_target_plugin");

    // Resolve the live interface for the stateful contract.
    let contract_id: u64 = guest_contract_id("cross.target", 1);
    let handle: GuestContractHandle = rt
        .find_guest_contract(contract_id, 0)
        .expect("cross.target must be registered after load");
    let interface: *const GuestContractInterface = rt
        .resolve_guest_contract(handle)
        .expect("handle must resolve to a live interface");

    // Create an instance through the runtime's own host-mediated path. This is
    // the exact mechanism the generated host/peer callers now use, so the
    // runtime's live-instance counter increments for the non-null instance.
    let host: *const HostApi = rt.host_abi();
    // SAFETY: `host` is the runtime's own non-null 'static HostApi pointer; the
    // interface came from `resolve_guest_contract` for a live handle and stays
    // valid while the runtime is alive.
    let host_api: &HostApi = unsafe { &*host };
    // SAFETY: `host`/`interface` are valid as above; a null `args` is honoured by
    // this contract's factory, which ignores its argument.
    let instance: GuestContractInstance =
        unsafe { (host_api.create_guest_instance)(host, interface, core::ptr::null()) };
    assert!(
        !instance.data.is_null(),
        "cross.target is stateful: create_guest_instance must return non-null data"
    );

    // Deliberately leak the instance (no destroy) and unload the bundle.
    let bundle_id: BundleId = BundleId::new("cross_target_plugin");
    rt.unload_bundle(bundle_id)
        .expect("unload should succeed (warning is informational, not blocking)");

    let captured: Vec<String> = warnings.lock().unwrap().clone();
    let warning: Option<&String> = captured
        .iter()
        .find(|w| w.contains("live guest instance(s)"));

    assert!(
        warning.is_some(),
        "expected a 'live guest instance(s)' warning on unload. Captured warnings: {captured:?}"
    );

    let warning: &String = warning.expect("checked is_some above");
    assert!(
        warning.contains("cross_target_plugin"),
        "warning should name the bundle. Got: {warning}"
    );
    assert!(
        warning.contains("use-after-free"),
        "warning should explain the UB hazard. Got: {warning}"
    );

    // Reclaim the leaked instance so the test itself does not leak heap memory.
    // SAFETY: `instance` was produced by this contract's `create_instance` above
    // and has not been destroyed; the interface pointer is still valid because
    // the retire-not-drop model keeps the superseded interface alive for the
    // runtime's lifetime.
    unsafe { (host_api.destroy_guest_instance)(host, interface, instance) };
}
