//! Opaque handle to a guest contract instance.
//!
//! This module defines `GuestContractInstance`, the handle returned from
//! `GuestContractInterface::create_instance` and passed to dispatch calls.
//!
//! # Who provides
//! Created by `GuestContractInterface::create_instance`, owned by guest.
//!
//! # Who calls
//! Host code passes instances to dispatch functions for method calls.
//!
//! # Ownership
//! This is an owned handle - the instance must be destroyed via
//! `GuestContractInterface::destroy_instance` before hot-reload.
//!
//! # Layout
//! - `data`: Opaque instance pointer (owned by guest)
//! - `contract_id`: Contract ID stamped at instance creation

use core::ffi::c_void;
use core::ptr;

use polyplug_utils::GuestContractId;

/// Opaque handle to a guest contract instance.
///
/// Created by `GuestContractInterface::create_instance`, destroyed by `destroy_instance`.
///
/// # Who provides
/// Guest code creates instances via create_instance factory.
/// The guest owns the underlying data.
///
/// # Who calls
/// Host code passes instances to dispatch functions and destroy_instance.
///
/// # Ownership
/// This is an owned handle - the instance must be destroyed via
/// `GuestContractInterface::destroy_instance` before hot-reload.
/// Failure to destroy causes memory leaks and prevents safe hot-reload.
///
/// # Lifetime
/// Lives until `destroy_instance` is called. Must be destroyed before
/// the bundle is unloaded or hot-reloaded.
///
/// # Layout
/// - `data`: Opaque instance pointer (owned by guest)
/// - `contract_id`: Contract ID stamped at instance creation
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GuestContractInstance {
    /// Opaque instance data pointer.
    /// The actual data is owned by the guest plugin.
    ///
    /// # Ownership
    /// Guest owns the memory. Host must not free or modify.
    /// Must be freed via GuestContractInterface::destroy_instance.
    pub data: *mut c_void,
    /// Contract ID stamped at instance creation.
    ///
    /// # Purpose
    /// Identifies which contract an instance belongs to. Set by `create_instance`.
    pub contract_id: GuestContractId,
}

// SAFETY: GuestContractInstance is an opaque handle.
// The underlying data is managed by the guest plugin.
unsafe impl Send for GuestContractInstance {}

// SAFETY: GuestContractInstance is an opaque handle.
// Concurrent access to the underlying data is the guest's responsibility.
unsafe impl Sync for GuestContractInstance {}

impl GuestContractInstance {
    /// Create a null instance handle.
    pub fn null() -> Self {
        Self {
            data: ptr::null_mut(),
            contract_id: GuestContractId::from_u64(0),
        }
    }

    /// Check if this is a null handle.
    pub fn is_null(&self) -> bool {
        self.data.is_null()
    }
}

#[cfg(test)]
mod tests {
    use super::GuestContractInstance;
    use core::mem::{align_of, offset_of, size_of};

    #[test]
    fn layout_guest_contract_instance() {
        // GuestContractInstance: pointer (8) + GuestContractId/u64 (8) = 16 bytes
        assert_eq!(size_of::<GuestContractInstance>(), 16);
        assert_eq!(align_of::<GuestContractInstance>(), 8);
        assert_eq!(offset_of!(GuestContractInstance, data), 0);
        assert_eq!(offset_of!(GuestContractInstance, contract_id), 8);
    }

    #[test]
    fn null_instance() {
        let instance = GuestContractInstance::null();
        assert!(instance.is_null());
    }

    /// TH-08: Verify GuestContractInstance has #[repr(C)] annotation.
    #[test]
    fn guest_contract_instance_repr_c() {
        // #[repr(C)] guarantees: 16 bytes, 8-byte alignment
        assert_eq!(size_of::<GuestContractInstance>(), 16);
        assert_eq!(align_of::<GuestContractInstance>(), 8);
    }
}
