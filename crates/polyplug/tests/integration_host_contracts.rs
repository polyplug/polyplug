#![allow(clippy::expect_used)]

//! Integration tests for host contract registration and lookup in Runtime.
//!
//! Tests cover:
//! - Registration (register_host_contract)
//! - Lookup (get_host_contract)
//! - Version checking (min_version parameter)
//! - Thread safety (concurrent access)
//! - Missing contracts return None
//! - Unregister functionality

use polyplug::runtime::Runtime;
use polyplug_abi::{
    DispatchType, HostContractDispatch, HostContractVTable, HostContractVTableHeader,
    NativeHostContractDispatch,
};

// ─── Helper: Create a static host contract vtable ─────────────────────────────

/// Create a leaked (static) HostContractVTable for testing.
/// The vtable is leaked and lives for the process lifetime.
fn create_static_vtable(
    contract_id: u64,
    major: u32,
    minor: u32,
    function_count: u32,
) -> &'static HostContractVTable {
    // Create a dummy function pointer - use null for simplicity in tests
    // since we never actually call the functions in these tests.
    let vtable: Box<HostContractVTable> = Box::new(HostContractVTable {
        header: HostContractVTableHeader {
            vtable_version: 1,
            contract_id,
            contract_major: major,
            contract_minor: minor,
            function_count,
            dispatch_type: DispatchType::Native,
        },
        dispatch: HostContractDispatch {
            native: NativeHostContractDispatch {
                impl_ptr: core::ptr::null(),
                functions: core::ptr::null(),
            },
        },
    });

    Box::leak(vtable)
}

// ─── Registration Tests ───────────────────────────────────────────────────────

#[test]
fn register_host_contract_success() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x1234_5678_9ABC_DEF0;
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);

    let result: Result<(), polyplug::error::HostContractError> =
        runtime.register_host_contract(contract_id, vtable);

    assert!(result.is_ok(), "registration should succeed: {result:?}");
}

#[test]
fn register_host_contract_duplicate_returns_error() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0xABCD_EF01_2345_6789;
    let vtable1: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);
    let vtable2: &'static HostContractVTable = create_static_vtable(contract_id, 2, 0, 1);

    // First registration succeeds
    let result1: Result<(), polyplug::error::HostContractError> =
        runtime.register_host_contract(contract_id, vtable1);
    assert!(
        result1.is_ok(),
        "first registration should succeed: {result1:?}"
    );

    // Second registration fails with DuplicateContract
    let result2: Result<(), polyplug::error::HostContractError> =
        runtime.register_host_contract(contract_id, vtable2);
    assert!(
        result2.is_err(),
        "duplicate registration should return error: {result2:?}"
    );
    match result2 {
        Err(polyplug::error::HostContractError::DuplicateContract { contract_id: id }) => {
            assert_eq!(id, contract_id, "error contract_id should match");
        }
        _ => panic!("error should be DuplicateContract variant, got: {result2:?}"),
    }
}

#[test]
fn register_multiple_contracts() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id1: u64 = 0x1111_1111_1111_1111;
    let contract_id2: u64 = 0x2222_2222_2222_2222;
    let contract_id3: u64 = 0x3333_3333_3333_3333;

    let vtable1: &'static HostContractVTable = create_static_vtable(contract_id1, 1, 0, 1);
    let vtable2: &'static HostContractVTable = create_static_vtable(contract_id2, 1, 0, 2);
    let vtable3: &'static HostContractVTable = create_static_vtable(contract_id3, 2, 0, 3);

    assert!(runtime
        .register_host_contract(contract_id1, vtable1)
        .is_ok());
    assert!(runtime
        .register_host_contract(contract_id2, vtable2)
        .is_ok());
    assert!(runtime
        .register_host_contract(contract_id3, vtable3)
        .is_ok());
}

// ─── Lookup Tests ─────────────────────────────────────────────────────────────

#[test]
fn get_host_contract_found() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0xAAAA_AAAA_AAAA_AAAA;
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 5, 3);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);

    assert!(result.is_some(), "lookup should return Some(vtable)");
    let returned_vtable: &HostContractVTable = result.expect("vtable should be Some");
    assert_eq!(
        returned_vtable.header.contract_id, contract_id,
        "returned contract_id should match"
    );
    assert_eq!(
        returned_vtable.header.contract_major, 1,
        "returned major version should match"
    );
    assert_eq!(
        returned_vtable.header.contract_minor, 5,
        "returned minor version should match"
    );
}

#[test]
fn get_host_contract_not_found_returns_none() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let nonexistent_id: u64 = 0xFFFF_FFFF_FFFF_FFFF;
    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(nonexistent_id, 0);

    assert!(
        result.is_none(),
        "lookup of nonexistent contract should return None"
    );
}

#[test]
fn get_host_contract_after_unregister() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0xBBBB_BBBB_BBBB_BBBB;
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Verify it's registered
    assert!(runtime.get_host_contract(contract_id, 0).is_some());

    // Unregister
    let removed: bool = runtime.unregister_host_contract(contract_id);
    assert!(
        removed,
        "unregister should return true for existing contract"
    );

    // Verify it's gone
    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);
    assert!(
        result.is_none(),
        "lookup after unregister should return None"
    );
}

