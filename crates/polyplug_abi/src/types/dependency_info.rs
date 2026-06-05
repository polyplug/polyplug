//! Dependency info for introspection API.
//!
//! This module defines `DependencyInfo`, the type returned by `get_dependencies`
//! for plugins to query their own declared dependencies at runtime.
//!
//! # Who provides
//! Runtime returns this from `HostApi::get_dependencies`.
//!
//! # Who calls
//! Guest (plugin) code calls `get_dependencies` during initialization.
//!
//! # Ownership
//! Caller owns the returned Array and must free via `host->free`.
//!
//! # Manifest Relationship
//! Mirrors the `[[dependency]]` table structure from `manifest.toml`.

use polyplug_utils::{BundleId, GuestContractId};

/// Dependency information returned by get_dependencies introspection API.
///
/// Mirrors `manifest.toml` `\[dependency\]` table structure for plugins to query
/// their own declared dependencies at runtime.
///
/// # Who provides
/// Runtime returns this from `HostApi::get_dependencies`.
///
/// # Who calls
/// Guest (plugin) code calls `get_dependencies` during initialization
/// to discover available dependencies.
///
/// # Ownership
/// Returned in an Array that caller owns and must free via `host->free`.
///
/// # Fields
/// - `contract_id`: The contract being depended upon
/// - `min_version`: Minimum version required
/// - `bundle_id`: Specific bundle if ByBundle, 0 if ByContract
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DependencyInfo {
    /// Contract ID of the dependency.
    ///
    /// FNV-1a hash of the contract name and major version.
    /// Use this to find matching contracts via `find_guest_contract`.
    pub contract_id: GuestContractId,
    /// Minimum version required.
    ///
    /// Used for version compatibility checks during contract lookup.
    /// Contracts with version >= min_version will match.
    pub min_version: u32,
    /// Bundle ID if dependency is ByBundle, 0 if ByContract.
    ///
    /// # ByBundle vs ByContract
    /// - `bundle_id != 0`: Dependency is on a specific bundle
    /// - `bundle_id == 0`: Dependency is on any bundle providing the contract
    pub bundle_id: BundleId,
}

impl DependencyInfo {
    /// Create a dependency info for a ByContract dependency.
    pub fn by_contract(contract_id: GuestContractId, min_version: u32) -> Self {
        Self {
            contract_id,
            min_version,
            bundle_id: BundleId::from_u64(0),
        }
    }

    /// Create a dependency info for a ByBundle dependency.
    pub fn by_bundle(contract_id: GuestContractId, min_version: u32, bundle_id: BundleId) -> Self {
        Self {
            contract_id,
            min_version,
            bundle_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DependencyInfo;
    use core::mem::{align_of, offset_of, size_of};
    use polyplug_utils::{BundleId, GuestContractId};

    #[test]
    fn layout_dependency_info() {
        // DependencyInfo: GuestContractId/u64 (8) + u32 (4) + 4 padding + BundleId/u64 (8) = 24 bytes
        assert_eq!(size_of::<DependencyInfo>(), 24);
        assert_eq!(align_of::<DependencyInfo>(), 8);
        assert_eq!(offset_of!(DependencyInfo, contract_id), 0);
        assert_eq!(offset_of!(DependencyInfo, min_version), 8);
        assert_eq!(offset_of!(DependencyInfo, bundle_id), 16);
    }

    #[test]
    fn by_contract_factory() {
        let dep = DependencyInfo::by_contract(GuestContractId::from_u64(42), 1);
        assert_eq!(dep.contract_id, GuestContractId::from_u64(42));
        assert_eq!(dep.min_version, 1);
        assert_eq!(dep.bundle_id, BundleId::from_u64(0));
    }

    #[test]
    fn by_bundle_factory() {
        let dep = DependencyInfo::by_bundle(
            GuestContractId::from_u64(42),
            1,
            BundleId::new("test-bundle"),
        );
        assert_eq!(dep.contract_id, GuestContractId::from_u64(42));
        assert_eq!(dep.min_version, 1);
        assert_ne!(dep.bundle_id, BundleId::from_u64(0));
    }
}
