#![allow(clippy::expect_used)]

//! Regression tests for review findings reachable through the public
//! `RuntimeStore` surface and the `find_all_guest_contracts` HostApi callback.
//!
//! Findings that exercise the `pub(crate)` reload primitives (`begin_reload`,
//! `apply_reload_swap`, `abort_reload`) live as unit tests inside
//! `runtime_store.rs` because those methods are crate-private; see the tests
//! `pending_reload_slot_not_returned_by_find_by_bundle` and
//! `apply_reload_swap_bumps_consumed_new_slot_generation` there.
//!
//! Covered here (public surface):
//! 1. find_all alloc/free layout: the returned `Array.len` must equal the live
//!    provider count under a single registry guard (no shrink-between-locks UB),
//!    and `host->free` with `len * sizeof(T)` must round-trip.
//! 3. `DuplicateProvider` enforced for same-bundle/same-contract; different
//!    bundles registering the same contract stays allowed.
//! 4. `min_version` is a MAJOR-version floor (doc-rot pin).
//! 5. `get_guest_contract_descriptor` honours `handle.generation`.

use std::sync::Arc;

use core::ffi::c_void;
use core::mem::size_of;
use core::ptr::{self, null};

use polyplug::Runtime;
use polyplug::error::RegistryError;
use polyplug::ffi::{polyplug_runtime_create, polyplug_runtime_destroy};
use polyplug::runtime_store::RuntimeStore;
use polyplug_abi::dispatch::VmLoaderData;
use polyplug_abi::runtime::RuntimeConfig;
use polyplug_abi::{
    Array, DispatchMechanisms, DispatchType, GuestContractHandle, GuestContractInstance,
    GuestContractInterface, HostApi, NativeDispatch, PluginDescriptor, StringView, Version,
};
use polyplug_utils::BundleId;
use polyplug_utils::GuestContractId;

const MOCK_FUNCTIONS: [*const (); 0] = [];

/// No-op create_instance callback.
unsafe extern "C" fn noop_create_instance(
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

/// No-op destroy_instance callback.
unsafe extern "C" fn noop_destroy_instance(
    _adapter_context: *mut c_void,
    _loader_data: VmLoaderData,
    _host: *const HostApi,
    _instance: GuestContractInstance,
) {
}

fn make_interface(contract_id: u64, major: u32) -> GuestContractInterface {
    GuestContractInterface {
        contract_id: GuestContractId::from_u64(contract_id),
        contract_version: Version {
            major,
            minor: 0,
            patch: 0,
        },
        dispatch_type: DispatchType::Native,
        adapter_context: ptr::null_mut(),
        create_instance: noop_create_instance,
        destroy_instance: noop_destroy_instance,
        dispatch: DispatchMechanisms {
            native: NativeDispatch {
                function_count: 0,
                functions: MOCK_FUNCTIONS.as_ptr(),
            },
        },
    }
}

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

// =============================================================================
// Finding 1 — collect_guest_contracts counts AND collects under ONE guard.
// =============================================================================

/// Finding 1 (unit): collect_guest_contracts filters by min_version, skips
/// vacancies left by an unload, and returns exactly the live providers.
#[test]
fn collect_guest_contracts_filters_versions_and_vacancies() {
    const CID: u64 = 0x1234_0000_0000_0001_u64;
    let registry: RuntimeStore = RuntimeStore::new();
    let contract_id: GuestContractId = GuestContractId::from_u64(CID);

    let iface_v1: GuestContractInterface = make_interface(CID, 1);
    let iface_v2: GuestContractInterface = make_interface(CID, 2);
    let iface_v3: GuestContractInterface = make_interface(CID, 3);

    let bundle_a: BundleId = BundleId::from_u64(0xA1);
    let bundle_b: BundleId = BundleId::from_u64(0xB2);
    let bundle_c: BundleId = BundleId::from_u64(0xC3);

    // SAFETY: interfaces are local values valid for this test's lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("a", "multi.contract"),
                &iface_v1,
                "multi.contract".to_owned(),
                bundle_a,
            )
            .expect("register a");
        registry
            .register_guest_contract(
                make_descriptor("b", "multi.contract"),
                &iface_v2,
                "multi.contract".to_owned(),
                bundle_b,
            )
            .expect("register b");
        registry
            .register_guest_contract(
                make_descriptor("c", "multi.contract"),
                &iface_v3,
                "multi.contract".to_owned(),
                bundle_c,
            )
            .expect("register c");
    }

    // All three providers at min_version=0.
    let all: Vec<GuestContractHandle> = registry.collect_guest_contracts(contract_id, 0);
    assert_eq!(all.len(), 3, "three providers at min_version=0");

    // min_version filters by MAJOR: only v2 and v3 satisfy >= 2.
    let ge2: Vec<GuestContractHandle> = registry.collect_guest_contracts(contract_id, 2);
    assert_eq!(ge2.len(), 2, "two providers at min_version=2");

    // Unload bundle_b — its slot becomes a vacancy; collect must skip it.
    registry
        .invalidate_bundle(bundle_b)
        .expect("invalidate bundle_b");
    let after_unload: Vec<GuestContractHandle> = registry.collect_guest_contracts(contract_id, 0);
    assert_eq!(
        after_unload.len(),
        2,
        "two live providers after one unload (vacancy skipped)"
    );

    // No matches → empty vec (no allocation contract for callers).
    let none: Vec<GuestContractHandle> =
        registry.collect_guest_contracts(GuestContractId::from_u64(0xDEAD), 0);
    assert!(none.is_empty(), "unknown contract collects nothing");
}