#[test]
fn unregister_nonexistent_contract_returns_false() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let nonexistent_id: u64 = 0xEEEE_EEEE_EEEE_EEEE;
    let removed: bool = runtime.unregister_host_contract(nonexistent_id);

    assert!(
        !removed,
        "unregister should return false for nonexistent contract"
    );
}

// ─── Version Checking Tests ───────────────────────────────────────────────────

#[test]
fn get_host_contract_version_check_exact_match() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x1111_2222_3333_4444;
    // Register with version 1.5 (major=1, minor=5)
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 5, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Request exact version 1.5 (encoded as (1 << 16) | 5 = 65541)
    let min_version: u32 = (1 << 16) | 5;
    let result: Option<&'static HostContractVTable> =
        runtime.get_host_contract(contract_id, min_version);

    assert!(
        result.is_some(),
        "should find contract with exact version match"
    );
}

#[test]
fn get_host_contract_version_check_lower_minor_succeeds() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x2222_3333_4444_5555;
    // Register with version 2.10 (major=2, minor=10)
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 2, 10, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Request version 2.5 (lower minor) - should succeed
    let min_version: u32 = (2 << 16) | 5;
    let result: Option<&'static HostContractVTable> =
        runtime.get_host_contract(contract_id, min_version);

    assert!(
        result.is_some(),
        "should find contract when requesting lower minor version"
    );
}

#[test]
fn get_host_contract_version_check_higher_minor_fails() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x3333_4444_5555_6666;
    // Register with version 1.3 (major=1, minor=3)
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 3, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Request version 1.5 (higher minor) - should fail
    let min_version: u32 = (1 << 16) | 5;
    let result: Option<&'static HostContractVTable> =
        runtime.get_host_contract(contract_id, min_version);

    assert!(
        result.is_none(),
        "should NOT find contract when requesting higher minor version"
    );
}

#[test]
fn get_host_contract_version_check_higher_major_fails() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x4444_5555_6666_7777;
    // Register with version 1.0 (major=1, minor=0)
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Request version 2.0 (higher major) - should fail
    let min_version: u32 = 2 << 16;
    let result: Option<&'static HostContractVTable> =
        runtime.get_host_contract(contract_id, min_version);

    assert!(
        result.is_none(),
        "should NOT find contract when requesting higher major version"
    );
}

#[test]
fn get_host_contract_version_check_zero_succeeds() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x5555_6666_7777_8888;
    // Register with any version
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 3, 7, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // Request version 0 - should always succeed
    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);

    assert!(
        result.is_some(),
        "should find contract when requesting version 0"
    );
}

// ─── Thread Safety Tests ──────────────────────────────────────────────────────

#[test]
fn concurrent_register_and_lookup() {
    use std::sync::Arc;
    use std::thread;

    let runtime: Arc<Runtime> = Arc::new(
        Runtime::builder()
            .build()
            .expect("runtime build should succeed"),
    );

    let contract_id: u64 = 0x6666_7777_8888_9999;
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);

    // Register before spawning threads
    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    let runtime_clone1: Arc<Runtime> = Arc::clone(&runtime);
    let runtime_clone2: Arc<Runtime> = Arc::clone(&runtime);

    // Thread 1: Repeatedly lookup the contract
    let handle1: thread::JoinHandle<bool> = thread::spawn(move || {
        for _ in 0..100 {
            let result: Option<&'static HostContractVTable> =
                runtime_clone1.get_host_contract(contract_id, 0);
            if result.is_none() {
                return false;
            }
        }
        true
    });

    // Thread 2: Also lookup the contract
    let handle2: thread::JoinHandle<bool> = thread::spawn(move || {
        for _ in 0..100 {
            let result: Option<&'static HostContractVTable> =
                runtime_clone2.get_host_contract(contract_id, 0);
            if result.is_none() {
                return false;
            }
        }
        true
    });

    let success1: bool = handle1.join().expect("thread1 should not panic");
    let success2: bool = handle2.join().expect("thread2 should not panic");

    assert!(success1, "thread1 lookups should all succeed");
    assert!(success2, "thread2 lookups should all succeed");
}

