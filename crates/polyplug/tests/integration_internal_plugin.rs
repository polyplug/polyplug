#![allow(clippy::expect_used)]

//! Focused behavioral coverage for staged internal-plugin registration through
//! the canonical manifest and generated provider-binding transaction.

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use polyplug::Runtime;
use polyplug::error::{LoaderError, RegistryError, RuntimeError};
use polyplug::ffi::{
    polyplug_abort_internal_plugin, polyplug_attach_internal_plugin_resident,
    polyplug_begin_internal_plugin, polyplug_commit_internal_plugin, polyplug_current_os_thread_id,
};
use polyplug::runtime::RustGeneratedInternalPlugin;
use polyplug::runtime::RustGeneratedInternalPluginRegistrar;
use polyplug_abi::dispatch::{DispatchMechanisms, DispatchType, NativeDispatch, VmLoaderData};
use polyplug_abi::guest::GuestContractInstance;
use polyplug_abi::{
    AbiError, GuestContractInterface, HostApi, PluginDescriptor, StringView, Version,
};
use polyplug_common::ManifestData;
use polyplug_utils::{BundleId, GuestContractId};

const CONTRACT_A: u64 = 0xE1B0_0000_0000_0001;
const CONTRACT_B: u64 = 0xE1B0_0000_0000_0002;
const CONTRACT_STATEFUL: u64 = 0xE1B0_0000_0000_0003;
const VERSION: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};

unsafe extern "C" fn create_stateless(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: the non-null output belongs to the caller for this ABI call.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn destroy_stateless(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

unsafe extern "C" fn create_stateful(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        let state: Box<u8> = Box::new(1);
        // SAFETY: the non-null output belongs to the caller for this ABI call.
        unsafe {
            out_instance.write(GuestContractInstance {
                data: Box::into_raw(state).cast(),
                contract_id: GuestContractId::from_u64(CONTRACT_STATEFUL),
            })
        };
    }
}

unsafe extern "C" fn destroy_stateful(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    instance: GuestContractInstance,
) {
    if !instance.data.is_null() {
        // SAFETY: create_stateful allocated this exact Box for a stateful instance.
        unsafe { drop(Box::from_raw(instance.data.cast::<u8>())) };
    }
}

macro_rules! interface {
    ($contract_id:expr, $create:ident, $destroy:ident) => {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64($contract_id),
            contract_version: VERSION,
            dispatch_type: DispatchType::Native,
            adapter_context: ptr::null_mut(),
            create_instance: $create,
            destroy_instance: $destroy,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        }
    };
}

static INTERFACE_A: GuestContractInterface =
    interface!(CONTRACT_A, create_stateless, destroy_stateless);
static INTERFACE_B: GuestContractInterface =
    interface!(CONTRACT_B, create_stateless, destroy_stateless);
static INTERFACE_STATEFUL: GuestContractInterface =
    interface!(CONTRACT_STATEFUL, create_stateful, destroy_stateful);

fn descriptor(provider: &'static str, contract: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(provider.as_bytes()),
        contract_name: StringView::from_static(contract.as_bytes()),
        version: VERSION,
    }
}

fn manifest(name: &str, providers: &[&str]) -> ManifestData {
    let mut function_count: HashMap<String, u32> = HashMap::new();
    for provider in providers {
        function_count.insert(format!("{provider}@1"), 0);
    }
    ManifestData {
        loader: "rust".to_owned(),
        name: name.to_owned(),
        dependencies: Vec::new(),
        id: BundleId::new(name).id(),
        version: "1.0.0".to_owned(),
        file: String::new(),
        provides: providers
            .iter()
            .map(|provider| format!("{provider}@1.0.0"))
            .collect(),
        function_count,
        needs_reinit_on_dep_reload: false,
        bundle_dependencies: Vec::new(),
        path: PathBuf::new(),
    }
}

