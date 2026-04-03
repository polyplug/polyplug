//! Reload Phase Data — FFI-safe representation of reload phases.

use polyplug_utils::BundleId;
use crate::types::StringView;

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
}

/// FFI-safe reload phase data for hot-reload callbacks.
///
/// Unlike the Rust `ReloadPhase` enum (which has String fields),
/// this struct uses `StringView` for FFI compatibility.
///
/// # Lifetime
/// `StringView` fields are borrowed from the caller's strings.
/// The callback must not store these views beyond the callback scope.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReloadPhaseData {
    /// Type of reload phase.
    pub phase_type: ReloadPhaseType,
    /// Bundle being reloaded.
    pub bundle_id: BundleId,
    /// Bundle name (borrowed string).
    pub bundle_name: StringView,
    /// Retry count (only for Preparing phase).
    pub retry_count: u32,
    /// Failure reason (only for Failed phase).
    pub reason: StringView,
}

impl ReloadPhaseData {
    /// Create Preparing phase data.
    pub fn preparing(bundle_id: BundleId, bundle_name: StringView, retry_count: u32) -> Self {
        Self {
            phase_type: ReloadPhaseType::Preparing,
            bundle_id,
            bundle_name,
            retry_count,
            reason: StringView::null(),
        }
    }

    /// Create Reloaded phase data.
    pub fn reloaded(bundle_id: BundleId, bundle_name: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Reloaded,
            bundle_id,
            bundle_name,
            retry_count: 0,
            reason: StringView::null(),
        }
    }

    /// Create Failed phase data.
    pub fn failed(bundle_id: BundleId, bundle_name: StringView, reason: StringView) -> Self {
        Self {
            phase_type: ReloadPhaseType::Failed,
            bundle_id,
            bundle_name,
            retry_count: 0,
            reason,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, size_of};
    use super::{ReloadPhaseData, ReloadPhaseType};
    use polyplug_utils::BundleId;
    use crate::types::StringView;

    #[test]
    fn layout_reload_phase_data() {
        // BundleId: 8, StringView: 16 (ptr + len), ReloadPhaseType: 4 (u32)
        // Fields: phase_type(4) + bundle_id(8) + bundle_name(16) + retry_count(4) + reason(16)
        // Layout with alignment=8:
        //   phase_type: 0x00 (4 bytes)
        //   padding: 0x04-0x07 (4 bytes)
        //   bundle_id: 0x08 (8 bytes)
        //   bundle_name.ptr: 0x10 (8 bytes)
        //   bundle_name.len: 0x18 (8 bytes)
        //   retry_count: 0x20 (4 bytes)
        //   padding: 0x24-0x27 (4 bytes)
        //   reason.ptr: 0x28 (8 bytes)
        //   reason.len: 0x30 (8 bytes)
        // Total: 56 bytes
        assert_eq!(size_of::<ReloadPhaseData>(), 56);
        assert_eq!(align_of::<ReloadPhaseData>(), 8);
    }

    #[test]
    fn reload_phase_type_values() {
        assert_eq!(ReloadPhaseType::Preparing as u32, 0);
        assert_eq!(ReloadPhaseType::Reloaded as u32, 1);
        assert_eq!(ReloadPhaseType::Failed as u32, 2);
    }

    #[test]
    fn preparing_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let data = ReloadPhaseData::preparing(bundle_id, bundle_name, 3);

        assert_eq!(data.phase_type, ReloadPhaseType::Preparing);
        assert_eq!(data.bundle_id, bundle_id);
        assert_eq!(data.retry_count, 3);
        assert!(data.reason.ptr.is_null());
        assert_eq!(data.reason.len, 0);
    }

    #[test]
    fn reloaded_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let data = ReloadPhaseData::reloaded(bundle_id, bundle_name);

        assert_eq!(data.phase_type, ReloadPhaseType::Reloaded);
        assert_eq!(data.bundle_id, bundle_id);
        assert_eq!(data.retry_count, 0);
        assert!(data.reason.ptr.is_null());
        assert_eq!(data.reason.len, 0);
    }

    #[test]
    fn failed_constructor() {
        let bundle_id = BundleId::new("test-bundle");
        let bundle_name = StringView::from_static(b"test_bundle");
        let reason = StringView::from_static(b"init failed");
        let data = ReloadPhaseData::failed(bundle_id, bundle_name, reason);

        assert_eq!(data.phase_type, ReloadPhaseType::Failed);
        assert_eq!(data.bundle_id, bundle_id);
        assert_eq!(data.retry_count, 0);
        assert!(!reason.ptr.is_null());
        assert_eq!(data.reason.len, 11);
    }
}