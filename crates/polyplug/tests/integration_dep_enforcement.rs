//! Integration test: declared-dependency enforcement during the init window.
//!
//! Proves the three guarantees docs/TRUST_MODEL.md §3/§4 describes:
//!   (a) A bundle CAN resolve a contract it declared as a dependency while its
//!       `polyplug_init` is running (the enforcement window).
//!   (b) A bundle CANNOT resolve a contract it did NOT declare during init —
//!       `host_find_guest_contract` returns a null handle.
//!   (c) Host-side lookups (outside any init window, bundle_id == 0) are
//!       unaffected and can resolve any registered contract.
//!
//! The test drives the real `Runtime::load_bundle` path, which now calls
//! `RuntimeStore::declare_bundle_dependencies` from the parsed manifest BEFORE
//! invoking the loader. A custom in-process `BundleLoader` simulates a plugin's
//! init by setting the init-window bundle id (exactly as the native loader does)
//! and probing the host's `find_guest_contract` callback for both a declared and
//! an undeclared contract, recording each result.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::Runtime;
use polyplug::error::RuntimeError;
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostApi, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::{BundleId, GuestContractId};

/// Results captured by the probing loader during the simulated init window.
struct ProbeResults {
    declared_lookup_null: Option<bool>,
    undeclared_lookup_null: Option<bool>,
    declared_find_all_count: Option<usize>,
    undeclared_find_all_count: Option<usize>,
}

/// In-process loader that simulates a plugin's `polyplug_init` by entering the
/// dependency-enforcement window (set init bundle id) and probing the host's
/// `find_guest_contract` callback for one declared and one undeclared contract.
struct ProbeLoader {
    declared_contract_id: u64,
    undeclared_contract_id: u64,
    results: Arc<Mutex<ProbeResults>>,
}

impl BundleLoader for ProbeLoader {
    fn runtime_name(&self) -> &'static str {
        "probe-enforce"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        let host_abi: &'static HostApi = runtime.host_abi();
        let bundle_id: BundleId = BundleId::new(&manifest.name);

        // Enter the enforcement window, exactly as the native loader does.
        runtime.push_init_bundle_id(bundle_id.id());

        // Probe the declared dependency: must resolve (non-null handle).
        // SAFETY: host_abi is a valid HostApi from the runtime.
        let declared_handle: GuestContractHandle = unsafe {
            (host_abi.find_guest_contract)(
                host_abi as *const HostApi,
                self.declared_contract_id,
                0_u32,
            )
        };

        // Probe the undeclared contract: must be denied (null handle).
        // SAFETY: host_abi is a valid HostApi from the runtime.
        let undeclared_handle: GuestContractHandle = unsafe {
            (host_abi.find_guest_contract)(
                host_abi as *const HostApi,
                self.undeclared_contract_id,
                0_u32,
            )
        };

        // Probe the enumeration API for both contracts. The declared one must be
        // enumerable; the undeclared one must come back empty during init.
        // SAFETY: host_abi is a valid HostApi from the runtime.
        let declared_all: polyplug_abi::Array<GuestContractHandle> = unsafe {
            (host_abi.find_all_guest_contracts)(
                host_abi as *const HostApi,
                self.declared_contract_id,
                0_u32,
            )
        };
        // SAFETY: host_abi is a valid HostApi from the runtime.
        let undeclared_all: polyplug_abi::Array<GuestContractHandle> = unsafe {
            (host_abi.find_all_guest_contracts)(
                host_abi as *const HostApi,
                self.undeclared_contract_id,
                0_u32,
            )
        };
        let declared_all_len: usize = declared_all.len;
        let undeclared_all_len: usize = undeclared_all.len;

        runtime.pop_init_bundle_id();

        let mut guard: std::sync::MutexGuard<'_, ProbeResults> =
            self.results.lock().unwrap_or_else(|e| e.into_inner());
        guard.declared_lookup_null = Some(declared_handle.is_null());
        guard.undeclared_lookup_null = Some(undeclared_handle.is_null());
        guard.declared_find_all_count = Some(declared_all_len);
        guard.undeclared_find_all_count = Some(undeclared_all_len);
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
        Err(RuntimeError::HotReloadDisabled)
    }
}