fn runtime() -> Arc<Runtime> {
    Runtime::builder().build().expect("runtime build")
}

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct GeneratedBinding {
    manifest: ManifestData,
    entries: Vec<(PluginDescriptor, &'static GuestContractInterface)>,
    fail: bool,
    _root: Box<dyn Send + Sync>,
}

impl RustGeneratedInternalPlugin for GeneratedBinding {
    fn manifest(&self) -> ManifestData {
        self.manifest.clone()
    }

    fn stage(
        &self,
        registrar: &mut RustGeneratedInternalPluginRegistrar,
    ) -> Result<(), RuntimeError> {
        if self.fail {
            return Err(RuntimeError::Loader(LoaderError::InitFailed {
                bundle: self.manifest.name.clone(),
                error: "test producer failure".to_owned(),
            }));
        }
        for (descriptor, interface) in &self.entries {
            registrar.register_contract(descriptor, interface)?;
        }
        Ok(())
    }
}

fn register<R: Send + Sync + 'static>(
    runtime: &Runtime,
    manifest: ManifestData,
    entries: &[(PluginDescriptor, &'static GuestContractInterface)],
    root: R,
) -> Result<BundleId, RuntimeError> {
    runtime
        .register_generated_internal_plugin(GeneratedBinding {
            manifest,
            entries: entries.to_vec(),
            fail: false,
            _root: Box::new(root),
        })
        .map(|registered| registered.bundle_id)
}

unsafe extern "C" fn release_counter(resident: *mut c_void) {
    // SAFETY: make_resident allocated this exact Box and transfers it once.
    let counter: Box<Arc<AtomicUsize>> = unsafe { Box::from_raw(resident.cast()) };
    counter.fetch_add(1, Ordering::SeqCst);
}

fn make_resident(counter: Arc<AtomicUsize>) -> *mut c_void {
    Box::into_raw(Box::new(counter)).cast()
}

fn native_manifest(name: &str, providers: &[&str], dependency: Option<&str>) -> String {
    let provider_entries: String = providers
        .iter()
        .map(|provider| format!("\"{provider}@1.0.0\""))
        .collect::<Vec<String>>()
        .join(", ");
    let function_counts: String = providers
        .iter()
        .map(|provider| format!("\"{provider}@1\" = 0"))
        .collect::<Vec<String>>()
        .join(", ");
    let dependency: String = dependency.map_or_else(String::new, |contract| {
        format!(
            "\n[[dependency]]\nkind = \"contract\"\ncontract = \"{contract}@1\"\ncontract_id = {}\nmin_version = \"1.0\"\n",
            GuestContractId::new(contract, 1).id()
        )
    });
    format!(
        "id = {}\nname = \"{name}\"\nloader = \"\"\nfile = \"\"\nversion = \"1.0.0\"\nprovides = [{provider_entries}]\nfunction_count = {{ {function_counts} }}{dependency}",
        BundleId::new(name).id()
    )
}

fn begin_native(host: *const HostApi, manifest: &str) -> u64 {
    let mut bundle_id: u64 = 0;
    let mut error: AbiError = AbiError::ok();
    // SAFETY: host is live, manifest bytes and outputs remain valid for this call.
    unsafe {
        polyplug_begin_internal_plugin(
            host,
            manifest.as_ptr(),
            manifest.len(),
            4,
            &mut bundle_id,
            &mut error,
        )
    };
    assert!(error.is_ok(), "begin failed");
    bundle_id
}

fn stage_native_contract(
    host: *const HostApi,
    descriptor: &PluginDescriptor,
    interface: &GuestContractInterface,
) {
    let mut error: AbiError = AbiError::ok();
    // SAFETY: all inputs belong to this live host and are valid for this call.
    unsafe {
        ((*host).register_guest_contract)(host, descriptor, interface, &mut error);
    }
    assert!(error.is_ok(), "staging contract failed");
}

fn attach_native(
    host: *const HostApi,
    bundle_id: u64,
    resident: *mut c_void,
    owner_thread_id: u64,
) -> bool {
    let mut error: AbiError = AbiError::ok();
    // SAFETY: host is live and resident/release meet the requested attachment contract.
    unsafe {
        polyplug_attach_internal_plugin_resident(
            host,
            bundle_id,
            resident,
            owner_thread_id,
            Some(release_counter),
            &mut error,
        )
    }
}

fn commit_native(host: *const HostApi, bundle_id: u64) -> bool {
    let mut error: AbiError = AbiError::ok();
    // SAFETY: host is live and began this bundle transaction.
    unsafe { polyplug_commit_internal_plugin(host, bundle_id, &mut error) };
    error.is_ok()
}

#[test]
fn complete_registration_publishes_multiple_contracts_atomically() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [
        (descriptor("provider-a", "internal.a"), &INTERFACE_A),
        (descriptor("provider-b", "internal.b"), &INTERFACE_B),
    ];
    let id = register(
        &runtime,
        manifest("atomic-success", &["internal.a", "internal.b"]),
        &entries,
        (),
    )
    .expect("internal-plugin registration");

    assert_eq!(id, BundleId::new("atomic-success"));
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert!(runtime.find_guest_contract(CONTRACT_B, 0).is_ok());
}

#[test]
fn duplicate_second_contract_rolls_back_the_complete_bundle_and_reclaims_resident() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [
        (
            descriptor("provider-first", "internal.duplicate"),
            &INTERFACE_A,
        ),
        (
            descriptor("provider-first", "internal.duplicate"),
            &INTERFACE_A,
        ),
    ];
    let reclaimed: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let result = register(
        &runtime,
        manifest("duplicate-rollback", &["internal.duplicate"]),
        &entries,
        DropProbe(Arc::clone(&reclaimed)),
    );

    assert!(matches!(
        result,
        Err(RuntimeError::Registry(
            RegistryError::DuplicateProvider { .. }
        ))
    ));
    assert!(matches!(
        runtime.find_guest_contract(CONTRACT_A, 0),
        Err(RegistryError::PluginNotFound { .. })
    ));
    assert_eq!(
        reclaimed.load(Ordering::SeqCst),
        1,
        "failed registration must reclaim the runtime resident",
    );
}

