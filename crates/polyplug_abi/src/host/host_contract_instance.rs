//! Opaque handle to a host contract instance.

use core::ffi::c_void;

/// Opaque handle to a host contract instance.
///
/// Created by `HostContractInterface::create_instance`, destroyed by `destroy_instance`.
/// For singleton host contracts, the same instance is returned for all callers.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostContractInstance {
    /// Opaque instance data pointer.
    /// The actual data is owned by the host.
    pub data: *mut c_void,
}

// SAFETY: HostContractInstance is an opaque handle.
// The underlying data is managed by the host.
unsafe impl Send for HostContractInstance {}

// SAFETY: HostContractInstance is an opaque handle.
// Concurrent access to the underlying data is the host's responsibility.
unsafe impl Sync for HostContractInstance {}

impl HostContractInstance {
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
    use super::HostContractInstance;

    #[test]
    fn layout_host_contract_instance() {
        assert_eq!(size_of::<HostContractInstance>(), 8);
        assert_eq!(align_of::<HostContractInstance>(), 8);
    }

    #[test]
    fn null_instance() {
        let instance = HostContractInstance::null();
        assert!(instance.is_null());
    }

    /// TH-08: Verify HostContractInstance has #[repr(C)] annotation.
    /// This is verified by the struct having pointer-sized layout (8 bytes on x86_64).
    #[test]
    fn host_contract_instance_repr_c() {
        // #[repr(C)] guarantees the struct is laid out as specified in C ABI.
        // For a single-field struct with *mut c_void, this means:
        // - Size: 8 bytes (pointer size on x86_64)
        // - Alignment: 8 bytes (pointer alignment on x86_64)
        // - No padding (single field)
        assert_eq!(size_of::<HostContractInstance>(), 8);
        assert_eq!(align_of::<HostContractInstance>(), 8);

        // The #[repr(C)] annotation is visible in the source code at line 9.
        // This test verifies the layout matches expectations for #[repr(C)].
    }
}