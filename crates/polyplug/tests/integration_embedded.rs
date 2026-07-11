//! Focused behavioral coverage for in-process embedded bundle registration.

use core::ptr;
use std::sync::Arc;
use std::thread;

use polyplug::error::{RegistryError, RuntimeError};
use polyplug::{EmbeddedBundle, EmbeddedContract, Runtime};
use polyplug_abi::dispatch::{DispatchMechanisms, DispatchType, NativeDispatch, VmLoaderData};
use polyplug_abi::guest::GuestContractInstance;
use polyplug_abi::{
    AbiError, AbiErrorCode, GuestContractHandle, GuestContractInterface, HostApi, StringView,
    SupportedLanguage, Version,
};
use polyplug_utils::{BundleId, GuestContractId};

const CONTRACT_A: u64 = 0xE1B0_0000_0000_0001;
const CONTRACT_B: u64 = 0xE1B0_0000_0000_0002;
const CONTRACT_C: u64 = 0xE1B0_0000_0000_0003;

const VERSION: Version = Version {
    major: 1,
    minor: 0,
    patch: 0,
};

unsafe extern "C" fn create_instance(
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,
) {
    if !out_instance.is_null() {
        // SAFETY: out_instance is non-null and owned by the host callback caller.
        unsafe { out_instance.write(GuestContractInstance::null()) };
    }
}

unsafe extern "C" fn destroy_instance(
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

macro_rules! embedded_interface {
    ($contract_id:expr) => {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64($contract_id),
            contract_version: VERSION,
            dispatch_type: DispatchType::Native,
            create_instance,
            destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: ptr::null(),
                },
            },
        }
    };
}

static INTERFACE_A: GuestContractInterface = embedded_interface!(CONTRACT_A);
static INTERFACE_B: GuestContractInterface = embedded_interface!(CONTRACT_B);
static INTERFACE_C: GuestContractInterface = embedded_interface!(CONTRACT_C);

static CONTRACTS_SINGLE_A: [EmbeddedContract; 1] = [EmbeddedContract {
    plugin_name: "embedded-provider-a",
    contract_name: "embedded.contract.a",
    version: VERSION,
    interface: &INTERFACE_A,
}];

static CONTRACTS_MULTI_AB: [EmbeddedContract; 2] = [
    EmbeddedContract {
        plugin_name: "embedded-provider-a",
        contract_name: "embedded.contract.a",
        version: VERSION,
        interface: &INTERFACE_A,
    },
    EmbeddedContract {
        plugin_name: "embedded-provider-b",
        contract_name: "embedded.contract.b",
        version: VERSION,
        interface: &INTERFACE_B,
    },
];

static CONTRACTS_DUPLICATE: [EmbeddedContract; 2] = [
    EmbeddedContract {
        plugin_name: "first-duplicate-provider",
        contract_name: "embedded.duplicate",
        version: VERSION,
        interface: &INTERFACE_C,
    },
    EmbeddedContract {
        plugin_name: "second-duplicate-provider",
        contract_name: "embedded.duplicate",
        version: VERSION,
        interface: &INTERFACE_C,
    },
];

static CONTRACTS_COLLISION: [EmbeddedContract; 2] = [
    EmbeddedContract {
        plugin_name: "first-collision-provider",
        contract_name: "embedded.collision.first",
        version: VERSION,
        interface: &INTERFACE_C,
    },
    EmbeddedContract {
        plugin_name: "second-collision-provider",
        contract_name: "embedded.collision.second",
        version: VERSION,
        interface: &INTERFACE_C,
    },
];

static CONTRACTS_SECOND_INVALID: [EmbeddedContract; 2] = [
    EmbeddedContract {
        plugin_name: "rollback-first",
        contract_name: "embedded.contract.a",
        version: VERSION,
        interface: &INTERFACE_A,
    },
    EmbeddedContract {
        plugin_name: "rollback-second",
        contract_name: "",
        version: VERSION,
        interface: &INTERFACE_B,
    },
];