/// No-op create_instance callback for the registered provider interface.
unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// No-op destroy_instance callback for the registered provider interface.
unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Register a provider for `contract_id` from `bundle_id`, leaking a 'static
/// interface (lives for the test process lifetime).
fn register_provider(runtime: &Runtime, contract_id: u64, bundle_id: u64) -> GuestContractHandle {
    let interface: &'static GuestContractInterface = Box::leak(Box::new(GuestContractInterface {
        contract_id: GuestContractId::from_u64(contract_id),
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
                functions: core::ptr::null(),
            },
        },
    }));
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"provider"),
        contract_name: StringView::from_static(b"provider.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };
    // SAFETY: interface is leaked and lives for the process lifetime.
    unsafe {
        runtime.registry().register_guest_contract(
            descriptor,
            interface,
            "provider.contract".to_owned(),
            BundleId::from_u64(bundle_id),
        )
    }
    .expect("provider registration should succeed")
}

/// Write a bundle directory with a manifest declaring `declared_contract` as a
/// `[[dependency]]` (with its explicit contract_id), matching the depender
/// fixture's manifest shape.
fn write_bundle(temp: &tempfile::TempDir, bundle_name: &str, declared_contract_id: u64) -> PathBuf {
    let bundle_dir: PathBuf = temp.path().join(bundle_name);
    std::fs::create_dir_all(&bundle_dir).expect("create bundle dir");
    std::fs::write(bundle_dir.join("dummy.so"), b"").expect("write dummy so");
    let bundle_id: u64 = polyplug_utils::bundle_id(bundle_name);
    let manifest: String = format!(
        "id = {bundle_id}\n\
         name = \"{bundle_name}\"\n\
         runtime = \"probe-enforce\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n\n\
         [[dependency]]\n\
         kind = \"contract\"\n\
         contract = \"declared.dep@1\"\n\
         min_version = \"1.0\"\n\
         contract_id = {declared_contract_id}\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

#[test]
fn declared_dep_resolves_undeclared_denied_during_init_host_unaffected() {
    let temp: tempfile::TempDir = tempfile::TempDir::new().expect("temp dir");

    let declared_contract_id: u64 = polyplug_utils::guest_contract_id("declared.dep", 1_u32);
    let undeclared_contract_id: u64 = polyplug_utils::guest_contract_id("undeclared.other", 1_u32);

    let results: Arc<Mutex<ProbeResults>> = Arc::new(Mutex::new(ProbeResults {
        declared_lookup_null: None,
        undeclared_lookup_null: None,
        declared_find_all_count: None,
        undeclared_find_all_count: None,
    }));

    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(ProbeLoader {
            declared_contract_id,
            undeclared_contract_id,
            results: Arc::clone(&results),
        })
        .build()
        .expect("runtime build should succeed");

    // Register providers for BOTH contracts so the only thing gating resolution
    // is the declared-dependency enforcement, not provider absence.
    register_provider(&runtime, declared_contract_id, 0xAAAA_u64);
    register_provider(&runtime, undeclared_contract_id, 0xBBBB_u64);

    let bundle_path: PathBuf = write_bundle(&temp, "probe_bundle", declared_contract_id);
    runtime
        .load_bundle(bundle_path.as_path())
        .expect("load_bundle should succeed");

    let guard: std::sync::MutexGuard<'_, ProbeResults> =
        results.lock().unwrap_or_else(|e| e.into_inner());

    // (a) Declared dependency resolved during init.
    assert_eq!(
        guard.declared_lookup_null,
        Some(false),
        "declared dependency must resolve to a non-null handle during init"
    );

    // (b) Undeclared contract denied during init.
    assert_eq!(
        guard.undeclared_lookup_null,
        Some(true),
        "undeclared contract must be denied (null handle) during init"
    );

    // (a'/b') The enumeration API (find_all_guest_contracts) enforces identically:
    // declared contract is enumerable, undeclared comes back empty during init.
    assert_eq!(
        guard.declared_find_all_count,
        Some(1),
        "declared dependency must be enumerable via find_all during init"
    );
    assert_eq!(
        guard.undeclared_find_all_count,
        Some(0),
        "undeclared contract must enumerate empty via find_all during init"
    );
    drop(guard);

    // (c) Host-side lookups after init (no init window, bundle_id == 0) are
    // unaffected: even the undeclared-during-init contract resolves for the host.
    let host_declared: Result<GuestContractHandle, _> =
        runtime.find_guest_contract(declared_contract_id, 0_u32);
    assert!(
        host_declared.is_ok(),
        "host lookup of declared contract must succeed after init"
    );
    let host_undeclared: Result<GuestContractHandle, _> =
        runtime.find_guest_contract(undeclared_contract_id, 0_u32);
    assert!(
        host_undeclared.is_ok(),
        "host lookup of the (init-undeclared) contract must succeed after init — \
         enforcement applies only inside the init window"
    );
}
