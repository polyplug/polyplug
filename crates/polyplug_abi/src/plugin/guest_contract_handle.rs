//! Guest Contract Handle — index-based handle to a registered guest contract.
//!
//! This module defines `GuestContractHandle`, the handle returned by `find_by_contract`
//! and passed to `resolve_contract` to obtain a `GuestContractInterface`.
//!
//! # Who provides
//! The registry creates handles during guest contract registration.
//!
//! # Who calls
//! Host code calls `find_by_contract` to obtain handles, then `resolve_contract`
//! to get the interface.
//!
//! # Ownership
//! Handles are copyable (just a u32 index). No ownership tracking.
//!
//! # Lifetime
//! Valid until the contract is unregistered or the bundle is unloaded.
//! Use `resolve_contract` to check validity — returns null if stale.

/// Opaque handle to a registered guest contract.
///
/// The handle is just an index into the registry array.
/// Out-of-bounds indices return InvalidHandle error.
///
/// # Naming
/// Named `GuestContractHandle` for consistency with `GuestContractInterface`
/// and `GuestContractInstance`.
///
/// # Layout
/// - `index`: Slot index in the registry (u32)
///
/// # Safety
/// Handles become stale after unload. Call `resolve_contract` to validate.
/// Returns null pointer if the handle is invalid.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestContractHandle {
    /// Slot in the registry array.
    pub index: u32,
}

impl GuestContractHandle {
    /// The null/invalid handle. Never returned by a successful lookup.
    pub const fn null() -> GuestContractHandle {
        GuestContractHandle { index: u32::MAX }
    }

    /// Returns true if this is the null handle.
    pub const fn is_null(&self) -> bool {
        self.index == u32::MAX
    }

    /// Pack the handle into a u64 for FFI calls.
    ///
    /// Used when passing the handle to FFI functions like
    /// `polyplug_runtime_resolve_contract`.
    pub const fn pack(&self) -> u64 {
        if self.is_null() {
            u64::MAX
        } else {
            self.index as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::GuestContractHandle;

    #[test]
    fn test_guest_contract_handle_null() {
        let h: GuestContractHandle = GuestContractHandle::null();
        assert!(h.is_null());
        let valid: GuestContractHandle = GuestContractHandle { index: 0 };
        assert!(!valid.is_null());
    }

    #[test]
    fn layout_guest_contract_handle() {
        assert_eq!(size_of::<GuestContractHandle>(), 4);
        assert_eq!(align_of::<GuestContractHandle>(), 4);
        assert_eq!(offset_of!(GuestContractHandle, index), 0);
    }
}