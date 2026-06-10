//! Regression tests for review findings on the polyplug core runtime.
//!
//! Each test pins a specific finding from the review and fails on the pre-fix
//! behaviour:
//!   1. Builder scan path now routes through the shared explicit-load path, so
//!      discovered bundles get dependency declaration + non-empty descriptors.
//!   2. `call_guest_method` refuses ambiguous routing when >1 provider exists.
//!   3. Concurrent double-destroy is race-free (atomic swap).
//!   4. `host_register_guest_contract` null-checks descriptor and interface.
//!   5. `host_get_host_contract` drops the read guard before create_instance
//!      (no deadlock) and never caches a NULL singleton.
//!   7. `provides`/dependency `name@version` stripping is consistent across the
//!      capability graph.
//!   8. Reload validates the manifest (id-tamper check) and fires Failed.
//!   9. Manifest dependency `contract_id` is cross-checked in `validate()`.

#![allow(clippy::expect_used)]
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use polyplug::Runtime;
use polyplug::compatibility::CapabilityGraph;
use polyplug::error::RuntimeError;
use polyplug::loader::{BundleLoader, ManifestData};
use polyplug_abi::runtime::{Compatibility, ReloadPhaseType, RuntimeConfig, UnloadMode};
use polyplug_abi::{
    AbiErrorCode, CallArena, DispatchMechanisms, DispatchType, GuestContractInstance,
    GuestContractInterface, HostApi, HostContractInstance, HostContractInterface, NativeDispatch,
    PluginDescriptor, StringView, Version,
};
use polyplug_utils::{BundleId, GuestContractId, HostContractId};

// ─── Shared guest-interface helpers ──────────────────────────────────────────

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

fn leak_guest_interface(contract_id: u64) -> &'static GuestContractInterface {
    Box::leak(Box::new(GuestContractInterface {
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
    }))
}

fn register_provider(runtime: &Runtime, contract_id: u64, bundle_id: u64) {
    let interface: &'static GuestContractInterface = leak_guest_interface(contract_id);
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
    .expect("provider registration should succeed");
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 1 — builder scan path routes through the shared explicit-load path.
// ════════════════════════════════════════════════════════════════════════════

/// In-process loader that, during `load`, enters the init window for the bundle
/// being loaded and (for the dependent bundle) probes the host's
/// `find_guest_contract` for its declared dependency, recording the result.
struct DepProbeLoader {
    declared_contract_id: u64,
    // Records: for the dependent bundle, whether the declared dep resolved (non-null).
    declared_resolved: Arc<Mutex<Option<bool>>>,
    // Name of the bundle that should probe (the dependent "B").
    probing_bundle: String,
}

impl BundleLoader for DepProbeLoader {
    fn runtime_name(&self) -> &'static str {
        "dep-probe"
    }

    fn load(
        &self,
        manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        let host_abi: &'static HostApi = runtime.host_abi();
        let bundle_id: BundleId = BundleId::new(&manifest.name);
        runtime.push_init_bundle_id(bundle_id.id());

        if manifest.name == self.probing_bundle {
            // SAFETY: host_abi is a valid HostApi from the runtime.
            let handle = unsafe {
                (host_abi.find_guest_contract)(
                    host_abi as *const HostApi,
                    self.declared_contract_id,
                    0_u32,
                )
            };
            *self.declared_resolved.lock().unwrap() = Some(!handle.is_null());
        } else {
            // Provider bundle "A": register the contract it provides so the
            // dependent can resolve it.
            register_provider_for_loader(runtime, self.declared_contract_id, bundle_id.id());
        }

        runtime.pop_init_bundle_id();
        Ok(())
    }

    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
        Err(RuntimeError::HotReloadDisabled)
    }
}

/// Register a provider from inside a loader's init window (bundle_id is the
/// caller's). Mirrors how a real plugin's `polyplug_init` registers its contract.
fn register_provider_for_loader(runtime: &Runtime, contract_id: u64, bundle_id: u64) {
    let interface: &'static GuestContractInterface = leak_guest_interface(contract_id);
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"provider-A"),
        contract_name: StringView::from_static(b"declared.dep"),
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
            "declared.dep".to_owned(),
            BundleId::from_u64(bundle_id),
        )
    }
    .expect("provider registration should succeed");
}