#[test]
fn residents_are_isolated_and_logical_unload_releases_them() {
    let first: Arc<Runtime> = runtime();
    let second: Arc<Runtime> = runtime();
    let entries = [(descriptor("provider", "internal.a"), &INTERFACE_A)];
    let first_id = register(
        &first,
        manifest("resident-isolation", &["internal.a"]),
        &entries,
        11_u32,
    )
    .expect("first runtime registration");
    let second_id = register(
        &second,
        manifest("resident-isolation", &["internal.a"]),
        &entries,
        29_u32,
    )
    .expect("second runtime registration");
    assert_eq!(first_id, second_id);
    first.unload_bundle(first_id).expect("logical unload");
    register(
        &first,
        manifest("resident-isolation", &["internal.a"]),
        &entries,
        2_u32,
    )
    .expect("re-register after logical unload");
}

#[test]
fn unload_waits_for_stateful_instance_quiescence() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [(
        descriptor("provider", "internal.stateful"),
        &INTERFACE_STATEFUL,
    )];
    let id = register(
        &runtime,
        manifest("active-gate", &["internal.stateful"]),
        &entries,
        (),
    )
    .expect("internal-plugin registration");
    let handle = runtime
        .find_guest_contract(CONTRACT_STATEFUL, 0)
        .expect("registered stateful contract");
    let interface = runtime
        .resolve_guest_contract(handle)
        .expect("resolve stateful contract");
    let mut instance = GuestContractInstance::null();
    let host = runtime.host_abi();
    // SAFETY: host and interface belong to the live runtime; instance is writable.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };
    assert!(matches!(
        runtime.unload_bundle(id),
        Err(RuntimeError::InternalPluginInUse {
            active_instances: 1,
            ..
        })
    ));
    // SAFETY: instance was created through the matching live HostApi callback.
    unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    runtime.unload_bundle(id).expect("unload after destroy");
}

