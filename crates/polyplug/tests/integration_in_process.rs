#![allow(clippy::expect_used)]

//! Focused behavioral coverage for staged in-process registration through the
//! canonical manifest and existing guest-contract callback.

use core::ffi::c_void;
use core::ptr;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use polyplug::Runtime;
use polyplug::error::{RegistryError, RuntimeError};
use polyplug_abi::dispatch::{DispatchMechanisms, DispatchType, NativeDispatch, VmLoaderData};
use polyplug_abi::guest::GuestContractInstance;
use polyplug_abi::{
    AbiError, GuestContractInterface, HostApi, PluginDescriptor, StringView, SupportedLanguage,
    Version,
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
        file: "in-process".to_owned(),
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

fn register<R: Send + Sync + 'static>(
    runtime: &Runtime,
    manifest: ManifestData,
    entries: &[(PluginDescriptor, &'static GuestContractInterface)],
    resident: R,
) -> Result<BundleId, RuntimeError> {
    runtime.register_in_process_bundle(manifest, SupportedLanguage::Rust, resident, |host| {
        for (descriptor, interface) in entries {
            let mut error: AbiError = AbiError::ok();
            // SAFETY: this closure runs inside the runtime's active registration transaction.
            unsafe {
                ((*host).register_guest_contract)(host, descriptor, *interface, &mut error);
            }
            if !error.is_ok() {
                return error;
            }
        }
        AbiError::ok()
    })
}

#[test]
fn complete_registration_publishes_multiple_contracts_atomically() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [
        (descriptor("provider-a", "in.process.a"), &INTERFACE_A),
        (descriptor("provider-b", "in.process.b"), &INTERFACE_B),
    ];
    let id = register(
        &runtime,
        manifest("atomic-success", &["in.process.a", "in.process.b"]),
        &entries,
        (),
    )
    .expect("in-process registration");

    assert_eq!(id, BundleId::new("atomic-success"));
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert!(runtime.find_guest_contract(CONTRACT_B, 0).is_ok());
}

#[test]
fn duplicate_second_contract_rolls_back_the_complete_bundle() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [
        (
            descriptor("provider-first", "in.process.duplicate"),
            &INTERFACE_A,
        ),
        (
            descriptor("provider-second", "in.process.duplicate"),
            &INTERFACE_A,
        ),
    ];
    let result = register(
        &runtime,
        manifest(
            "duplicate-rollback",
            &["in.process.duplicate", "in.process.duplicate"],
        ),
        &entries,
        (),
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
}

#[test]
fn residents_are_isolated_and_logical_unload_releases_them() {
    let first: Arc<Runtime> = runtime();
    let second: Arc<Runtime> = runtime();
    let entries = [(descriptor("provider", "in.process.a"), &INTERFACE_A)];
    let first_id = register(
        &first,
        manifest("resident-isolation", &["in.process.a"]),
        &entries,
        11_u32,
    )
    .expect("first runtime registration");
    let second_id = register(
        &second,
        manifest("resident-isolation", &["in.process.a"]),
        &entries,
        29_u32,
    )
    .expect("second runtime registration");
    assert_eq!(first_id, second_id);
    first.unload_bundle(first_id).expect("logical unload");
    register(
        &first,
        manifest("resident-isolation", &["in.process.a"]),
        &entries,
        2_u32,
    )
    .expect("re-register after logical unload");
}

#[test]
fn unload_waits_for_stateful_instance_quiescence() {
    let runtime: Arc<Runtime> = runtime();
    let entries = [(
        descriptor("provider", "in.process.stateful"),
        &INTERFACE_STATEFUL,
    )];
    let id = register(
        &runtime,
        manifest("active-gate", &["in.process.stateful"]),
        &entries,
        (),
    )
    .expect("in-process registration");
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
        Err(RuntimeError::InProcessBundleInUse {
            active_instances: 1,
            ..
        })
    ));
    // SAFETY: instance was created through the matching live HostApi callback.
    unsafe { ((*host).destroy_guest_instance)(host, interface, instance) };
    runtime.unload_bundle(id).expect("unload after destroy");
}
