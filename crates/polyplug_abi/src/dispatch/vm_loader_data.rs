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
    use super::VmLoaderData;

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
}