#[test]
fn wrapper_owned_internal_bundle_refuses_direct_unload_while_instance_is_live() {
    let runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = runtime.host_abi();
    let bundle_id = begin_native(
        host,
        &native_manifest(
            "wrapper-owned-direct-lifecycle",
            &["wrapper.owned.direct"],
            None,
        ),
    );
    // Native wrapper bindings retain their own backing and attach no core resident.
    stage_native_contract(
        host,
        &descriptor("wrapper-owned-direct", "wrapper.owned.direct"),
        &INTERFACE_STATEFUL,
    );
    assert!(commit_native(host, bundle_id));

    let handle = runtime
        .find_guest_contract(CONTRACT_STATEFUL, 0)
        .expect("wrapper-owned provider must resolve");
    let interface = runtime
        .resolve_guest_contract(handle)
        .expect("runtime-issued wrapper-owned interface");
    let mut instance = GuestContractInstance::null();
    // SAFETY: host and interface belong to the live runtime; instance is writable.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };

    assert!(matches!(
        runtime.unload_bundle(BundleId::from_u64(bundle_id)),
        Err(RuntimeError::InternalPluginInUse {
            active_instances: 1,
            ..
        })
    ));

    // SAFETY: instance was created through this interface.
    unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    runtime
        .unload_bundle(BundleId::from_u64(bundle_id))
        .expect("destroying the live instance permits direct unload");
}

#[test]
fn wrapper_owned_internal_bundle_refuses_cascade_unload_while_instance_is_live() {
    let runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = runtime.host_abi();
    let provider_name = "wrapper.owned.cascade.provider";
    let dependent_name = "wrapper.owned.cascade.dependent";
    let provider_bundle = begin_native(
        host,
        &native_manifest("wrapper-owned-cascade-provider", &[provider_name], None),
    );
    let provider_interface = interface!(
        GuestContractId::new(provider_name, 1).id(),
        create_stateless,
        destroy_stateless
    );
    stage_native_contract(
        host,
        &descriptor("wrapper-owned-provider", provider_name),
        &provider_interface,
    );
    assert!(commit_native(host, provider_bundle));

    let dependent_bundle = begin_native(
        host,
        &native_manifest(
            "wrapper-owned-cascade-dependent",
            &[dependent_name],
            Some(provider_name),
        ),
    );
    let dependent_interface = interface!(
        GuestContractId::new(dependent_name, 1).id(),
        create_stateful,
        destroy_stateful
    );
    stage_native_contract(
        host,
        &descriptor("wrapper-owned-dependent", dependent_name),
        &dependent_interface,
    );
    assert!(commit_native(host, dependent_bundle));

    let handle = runtime
        .find_guest_contract(GuestContractId::new(dependent_name, 1).id(), 0)
        .expect("wrapper-owned dependent must resolve");
    let interface = runtime
        .resolve_guest_contract(handle)
        .expect("runtime-issued dependent interface");
    let mut instance = GuestContractInstance::null();
    // SAFETY: host and interface belong to the live runtime; instance is writable.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };

    assert!(matches!(
        runtime.unload_bundle_cascade(BundleId::from_u64(provider_bundle)),
        Err(RuntimeError::InternalPluginInUse {
            active_instances: 1,
            ..
        })
    ));

    // SAFETY: instance was created through this interface.
    unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    runtime
        .unload_bundle_cascade(BundleId::from_u64(provider_bundle))
        .expect("destroying the dependent instance permits cascade unload");
}

#[test]
fn internal_registration_consumes_failed_input_and_accepts_artifactless_metadata() {
    let runtime: Arc<Runtime> = runtime();
    let reclaimed: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let mut internal_manifest = manifest("internal-fresh-retry", &["internal.a"]);
    internal_manifest.loader.clear();
    internal_manifest.file.clear();

    let failed = runtime.register_generated_internal_plugin(GeneratedBinding {
        manifest: internal_manifest.clone(),
        entries: Vec::new(),
        fail: true,
        _root: Box::new(DropProbe(Arc::clone(&reclaimed))),
    });
    assert!(failed.is_err(), "failed producer must abort registration");
    assert_eq!(
        reclaimed.load(Ordering::SeqCst),
        1,
        "failed input must be released exactly once"
    );
    assert!(matches!(
        runtime.find_guest_contract(CONTRACT_A, 0),
        Err(RegistryError::PluginNotFound { .. })
    ));

    let success_probe: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let registered = runtime
        .register_generated_internal_plugin(GeneratedBinding {
            manifest: internal_manifest,
            entries: vec![(descriptor("internal-provider", "internal.a"), &INTERFACE_A)],
            fail: false,
            _root: Box::new(DropProbe(Arc::clone(&success_probe))),
        })
        .expect("fresh internal input registers");
    let bundle_id = registered.bundle_id;
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert_eq!(success_probe.load(Ordering::SeqCst), 0);
    runtime
        .unload_bundle(bundle_id)
        .expect("internal bundle unload");
    assert_eq!(
        success_probe.load(Ordering::SeqCst),
        1,
        "successful input transfers until canonical unload"
    );
}

