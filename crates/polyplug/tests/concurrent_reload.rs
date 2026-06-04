#![allow(clippy::expect_used)]

//! Dispatch-during-reload concurrency test.
//!
//! The hot-reload safety story rests on one defining condition: a slot's
//! interface may be swapped *while* other threads are resolving that slot and
//! calling through its function pointers. This test exercises exactly that —
//! eight reader threads continuously resolve a contract and invoke a dispatch
//! function pointer while the main thread swaps the slot's interface under the
//! registry write lock.
//!
//! Why this is sound: `find_guest_contract` followed by `resolve_guest_contract`
//! reads the slot's `Arc<GuestContractInterface>` under the read lock. The swap
//! (`swap_guest_contract_interface`) takes the write lock, replaces the slot's
//! `Arc`, and *retires* (does not drop) the old `Arc` for the runtime lifetime.
//! A reader therefore observes either the complete old interface or the complete
//! new one — never a half-swapped struct — and any interface memory it touched
//! stays alive because the retired `Arc` is never freed. The same retire-not-drop
//! mechanism backs `apply_reload_swap`, the reconciliation step the reload driver
//! runs after a bundle re-initializes; `swap_guest_contract_interface` is its
//! single-slot, publicly reachable equivalent.

use core::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;

use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractId, GuestContractInstance,
    GuestContractInterface, HostInterface, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;

const CONTRACT_ID: u64 = 0xCAFE_F00D_0000_0001_u64;
const READER_THREADS: usize = 8;
const ITERATIONS: usize = 10_000;

const MOCK_FNS: [*const (); 0] = [];

/// `create_instance` for the pre-reload interface. Returns a tagged instance so
/// readers can distinguish which interface version they resolved.
unsafe extern "C" fn create_instance_v1(
    _host: *const HostInterface,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

/// `create_instance` for the reloaded interface — a distinct function pointer so
/// callers can confirm the swap took effect.
unsafe extern "C" fn create_instance_v2(
    _host: *const HostInterface,
    _args: *const (),
) -> GuestContractInstance {
    GuestContractInstance::null()
}

unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostInterface,
    _instance: GuestContractInstance,
) {
}

static INTERFACE_V1: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(CONTRACT_ID),
    contract_version: Version {
        major: 1,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: create_instance_v1,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

static INTERFACE_V2: GuestContractInterface = GuestContractInterface {
    contract_id: GuestContractId::from_u64(CONTRACT_ID),
    contract_version: Version {
        major: 2,
        minor: 0,
        patch: 0,
    },
    dispatch_type: DispatchType::Native,
    create_instance: create_instance_v2,
    destroy_instance: noop_destroy_instance,
    dispatch: DispatchMechanisms {
        native: NativeDispatch {
            function_count: 0,
            functions: MOCK_FNS.as_ptr(),
        },
    },
};

fn make_descriptor(name: &'static str, contract_name: &'static str) -> PluginDescriptor {
    PluginDescriptor {
        name: StringView::from_static(name.as_bytes()),
        contract_name: StringView::from_static(contract_name.as_bytes()),
        version: Version {
            major: 1,
            minor: 0,
            patch: 0,
        },
    }
}

/// Eight readers dispatch through a contract's function pointers while the slot's
/// interface is swapped. No reader may observe a torn struct, crash, or
/// use-after-free; after the swap the contract must resolve to the reloaded
/// interface.
#[test]
fn dispatch_concurrent_with_reload_is_safe() {
    let registry: RuntimeStore = RuntimeStore::new();
    let bundle_id: BundleId = BundleId::new("bundle-a");
    let descriptor: PluginDescriptor = make_descriptor("bundle-a-plugin", "concurrent.contract");

    // SAFETY: INTERFACE_V1 is 'static, valid for the registry's lifetime.
    let handle: GuestContractHandle = unsafe {
        registry.register_guest_contract(
            descriptor,
            &INTERFACE_V1,
            "concurrent.contract".to_owned(),
            bundle_id,
        )
    }
    .expect("registration should succeed");
    let slot_idx: u32 = handle.index;

    let contract_id: GuestContractId = GuestContractId::from_u64(CONTRACT_ID);
    let dispatch_count: AtomicU64 = AtomicU64::new(0);

    thread::scope(|scope| {
        let reader_handles: Vec<thread::ScopedJoinHandle<'_, usize>> = (0..READER_THREADS)
            .map(|_| {
                let registry_ref: &RuntimeStore = &registry;
                let dispatch_count_ref: &AtomicU64 = &dispatch_count;
                scope.spawn(move || -> usize {
                    let mut completed: usize = 0;
                    for _ in 0..ITERATIONS {
                        let resolved: GuestContractHandle = registry_ref
                            .find_guest_contract(contract_id, 0)
                            .expect("contract must always resolve during reload");
                        assert!(!resolved.is_null(), "resolved handle must be valid");

                        let interface_ptr: *const GuestContractInterface = registry_ref
                            .resolve_guest_contract(resolved)
                            .expect("interface must always resolve during reload");
                        assert!(!interface_ptr.is_null(), "interface pointer must be valid");

                        // SAFETY: interface_ptr was returned by resolve_guest_contract
                        // and points at a slot interface whose Arc is retained
                        // (retire-not-drop) for the registry's lifetime, so it stays
                        // valid even if the slot is swapped concurrently.
                        unsafe {
                            let create_fn: unsafe extern "C" fn(
                                *const HostInterface,
                                *const (),
                            )
                                -> GuestContractInstance =
                                (*interface_ptr).create_instance;
                            let instance: GuestContractInstance =
                                create_fn(core::ptr::null(), core::ptr::null());
                            assert!(instance.is_null(), "mock create_instance returns null");
                        }

                        completed += 1;
                        dispatch_count_ref.fetch_add(1, Ordering::Relaxed);
                    }
                    completed
                })
            })
            .collect();

        // Wait until readers are actively dispatching before swapping the slot,
        // so the swap genuinely races in-flight resolves.
        while dispatch_count.load(Ordering::Relaxed) < 1_000 {
            core::hint::spin_loop();
        }

        let new_interface: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
        registry
            .swap_guest_contract_interface(slot_idx, new_interface)
            .expect("interface swap should succeed");

        for reader in reader_handles {
            let completed: usize = reader.join().expect("reader thread must not panic");
            assert_eq!(
                completed, ITERATIONS,
                "every reader must complete all dispatches"
            );
        }
    });

    let resolved_after: GuestContractHandle = registry
        .find_guest_contract(contract_id, 0)
        .expect("contract must resolve after reload");
    let interface_after: *const GuestContractInterface = registry
        .resolve_guest_contract(resolved_after)
        .expect("interface must resolve after reload");

    // SAFETY: interface_after points at the live slot interface, retained for the
    // registry's lifetime.
    let version_after: Version = unsafe { (*interface_after).contract_version };
    assert_eq!(
        version_after.major, 2,
        "after the swap the contract must resolve to the reloaded interface"
    );

    // SAFETY: same retained slot interface; reading the function pointer field is
    // a plain pointer comparison against the known reloaded callback.
    let create_after: unsafe extern "C" fn(
        *const HostInterface,
        *const (),
    ) -> GuestContractInstance = unsafe { (*interface_after).create_instance };
    let expected_create: unsafe extern "C" fn(
        *const HostInterface,
        *const (),
    ) -> GuestContractInstance = create_instance_v2;
    assert!(
        core::ptr::fn_addr_eq(create_after, expected_create),
        "reloaded interface must expose the v2 create_instance pointer"
    );
}
