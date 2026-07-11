#![allow(clippy::expect_used)]

//! Integration tests: cross-plugin lookup, multi-impl registry, stale handle detection,
//! and dependency enforcement via the new Epic 9.7 ABI.
//!
//! Tests a-d: pure Registry API (find_guest_contract, find_by_bundle, find_all, resolve).
//! Tests e-g: dependency enforcement -- see `crates/polyplug/src/runtime/mod.rs`
//!            unit tests (`cross_plugin_dep_tests` submodule) because `INIT_BUNDLE_ID`
//!            is `pub(crate)` and cannot be accessed from an external crate.
//!

use core::ffi::c_void;
use core::ptr;
use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractInstance, GuestContractInterface, HostApi,
    NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::GuestContractId;

// --- Helpers -----------------------------------------------------------------

/// Create a null instance (stub for tests).
unsafe extern "C" fn null_create_instance(
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

/// Destroy instance (stub for tests).
unsafe extern "C" fn null_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

/// Allocate a `'static` `GuestContractInterface` with the given contract_id.
///
/// Intentional leak -- test interfaces are pointer-sized and tests are short-lived.
/// The interface must be `'static` because `Registry::register` stores a raw pointer
/// that must remain valid for the registry's lifetime.
fn make_static_interface(cid: GuestContractId) -> &'static GuestContractInterface {
    make_static_interface_minor(cid, 0)
}

/// Like `make_static_interface`, but with a caller-chosen minor version.
///
/// `RuntimeStore::register_guest_contract` copies the interface into an
/// `Arc<GuestContractInterface>` (`Arc::new(*ptr)`), so a resolved handle never
/// points back at the original leaked allocation. Tests that must tell two
/// otherwise-identical registrations apart distinguish them by interface *content*
/// (here, the minor version) rather than by pointer identity.
fn make_static_interface_minor(
    cid: GuestContractId,
    minor: u32,
) -> &'static GuestContractInterface {
    Box::leak(Box::new(GuestContractInterface {
        contract_id: cid,
        contract_version: Version {
            major: 1,
            minor,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        adapter_context: ptr::null_mut(),
        create_instance: null_create_instance,
        destroy_instance: null_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: ptr::null(),
            },
        },
    }))
}

