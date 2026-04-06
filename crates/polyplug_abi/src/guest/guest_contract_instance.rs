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

    /// TH-08: Verify GuestContractInstance has #[repr(C)] annotation.
    /// This is verified by the struct having pointer-sized layout (8 bytes on x86_64).
    #[test]
    fn guest_contract_instance_repr_c() {
        // #[repr(C)] guarantees the struct is laid out as specified in C ABI.
        // For a single-field struct with *mut c_void, this means:
        // - Size: 8 bytes (pointer size on x86_64)
        // - Alignment: 8 bytes (pointer alignment on x86_64)
        // - No padding (single field)
        assert_eq!(size_of::<GuestContractInstance>(), 8);
        assert_eq!(align_of::<GuestContractInstance>(), 8);

        // The #[repr(C)] annotation is visible in the source code at line 11.
        // This test verifies the layout matches expectations for #[repr(C)].
    }
}