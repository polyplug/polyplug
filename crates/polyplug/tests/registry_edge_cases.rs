#![allow(clippy::expect_used)]

//! Edge case tests for Registry.
//!
//! Tests for:
//! - resolve with valid/invalid handles
//! - concurrent access thread safety
//! - find_guest_contract with multiple implementations
//! - swap_interface during active resolve

use std::sync::Arc;
use std::sync::Barrier;

use polyplug::error::RegistryError;
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::{
    DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractId, GuestContractInterface,
    HostApi, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;

const MOCK_FUNCTIONS: [*const (); 0] = [];

/// No-op create_instance callback.
unsafe extern "C" fn noop_create_instance(
    _host: *const HostApi,
    _args: *const (),
) -> polyplug_abi::GuestContractInstance {
    polyplug_abi::GuestContractInstance::null()
}

/// No-op destroy_instance callback.
unsafe extern "C" fn noop_destroy_instance(
    _host: *const HostApi,
    _instance: polyplug_abi::GuestContractInstance,
) {
}

macro_rules! make_interface {
    ($contract_id:expr, $version:expr) => {
        GuestContractInterface {
            contract_id: GuestContractId::from_u64($contract_id),
            contract_version: $version,
            dispatch_type: DispatchType::Native,
            create_instance: noop_create_instance,
            destroy_instance: noop_destroy_instance,
            dispatch: DispatchMechanisms {
                native: NativeDispatch {
                    function_count: 0,
                    functions: MOCK_FUNCTIONS.as_ptr(),
                },
            },
        }
    };
}

// =============================================================================
// Test 1: resolve with valid handle after multiple registrations
// =============================================================================

#[test]
fn resolve_valid_handle_after_multiple_registrations() {
    static INTERFACE_A: GuestContractInterface = make_interface!(
        0xEEEE_0000_0000_0001_u64,
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    );
    static INTERFACE_B: GuestContractInterface = make_interface!(
        0xEEEE_0000_0000_0002_u64,
        Version {
            major: 2,
            minor: 0,
            patch: 0
        }
    );
    static INTERFACE_C: GuestContractInterface = make_interface!(
        0xEEEE_0000_0000_0003_u64,
        Version {
            major: 3,
            minor: 0,
            patch: 0
        }
    );

    let registry: RuntimeStore = RuntimeStore::new();

    let descriptor_a: PluginDescriptor = make_descriptor("plugin_a", "contract.a");
    let descriptor_b: PluginDescriptor = make_descriptor("plugin_b", "contract.b");
    let descriptor_c: PluginDescriptor = make_descriptor("plugin_c", "contract.c");

    // SAFETY: INTERFACE_A, INTERFACE_B, INTERFACE_C are 'static, pointers are valid for Registry lifetime.
    let handle_a: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_a,
                &INTERFACE_A,
                "contract.a".to_owned(),
                BundleId::from_u64(1_u64),
            )
            .expect("registration A should succeed")
    };

    // SAFETY: INTERFACE_B is 'static, pointer is valid for Registry lifetime.
    let handle_b: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_b,
                &INTERFACE_B,
                "contract.b".to_owned(),
                BundleId::from_u64(2_u64),
            )
            .expect("registration B should succeed")
    };

    // SAFETY: INTERFACE_C is 'static, pointer is valid for Registry lifetime.
    let handle_c: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_c,
                &INTERFACE_C,
                "contract.c".to_owned(),
                BundleId::from_u64(3_u64),
            )
            .expect("registration C should succeed")
    };

    let interface_ptr_a: *const GuestContractInterface = registry
        .resolve_guest_contract(handle_a)
        .expect("resolve for handle_a should succeed");
    // SAFETY: interface_ptr_a points to INTERFACE_A which is 'static.
    let contract_id_a: GuestContractId = unsafe { (*interface_ptr_a).contract_id };
    assert_eq!(
        contract_id_a, INTERFACE_A.contract_id,
        "handle_a should return INTERFACE_A"
    );

    let interface_ptr_b: *const GuestContractInterface = registry
        .resolve_guest_contract(handle_b)
        .expect("resolve for handle_b should succeed");
    // SAFETY: interface_ptr_b points to INTERFACE_B which is 'static.
    let contract_id_b: GuestContractId = unsafe { (*interface_ptr_b).contract_id };
    assert_eq!(
        contract_id_b, INTERFACE_B.contract_id,
        "handle_b should return INTERFACE_B"
    );

    let interface_ptr_c: *const GuestContractInterface = registry
        .resolve_guest_contract(handle_c)
        .expect("resolve for handle_c should succeed");
    // SAFETY: interface_ptr_c points to INTERFACE_C which is 'static.
    let contract_id_c: GuestContractId = unsafe { (*interface_ptr_c).contract_id };
    assert_eq!(
        contract_id_c, INTERFACE_C.contract_id,
        "handle_c should return INTERFACE_C"
    );
}

