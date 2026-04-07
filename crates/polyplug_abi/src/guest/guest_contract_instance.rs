//! Opaque handle to a guest contract instance.
//!
//! Created by `GuestContractInterface::create_instance`, destroyed by `destroy_instance`.
//! The contract_id field enables zero-overhead dispatch in call_guest_method.

use core::ffi::c_void;

use polyplug_utils::GuestContractId;

/// Opaque handle to a guest contract instance.
///
/// This is an owned handle - the instance must be destroyed via
/// `GuestContractInterface::destroy_instance` before hot-reload.
///
/// The contract_id field enables zero-overhead dispatch in call_guest_method
/// by eliminating the need to look up which contract an instance belongs to.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GuestContractInstance {
    /// Opaque instance data pointer.
    /// The actual data is owned by the plugin/guest.
    pub data: *mut c_void,
    /// Contract ID for zero-overhead dispatch.
    /// Enables call_guest_method to dispatch without lookup.
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
            data: core::ptr::null_mut(),
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
    use core::mem::{align_of, offset_of, size_of};
    use super::GuestContractInstance;

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