/// Finding 1 (regression, end-to-end): register N providers, unload one bundle,
/// then call the HostApi `find_all_guest_contracts` callback. The returned
/// `Array.len` must equal the number of LIVE providers (not a stale pre-count),
/// and freeing `len * sizeof(T)` via `host->free` must round-trip cleanly.
#[test]
fn host_find_all_array_len_matches_live_providers_and_frees() {
    const CID: u64 = 0x9999_0000_0000_0001_u64;

    // SAFETY: null config is accepted (default runtime config).
    let host: *const HostApi = unsafe { polyplug_runtime_create(null::<RuntimeConfig>()) };
    assert!(!host.is_null(), "runtime create must yield a host");

    // SAFETY: host is non-null and its runtime field points to a live Runtime.
    let runtime: &Runtime = unsafe { &*((*host).runtime as *const Runtime) };
    let registry: &Arc<RuntimeStore> = runtime.registry();

    let iface_a: GuestContractInterface = make_interface(CID, 1);
    let iface_b: GuestContractInterface = make_interface(CID, 1);
    let iface_c: GuestContractInterface = make_interface(CID, 1);

    let bundle_a: BundleId = BundleId::from_u64(0x501);
    let bundle_b: BundleId = BundleId::from_u64(0x502);
    let bundle_c: BundleId = BundleId::from_u64(0x503);

    // SAFETY: interfaces are local values valid for this test's lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("a", "find.all"),
                &iface_a,
                "find.all".to_owned(),
                bundle_a,
            )
            .expect("register a");
        registry
            .register_guest_contract(
                make_descriptor("b", "find.all"),
                &iface_b,
                "find.all".to_owned(),
                bundle_b,
            )
            .expect("register b");
        registry
            .register_guest_contract(
                make_descriptor("c", "find.all"),
                &iface_c,
                "find.all".to_owned(),
                bundle_c,
            )
            .expect("register c");
    }

    // Unload one bundle so the live count is 2.
    registry
        .invalidate_bundle(bundle_b)
        .expect("invalidate bundle_b");

    // Invoke the HostApi callback exactly as a guest/host would.
    // SAFETY: host is a valid HostApi pointer from polyplug_runtime_create.
    let array: Array<GuestContractHandle> =
        unsafe { ((*host).find_all_guest_contracts)(host, CID, 0) };

    assert_eq!(
        array.len, 2,
        "Array.len must equal the live provider count (2), not a stale pre-count"
    );
    assert!(
        !array.items.is_null(),
        "non-empty array must carry a buffer"
    );

    // Free using the Array contract: len * sizeof(T) with align — must round-trip.
    let size: usize = array.len * size_of::<GuestContractHandle>();
    // SAFETY: items was allocated by host->alloc with size == len * sizeof(T) and
    // matching alignment; freeing with the same size/align is the documented contract.
    unsafe {
        ((*host).free)(host, array.items as *mut u8, size, array.align);
    }

    // SAFETY: host was produced by polyplug_runtime_create and is destroyed once.
    assert!(unsafe { polyplug_runtime_destroy(host) });
}

// =============================================================================
// Finding 3 — DuplicateProvider enforced (same bundle); multi-impl still OK.
// =============================================================================

/// Finding 3: same bundle registering the same contract twice → DuplicateProvider.
#[test]
fn same_bundle_same_contract_twice_is_duplicate_provider() {
    const CID: u64 = 0x4444_0000_0000_0001_u64;
    let registry: RuntimeStore = RuntimeStore::new();
    let bundle_id: BundleId = BundleId::from_u64(0x1010);

    let iface: GuestContractInterface = make_interface(CID, 1);
    // SAFETY: local value valid for this test's lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("first", "dup.contract"),
                &iface,
                "dup.contract".to_owned(),
                bundle_id,
            )
            .expect("first register succeeds");
    }

    // SAFETY: local value valid for this test's lifetime.
    let result: Result<GuestContractHandle, RegistryError> = unsafe {
        registry.register_guest_contract(
            make_descriptor("second", "dup.contract"),
            &iface,
            "dup.contract".to_owned(),
            bundle_id,
        )
    };
    assert!(
        matches!(result, Err(RegistryError::DuplicateProvider { .. })),
        "same bundle + same contract must be DuplicateProvider, got {result:?}"
    );
}

