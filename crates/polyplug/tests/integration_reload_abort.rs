#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Integration test: a failed reload must not leak pending slots.
//!
//! During a reload the loader's `polyplug_init` registers the new version's
//! contracts into fresh "pending" slots (kept out of the find index until the
//! swap). If `loader.reload()` then fails, those pending slots must be purged —
//! otherwise they accumulate in the bundle's `plugin_slots` across every retry.
//!
//! These tests drive the real `Runtime::reload_bundle` path with an in-process
//! mock loader whose `reload` registers a pending contract and then returns an
//! error, simulating an init that fails after partial registration.

use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::error::{LoaderError, RuntimeError};
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug::runtime::Runtime;
use polyplug_abi::{
    Compatibility, DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface,
    HostApi, NativeDispatch, PluginDescriptor, RuntimeConfig, StringView, Version,
};
use polyplug_utils::{BundleId, GuestContractId};

const MOCK_FNS_EMPTY: [*const (); 0] = [];

unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// In-process loader that registers a single contract for its bundle during
/// `load`. During `reload` it registers the contract again (creating a pending
/// slot) and then optionally fails, simulating an init that errors after partial
/// registration.
struct AbortLoader {
    contract_id: u64,
    /// When true, `reload` registers a pending slot and then returns an error.
    fail_reload: Arc<AtomicBool>,
}

impl AbortLoader {
    fn register(&self, manifest: &ManifestData, runtime: &Runtime) {
        let interface: &'static GuestContractInterface =
            Box::leak(Box::new(GuestContractInterface {
                contract_id: GuestContractId::from_u64(self.contract_id),
                contract_version: Version {
                    major: 1,
                    minor: 0,
                    patch: 0,
                },
                dispatch_type: DispatchType::Native,
                create_instance: noop_create_instance,
                destroy_instance: noop_destroy_instance,
                dispatch: DispatchMechanisms {
                    native: NativeDispatch {
                        function_count: 0,
                        functions: MOCK_FNS_EMPTY.as_ptr(),
                    },
                },
            }));
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"abort"),
            contract_name: StringView::from_static(b"abort.contract"),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        // SAFETY: interface is leaked and lives for the process lifetime.
        unsafe {
            runtime.registry().register_guest_contract(
                descriptor,
                interface,
                "abort.contract".to_owned(),
                bundle_id,
            )
        }
        .expect("contract registration should succeed");
    }
}

impl BundleLoader for AbortLoader {
    fn loader_name(&self) -> &'static str {
        "abort-loader"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        self.register(manifest, runtime);
        Ok(())
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
        // Register the contract first — this creates a pending slot exactly as a
        // real plugin's polyplug_init would before its init logic fails.
        self.register(manifest, runtime);
        if self.fail_reload.load(Ordering::SeqCst) {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: "simulated init failure after partial registration".to_owned(),
            }));
        }
        Ok(())
    }
}

fn hot_reload_config() -> RuntimeConfig {
    RuntimeConfig {
        compatibility: Compatibility::Yolo,
        unload_mode: polyplug_abi::UnloadMode::Retire,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
        ..Default::default()
    }
}

fn write_bundle(temp: &tempfile::TempDir, bundle_name: &str) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    std::fs::write(bundle_dir.join("dummy.so"), b"").expect("write dummy so");

    let bundle_id: u64 = polyplug_utils::bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {bundle_id}\n\
         name = \"{bundle_name}\"\n\
         loader = \"abort-loader\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

#[test]
fn failed_reload_does_not_leak_pending_slots() {
    let temp: tempfile::TempDir = tempfile::TempDir::new().expect("temp dir");

    let contract_id: u64 = polyplug_utils::guest_contract_id("abort.contract", 1_u32);
    let fail_reload: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(hot_reload_config())
        .loader(AbortLoader {
            contract_id,
            fail_reload: Arc::clone(&fail_reload),
        })
        .build()
        .expect("runtime build should succeed");

    let bundle_path: PathBuf = write_bundle(&temp, "abort_bundle");
    runtime.load_bundle(bundle_path.as_path()).expect("load");

    let bundle_id: BundleId = BundleId::new("abort_bundle");

    // After the initial load: exactly one live slot, findable.
    let baseline_slots: usize = runtime.registry().get_bundle_plugin_slots(bundle_id).len();
    assert_eq!(baseline_slots, 1, "one slot after initial load");
    assert!(
        runtime.find_guest_contract(contract_id, 0).is_ok(),
        "contract must be findable after load"
    );

    // Make reload fail, then attempt it three times. Each failed reload registers a
    // pending slot; without the abort-purge they would accumulate.
    fail_reload.store(true, Ordering::SeqCst);
    for attempt in 0..3 {
        let err: RuntimeError = runtime
            .reload_bundle(bundle_path.as_path())
            .expect_err("reload must fail when init fails");
        assert!(
            matches!(err, RuntimeError::Loader(LoaderError::InitFailed { .. })),
            "attempt {attempt}: expected InitFailed, got {err:?}"
        );

        // The pending slot from the failed attempt must be purged: slot count and
        // find results stay exactly as the baseline.
        let slots_now: usize = runtime.registry().get_bundle_plugin_slots(bundle_id).len();
        assert_eq!(
            slots_now, baseline_slots,
            "attempt {attempt}: failed reload must not accumulate pending slots"
        );
        assert!(
            runtime.find_guest_contract(contract_id, 0).is_ok(),
            "attempt {attempt}: the pre-reload contract must remain findable"
        );
    }

    // A subsequent successful reload still works and keeps a single live slot.
    fail_reload.store(false, Ordering::SeqCst);
    runtime
        .reload_bundle(bundle_path.as_path())
        .expect("successful reload after prior failures");

    let slots_after_success: usize = runtime.registry().get_bundle_plugin_slots(bundle_id).len();
    assert_eq!(
        slots_after_success, baseline_slots,
        "successful reload must reconcile to a single live slot"
    );
    assert!(
        runtime.find_guest_contract(contract_id, 0).is_ok(),
        "contract must be findable after a successful reload"
    );
}
