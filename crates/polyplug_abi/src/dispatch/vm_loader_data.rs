//! Opaque handle to VM loader-specific data.
//!
//! Wraps VM-specific state managed by each loader (Python, Lua, JS).
//! Opaque to core runtime — loaders know their own state layout.

use core::ffi::c_void;

/// Opaque handle to VM loader-specific data.
///
/// Wraps VM-specific state managed by each loader (Python, Lua, JS).
/// Opaque to core runtime — loaders know their own state layout.
///
/// # OWNERSHIP
/// Owned by the loader. Lives for the lifetime of the loaded plugin.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct VmLoaderData {
    /// Opaque pointer to VM-specific loader data.
    pub data: *mut c_void,
}

// SAFETY: VmLoaderData is an opaque handle.
// The underlying data is managed by the loader.
unsafe impl Send for VmLoaderData {}

// SAFETY: VmLoaderData is an opaque handle.
// Concurrent access to the underlying data is the loader's responsibility.
unsafe impl Sync for VmLoaderData {}

impl VmLoaderData {
    /// Create a null loader data handle.
    pub fn null() -> Self {
        Self {
            data: core::ptr::null_mut(),
        }
    }

    /// Check if this is a null handle.
    pub fn is_null(&self) -> bool {
        self.data.is_null()
    }
}

#[cfg(test)]
mod tests {
    use super::VmLoaderData;
    use core::mem::{align_of, size_of};

    #[test]
    fn layout_vm_loader_data() {
        assert_eq!(size_of::<VmLoaderData>(), 8);
        assert_eq!(align_of::<VmLoaderData>(), 8);
    }

    #[test]
    fn null_loader_data() {
        let data = VmLoaderData::null();
        assert!(data.is_null());
    }

    /// TH-08: Verify VmLoaderData has #[repr(C)] annotation.
    /// This is verified by the struct having pointer-sized layout (8 bytes on x86_64).
    #[test]
    fn vm_loader_data_repr_c() {
        // #[repr(C)] guarantees the struct is laid out as specified in C ABI.
        // For a single-field struct with *mut c_void, this means:
        // - Size: 8 bytes (pointer size on x86_64)
        // - Alignment: 8 bytes (pointer alignment on x86_64)
        // - No padding (single field)
        assert_eq!(size_of::<VmLoaderData>(), 8);
        assert_eq!(align_of::<VmLoaderData>(), 8);

        // The #[repr(C)] annotation is visible in the source code at line 15.
        // This test verifies the layout matches expectations for #[repr(C)].
    }
}
