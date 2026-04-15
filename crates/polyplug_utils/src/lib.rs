//! Utility functions for polyplug.
//!
//! This crate provides common utility functions used across the polyplug ecosystem,
//! with zero external dependencies (only std).

pub mod bundle_id;
pub mod guest_contract_id;
pub mod host_contract_id;

pub use bundle_id::BundleId;
pub use guest_contract_id::GuestContractId;
pub use host_contract_id::HostContractId;

// ─── Public ID Computation Functions ─────────────────────────────────────────

/// Compute a bundle ID from its name using FNV-1a 64-bit hash.
///
/// Convenience function that wraps `BundleId::new(name).id()`.
pub fn bundle_id(name: &str) -> u64 {
    BundleId::new(name).id()
}

/// Compute a guest contract ID from name and major version.
///
/// Convenience function that wraps `GuestContractId::new(name, major_version).id()`.
pub fn guest_contract_id(name: &str, major_version: u32) -> u64 {
    GuestContractId::new(name, major_version).id()
}

/// Compute a host contract ID from name and major version.
///
/// Convenience function that wraps `HostContractId::new(name, major_version).id()`.
pub fn host_contract_id(name: &str, major_version: u32) -> u64 {
    HostContractId::new(name, major_version).id()
}

// ─── FNV-1a 64-bit Hash ───────────────────────────────────────────────────────

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x00000100000001B3;

/// Compute FNV-1a 64-bit hash of the input bytes.
///
/// FNV-1a is a non-cryptographic hash function with excellent distribution
/// and avalanche properties, suitable for hash tables and checksums.
fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Compute the contract ID for `"name@major_version"` using FNV-1a 64-bit.
///
/// The contract ID is a stable identifier for a contract interface.
/// Same name and major version always produces the same ID.
fn contract_id(prefix: &str, name: &str, major_version: u32) -> u64 {
    let canonical: String = format!("{}{}@{}", prefix, name, major_version);
    fnv1a_64(canonical.as_bytes())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::{FNV_OFFSET, FNV_PRIME, GuestContractId, HostContractId};

    use super::{contract_id, fnv1a_64};

    #[test]
    fn fnv1a_known_values() {
        // Known FNV-1a 64-bit value for empty string (FNV offset basis)
        assert_eq!(fnv1a_64(b""), 0xcbf29ce484222325);
        // Golden value: FNV-1a of "image.decode@1"
        assert_eq!(fnv1a_64(b"image.decode@1"), 0xa1ba05dd7da18569_u64);
        // Verify determinism
        assert_eq!(fnv1a_64(b"image.decode@1"), fnv1a_64(b"image.decode@1"));
        // Different inputs produce different hashes
        assert_ne!(fnv1a_64(b"image.decode@1"), fnv1a_64(b"image.decode@2"));
    }

    #[test]
    fn contract_id_canonical_format() {
        // Same name+major always produces same ID
        let id1: u64 = contract_id("", "image.decode", 1);
        let id2: u64 = contract_id("", "image.decode", 1);
        assert_eq!(id1, id2);
        // Different major versions produce different IDs
        let id3: u64 = contract_id("", "image.decode", 2);
        assert_ne!(id1, id3);
        // Different names produce different IDs
        let id4: u64 = contract_id("", "audio.decode", 1);
        assert_ne!(id1, id4);
    }

    #[test]
    fn contract_id_golden_values() {
        // Golden: FNV-1a of "image.decode@1"
        assert_eq!(contract_id("", "image.decode", 1), 0xA1BA05DD7DA18569_u64);
        // Golden: FNV-1a of "audio.encode@2"
        assert_eq!(contract_id("", "audio.encode", 2), 0x7A7958404B1D72A5_u64);
    }

    #[test]
    fn contract_id_collision() {
        // Host and guest contract IDs must never collide for same name+major
        assert_ne!(
            HostContractId::new("logger", 1).id(),
            GuestContractId::new("logger", 1).id(),
            "host and guest contract IDs must differ"
        );
    }

    #[test]
    fn fnv1a_empty_input() {
        // Empty input returns offset basis
        assert_eq!(fnv1a_64(b""), FNV_OFFSET);
    }

    #[test]
    fn fnv1a_single_byte() {
        // Single byte: hash = (offset ^ byte) * prime
        let expected: u64 = FNV_OFFSET.wrapping_mul(FNV_PRIME);
        assert_eq!(fnv1a_64(b"\x00"), expected);
    }

    #[test]
    fn contract_id_matches_fnv1a_directly() {
        // contract_id("image.decode", 1) should equal fnv1a_64(b"image.decode@1")
        assert_eq!(
            contract_id("", "image.decode", 1),
            fnv1a_64(b"image.decode@1")
        );
    }
}