static DEPENDENT_CONTRACTS: [EmbeddedContract; 1] = [EmbeddedContract {
    plugin_name: "embedded-dependent",
    contract_name: "embedded.contract.b",
    version: VERSION,
    interface: &INTERFACE_B,
}];

static DEPENDENCIES_A: [GuestContractId; 1] = [GuestContractId::from_u64(CONTRACT_A)];

trait Required<T> {
    fn required(self, message: &str) -> T;
}

impl<T, E> Required<T> for Result<T, E> {
    fn required(self, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(_) => panic!("{message}"),
        }
    }
}

impl<T> Required<T> for Option<T> {
    fn required(self, message: &str) -> T {
        match self {
            Some(value) => value,
            None => panic!("{message}"),
        }
    }
}

fn require<T>(value: impl Required<T>, message: &str) -> T {
    value.required(message)
}

fn runtime() -> Arc<Runtime> {
    require(Runtime::builder().build(), "runtime build must succeed")
}

fn bundle(
    name: &'static str,
    contracts: &'static [EmbeddedContract],
    dependencies: &'static [GuestContractId],
) -> EmbeddedBundle {
    EmbeddedBundle {
        name,
        version: VERSION,
        runtime: SupportedLanguage::Rust,
        dependencies,
        contracts,
    }
}

#[test]
fn embedded_registration_handles_single_and_multiple_contracts() {
    let runtime = runtime();
    let single = bundle("embedded-single", &CONTRACTS_SINGLE_A, &[]);
    let single_id = require(
        runtime.register_embedded_bundle(&single),
        "single embedded bundle must register",
    );
    assert_eq!(single_id, BundleId::new(single.name));
    assert_ne!(single_id.id(), 0, "embedded bundle IDs must never be zero");

    let handle_a = require(
        runtime.find_guest_contract(CONTRACT_A, 0),
        "single contract must be discoverable",
    );
    assert!(
        !require(
            runtime.resolve_guest_contract(handle_a),
            "single contract handle must resolve",
        )
        .is_null()
    );

    let multi = bundle("embedded-multi", &CONTRACTS_MULTI_AB, &[]);
    let multi_id = require(
        runtime.register_embedded_bundle(&multi),
        "multi-contract embedded bundle must register",
    );
    assert!(runtime.find_guest_contract(CONTRACT_B, 0).is_ok());

    let metadata = require(
        runtime.registry().get_bundle_descriptor(multi_id),
        "embedded metadata must be registered",
    );
    assert_eq!(metadata.name, multi.name);
    assert_eq!(metadata.runtime, SupportedLanguage::Rust);
}

#[test]
fn embedded_registration_rolls_back_when_second_contract_is_invalid() {
    let runtime = runtime();
    let invalid = bundle(
        "embedded-rollback",
        &CONTRACTS_SECOND_INVALID,
        &DEPENDENCIES_A,
    );
    let result = runtime.register_embedded_bundle(&invalid);
    assert!(
        matches!(
            result,
            Err(RuntimeError::Registry(
                RegistryError::EmptyEmbeddedContract { index: 1 }
            ))
        ),
        "the second invalid contract must reject the complete bundle: {result:?}"
    );

    let id = BundleId::new(invalid.name);
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_err());
    assert!(runtime.registry().get_bundle_descriptor(id).is_none());

    let corrected = bundle("embedded-rollback", &CONTRACTS_SINGLE_A, &DEPENDENCIES_A);
    require(
        runtime.register_embedded_bundle(&corrected),
        "a corrected bundle must register after atomic rollback",
    );
}

