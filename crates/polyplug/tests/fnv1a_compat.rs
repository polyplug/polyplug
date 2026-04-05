#![allow(clippy::expect_used)]

//! FNV-1a cross-language compatibility test.
//!
//! Verifies that the Rust `guest_contract_id` function produces the expected values
//! that were independently computed and are hard-coded in both the C++ header
//! (`polyplug::fnv1a_contract_id`) and the test plugin
//! (`TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2B`).
//!
//! These constants are the ABI freeze verification: if anyone changes the
//! FNV-1a algorithm or the canonical format, these assertions will catch it.

use polyplug_utils::guest_contract_id;

/// Known-good FNV-1a("test.add@1") — matches C++ constexpr and test_plugin constant.
const EXPECTED_TEST_ADD: u64 = 0xCC4232FAB0410D2B;

/// Known-good FNV-1a("image.decode@1") — secondary cross-check.
const EXPECTED_IMAGE_DECODE: u64 = 0xA1BA05DD7DA18569;

#[test]
fn fnv1a_contract_id_test_add() {
    let id: u64 = guest_contract_id("test.add", 1);
    assert_eq!(
        id, EXPECTED_TEST_ADD,
        "guest_contract_id(\"test.add\", 1) must equal 0x{:016X} (FNV-1a of \"test.add@1\")",
        EXPECTED_TEST_ADD,
    );
}

#[test]
fn fnv1a_contract_id_image_decode() {
    let id: u64 = guest_contract_id("image.decode", 1);
    assert_eq!(
        id, EXPECTED_IMAGE_DECODE,
        "guest_contract_id(\"image.decode\", 1) must equal 0x{:016X} (FNV-1a of \"image.decode@1\")",
        EXPECTED_IMAGE_DECODE,
    );
}

#[test]
fn fnv1a_algorithm_stability() {
    // Verify major version changes produce different IDs (algorithm soundness)
    let v1: u64 = guest_contract_id("test.add", 1);
    let v2: u64 = guest_contract_id("test.add", 2);
    assert_ne!(
        v1, v2,
        "different major versions must produce different contract IDs"
    );

    // Verify name changes produce different IDs
    let id_a: u64 = guest_contract_id("test.add", 1);
    let id_b: u64 = guest_contract_id("test.sub", 1);
    assert_ne!(
        id_a, id_b,
        "different names must produce different contract IDs"
    );
}