// =============================================================================
// Test 2: resolve with handle pointing to vacant slot
// =============================================================================

#[test]
fn resolve_vacant_slot_returns_invalid_handle() {
    static INTERFACE: GuestContractInterface = make_interface!(
        0xEEEE_0000_0000_0010_u64,
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    );

    let registry: RuntimeStore = RuntimeStore::new();

    let descriptor: PluginDescriptor = make_descriptor("plugin_single", "contract.single");
    // SAFETY: INTERFACE is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE,
                "contract.single".to_owned(),
                BundleId::from_u64(100_u64),
            )
            .expect("registration should succeed")
    };

    // Test 1: Handle with index beyond slots length
    let out_of_bounds_handle: GuestContractHandle = GuestContractHandle { index: 9999_u32 };
    let result: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve_guest_contract(out_of_bounds_handle);
    assert!(
        matches!(result, Err(RegistryError::InvalidHandle { .. })),
        "out of bounds handle should return InvalidHandle error"
    );

    // Test 2: Handle pointing to slot that was never used (index 1 when only slot 0 exists)
    let unused_slot_handle: GuestContractHandle = GuestContractHandle { index: 1_u32 };
    let result_unused: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve_guest_contract(unused_slot_handle);
    assert!(
        matches!(result_unused, Err(RegistryError::InvalidHandle { .. })),
        "unused slot handle should return InvalidHandle error"
    );

    // Valid handle should still work
    let valid_result: Result<*const GuestContractInterface, RegistryError> =
        registry.resolve_guest_contract(handle);
    assert!(
        valid_result.is_ok(),
        "valid handle should resolve successfully"
    );
}

// =============================================================================
// Test 3: resolve concurrent access (thread safety)
// =============================================================================

const CONCURRENT_THREADS: usize = 8_usize;
const CONCURRENT_ROUNDS: usize = 100_usize;

const CONCURRENT_CONTRACT_IDS: [u64; CONCURRENT_THREADS] = [
    0xEEEE_1000_0000_0001_u64,
    0xEEEE_1000_0000_0002_u64,
    0xEEEE_1000_0000_0003_u64,
    0xEEEE_1000_0000_0004_u64,
    0xEEEE_1000_0000_0005_u64,
    0xEEEE_1000_0000_0006_u64,
    0xEEEE_1000_0000_0007_u64,
    0xEEEE_1000_0000_0008_u64,
];

const CONCURRENT_CONTRACT_NAMES: [&str; CONCURRENT_THREADS] = [
    "concurrent.contract.1",
    "concurrent.contract.2",
    "concurrent.contract.3",
    "concurrent.contract.4",
    "concurrent.contract.5",
    "concurrent.contract.6",
    "concurrent.contract.7",
    "concurrent.contract.8",
];

const CONCURRENT_PLUGIN_NAMES: [&str; CONCURRENT_THREADS] = [
    "concurrent_plugin_0",
    "concurrent_plugin_1",
    "concurrent_plugin_2",
    "concurrent_plugin_3",
    "concurrent_plugin_4",
    "concurrent_plugin_5",
    "concurrent_plugin_6",
    "concurrent_plugin_7",
];