#[test]
fn native_resident_rejects_invalid_attachment_without_transferring_ownership() {
    let runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = runtime.host_abi();
    let bundle_id = begin_native(
        host,
        &native_manifest("native-attach-validation", &["native.attach"], None),
    );
    let rejected: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let rejected_resident = make_resident(Arc::clone(&rejected));
    let mut error: AbiError = AbiError::ok();
    // SAFETY: host is live; this deliberately supplies a null callback to reject attachment.
    let attached = unsafe {
        polyplug_attach_internal_plugin_resident(
            host,
            bundle_id,
            rejected_resident,
            polyplug_current_os_thread_id(),
            None,
            &mut error,
        )
    };
    assert!(!attached);
    assert_eq!(rejected.load(Ordering::SeqCst), 0);
    // SAFETY: failed attachment left this resident with the test.
    unsafe { release_counter(rejected_resident) };

    let wrong_owner: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let wrong_owner_resident = make_resident(Arc::clone(&wrong_owner));
    assert!(!attach_native(
        host,
        bundle_id,
        wrong_owner_resident,
        polyplug_current_os_thread_id().wrapping_add(1)
    ));
    assert_eq!(wrong_owner.load(Ordering::SeqCst), 0);
    // SAFETY: owner-thread validation rejected this resident without transfer.
    unsafe { release_counter(wrong_owner_resident) };

    let attached_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let attached_resident = make_resident(Arc::clone(&attached_counter));
    assert!(attach_native(
        host,
        bundle_id,
        attached_resident,
        polyplug_current_os_thread_id()
    ));

    let duplicate_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    let duplicate_resident = make_resident(Arc::clone(&duplicate_counter));
    assert!(!attach_native(
        host,
        bundle_id,
        duplicate_resident,
        polyplug_current_os_thread_id()
    ));
    assert_eq!(duplicate_counter.load(Ordering::SeqCst), 0);
    // SAFETY: duplicate attachment also leaves ownership with the test.
    unsafe { release_counter(duplicate_resident) };

    // SAFETY: this host owns the pending transaction.
    unsafe { polyplug_abort_internal_plugin(host, bundle_id) };
    assert_eq!(attached_counter.load(Ordering::SeqCst), 1);
    drop(runtime);
    assert_eq!(attached_counter.load(Ordering::SeqCst), 1);
}

#[test]
fn native_resident_releases_on_commit_failure_and_runtime_drop() {
    let failed_runtime: Arc<Runtime> = runtime();
    let failed_host: *const HostApi = failed_runtime.host_abi();
    let failed_bundle = begin_native(
        failed_host,
        &native_manifest("native-commit-failure", &["native.failure"], None),
    );
    let failed_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        failed_host,
        failed_bundle,
        make_resident(Arc::clone(&failed_counter)),
        polyplug_current_os_thread_id()
    ));
    assert!(!commit_native(failed_host, failed_bundle));
    assert_eq!(failed_counter.load(Ordering::SeqCst), 1);
    drop(failed_runtime);
    assert_eq!(failed_counter.load(Ordering::SeqCst), 1);

    let dropped_runtime: Arc<Runtime> = runtime();
    let dropped_host: *const HostApi = dropped_runtime.host_abi();
    let dropped_bundle = begin_native(
        dropped_host,
        &native_manifest("native-runtime-drop", &["native.drop"], None),
    );
    let dropped_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        dropped_host,
        dropped_bundle,
        make_resident(Arc::clone(&dropped_counter)),
        polyplug_current_os_thread_id()
    ));
    stage_native_contract(
        dropped_host,
        &descriptor("native-drop", "native.drop"),
        &INTERFACE_A,
    );
    assert!(commit_native(dropped_host, dropped_bundle));
    assert_eq!(dropped_counter.load(Ordering::SeqCst), 0);
    drop(dropped_runtime);
    assert_eq!(dropped_counter.load(Ordering::SeqCst), 1);
}