/// Finding 3: different bundles registering the same contract → allowed (multi-impl).
#[test]
fn different_bundles_same_contract_allowed() {
    const CID: u64 = 0x4444_0000_0000_0002_u64;
    let registry: RuntimeStore = RuntimeStore::new();

    let iface_a: GuestContractInterface = make_interface(CID, 1);
    let iface_b: GuestContractInterface = make_interface(CID, 1);

    // SAFETY: local values valid for this test's lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("a", "multi.ok"),
                &iface_a,
                "multi.ok".to_owned(),
                BundleId::from_u64(0x2020),
            )
            .expect("bundle a register");
        registry
            .register_guest_contract(
                make_descriptor("b", "multi.ok"),
                &iface_b,
                "multi.ok".to_owned(),
                BundleId::from_u64(0x3030),
            )
            .expect("bundle b register (different bundle, same contract is allowed)");
    }
}

// =============================================================================
// Finding 4 — min_version is a MAJOR-version floor (doc-rot pin).
// =============================================================================

/// Finding 4: `find`/`find_all` compare against the interface's MAJOR version.
/// A provider with major=2 satisfies min_version 0,1,2 but not 3.
#[test]
fn min_version_is_major_floor() {
    const CID: u64 = 0x5555_0000_0000_0001_u64;
    let registry: RuntimeStore = RuntimeStore::new();
    let contract_id: GuestContractId = GuestContractId::from_u64(CID);

    let iface: GuestContractInterface = make_interface(CID, 2);
    // SAFETY: local value valid for this test's lifetime.
    unsafe {
        registry
            .register_guest_contract(
                make_descriptor("p", "major.floor"),
                &iface,
                "major.floor".to_owned(),
                BundleId::from_u64(0x5050),
            )
            .expect("register");
    }

    assert!(
        registry.find(contract_id, 0).is_ok(),
        "major=2 satisfies min_version=0"
    );
    assert!(
        registry.find(contract_id, 2).is_ok(),
        "major=2 satisfies min_version=2 (floor is inclusive)"
    );
    assert!(
        registry.find(contract_id, 3).is_err(),
        "major=2 does NOT satisfy min_version=3"
    );
    assert_eq!(
        registry.collect_guest_contracts(contract_id, 3).len(),
        0,
        "collect honours the same MAJOR floor"
    );
}

// =============================================================================
// Finding 5 — get_guest_contract_descriptor honours handle.generation.
// =============================================================================

/// Finding 5: a stale handle (whose slot was vacated and reused) must NOT return
/// the new occupant's descriptor.
#[test]
fn descriptor_honours_handle_generation() {
    const CID: u64 = 0x6666_0000_0000_0001_u64;
    let registry: RuntimeStore = RuntimeStore::new();
    let bundle_a: BundleId = BundleId::from_u64(0x6060);

    let iface_a: GuestContractInterface = make_interface(CID, 1);
    // SAFETY: local value valid for this test's lifetime.
    let stale_handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                make_descriptor("original", "gen.contract"),
                &iface_a,
                "gen.contract".to_owned(),
                bundle_a,
            )
            .expect("register original")
    };

    // Unload bundle_a — the slot is vacated (generation bumped, entry cleared).
    registry
        .invalidate_bundle(bundle_a)
        .expect("invalidate bundle_a");

    // Reuse the recycled slot with a NEW occupant.
    let bundle_b: BundleId = BundleId::from_u64(0x6061);
    let iface_b: GuestContractInterface = make_interface(0x6666_0000_0000_0002_u64, 1);
    // SAFETY: local value valid for this test's lifetime.
    let new_handle: GuestContractHandle = unsafe {
        registry
            .register_guest_contract(
                make_descriptor("replacement", "gen.contract.new"),
                &iface_b,
                "gen.contract.new".to_owned(),
                bundle_b,
            )
            .expect("register replacement")
    };
    assert_eq!(
        new_handle.index, stale_handle.index,
        "the recycled slot index must be reused"
    );

    // The stale handle must NOT resolve to the new occupant's descriptor.
    let descriptor = registry.get_guest_contract_descriptor(stale_handle);
    assert!(
        descriptor.is_none(),
        "stale handle must not return the new occupant's descriptor, got {descriptor:?}"
    );

    // The current handle still works.
    let current = registry.get_guest_contract_descriptor(new_handle);
    assert!(
        current.is_some(),
        "the current handle must return its descriptor"
    );
}