#[test]
fn concurrent_lookups_multiple_contracts() {
    use std::sync::Arc;
    use std::thread;

    let runtime: Arc<Runtime> = Arc::new(
        Runtime::builder()
            .build()
            .expect("runtime build should succeed"),
    );

    // Register multiple contracts
    let contract_ids: [u64; 5] = [
        0x1000_0000_0000_0001,
        0x1000_0000_0000_0002,
        0x1000_0000_0000_0003,
        0x1000_0000_0000_0004,
        0x1000_0000_0000_0005,
    ];

    for id in &contract_ids {
        let vtable: &'static HostContractVTable = create_static_vtable(*id, 1, 0, 1);
        runtime
            .register_host_contract(*id, vtable)
            .expect("registration should succeed");
    }

    let mut handles: Vec<thread::JoinHandle<bool>> = Vec::new();

    // Spawn 10 threads, each doing lookups on all contracts
    for thread_idx in 0..10 {
        let runtime_clone: Arc<Runtime> = Arc::clone(&runtime);
        let ids: [u64; 5] = contract_ids;

        let handle: thread::JoinHandle<bool> = thread::spawn(move || {
            for _ in 0..50 {
                for id in &ids {
                    let result: Option<&'static HostContractVTable> =
                        runtime_clone.get_host_contract(*id, 0);
                    if result.is_none() {
                        return false;
                    }
                }
            }
            // Each thread verifies it ran
            assert!(thread_idx < 10);
            true
        });

        handles.push(handle);
    }

    for (idx, handle) in handles.into_iter().enumerate() {
        let success: bool = handle.join().expect("thread should not panic");
        assert!(success, "thread {} lookups should all succeed", idx);
    }
}

#[test]
fn concurrent_register_different_contracts() {
    use std::sync::Arc;
    use std::thread;

    let runtime: Arc<Runtime> = Arc::new(
        Runtime::builder()
            .build()
            .expect("runtime build should succeed"),
    );

    let contract_ids: [u64; 10] = [
        0x2000_0000_0000_0001,
        0x2000_0000_0000_0002,
        0x2000_0000_0000_0003,
        0x2000_0000_0000_0004,
        0x2000_0000_0000_0005,
        0x2000_0000_0000_0006,
        0x2000_0000_0000_0007,
        0x2000_0000_0000_0008,
        0x2000_0000_0000_0009,
        0x2000_0000_0000_000A,
    ];

    let mut handles: Vec<thread::JoinHandle<Result<(), polyplug::error::HostContractError>>> =
        Vec::new();

    // Spawn 10 threads, each registering a different contract
    for (idx, &id) in contract_ids.iter().enumerate() {
        let runtime_clone: Arc<Runtime> = Arc::clone(&runtime);
        let vtable: &'static HostContractVTable = create_static_vtable(id, 1, 0, 1);

        let handle: thread::JoinHandle<Result<(), polyplug::error::HostContractError>> =
            thread::spawn(move || {
                // Small delay to increase race probability
                std::thread::sleep(core::time::Duration::from_millis(idx as u64));
                runtime_clone.register_host_contract(id, vtable)
            });

        handles.push(handle);
    }

    // All registrations should succeed (different contract IDs)
    for (idx, handle) in handles.into_iter().enumerate() {
        let result: Result<(), polyplug::error::HostContractError> =
            handle.join().expect("thread should not panic");
        assert!(
            result.is_ok(),
            "registration in thread {} should succeed: {result:?}",
            idx
        );
    }

    // Verify all contracts are registered
    for id in &contract_ids {
        let result: Option<&'static HostContractVTable> = runtime.get_host_contract(*id, 0);
        assert!(
            result.is_some(),
            "contract 0x{id:016X} should be registered"
        );
    }
}

// ─── Edge Cases ───────────────────────────────────────────────────────────────

#[test]
fn get_host_contract_with_contract_id_helper() {
    use polyplug_abi::host_contract_id;

    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    // Use the helper function to compute contract ID
    let contract_name: &str = "test.logger";
    let major: u32 = 1;
    let contract_id: u64 = host_contract_id(contract_name, major);

    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, major, 0, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);

    assert!(
        result.is_some(),
        "should find contract registered with helper"
    );
}

#[test]
fn register_unregister_register_same_contract() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0x9999_AAAA_BBBB_CCCC;
    let vtable1: &'static HostContractVTable = create_static_vtable(contract_id, 1, 0, 1);
    let vtable2: &'static HostContractVTable = create_static_vtable(contract_id, 2, 0, 2);

    // Register v1
    assert!(runtime.register_host_contract(contract_id, vtable1).is_ok());
    assert!(runtime.get_host_contract(contract_id, 0).is_some());

    // Unregister
    assert!(runtime.unregister_host_contract(contract_id));
    assert!(runtime.get_host_contract(contract_id, 0).is_none());

    // Register v2
    assert!(runtime.register_host_contract(contract_id, vtable2).is_ok());

    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);
    assert!(result.is_some(), "should find re-registered contract");

    let returned_vtable: &HostContractVTable = result.expect("vtable should be Some");
    assert_eq!(
        returned_vtable.header.contract_major, 2,
        "should return v2 after re-registration"
    );
}

#[test]
fn get_host_contract_min_version_zero_matches_all() {
    let runtime: Runtime = Runtime::builder()
        .build()
        .expect("runtime build should succeed");

    let contract_id: u64 = 0xAAAA_BBBB_CCCC_DDDD;
    // Register with version 0.1 (very low version)
    let vtable: &'static HostContractVTable = create_static_vtable(contract_id, 0, 1, 1);

    runtime
        .register_host_contract(contract_id, vtable)
        .expect("registration should succeed");

    // min_version=0 should match any version
    let result: Option<&'static HostContractVTable> = runtime.get_host_contract(contract_id, 0);
    assert!(result.is_some(), "min_version=0 should match any version");
}