#[test]
fn embedded_same_bundle_duplicate_is_deterministic() {
    let runtime = runtime();
    let duplicate = bundle("embedded-duplicate", &CONTRACTS_DUPLICATE, &[]);

    for _ in 0..2 {
        let result = runtime.register_embedded_bundle(&duplicate);
        assert!(
            matches!(
                result,
                Err(RuntimeError::Registry(RegistryError::DuplicateProvider {
                    ref contract,
                    ref existing,
                })) if contract == "embedded.duplicate" && existing == "first-duplicate-provider"
            ),
            "duplicate provider error must be deterministic: {result:?}"
        );
        assert!(
            runtime
                .registry()
                .get_bundle_descriptor(BundleId::new(duplicate.name))
                .is_none()
        );
    }

    let registered = bundle("embedded-already-registered", &CONTRACTS_SINGLE_A, &[]);
    require(
        runtime.register_embedded_bundle(&registered),
        "initial same-bundle provider must register",
    );
    let result = runtime.register_embedded_bundle(&registered);
    assert!(
        matches!(
            result,
            Err(RuntimeError::Registry(RegistryError::DuplicateProvider {
                ref contract,
                ref existing,
            })) if contract == "embedded.contract.a" && existing == "embedded-provider-a"
        ),
        "a repeated same-bundle provider must use the normal duplicate error: {result:?}"
    );
}

#[test]
fn embedded_contract_id_collision_is_deterministic_and_atomic() {
    let runtime = runtime();
    let collision = bundle("embedded-collision", &CONTRACTS_COLLISION, &[]);

    for _ in 0..2 {
        let result = runtime.register_embedded_bundle(&collision);
        assert!(
            matches!(
                result,
                Err(RuntimeError::Registry(RegistryError::ContractIdCollision {
                    id: CONTRACT_C,
                    ref name_a,
                    ref name_b,
                })) if name_a == "embedded.collision.first"
                    && name_b == "embedded.collision.second"
            ),
            "contract collision error must be deterministic: {result:?}"
        );
        assert!(runtime.find_guest_contract(CONTRACT_C, 0).is_err());
        assert!(
            runtime
                .registry()
                .get_bundle_descriptor(BundleId::new(collision.name))
                .is_none()
        );
    }
}

#[test]
fn embedded_different_bundles_can_provide_the_same_contract() {
    let runtime = runtime();
    let first = bundle("embedded-provider-first", &CONTRACTS_SINGLE_A, &[]);
    let second = bundle("embedded-provider-second", &CONTRACTS_SINGLE_A, &[]);
    let first_id = require(
        runtime.register_embedded_bundle(&first),
        "first provider must register",
    );
    let second_id = require(
        runtime.register_embedded_bundle(&second),
        "second provider must register",
    );

    let mut handles = [GuestContractHandle::null(); 2];
    assert_eq!(runtime.find_all_by_contract(CONTRACT_A, 0, &mut handles), 2);
    assert!(
        runtime
            .find_guest_contract_by_bundle(first_id.id(), CONTRACT_A, 0)
            .is_ok()
    );
    assert!(
        runtime
            .find_guest_contract_by_bundle(second_id.id(), CONTRACT_A, 0)
            .is_ok()
    );
}

