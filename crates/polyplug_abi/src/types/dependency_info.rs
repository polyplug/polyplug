//! Dependency info for introspection API.
//!
//! Mirrors the [[dependency]] table structure from manifest.toml.

use polyplug_utils::{BundleId, GuestContractId};

/// Dependency information returned by get_dependencies introspection API.
///
/// Mirrors manifest.toml [[dependency]] structure for plugins to query
/// their own declared dependencies at runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct DependencyInfo {
    /// Contract ID of the dependency.
    pub contract_id: GuestContractId,
    /// Minimum version required.
    pub min_version: u32,
    /// Bundle ID if dependency is ByBundle, 0 if ByContract.
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
    use core::mem::{align_of, offset_of, size_of};
    use super::DependencyInfo;
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