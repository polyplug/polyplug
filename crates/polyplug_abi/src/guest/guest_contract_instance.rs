//! Opaque handle to a guest contract instance.
//!
//! Created by `GuestContractInterface::create_instance`, destroyed by `destroy_instance`.

use core::ffi::c_void;

/// Opaque handle to a guest contract instance.
///
/// This is an owned handle - the instance must be destroyed via
/// `GuestContractInterface::destroy_instance` before hot-reload.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GuestContractInstance {
    /// Opaque instance data pointer.
    /// The actual data is owned by the plugin/guest.
    pub data: *mut c_void,
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
        Self { data: core::ptr::null_mut() }
    }

    /// Check if this is a null handle.
    pub fn is_null(&self) -> bool {
        self.data.is_null()
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{size_of, align_of};
    use super::GuestContractInstance;

    #[test]
    fn layout_guest_contract_instance() {
        assert_eq!(size_of::<GuestContractInstance>(), 8);
        assert_eq!(align_of::<GuestContractInstance>(), 8);
    }

    #[test]
    fn null_instance() {
        let instance = GuestContractInstance::null();
        assert!(instance.is_null());
    }
}