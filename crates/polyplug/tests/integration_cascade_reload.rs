#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

//! Integration test: cascade hot-reload.
//!
//! When a bundle is hot-reloaded, every other loaded bundle that declared a
//! dependency on one of the reloaded bundle's contracts AND opted in via
//! `needs_reinit_on_dep_reload = true` is reloaded automatically.
//!
//! These tests drive the real `Runtime::reload_bundle` path with in-process mock
//! loaders. Each mock loader registers its bundle's contract during `load`,
//! re-registers it during `reload` (so `apply_reload_swap` can reconcile the
//! pre-reload slot), and records whether `reload` was invoked via an
//! `Arc<AtomicBool>` flag — letting the test assert which bundles cascaded.

use core::sync::atomic::AtomicBool;
use core::sync::atomic::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

use core::ffi::c_void;
use core::ptr;
use polyplug::error::LoaderError;
use polyplug::loader::BundleLoader;
use polyplug::loader::BundleSource;
use polyplug::runtime::Runtime;

use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::{
    AbiError, Compatibility, DispatchMechanisms, DispatchType, GuestContractInstance,
    GuestContractInterface, HostApi, NativeDispatch, PluginDescriptor, RuntimeConfig, StringView,
    Version,
};
use polyplug_common::ManifestData;
use polyplug_utils::bundle_id;
use polyplug_utils::guest_contract_id;
use polyplug_utils::{BundleId, GuestContractId};
use std::fs;
use tempfile::TempDir;

// ─── Shared callbacks ──────────────────────────────────────────────────────────

const MOCK_FNS_EMPTY: [*const (); 0] = [];

unsafe extern "C" fn noop_create_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
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

// ─── Mock loader ───────────────────────────────────────────────────────────────

/// In-process loader that registers a single contract for its bundle during both
/// `load` and `reload`, and records whether `reload` was ever called.
struct CascadeLoader {
    loader_name: &'static str,
    contract_id: u64,
    contract_name: &'static str,
    reload_called: Arc<AtomicBool>,
}

impl CascadeLoader {
    fn register(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), LoaderError> {
        let interface: GuestContractInterface = GuestContractInterface {
            contract_id: GuestContractId::from_u64(self.contract_id),
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
                    function_count: 0,
                    functions: MOCK_FNS_EMPTY.as_ptr(),
                },
            },
        };
        let descriptor: PluginDescriptor = PluginDescriptor {
            name: StringView::from_static(b"cascade"),
            contract_name: StringView::from_static(self.contract_name.as_bytes()),
            version: Version {
                major: 1,
                minor: 0,
                patch: 0,
            },
        };
        let host: *const HostApi = runtime.host_abi();
        let mut registration: AbiError = AbiError::ok();
        // SAFETY: the host belongs to runtime; descriptor, interface, and error
        // output remain valid for the synchronous registration call.
        unsafe {
            ((*host).register_guest_contract)(host, &descriptor, &interface, &mut registration);
        }
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
}

