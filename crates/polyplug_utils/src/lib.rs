//! Utility functions for polyplug.
//!
//! This crate provides common utility functions used across the polyplug ecosystem,
//! with zero external dependencies (only std).

// ─── FNV-1a 64-bit Hash ───────────────────────────────────────────────────────

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x00000100000001B3;

/// Compute FNV-1a 64-bit hash of the input bytes.
///
/// FNV-1a is a non-cryptographic hash function with excellent distribution
/// and avalanche properties, suitable for hash tables and checksums.
///
/// # Example
///
/// ```
/// use polyplug_utils::fnv1a_64;
///
/// let hash: u64 = fnv1a_64(b"hello world");
/// assert_ne!(hash, 0);
/// ```
pub fn fnv1a_64(data: &[u8]) -> u64 {
    let mut hash: u64 = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ─── Contract ID Functions ────────────────────────────────────────────────────

/// Compute the contract ID for `"name@major_version"` using FNV-1a 64-bit.
///
/// The contract ID is a stable identifier for a plugin contract interface.
/// Same name and major version always produces the same ID.
///
/// # Example
///
/// ```
/// use polyplug_utils::contract_id;
///
/// let id: u64 = contract_id("image.decode", 1);
/// assert_eq!(id, 0xa1ba05dd7da18569_u64);
/// ```
pub fn contract_id(name: &str, major_version: u32) -> u64 {
    let canonical: String = format!("{}@{}", name, major_version);
    fnv1a_64(canonical.as_bytes())
}

/// Calculate host contract ID from name and major version.
///
/// Uses a distinct prefix `"host_contract:"` to avoid collisions with plugin contract IDs.
///
/// # Example
///
/// ```
/// use polyplug_utils::host_contract_id;
///
/// let id: u64 = host_contract_id("logger", 1);
/// assert_ne!(id, 0);
/// ```
pub fn host_contract_id(name: &str, major: u32) -> u64 {
    let input: String = format!("host_contract:{}@{}", name, major);
    fnv1a_64(input.as_bytes())
}

/// Calculate plugin contract ID from name and major version.
///
/// Uses a distinct prefix `"plugin_contract:"` to avoid collisions with host contract IDs.
///
/// # Example
///
/// ```
/// use polyplug_utils::plugin_contract_id;
///
/// let id: u64 = plugin_contract_id("logger", 1);
/// assert_ne!(id, 0);
/// ```
pub fn plugin_contract_id(name: &str, major: u32) -> u64 {
    let input: String = format!("plugin_contract:{}@{}", name, major);
    fnv1a_64(input.as_bytes())
}

/// Compute a bundle ID from its name using FNV-1a 64-bit hash.
///
/// The bundle ID is a stable identifier for a plugin bundle.
/// Same bundle name always produces the same ID.
///
/// # Example
///
/// ```
/// use polyplug_utils::bundle_id;
///
/// let id: u64 = bundle_id("my-bundle");
/// assert_eq!(id, 0xfe6226876e3a35b2_u64);
/// ```
pub fn bundle_id(name: &str) -> u64 {
    fnv1a_64(name.as_bytes())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let id1: u64 = contract_id("image.decode", 1);
        let id2: u64 = contract_id("image.decode", 1);
        assert_eq!(id1, id2);
        // Different major versions produce different IDs
        let id3: u64 = contract_id("image.decode", 2);
        assert_ne!(id1, id3);
        // Different names produce different IDs
        let id4: u64 = contract_id("audio.decode", 1);
        assert_ne!(id1, id4);
    }

    #[test]
    fn contract_id_golden_values() {
        // Golden: FNV-1a of "image.decode@1"
        assert_eq!(contract_id("image.decode", 1), 0xa1ba05dd7da18569_u64);
        // Golden: FNV-1a of "audio.encode@2"
        assert_eq!(contract_id("audio.encode", 2), 0x7a7958404b1d72a5_u64);
    }

    #[test]
    fn contract_id_collision() {
        // Host and plugin contract IDs must never collide for same name+major
        let host_id: u64 = host_contract_id("logger", 1);
        let plugin_id: u64 = plugin_contract_id("logger", 1);
        assert_ne!(
            host_id, plugin_id,
            "host and plugin contract IDs must differ"
        );

        // Both must be deterministic
        assert_eq!(host_contract_id("logger", 1), host_contract_id("logger", 1));
        assert_eq!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("logger", 1)
        );

        // Different names produce different IDs within same category
        assert_ne!(
            host_contract_id("logger", 1),
            host_contract_id("metrics", 1)
        );
        assert_ne!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("metrics", 1)
        );

        // Different major versions produce different IDs within same category
        assert_ne!(host_contract_id("logger", 1), host_contract_id("logger", 2));
        assert_ne!(
            plugin_contract_id("logger", 1),
            plugin_contract_id("logger", 2)
        );
    }

    #[test]
    fn bundle_id_stability() {
        // Same input always yields same output
        assert_eq!(bundle_id("my-bundle"), bundle_id("my-bundle"));
        // Golden: FNV-1a of "my-bundle"
        assert_eq!(bundle_id("my-bundle"), 0xfe6226876e3a35b2_u64);
        // Golden: FNV-1a of "polyplug-core"
        assert_eq!(bundle_id("polyplug-core"), 0x6ef4aee714f5f991_u64);
        // Different bundle names produce different IDs
        assert_ne!(bundle_id("bundle-a"), bundle_id("bundle-b"));
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
        assert_eq!(contract_id("image.decode", 1), fnv1a_64(b"image.decode@1"));
    }

    #[test]
    fn host_contract_id_format() {
        // host_contract_id("logger", 1) should equal fnv1a_64(b"host_contract:logger@1")
        assert_eq!(
            host_contract_id("logger", 1),
            fnv1a_64(b"host_contract:logger@1")
        );
    }

    #[test]
    fn plugin_contract_id_format() {
        // plugin_contract_id("logger", 1) should equal fnv1a_64(b"plugin_contract:logger@1")
        assert_eq!(
            plugin_contract_id("logger", 1),
            fnv1a_64(b"plugin_contract:logger@1")
        );
    }
}