#[test]
fn embedded_registration_isolated_between_runtimes() {
    let first_runtime = runtime();
    let second_runtime = runtime();
    let bundle = bundle("embedded-isolated", &CONTRACTS_SINGLE_A, &[]);
    require(
        first_runtime.register_embedded_bundle(&bundle),
        "first runtime must register its embedded bundle",
    );

    assert!(first_runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
    assert!(second_runtime.find_guest_contract(CONTRACT_A, 0).is_err());
    assert_ne!(first_runtime.host_abi(), second_runtime.host_abi());
}

#[test]
fn embedded_registration_is_atomic_under_competing_threads() {
    let runtime = runtime();
    let bundle = bundle("embedded-concurrent", &CONTRACTS_SINGLE_A, &[]);
    let first_runtime = Arc::clone(&runtime);
    let second_runtime = Arc::clone(&runtime);

    let (first, second) = thread::scope(|scope| {
        let first = scope.spawn(|| first_runtime.register_embedded_bundle(&bundle));
        let second = scope.spawn(|| second_runtime.register_embedded_bundle(&bundle));
        (
            require(first.join(), "first registration thread must not panic"),
            require(second.join(), "second registration thread must not panic"),
        )
    });

    assert_eq!(
        [first.is_ok(), second.is_ok()]
            .into_iter()
            .filter(|ok| *ok)
            .count(),
        1
    );
    let rejected = if first.is_err() { first } else { second };
    assert!(matches!(
        rejected,
        Err(RuntimeError::Registry(
            RegistryError::DuplicateProvider { .. }
        ))
    ));
    let mut handles = [GuestContractHandle::null(); 2];
    assert_eq!(runtime.find_all_by_contract(CONTRACT_A, 0, &mut handles), 1);
}

#[test]
fn embedded_unload_stales_handles_and_allows_reregistration() {
    let runtime = runtime();
    let bundle = bundle("embedded-reloadable", &CONTRACTS_SINGLE_A, &[]);
    let bundle_id = require(
        runtime.register_embedded_bundle(&bundle),
        "initial embedded registration must succeed",
    );
    let old_handle = require(
        runtime.find_guest_contract(CONTRACT_A, 0),
        "initial handle must resolve",
    );

    require(
        runtime.unload_bundle(bundle_id),
        "embedded bundle must use normal unload",
    );
    assert!(matches!(
        runtime.resolve_guest_contract(old_handle),
        Err(RegistryError::StaleHandle { .. })
    ));

    let replacement_id = require(
        runtime.register_embedded_bundle(&bundle),
        "unloaded embedded bundle must register again",
    );
    assert_eq!(replacement_id, bundle_id);
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_ok());
}

#[test]
fn embedded_dependencies_block_provider_unload_and_cascade_normally() {
    let runtime = runtime();
    let provider = bundle("embedded-dependency-provider", &CONTRACTS_SINGLE_A, &[]);
    let dependent = bundle(
        "embedded-dependency-dependent",
        &DEPENDENT_CONTRACTS,
        &DEPENDENCIES_A,
    );
    let provider_id = require(
        runtime.register_embedded_bundle(&provider),
        "provider must register",
    );
    require(
        runtime.register_embedded_bundle(&dependent),
        "dependent must register",
    );

    let result = runtime.unload_bundle(provider_id);
    assert!(
        matches!(
            result,
            Err(RuntimeError::DependencyInUse {
                ref provider,
                ref dependents,
            }) if provider == "embedded-dependency-provider"
                && dependents == &["embedded-dependency-dependent"]
        ),
        "declared dependency must block direct provider unload: {result:?}"
    );

    require(
        runtime.unload_bundle_cascade(provider_id),
        "cascade unload must remove dependent then provider",
    );
    assert!(runtime.find_guest_contract(CONTRACT_A, 0).is_err());
    assert!(runtime.find_guest_contract(CONTRACT_B, 0).is_err());
}

#[test]
fn raw_guest_registration_outside_loader_initialization_is_rejected() {
    let runtime = runtime();
    let host = runtime.host_abi();
    let descriptor = polyplug_abi::PluginDescriptor {
        name: StringView::from_static(b"raw-host-provider"),
        contract_name: StringView::from_static(b"raw.host.contract"),
        version: VERSION,
    };
    let mut result = AbiError::ok();

    // SAFETY: host comes from the runtime, descriptor and interface outlive the call,
    // and result is a valid out-parameter.
    unsafe {
        ((*host).register_guest_contract)(host, &descriptor, &INTERFACE_C, &mut result);
    }

    assert_eq!(result.code, AbiErrorCode::Generic as u32);
    assert!(runtime.find_guest_contract(CONTRACT_C, 0).is_err());
    // SAFETY: host comes from the live runtime and accepts its own HostApi pointer.
    let error_len = unsafe { ((*host).get_error_len)(host) };
    assert!(error_len > 0, "rejection must expose a runtime error");
}