impl BundleLoader for CascadeLoader {
    fn loader_name(&self) -> &'static str {
        self.loader_name
    }

    fn supports_hot_reload(&self) -> bool {
        true
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &BundleSource,
        runtime: &Runtime,
    ) -> Result<(), LoaderError> {
        runtime.push_init_bundle_id(BundleId::new(&manifest.name).id());
        let result: Result<(), LoaderError> = self.register(manifest, runtime);
        runtime.pop_init_bundle_id();
        result
    }

    fn reload(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), LoaderError> {
        self.reload_called.store(true, Ordering::SeqCst);
        runtime.push_init_bundle_id(BundleId::new(&manifest.name).id());
        let result: Result<(), LoaderError> = self.register(manifest, runtime);
        runtime.pop_init_bundle_id();
        result
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hot_reload_config() -> RuntimeConfig {
    RuntimeConfig {
        compatibility: Compatibility::Yolo,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: ptr::null_mut(),
        ..Default::default()
    }
}

/// Write a bundle directory with a manifest. `dep_contract` optionally declares a
/// `[[dependency]]` on that bare contract name (its `contract_id` is derived as
/// `guest_contract_id(dep_contract, 1)`, matching `min_version = "1.0"`), and
/// `needs_reinit` toggles the cascade opt-in.
///
/// The dependency `contract` name and its `contract_id` are kept consistent here so
/// the bundle passes `ManifestData::validate`'s contract_id cross-check.
fn write_bundle(
    temp: &TempDir,
    bundle_name: &str,
    loader_name: &str,
    provided_contract: &str,
    needs_reinit: bool,
    dep_contract: Option<&str>,
) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    fs::write(bundle_dir.join("dummy.so"), b"").expect("write dummy so");

    let bundle_id_val: u64 = bundle_id(bundle_name);
    let mut manifest: String = format!(
        "id = {bundle_id_val}\n\
         name = \"{bundle_name}\"\n\
         loader = \"{loader_name}\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n\
         provides = [\"{provided_contract}\"]\n\
         needs_reinit_on_dep_reload = {needs_reinit}\n"
    );
    if let Some(contract_name) = dep_contract {
        let contract_id: u64 = guest_contract_id(contract_name, 1_u32);
        manifest.push_str(&format!(
            "\n[[dependency]]\n\
             kind = \"contract\"\n\
             contract = \"{contract_name}\"\n\
             min_version = \"1.0\"\n\
             contract_id = {contract_id}\n"
        ));
    }
    fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

// ─── Test 1: opt-out bundle does not cascade ────────────────────────────────────

#[test]
fn cascade_reload_disabled_does_not_trigger() {
    let temp: TempDir = TempDir::new().expect("temp dir");

    let a_contract_id: u64 = guest_contract_id("dep.contract", 1_u32);
    let b_reload_called: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(hot_reload_config())
        .loader(CascadeLoader {
            loader_name: "cascade-a",
            contract_id: a_contract_id,
            contract_name: "dep.contract",
            reload_called: Arc::new(AtomicBool::new(false)),
        })
        .loader(CascadeLoader {
            loader_name: "cascade-b",
            contract_id: guest_contract_id("b.contract", 1_u32),
            contract_name: "b.contract",
            reload_called: Arc::clone(&b_reload_called),
        })
        .build()
        .expect("runtime build should succeed");

    let a_path: PathBuf = write_bundle(&temp, "bundle_a", "cascade-a", "dep.contract", false, None);
    // B depends on A's contract but opts OUT of cascade reload.
    let b_path: PathBuf = write_bundle(
        &temp,
        "bundle_b",
        "cascade-b",
        "b.contract",
        false,
        Some("dep.contract"),
    );

    runtime.load_bundle(a_path.as_path()).expect("load A");
    runtime.load_bundle(b_path.as_path()).expect("load B");

    runtime.reload_bundle(a_path.as_path()).expect("reload A");

    assert!(
        !b_reload_called.load(Ordering::SeqCst),
        "B must NOT be reloaded when needs_reinit_on_dep_reload is false"
    );
}

// ─── Test 2: opt-in dependent cascades ──────────────────────────────────────────

#[test]
fn cascade_reload_enabled_triggers_dependent() {
    let temp: TempDir = TempDir::new().expect("temp dir");

    let a_contract_id: u64 = guest_contract_id("dep.contract", 1_u32);
    let b_reload_called: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(hot_reload_config())
        .loader(CascadeLoader {
            loader_name: "cascade-a",
            contract_id: a_contract_id,
            contract_name: "dep.contract",
            reload_called: Arc::new(AtomicBool::new(false)),
        })
        .loader(CascadeLoader {
            loader_name: "cascade-b",
            contract_id: guest_contract_id("b.contract", 1_u32),
            contract_name: "b.contract",
            reload_called: Arc::clone(&b_reload_called),
        })
        .build()
        .expect("runtime build should succeed");

    let a_path: PathBuf = write_bundle(&temp, "bundle_a", "cascade-a", "dep.contract", false, None);
    // B depends on A's contract and opts IN to cascade reload.
    let b_path: PathBuf = write_bundle(
        &temp,
        "bundle_b",
        "cascade-b",
        "b.contract",
        true,
        Some("dep.contract"),
    );

    runtime.load_bundle(a_path.as_path()).expect("load A");
    runtime.load_bundle(b_path.as_path()).expect("load B");

    runtime.reload_bundle(a_path.as_path()).expect("reload A");

    assert!(
        b_reload_called.load(Ordering::SeqCst),
        "B must be reloaded when it depends on A and opts into cascade reload"
    );
}

// ─── Test 3: cyclic dependency terminates ───────────────────────────────────────

#[test]
fn cascade_reload_cycle_detection() {
    let temp: TempDir = TempDir::new().expect("temp dir");

    let a_contract_id: u64 = guest_contract_id("a.contract", 1_u32);
    let b_contract_id: u64 = guest_contract_id("b.contract", 1_u32);

    let a_reload_called: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let b_reload_called: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(hot_reload_config())
        .loader(CascadeLoader {
            loader_name: "cycle-a",
            contract_id: a_contract_id,
            contract_name: "a.contract",
            reload_called: Arc::clone(&a_reload_called),
        })
        .loader(CascadeLoader {
            loader_name: "cycle-b",
            contract_id: b_contract_id,
            contract_name: "b.contract",
            reload_called: Arc::clone(&b_reload_called),
        })
        .build()
        .expect("runtime build should succeed");

    // A depends on B's contract; B depends on A's contract. Both opt in.
    let a_path: PathBuf = write_bundle(
        &temp,
        "cycle_a",
        "cycle-a",
        "a.contract",
        true,
        Some("b.contract"),
    );
    let b_path: PathBuf = write_bundle(
        &temp,
        "cycle_b",
        "cycle-b",
        "b.contract",
        true,
        Some("a.contract"),
    );

    runtime.load_bundle(a_path.as_path()).expect("load A");
    runtime.load_bundle(b_path.as_path()).expect("load B");

    // Reloading A cascades to B; B's cascade back to A must be cut by cycle
    // detection. The call must terminate and report Ok for the primary reload.
    runtime.reload_bundle(a_path.as_path()).expect("reload A");

    assert!(
        b_reload_called.load(Ordering::SeqCst),
        "B must cascade-reload from A's reload"
    );
}