fn write_provider_bundle(dir: &std::path::Path, name: &str) -> PathBuf {
    let bundle_dir: PathBuf = dir.join(name);
    std::fs::create_dir_all(&bundle_dir).expect("create dir");
    std::fs::write(bundle_dir.join("dummy.so"), b"").expect("write so");
    let id: u64 = polyplug_utils::bundle_id(name);
    // Declare the provided contract (and its function_count) so the build-time
    // capability graph + Strict function_count check are satisfied. major is 1
    // (version 1.0 → key "declared.dep@1").
    let manifest: String = format!(
        "id = {id}\n\
         name = \"{name}\"\n\
         runtime = \"dep-probe\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n\
         provides = [\"declared.dep@1\"]\n\
         function_count = {{ \"declared.dep@1\" = 0 }}\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

fn write_dependent_bundle(dir: &std::path::Path, name: &str, declared_contract_id: u64) -> PathBuf {
    let bundle_dir: PathBuf = dir.join(name);
    std::fs::create_dir_all(&bundle_dir).expect("create dir");
    std::fs::write(bundle_dir.join("dummy.so"), b"").expect("write so");
    let id: u64 = polyplug_utils::bundle_id(name);
    let manifest: String = format!(
        "id = {id}\n\
         name = \"{name}\"\n\
         runtime = \"dep-probe\"\n\
         file = \"dummy.so\"\n\
         version = \"1.0\"\n\n\
         [[dependency]]\n\
         kind = \"contract\"\n\
         contract = \"declared.dep\"\n\
         min_version = \"1.0\"\n\
         contract_id = {declared_contract_id}\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");
    bundle_dir
}

#[test]
fn builder_discovered_bundle_declares_deps_and_has_descriptor() {
    // contract major must match how `min_version = "1.0"` resolves: major 1.
    let declared_contract_id: u64 = GuestContractId::new("declared.dep", 1_u32).id();

    let temp: tempfile::TempDir = tempfile::TempDir::new().expect("temp dir");
    // Bundle A provides declared.dep; bundle B depends on it. B sorts after A in
    // topo order (A is the provider) so A registers before B probes.
    write_provider_bundle(temp.path(), "bundle_a_provider");
    write_dependent_bundle(temp.path(), "bundle_b_dependent", declared_contract_id);

    let declared_resolved: Arc<Mutex<Option<bool>>> = Arc::new(Mutex::new(None));

    let runtime: Arc<Runtime> = Runtime::builder()
        .plugin_dir(temp.path().to_path_buf())
        .loader(DepProbeLoader {
            declared_contract_id,
            declared_resolved: Arc::clone(&declared_resolved),
            probing_bundle: "bundle_b_dependent".to_owned(),
        })
        .build()
        .expect("runtime build with discovered bundles should succeed");

    // The dependent bundle resolved its DECLARED dependency during init — proving
    // the builder declared dependencies before invoking the loader (pre-fix this
    // returned null because declare_bundle_dependencies was never called on the
    // scan path).
    assert_eq!(
        *declared_resolved.lock().unwrap(),
        Some(true),
        "discovered dependent bundle must resolve its declared dependency during init"
    );

    // The provider bundle has a non-empty descriptor — proving register_bundle_metadata
    // ran on the scan path (pre-fix descriptors were empty/absent).
    let provider_id: BundleId = BundleId::new("bundle_a_provider");
    let descriptor = runtime
        .registry()
        .get_bundle_descriptor(provider_id)
        .expect("discovered bundle must have a registered descriptor");
    assert_eq!(
        descriptor.name, "bundle_a_provider",
        "discovered bundle descriptor must carry the bundle name"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 2 — call_guest_method refuses ambiguous multi-provider routing.
// ════════════════════════════════════════════════════════════════════════════

fn build_bare_runtime() -> Arc<Runtime> {
    Runtime::builder()
        .build()
        .expect("bare runtime build should succeed")
}

#[test]
fn call_guest_method_single_provider_dispatches() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let contract_id: u64 = GuestContractId::new("solo.contract", 1_u32).id();
    register_provider(&runtime, contract_id, 0x1111_u64);

    let host_abi: &'static HostApi = runtime.host_abi();
    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    // fn_id 0 with function_count 0 → FunctionNotAvailable, but crucially NOT
    // DuplicateProvider: routing proceeded to a single live interface.
    // SAFETY: host_abi is valid; instance carries a registered contract_id.
    let err = unsafe {
        (host_abi.call_guest_method)(
            host_abi as *const HostApi,
            instance,
            0_u32,
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut::<CallArena>(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::FunctionNotAvailable as u32,
        "single provider must dispatch (reaching the function table), not be rejected"
    );
}

#[test]
fn call_guest_method_multiple_providers_rejected() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let contract_id: u64 = GuestContractId::new("dup.contract", 1_u32).id();
    // Two distinct bundles both provide the SAME contract.
    register_provider(&runtime, contract_id, 0xAAAA_u64);
    register_provider(&runtime, contract_id, 0xBBBB_u64);

    let host_abi: &'static HostApi = runtime.host_abi();
    let instance: GuestContractInstance = GuestContractInstance {
        data: core::ptr::null_mut(),
        contract_id: GuestContractId::from_u64(contract_id),
    };
    // SAFETY: host_abi is valid; instance carries a registered contract_id.
    let err = unsafe {
        (host_abi.call_guest_method)(
            host_abi as *const HostApi,
            instance,
            0_u32,
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut::<CallArena>(),
        )
    };
    assert_eq!(
        err.code,
        AbiErrorCode::DuplicateProvider as u32,
        "ambiguous routing with >1 provider must return DuplicateProvider, not mis-dispatch"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 3 — concurrent double-destroy is race-free.
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn concurrent_destroy_race_does_not_crash() {
    use std::thread;

    for _ in 0..200 {
        // SAFETY: create has no pointer preconditions.
        let host: *const HostApi =
            unsafe { polyplug::ffi::polyplug_runtime_create(core::ptr::null()) };
        assert!(!host.is_null());
        let host_addr: usize = host as usize;

        let handles: Vec<thread::JoinHandle<()>> = (0..8)
            .map(|_| {
                thread::spawn(move || {
                    let h: *const HostApi = host_addr as *const HostApi;
                    // SAFETY: every thread races destroy on the same handle. The
                    // atomic swap guarantees exactly one reclaim; all others no-op.
                    unsafe { polyplug::ffi::polyplug_runtime_destroy(h) };
                })
            })
            .collect();

        for h in handles {
            h.join().expect("destroy thread must not panic");
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 4 — host_register_guest_contract null-checks pointers.
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn register_guest_contract_null_descriptor_is_invalid_pointer() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let host_abi: &'static HostApi = runtime.host_abi();
    let interface: &'static GuestContractInterface =
        leak_guest_interface(GuestContractId::new("x", 1).id());
    // SAFETY: host_abi valid; descriptor deliberately null to exercise the guard.
    let err = unsafe {
        (host_abi.register_guest_contract)(
            host_abi as *const HostApi,
            core::ptr::null(),
            interface as *const GuestContractInterface,
        )
    };
    assert_eq!(err.code, AbiErrorCode::InvalidPointer as u32);
}

#[test]
fn register_guest_contract_null_interface_is_invalid_pointer() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let host_abi: &'static HostApi = runtime.host_abi();
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"n"),
        contract_name: StringView::from_static(b"c"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };
    // SAFETY: host_abi valid; interface deliberately null to exercise the guard.
    let err = unsafe {
        (host_abi.register_guest_contract)(
            host_abi as *const HostApi,
            &descriptor as *const PluginDescriptor,
            core::ptr::null(),
        )
    };
    assert_eq!(err.code, AbiErrorCode::InvalidPointer as u32);
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 5 — host_get_host_contract: no deadlock on re-entrant register, and
// no caching of a NULL singleton.
// ════════════════════════════════════════════════════════════════════════════

/// create_instance that registers ANOTHER host contract (takes the write lock).
/// Pre-fix this deadlocked because the read guard was still held.
unsafe extern "C" fn reentrant_create_instance(
    this: *const HostContractInterface,
    _args: *const (),
) -> HostContractInstance {
    // SAFETY: `this` is the registered interface; its `runtime` field points at the
    // owning Runtime (set when the interface was registered for this test).
    let runtime_ptr: *const Runtime = unsafe { (*this).runtime as *const Runtime };
    if !runtime_ptr.is_null() {
        // SAFETY: runtime_ptr is the live Runtime for this test.
        let runtime: &Runtime = unsafe { &*runtime_ptr };
        let nested: &'static HostContractInterface = leak_inert_host_interface(0xDEAD_u64, false);
        // Re-entrant registration takes the host_contracts WRITE lock. If the
        // read guard from host_get_host_contract were still held, this deadlocks.
        let _ = runtime.register_host_contract(0xDEAD_u64, nested);
    }
    HostContractInstance::null()
}

unsafe extern "C" fn inert_destroy_instance(
    _this: *const HostContractInterface,
    _instance: HostContractInstance,
) {
}

unsafe extern "C" fn inert_create_instance(
    _this: *const HostContractInterface,
    _args: *const (),
) -> HostContractInstance {
    HostContractInstance::null()
}

fn leak_inert_host_interface(id: u64, singleton: bool) -> &'static HostContractInterface {
    Box::leak(Box::new(HostContractInterface {
        contract_id: HostContractId::from(id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton,
        dispatch_type: DispatchType::Native,
        runtime: core::ptr::null_mut(),
        user_data: core::ptr::null_mut(),
        create_instance: inert_create_instance,
        destroy_instance: inert_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: core::ptr::null(),
            },
        },
    }))
}

#[test]
fn get_host_contract_reentrant_register_does_not_deadlock() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let contract_id: u64 = 0xC0DE_u64;
    let interface: &'static HostContractInterface = Box::leak(Box::new(HostContractInterface {
        contract_id: HostContractId::from(contract_id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton: false,
        dispatch_type: DispatchType::Native,
        // Point the interface at the runtime so create_instance can reach it.
        runtime: Arc::as_ptr(&runtime) as *mut core::ffi::c_void,
        user_data: core::ptr::null_mut(),
        create_instance: reentrant_create_instance,
        destroy_instance: inert_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: core::ptr::null(),
            },
        },
    }));
    runtime
        .register_host_contract(contract_id, interface)
        .expect("register host contract");

    let host_abi: &'static HostApi = runtime.host_abi();
    // SAFETY: host_abi valid; contract_id is registered. This call's
    // create_instance re-enters register_host_contract — must complete, not hang.
    let _instance =
        unsafe { (host_abi.get_host_contract)(host_abi as *const HostApi, contract_id, 0_u32) };
    // Reaching here at all proves no deadlock.
}

