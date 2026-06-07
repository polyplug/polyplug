//! Guest Contract Handle — index-based handle to a registered guest contract.
//!
//! This module defines `GuestContractHandle`, the handle returned by `find_guest_contract`
//! and passed to `resolve_guest_contract` to obtain a `GuestContractInterface`.
//!
//! # Who provides
//! The registry creates handles during guest contract registration.
//!
//! # Who calls
//! Host code calls `find_guest_contract` to obtain handles, then `resolve_guest_contract`
//! to get the interface.
//!
//! # Ownership
//! Handles are copyable (an index plus a generation counter). No ownership tracking.
//!
//! # Lifetime
//! Valid until the contract is unregistered or the bundle is unloaded. Each
//! registry slot carries a generation counter that is bumped whenever the slot is
//! retired, so a handle minted against an earlier generation can be detected as
//! stale even after its index is recycled by a later registration.
//! Use `resolve_guest_contract` to check validity — returns null / StaleHandle if stale.

/// Opaque handle to a registered guest contract.
///
/// The handle pairs the slot `index` with the `generation` the slot held when the
/// handle was minted. `resolve_guest_contract` rejects a handle whose `generation`
/// no longer matches the slot's current generation (the slot was retired and the
/// index possibly reused), returning `StaleHandle`. Out-of-bounds or empty-slot
/// indices return InvalidHandle.
///
/// # Naming
/// Named `GuestContractHandle` for consistency with `GuestContractInterface`
/// and `GuestContractInstance`.
///
/// # Layout
/// - `index`: Slot index in the registry (u32, offset 0)
/// - `generation`: Slot generation the handle was minted against (u32, offset 4)
///
/// # Safety
/// Handles become stale after unload. Call `resolve_guest_contract` to validate.
/// Returns null pointer if the handle is invalid.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestContractHandle {
    /// Slot in the registry array.
    pub index: u32,
    /// Generation the slot held when this handle was minted.
    pub generation: u32,
}

impl GuestContractHandle {
    /// The null/invalid handle. Never returned by a successful lookup.
    pub const fn null() -> GuestContractHandle {
        GuestContractHandle {
            index: u32::MAX,
            generation: 0,
        }
    }

    /// Returns true if this is the null handle.
    pub const fn is_null(&self) -> bool {
        self.index == u32::MAX
    }

    /// Pack the handle into a u64 for FFI calls.
    ///
    /// The generation occupies the high 32 bits and the index the low 32 bits, so
    /// the full `{ index, generation }` identity round-trips through a single u64.
    /// The null handle packs to `u64::MAX` (index `u32::MAX`, generation 0).
    ///
    /// Used when passing the handle to FFI functions like
    /// `polyplug_runtime_resolve_guest_contract`.
    pub const fn pack(&self) -> u64 {
        if self.is_null() {
            u64::MAX
        } else {
            ((self.generation as u64) << 32) | (self.index as u64)
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
        let valid: GuestContractHandle = GuestContractHandle {
            index: 0,
            generation: 0,
        };
        assert!(!valid.is_null());
    }

    #[test]
    fn layout_guest_contract_handle() {
        assert_eq!(size_of::<GuestContractHandle>(), 8);
        assert_eq!(align_of::<GuestContractHandle>(), 4);
        assert_eq!(offset_of!(GuestContractHandle, index), 0);
        assert_eq!(offset_of!(GuestContractHandle, generation), 4);
    }
}