#[test]
fn native_resident_releases_after_direct_and_cascade_unload() {
    let direct_runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = direct_runtime.host_abi();
    let direct_bundle = begin_native(
        host,
        &native_manifest("native-direct-unload", &["native.direct"], None),
    );
    let direct_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        host,
        direct_bundle,
        make_resident(Arc::clone(&direct_counter)),
        polyplug_current_os_thread_id()
    ));
    stage_native_contract(
        host,
        &descriptor("native-direct", "native.direct"),
        &INTERFACE_A,
    );
    assert!(commit_native(host, direct_bundle));
    direct_runtime
        .unload_bundle(BundleId::from_u64(direct_bundle))
        .expect("owner unload");
    assert_eq!(direct_counter.load(Ordering::SeqCst), 1);
    drop(direct_runtime);
    assert_eq!(direct_counter.load(Ordering::SeqCst), 1);

    let cascade_runtime: Arc<Runtime> = runtime();
    let cascade_host: *const HostApi = cascade_runtime.host_abi();
    let provider_bundle = begin_native(
        cascade_host,
        &native_manifest(
            "native-cascade-provider",
            &["native.cascade.provider"],
            None,
        ),
    );
    let provider_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        cascade_host,
        provider_bundle,
        make_resident(Arc::clone(&provider_counter)),
        polyplug_current_os_thread_id()
    ));
    let provider_interface = interface!(
        GuestContractId::new("native.cascade.provider", 1).id(),
        create_stateless,
        destroy_stateless
    );
    stage_native_contract(
        cascade_host,
        &descriptor("native-cascade-provider", "native.cascade.provider"),
        &provider_interface,
    );
    assert!(commit_native(cascade_host, provider_bundle));

    let dependent_bundle = begin_native(
        cascade_host,
        &native_manifest(
            "native-cascade-dependent",
            &["native.cascade.dependent"],
            Some("native.cascade.provider"),
        ),
    );
    let dependent_counter: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        cascade_host,
        dependent_bundle,
        make_resident(Arc::clone(&dependent_counter)),
        polyplug_current_os_thread_id()
    ));
    let dependent_interface = interface!(
        GuestContractId::new("native.cascade.dependent", 1).id(),
        create_stateless,
        destroy_stateless
    );
    stage_native_contract(
        cascade_host,
        &descriptor("native-cascade-dependent", "native.cascade.dependent"),
        &dependent_interface,
    );
    assert!(commit_native(cascade_host, dependent_bundle));
    cascade_runtime
        .unload_bundle_cascade(BundleId::from_u64(provider_bundle))
        .expect("owner cascade unload");
    assert_eq!(provider_counter.load(Ordering::SeqCst), 1);
    assert_eq!(dependent_counter.load(Ordering::SeqCst), 1);
}

#[test]
fn off_owner_native_unload_refuses_before_registry_invalidation() {
    let runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = runtime.host_abi();
    let bundle_id = begin_native(
        host,
        &native_manifest("native-off-owner", &["native.off.owner"], None),
    );
    let released: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
    assert!(attach_native(
        host,
        bundle_id,
        make_resident(Arc::clone(&released)),
        polyplug_current_os_thread_id()
    ));
    stage_native_contract(
        host,
        &descriptor("native-off-owner", "native.off.owner"),
        &INTERFACE_A,
    );
    assert!(commit_native(host, bundle_id));

    let other_runtime: Arc<Runtime> = Arc::clone(&runtime);
    let result = thread::spawn(move || other_runtime.unload_bundle(BundleId::from_u64(bundle_id)))
        .join()
        .expect("unload thread should not panic");
    assert!(matches!(
        result,
        Err(RuntimeError::InternalPluginResidentWrongThread { .. })
    ));
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert_eq!(released.load(Ordering::SeqCst), 0);

    runtime
        .unload_bundle(BundleId::from_u64(bundle_id))
        .expect("owner unload");
    assert_eq!(released.load(Ordering::SeqCst), 1);
}
