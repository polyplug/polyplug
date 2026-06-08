//! Reload Phase — FFI-safe representation of reload phases.

use crate::types::StringView;
use polyplug_utils::BundleId;

/// Type of reload phase for FFI callbacks.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadPhaseType {
    /// Bundle is being prepared for reload.
    Preparing = 0,
    /// Bundle has been successfully reloaded.
    Reloaded = 1,
    /// Bundle reload failed.
    Failed = 2,
    /// Bundle is being unloaded.
    Unloading = 3,
}

/// FFI-safe reload phase for hot-reload callbacks.
///
/// Tagged union style struct — `phase_type` indicates which variant is active.
/// Uses `StringView` for FFI compatibility (non-owning borrows).
///
/// # Lifetime
/// `StringView` fields are borrowed from the caller's strings.
/// The callback must not store these views beyond the callback scope.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReloadPhase {
    /// Type of reload phase.
    pub phase_type: ReloadPhaseType,
    /// Bundle being reloaded.
    pub bundle_id: BundleId,
    /// Bundle name (borrowed string).
    pub bundle_name: StringView,
    /// Failure reason (only for Failed phase).
    pub reason: StringView,
}

impl ReloadPhase {
    /// Create Preparing phase.
    pub fn preparing(bundle_id: BundleId, bundle_name: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Preparing,
            bundle_id,
            bundle_name,
            reason: StringView::null(),
        }
    }

    /// Create Reloaded phase.
    pub fn reloaded(bundle_id: BundleId, bundle_name: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Reloaded,
            bundle_id,
            bundle_name,
            reason: StringView::null(),
        }
    }

    /// Create Failed phase.
    pub fn failed(bundle_id: BundleId, bundle_name: StringView, reason: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Failed,
            bundle_id,
            bundle_name,
            reason,
        }
    }

    /// Create Unloading phase.
    pub fn unloading(bundle_id: BundleId, bundle_name: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Unloading,
            bundle_id,
            bundle_name,
            reason: StringView::null(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReloadPhase, ReloadPhaseType};
    use crate::types::StringView;
    use core::mem::{align_of, size_of};
    use polyplug_utils::BundleId;

    #[test]
    fn layout_reload_phase() {
        // Fields: phase_type(4) + padding(4) + bundle_id(8) + bundle_name(16) + reason(16)
        // Layout with alignment=8:
        //   phase_type: 0x00 (4 bytes)
        //   padding: 0x04-0x07 (4 bytes)
        //   bundle_id: 0x08 (8 bytes)
        //   bundle_name.ptr: 0x10 (8 bytes)
        //   bundle_name.len: 0x18 (8 bytes)
        //   reason.ptr: 0x20 (8 bytes)
        //   reason.len: 0x28 (8 bytes)
        // Total: 48 bytes
        assert_eq!(size_of::<ReloadPhase>(), 48);
        assert_eq!(align_of::<ReloadPhase>(), 8);
    }

    #[test]
    fn reload_phase_type_values() {
        assert_eq!(ReloadPhaseType::Preparing as u32, 0);
        assert_eq!(ReloadPhaseType::Reloaded as u32, 1);
        assert_eq!(ReloadPhaseType::Failed as u32, 2);
        assert_eq!(ReloadPhaseType::Unloading as u32, 3);
    }

    #[test]
    fn preparing_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let phase = ReloadPhase::preparing(bundle_id, bundle_name);

        assert_eq!(phase.phase_type, ReloadPhaseType::Preparing);
        assert_eq!(phase.bundle_id, bundle_id);
        assert!(phase.reason.ptr.is_null());
        assert_eq!(phase.reason.len, 0);
    }

    #[test]
    fn reloaded_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let phase = ReloadPhase::reloaded(bundle_id, bundle_name);

        assert_eq!(phase.phase_type, ReloadPhaseType::Reloaded);
        assert_eq!(phase.bundle_id, bundle_id);
        assert!(phase.reason.ptr.is_null());
        assert_eq!(phase.reason.len, 0);
    }

    #[test]
    fn unloading_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let phase = ReloadPhase::unloading(bundle_id, bundle_name);

        assert_eq!(phase.phase_type, ReloadPhaseType::Unloading);
        assert_eq!(phase.bundle_id, bundle_id);
        assert!(phase.reason.ptr.is_null());
        assert_eq!(phase.reason.len, 0);
    }

    #[test]
    fn failed_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let reason = StringView::from_static(b"init failed");
        let phase = ReloadPhase::failed(bundle_id, bundle_name, reason);

        assert_eq!(phase.phase_type, ReloadPhaseType::Failed);
        assert_eq!(phase.bundle_id, bundle_id);
        assert!(!reason.ptr.is_null());
        assert_eq!(phase.reason.len, 11);
    }
}
