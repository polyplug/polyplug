#![allow(clippy::expect_used)]

//! Focused behavioral coverage for canonical in-process bundle registration.

use core::ffi::c_void;
use core::ptr;

use std::sync::Arc;

use polyplug::error::{RegistryError, RuntimeError};
use polyplug::{InProcessBundle, Runtime};
use polyplug_abi::dispatch::{DispatchMechanisms, DispatchType, NativeDispatch, VmLoaderData};
use polyplug_abi::guest::GuestContractInstance;
use polyplug_abi::{
    AbiError, AbiErrorCode, GuestContractInterface, HostApi, InProcessBundleMetadata,
    InProcessBundleRegistration, InProcessContractRegistration, PluginDescriptor, StringView,
    SupportedLanguage, Version,
};
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

fn contract(
    provider: &'static str,
    contract_name: &'static str,
    interface: &'static GuestContractInterface,
) -> InProcessContractRegistration {
    InProcessContractRegistration {
        descriptor: descriptor(provider, contract_name),
        interface,
        adapter_context: ptr::null_mut(),
    }
}

fn registration(
    name: &'static str,
    dependencies: &[u64],
    contracts: &[InProcessContractRegistration],
) -> InProcessBundleRegistration {
    InProcessBundleRegistration {
        metadata: InProcessBundleMetadata {
            name: StringView::from_static(name.as_bytes()),
            version: VERSION,
            runtime: SupportedLanguage::Rust,
        },
        dependency_ids: dependencies.as_ptr(),
        dependency_count: dependencies.len(),
        contracts: contracts.as_ptr(),
        contract_count: contracts.len(),
    }
}

fn runtime() -> Arc<Runtime> {
    Runtime::builder().build().expect("runtime build")
}

fn register<R: Send + Sync + 'static>(
    runtime: &Runtime,
    input: InProcessBundleRegistration,
    resident: R,
) -> BundleId {
    runtime
        .register_in_process_bundle(InProcessBundle::new(input, resident))
        .expect("in-process registration")
}

#[test]
fn complete_registration_publishes_multiple_contracts_atomically() {
    let runtime: Arc<Runtime> = runtime();
    let contracts: [InProcessContractRegistration; 2] = [
        contract("provider-a", "in.process.a", &INTERFACE_A),
        contract("provider-b", "in.process.b", &INTERFACE_B),
    ];
    let id: BundleId = register(
        &runtime,
        registration("atomic-success", &[], &contracts),
        (),
    );

    assert_eq!(id, BundleId::new("atomic-success"));
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert!(runtime.find_guest_contract(CONTRACT_B, 0).is_ok());
}

#[test]
fn duplicate_second_contract_rolls_back_the_complete_bundle() {
    let runtime: Arc<Runtime> = runtime();
    let contracts: [InProcessContractRegistration; 2] = [
        contract("provider-first", "in.process.duplicate", &INTERFACE_A),
        contract("provider-second", "in.process.duplicate", &INTERFACE_A),
    ];
    let result: Result<BundleId, RuntimeError> = runtime.register_in_process_bundle(
        InProcessBundle::new(registration("duplicate-rollback", &[], &contracts), ()),
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
fn residents_are_isolated_between_runtimes() {
    let first: Arc<Runtime> = runtime();
    let second: Arc<Runtime> = runtime();
    let contracts: [InProcessContractRegistration; 1] =
        [contract("provider", "in.process.a", &INTERFACE_A)];
    let first_id: BundleId = register(
        &first,
        registration("resident-isolation", &[], &contracts),
        11_u32,
    );
    let second_id: BundleId = register(
        &second,
        registration("resident-isolation", &[], &contracts),
        29_u32,
    );

    assert_eq!(first_id, second_id);
    assert!(first.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert!(second.find_guest_contract(CONTRACT_A, 0).is_ok());
}

#[test]
fn unload_releases_resident_before_reregistering() {
    let runtime: Arc<Runtime> = runtime();
    let contracts: [InProcessContractRegistration; 1] =
        [contract("provider", "in.process.a", &INTERFACE_A)];
    let first: BundleId = register(
        &runtime,
        registration("unload-reregister", &[], &contracts),
        1_u32,
    );
    runtime.unload_bundle(first).expect("logical unload");

    let second: BundleId = register(
        &runtime,
        registration("unload-reregister", &[], &contracts),
        2_u32,
    );
    assert_eq!(first, second);
}

#[test]
fn unload_waits_for_stateful_instance_quiescence() {
    let runtime: Arc<Runtime> = runtime();
    let contracts: [InProcessContractRegistration; 1] = [contract(
        "provider",
        "in.process.stateful",
        &INTERFACE_STATEFUL,
    )];
    let id: BundleId = register(&runtime, registration("active-gate", &[], &contracts), ());
    let handle = runtime
        .find_guest_contract(CONTRACT_STATEFUL, 0)
        .expect("registered stateful contract");
    let interface = runtime
        .resolve_guest_contract(handle)
        .expect("resolve stateful contract");
    let mut instance: GuestContractInstance = GuestContractInstance::null();
    let host: *const HostApi = runtime.host_abi();

    // SAFETY: host and interface belong to the live runtime; instance is writable.
    unsafe { ((*host).create_guest_instance)(host, interface, ptr::null(), &mut instance) };
    assert!(!instance.data.is_null());
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

#[test]
fn host_api_rejects_null_and_malformed_registration_inputs() {
    let runtime: Arc<Runtime> = runtime();
    let host: *const HostApi = runtime.host_abi();
    let mut bundle_id: u64 = u64::MAX;
    let mut error: AbiError = AbiError::ok();

    // SAFETY: host is live and the callback accepts a null registration for validation.
    unsafe { ((*host).register_in_process_bundle)(host, ptr::null(), &mut bundle_id, &mut error) };
    assert_eq!(bundle_id, 0);
    assert_eq!(error.code, AbiErrorCode::InvalidPointer as u32);

    let malformed: InProcessBundleRegistration = InProcessBundleRegistration {
        metadata: InProcessBundleMetadata {
            name: StringView::from_static(b"malformed"),
            version: VERSION,
            runtime: SupportedLanguage::Rust,
        },
        dependency_ids: ptr::null(),
        dependency_count: 0,
        contracts: ptr::null(),
        contract_count: 1,
    };
    // SAFETY: host and malformed registration are live for this synchronous validation call.
    unsafe { ((*host).register_in_process_bundle)(host, &malformed, &mut bundle_id, &mut error) };
    assert_eq!(bundle_id, 0);
    assert_eq!(error.code, AbiErrorCode::Generic as u32);
}