static CONCURRENT_INTERFACES: [GuestContractInterface; CONCURRENT_THREADS] = [
    make_interface!(
        CONCURRENT_CONTRACT_IDS[0],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[1],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[2],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[3],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[4],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[5],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[6],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
    make_interface!(
        CONCURRENT_CONTRACT_IDS[7],
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    ),
];

#[test]
fn resolve_concurrent_access_thread_safety() {
    let registry: Arc<RuntimeStore> = Arc::new(RuntimeStore::new());
    let barrier: Arc<Barrier> = Arc::new(Barrier::new(CONCURRENT_THREADS));
    let mut thread_handles: Vec<std::thread::JoinHandle<()>> =
        Vec::with_capacity(CONCURRENT_THREADS);

    let mut handles: Vec<GuestContractHandle> = Vec::with_capacity(CONCURRENT_THREADS);
    for idx in 0_usize..CONCURRENT_THREADS {
        let descriptor: PluginDescriptor =
            make_descriptor(CONCURRENT_PLUGIN_NAMES[idx], CONCURRENT_CONTRACT_NAMES[idx]);
        // SAFETY: CONCURRENT_INTERFACES[idx] is 'static, pointer is valid for Registry lifetime.
        let handle: GuestContractHandle = unsafe {
            registry
                .register_guest_contract(
                    descriptor,
                    &CONCURRENT_INTERFACES[idx],
                    CONCURRENT_CONTRACT_NAMES[idx].to_owned(),
                    BundleId::from_u64(idx as u64),
                )
                .expect("registration should succeed")
        };
        handles.push(handle);
    }

    for idx in 0_usize..CONCURRENT_THREADS {
        let registry_clone: Arc<RuntimeStore> = Arc::clone(&registry);
        let barrier_clone: Arc<Barrier> = Arc::clone(&barrier);
        let handle: GuestContractHandle = handles[idx];
        let expected_contract_id: u64 = CONCURRENT_CONTRACT_IDS[idx];

        let thread_handle: std::thread::JoinHandle<()> = std::thread::spawn(move || {
            barrier_clone.wait();

            for _round in 0_usize..CONCURRENT_ROUNDS {
                let interface_ptr: *const GuestContractInterface = registry_clone
                    .resolve_guest_contract(handle)
                    .expect("resolve should succeed in concurrent context");
                // SAFETY: interface_ptr points to a 'static GuestContractInterface.
                let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
                assert_eq!(
                    contract_id.id(),
                    expected_contract_id,
                    "thread {} got wrong contract_id",
                    idx
                );
            }
        });
        thread_handles.push(thread_handle);
    }

    for handle in thread_handles {
        handle.join().expect("thread should not panic");
    }
}

// =============================================================================
// Test 4: find_guest_contract with multiple implementations
// =============================================================================

#[test]
fn find_by_contract_multiple_implementations_returns_first() {
    const MULTI_CONTRACT_ID: u64 = 0xEEEE_2000_0000_0001_u64;

    static INTERFACE_IMPL_A: GuestContractInterface = make_interface!(
        MULTI_CONTRACT_ID,
        Version {
            major: 1,
            minor: 0,
            patch: 0
        }
    );
    static INTERFACE_IMPL_B: GuestContractInterface = make_interface!(
        MULTI_CONTRACT_ID,
        Version {
            major: 2,
            minor: 0,
            patch: 0
        }
    );
    static INTERFACE_IMPL_C: GuestContractInterface = make_interface!(
        MULTI_CONTRACT_ID,
        Version {
            major: 3,
            minor: 0,
            patch: 0
        }
    );

    let registry: RuntimeStore = RuntimeStore::new();

    let descriptor_a: PluginDescriptor = make_descriptor("impl_a", "multi.contract");
    let descriptor_b: PluginDescriptor = make_descriptor("impl_b", "multi.contract");
    let descriptor_c: PluginDescriptor = make_descriptor("impl_c", "multi.contract");

    // SAFETY: INTERFACE_IMPL_A is 'static, pointer is valid for Registry lifetime.
    let handle_a: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_a,
                &INTERFACE_IMPL_A,
                "multi.contract".to_owned(),
                BundleId::from_u64(1000_u64),
            )
            .expect("registration A should succeed")
    };

    // SAFETY: INTERFACE_IMPL_B is 'static, pointer is valid for Registry lifetime.
    let handle_b: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_b,
                &INTERFACE_IMPL_B,
                "multi.contract".to_owned(),
                BundleId::from_u64(2000_u64),
            )
            .expect("registration B should succeed")
    };

    // SAFETY: INTERFACE_IMPL_C is 'static, pointer is valid for Registry lifetime.
    let handle_c: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor_c,
                &INTERFACE_IMPL_C,
                "multi.contract".to_owned(),
                BundleId::from_u64(3000_u64),
            )
            .expect("registration C should succeed")
    };

    assert_ne!(
        handle_a.index, handle_b.index,
        "each impl should have its own slot"
    );
    assert_ne!(
        handle_b.index, handle_c.index,
        "each impl should have its own slot"
    );
    assert_ne!(
        handle_a.index, handle_c.index,
        "each impl should have its own slot"
    );

    let found: GuestContractHandle = registry
        .find_guest_contract(GuestContractId::from_u64(MULTI_CONTRACT_ID), 0_u32)
        .expect("find_guest_contract should find an implementation");

    assert_eq!(
        found.index, handle_a.index,
        "find_guest_contract should return first registered implementation"
    );

    let interface_ptr: *const GuestContractInterface = registry
        .resolve_guest_contract(found)
        .expect("resolve should succeed");
    // SAFETY: interface_ptr points to INTERFACE_IMPL_A which is 'static.
    let version: &Version = unsafe { &(*interface_ptr).contract_version };
    assert_eq!(
        *version, INTERFACE_IMPL_A.contract_version,
        "should resolve to first implementation's interface"
    );

    let mut all_handles: [GuestContractHandle; 4] = [GuestContractHandle { index: 0_u32 }; 4];
    let count: usize = registry.find_all_guest_contracts(
        GuestContractId::from_u64(MULTI_CONTRACT_ID),
        0_u32,
        &mut all_handles,
    );
    assert_eq!(
        count, 3,
        "find_all_by_contract should return all 3 implementations"
    );
}