/// Singleton create_instance that returns NULL on the first call, then non-null.
static SINGLETON_CALLS: Mutex<u32> = Mutex::new(0);

unsafe extern "C" fn flaky_singleton_create_instance(
    _this: *const HostContractInterface,
    _args: *const (),
) -> HostContractInstance {
    let mut calls = SINGLETON_CALLS.lock().unwrap();
    *calls += 1;
    if *calls == 1 {
        HostContractInstance::null()
    } else {
        // Non-null sentinel: a non-null, never-dereferenced pointer. The test only
        // checks null-ness, never reads through `data`. `NonNull::dangling` yields a
        // well-aligned non-null pointer without fabricating a bogus integer address.
        HostContractInstance {
            data: core::ptr::NonNull::<core::ffi::c_void>::dangling().as_ptr(),
        }
    }
}

#[test]
fn get_host_contract_does_not_cache_null_singleton() {
    *SINGLETON_CALLS.lock().unwrap() = 0;
    let runtime: Arc<Runtime> = build_bare_runtime();
    let contract_id: u64 = 0x5151_u64;
    let interface: &'static HostContractInterface = Box::leak(Box::new(HostContractInterface {
        contract_id: HostContractId::from(contract_id),
        contract_version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
        singleton: true,
        dispatch_type: DispatchType::Native,
        runtime: core::ptr::null_mut(),
        user_data: core::ptr::null_mut(),
        create_instance: flaky_singleton_create_instance,
        destroy_instance: inert_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: core::ptr::null(),
            },
        },
    }));
    runtime
        .register_host_contract(contract_id, interface)
        .expect("register host contract");

    let host_abi: &'static HostApi = runtime.host_abi();
    // First call: create_instance returns NULL — must NOT be cached.
    // SAFETY: host_abi valid; contract_id registered.
    let first =
        unsafe { (host_abi.get_host_contract)(host_abi as *const HostApi, contract_id, 0_u32) };
    assert!(first.is_null(), "first singleton creation returned null");

    // Second call: a retry must occur (cache was not poisoned) and now succeed.
    // SAFETY: host_abi valid; contract_id registered.
    let second =
        unsafe { (host_abi.get_host_contract)(host_abi as *const HostApi, contract_id, 0_u32) };
    assert!(
        !second.is_null(),
        "null singleton must not be cached: a later call must retry and succeed"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 7 — name@version stripping is consistent in the capability graph.
// ════════════════════════════════════════════════════════════════════════════

fn manifest_with(
    name: &str,
    provides: Vec<String>,
    deps: Vec<polyplug::loader::RawManifestDependency>,
) -> ManifestData {
    let mut m: ManifestData = ManifestData::parse_from_str(&format!(
        "runtime=\"native\"\nname=\"{name}\"\nfile=\"x.so\"\n"
    ))
    .expect("parse base manifest");
    m.id = polyplug_utils::bundle_id(name);
    m.version = "1.0.0".to_owned();
    m.provides = provides;
    m.dependencies = deps;
    m
}

#[test]
fn capability_graph_bybundle_matches_versioned_provides() {
    // Provider provides a VERSIONED contract entry; dependent's ByBundle dep names
    // the bare contract. Pre-fix, p.contains(contract) compared "data.Reporter"
    // against "data.Reporter@1.0" and failed.
    let provider: ManifestData = manifest_with(
        "reporter_bundle",
        vec!["data.Reporter@1.0".to_owned()],
        Vec::new(),
    );

    let dep: polyplug::loader::RawManifestDependency = polyplug::loader::RawManifestDependency {
        kind: "bundle".to_owned(),
        contract: "data.Reporter".to_owned(),
        min_version: "1.0".to_owned(),
        bundle: Some("reporter_bundle".to_owned()),
        contract_id: GuestContractId::new("data.Reporter", 1),
        bundle_id: Some(BundleId::new("reporter_bundle")),
    };
    let dependent: ManifestData = manifest_with("consumer_bundle", Vec::new(), vec![dep]);

    let manifests: Vec<(PathBuf, ManifestData)> = vec![
        (PathBuf::from("reporter_bundle"), provider),
        (PathBuf::from("consumer_bundle"), dependent),
    ];

    let graph = CapabilityGraph::from_manifests(&manifests);
    assert!(
        graph.is_ok(),
        "ByBundle dep on a bare contract must be satisfied by a versioned provides entry: {:?}",
        graph.err()
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 8 — reload validates manifest (id tamper) and fires Failed callback.
// ════════════════════════════════════════════════════════════════════════════

struct NeverLoadsLoader;

impl BundleLoader for NeverLoadsLoader {
    fn runtime_name(&self) -> &'static str {
        "reload-probe"
    }
    fn load(
        &self,
        _manifest: &ManifestData,
        _source: &polyplug::loader::BundleSource,
        _runtime: &Runtime,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[test]
fn reload_with_tampered_manifest_id_fails_and_fires_failed_callback() {
    let temp: tempfile::TempDir = tempfile::TempDir::new().expect("temp dir");
    let bundle_dir: PathBuf = temp.path().join("tampered");
    std::fs::create_dir_all(&bundle_dir).expect("create dir");
    std::fs::write(bundle_dir.join("plugin.so"), b"").expect("write so");
    // id deliberately mismatches FNV1a(name) → BundleTampered on validate().
    let bogus_id: u64 = polyplug_utils::bundle_id("tampered").wrapping_add(1);
    let manifest: String = format!(
        "id = {bogus_id}\n\
         name = \"tampered\"\n\
         runtime = \"reload-probe\"\n\
         file = \"plugin.so\"\n\
         version = \"1.0\"\n"
    );
    std::fs::write(bundle_dir.join("manifest.toml"), manifest).expect("write manifest");

    let failed_fired: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let failed_fired_cb: Arc<Mutex<bool>> = Arc::clone(&failed_fired);

    let config: RuntimeConfig = RuntimeConfig {
        compatibility: Compatibility::Strict,
        unload_mode: UnloadMode::Retire,
        hot_reload_enabled: true,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
    };

    let runtime: Arc<Runtime> = Runtime::builder()
        .config(config)
        .loader(NeverLoadsLoader)
        .on_reload(move |_ud, phase| {
            if phase.phase_type == ReloadPhaseType::Failed {
                *failed_fired_cb.lock().unwrap() = true;
            }
        })
        .build()
        .expect("runtime build");

    let plugin_path: PathBuf = bundle_dir.join("plugin.so");
    let result: Result<(), RuntimeError> = runtime.reload_bundle(plugin_path.as_path());

    match result {
        Err(RuntimeError::Loader(polyplug::error::LoaderError::BundleTampered { .. })) => {}
        other => panic!("expected BundleTampered on reload of tampered manifest, got {other:?}"),
    }
    assert!(
        *failed_fired.lock().unwrap(),
        "reload of a tampered manifest must fire the Failed callback"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 9 — manifest dependency contract_id is cross-checked in validate().
// ════════════════════════════════════════════════════════════════════════════

fn dep_with_contract_id(
    contract: &str,
    min_version: &str,
    contract_id: GuestContractId,
) -> polyplug::loader::RawManifestDependency {
    polyplug::loader::RawManifestDependency {
        kind: "contract".to_owned(),
        contract: contract.to_owned(),
        min_version: min_version.to_owned(),
        bundle: None,
        contract_id,
        bundle_id: None,
    }
}

#[test]
fn validate_accepts_matching_dependency_contract_id() {
    let mut m: ManifestData = manifest_with("dep_match", Vec::new(), Vec::new());
    // major from min_version "1.0" is 1.
    let correct: GuestContractId = GuestContractId::new("math", 1);
    m.dependencies = vec![dep_with_contract_id("math", "1.0", correct)];
    assert!(
        m.validate().is_ok(),
        "validate must accept a dependency whose contract_id matches the canonical hash"
    );
}

#[test]
fn validate_rejects_mismatched_dependency_contract_id() {
    let mut m: ManifestData = manifest_with("dep_mismatch", Vec::new(), Vec::new());
    let wrong: GuestContractId =
        GuestContractId::from_u64(GuestContractId::new("math", 1).id().wrapping_add(7));
    m.dependencies = vec![dep_with_contract_id("math", "1.0", wrong)];
    match m.validate() {
        Err(polyplug::error::LoaderError::ManifestParse { reason, .. }) => {
            assert!(
                reason.contains("contract_id"),
                "mismatch error must mention contract_id: {reason}"
            );
        }
        other => panic!("expected ManifestParse for mismatched contract_id, got {other:?}"),
    }
}

#[test]
fn validate_accepts_absent_dependency_contract_id() {
    let mut m: ManifestData = manifest_with("dep_absent", Vec::new(), Vec::new());
    // contract_id default (0) means absent — must be accepted.
    m.dependencies = vec![dep_with_contract_id(
        "math",
        "1.0",
        GuestContractId::default(),
    )];
    assert!(
        m.validate().is_ok(),
        "validate must accept a dependency with an absent (0) contract_id"
    );
}

// ════════════════════════════════════════════════════════════════════════════
// Finding 11 — host_register_guest_contract surfaces specific error codes.
//
// A same-bundle duplicate registration must return
// AbiErrorCode::DuplicateProvider (the enum documents exactly this case), not a
// flattened Generic, and the error detail must reach get_last_error — not just
// stderr — so hosts and guests can react programmatically.
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn register_guest_contract_duplicate_returns_duplicate_provider_code() {
    let runtime: Arc<Runtime> = build_bare_runtime();
    let host_abi: &'static HostApi = runtime.host_abi();
    let contract_id: u64 = GuestContractId::new("dup.contract", 1).id();
    let interface: &'static GuestContractInterface = leak_guest_interface(contract_id);
    let descriptor: PluginDescriptor = PluginDescriptor {
        name: StringView::from_static(b"dup-plugin"),
        contract_name: StringView::from_static(b"dup.contract"),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    };

    // Attribute both registrations to the same (non-zero) bundle id, exactly as
    // a loader's init window would.
    runtime.push_init_bundle_id(0xD0D0_u64);
    // SAFETY: host_abi is valid; descriptor and interface are valid 'static refs.
    let first: polyplug_abi::AbiError = unsafe {
        (host_abi.register_guest_contract)(
            host_abi as *const HostApi,
            &descriptor as *const PluginDescriptor,
            interface as *const GuestContractInterface,
        )
    };
    assert_eq!(first.code, AbiErrorCode::Ok as u32, "first must register");
    // SAFETY: same as above — deliberate same-bundle duplicate registration.
    let second: polyplug_abi::AbiError = unsafe {
        (host_abi.register_guest_contract)(
            host_abi as *const HostApi,
            &descriptor as *const PluginDescriptor,
            interface as *const GuestContractInterface,
        )
    };
    runtime.pop_init_bundle_id();

    assert_eq!(
        second.code,
        AbiErrorCode::DuplicateProvider as u32,
        "same-bundle duplicate must return DuplicateProvider, not Generic"
    );

    // The detail must be readable through get_last_error.
    // SAFETY: host_abi is valid for both error-introspection calls.
    let len: usize = unsafe { (host_abi.get_error_len)(host_abi as *const HostApi) };
    assert!(len > 0, "last error must be set on registration failure");
    let mut buf: Vec<u8> = vec![0_u8; len];
    // SAFETY: buf is valid for len bytes.
    let written: usize =
        unsafe { (host_abi.get_last_error)(host_abi as *const HostApi, buf.as_mut_ptr(), len) };
    let msg: String = String::from_utf8_lossy(&buf[..written]).into_owned();
    assert!(
        msg.contains("duplicate provider"),
        "last error must carry the registry detail, got: {msg}"
    );
}