fn make_desc(plugin_name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(plugin_name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

// --- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::make_desc;
    use super::make_static_interface;
    use super::make_static_interface_minor;
    use polyplug::error::RegistryError;
    use polyplug::runtime_store::RuntimeStore;
    use polyplug_abi::GuestContractHandle;
    use polyplug_abi::GuestContractInterface;
    use polyplug_abi::PluginDescriptor;
    use polyplug_utils::{BundleId, GuestContractId};

    // -- Test a ----------------------------------------------------------------

    /// Single plugin registered for a contract -- find_guest_contract returns a valid handle.
    #[test]
    fn find_by_contract_single_plugin() {
        let registry: RuntimeStore = RuntimeStore::new();
        let cid: GuestContractId = GuestContractId::new("audio.Decoder", 0);
        let bid: BundleId = BundleId::new("audio-engine");
        let interface: &'static GuestContractInterface = make_static_interface(cid);
        let desc: PluginDescriptor = make_desc("decoder", "audio.Decoder");
        // SAFETY: interface is 'static and valid for the duration of this test.
        unsafe {
            registry.register_guest_contract(desc, interface, "audio.Decoder".to_owned(), bid)
        }
        .expect("register should succeed");

        let handle: GuestContractHandle = registry
            .find_guest_contract(cid, 0)
            .expect("find_guest_contract should return Ok");

        assert!(!handle.is_null(), "returned handle must not be null");
    }

    // -- Test b ----------------------------------------------------------------

    /// Two plugins from different bundles implement the same contract --
    /// find_all_by_contract returns both.
    #[test]
    fn find_all_returns_two_impls() {
        let registry: RuntimeStore = RuntimeStore::new();
        let cid: GuestContractId = GuestContractId::new("audio.Decoder", 0);
        let interface_a: &'static GuestContractInterface = make_static_interface(cid);
        let interface_b: &'static GuestContractInterface = make_static_interface(cid);

        // SAFETY: interfaces are 'static.
        unsafe {
            registry
                .register_guest_contract(
                    make_desc("decoder-a", "audio.Decoder"),
                    interface_a,
                    "audio.Decoder".to_owned(),
                    BundleId::new("bundle-a"),
                )
                .expect("register bundle-a")
        };
        // SAFETY: interface_b is 'static.
        unsafe {
            registry
                .register_guest_contract(
                    make_desc("decoder-b", "audio.Decoder"),
                    interface_b,
                    "audio.Decoder".to_owned(),
                    BundleId::new("bundle-b"),
                )
                .expect("register bundle-b")
        };

        let mut handles: [GuestContractHandle; 4] = [GuestContractHandle::null(); 4];
        let count: usize = registry.find_all_guest_contracts(cid, 0, &mut handles);
        assert_eq!(count, 2, "must find exactly 2 providers");
    }

    // -- Test c ----------------------------------------------------------------

    /// find_by_bundle returns the handle for the specific requested bundle,
    /// not the first-registered one.
    #[test]
    fn find_by_bundle_specificity() {
        let registry: RuntimeStore = RuntimeStore::new();
        let cid: GuestContractId = GuestContractId::new("audio.Decoder", 0);
        let bid_a: BundleId = BundleId::new("bundle-a");
        let bid_b: BundleId = BundleId::new("bundle-b");
        // Distinguish the two registrations by interface content (minor version):
        // the store copies interfaces on register, so pointer identity cannot be used.
        let interface_a: &'static GuestContractInterface = make_static_interface_minor(cid, 7);
        let interface_b: &'static GuestContractInterface = make_static_interface_minor(cid, 9);

        // SAFETY: interfaces are 'static.
        unsafe {
            registry
                .register_guest_contract(
                    make_desc("decoder-a", "audio.Decoder"),
                    interface_a,
                    "audio.Decoder".to_owned(),
                    bid_a,
                )
                .expect("register bundle-a")
        };
        // SAFETY: interface_b is 'static.
        unsafe {
            registry
                .register_guest_contract(
                    make_desc("decoder-b", "audio.Decoder"),
                    interface_b,
                    "audio.Decoder".to_owned(),
                    bid_b,
                )
                .expect("register bundle-b")
        };

        let found: GuestContractHandle = registry
            .find_guest_contract_by_bundle(bid_b, cid, 0)
            .expect("find_by_bundle(bundle-b) should succeed");

        // Resolve and verify the interface content belongs to bundle-b, not bundle-a.
        // The store copies interfaces on register (`Arc::new(*ptr)`), so we compare
        // content (minor version) rather than pointer identity against the original.
        let resolved_ptr: *const GuestContractInterface = registry
            .resolve_guest_contract(found)
            .expect("resolve must succeed for a freshly registered handle");

        // SAFETY: `resolved_ptr` points to a live interface owned by the registry,
        // which outlives this borrow.
        let resolved_minor: u32 = unsafe { (*resolved_ptr).contract_version.minor };

        assert_eq!(
            resolved_minor, interface_b.contract_version.minor,
            "resolved interface must be bundle-b's interface (minor=9), not bundle-a's (minor=7)"
        );
        assert_ne!(
            resolved_minor, interface_a.contract_version.minor,
            "resolved interface must not be bundle-a's interface"
        );
    }

    // -- Test d ----------------------------------------------------------------

    /// A handle with an out-of-bounds index is rejected by resolve with InvalidHandle.
    #[test]
    fn invalid_handle_rejected() {
        let registry: RuntimeStore = RuntimeStore::new();

        // Construct a handle with an out-of-bounds index.
        let invalid: GuestContractHandle = GuestContractHandle {
            index: 999,
            generation: 0,
        };

        let result = registry.resolve_guest_contract(invalid);
        assert!(
            matches!(result, Err(RegistryError::InvalidHandle { .. })),
            "invalid handle must return Err(InvalidHandle)"
        );
    }
}