// =============================================================================
// Test 5: swap_interface during active resolve
// =============================================================================

#[test]
fn swap_interface_during_active_resolve() {
    const SWAP_TEST_CONTRACT_ID: u64 = 0xEEEE_3000_0000_0001_u64;
    const VERSION_V1: Version = Version {
        major: 1,
        minor: 0,
        patch: 0,
    };
    const VERSION_V2: Version = Version {
        major: 2,
        minor: 0,
        patch: 0,
    };

    static INTERFACE_V1: GuestContractInterface =
        make_interface!(SWAP_TEST_CONTRACT_ID, VERSION_V1);
    static INTERFACE_V2: GuestContractInterface =
        make_interface!(SWAP_TEST_CONTRACT_ID, VERSION_V2);

    let registry: RuntimeStore = RuntimeStore::new();

    let descriptor: PluginDescriptor = make_descriptor("swap_test_plugin", "swap.test.contract");
    // SAFETY: INTERFACE_V1 is 'static, pointer is valid for Registry lifetime.
    let handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                descriptor,
                &INTERFACE_V1,
                "swap.test.contract".to_owned(),
                BundleId::from_u64(5000_u64),
            )
            .expect("initial registration should succeed")
    };

    let interface_ptr_before: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve before swap should succeed");
    // SAFETY: interface_ptr_before points to INTERFACE_V1 which is 'static.
    let version_before: &Version = unsafe { &(*interface_ptr_before).contract_version };
    assert_eq!(
        *version_before, VERSION_V1,
        "interface before swap should have V1"
    );

    // Perform the swap - direct swap_interface takes Arc<GuestContractInterface>
    let new_arc: Arc<GuestContractInterface> = Arc::new(INTERFACE_V2);
    registry
        .swap_guest_contract_interface(handle.index, new_arc)
        .expect("swap_interface should succeed");

    // The same handle should now resolve to V2 (no generation tracking)
    let interface_ptr_after: *const GuestContractInterface = registry
        .resolve_guest_contract(handle)
        .expect("resolve should succeed after swap");

    // SAFETY: interface_ptr_after points to INTERFACE_V2 which is 'static.
    let version_after: &Version = unsafe { &(*interface_ptr_after).contract_version };
    assert_eq!(*version_after, VERSION_V2, "after swap should point to V2");
}

// =============================================================================
// Helper functions
// =============================================================================

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
