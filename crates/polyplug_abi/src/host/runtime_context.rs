//! Opaque handle to the runtime context passed to plugin functions.
//!
//! Wraps a `*mut HostContext` pointer passed to plugins during `polyplug_init`.
//! This typed handle improves type safety at the FFI boundary.

use core::ffi::c_void;

/// Opaque handle to the runtime context.
///
/// Wraps a `*mut HostContext` pointer passed to plugins during `polyplug_init`.
/// This typed handle improves type safety at the FFI boundary.
///
/// # OWNERSHIP
/// `'static`, lives as long as the runtime. The underlying HostContext
/// is managed by the host runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RuntimeContext {
    /// Opaque pointer to HostContext.
    pub data: *mut c_void,
}

// SAFETY: RuntimeContext is an opaque handle.
// The underlying data is managed by the host runtime.
unsafe impl Send for RuntimeContext {}

// SAFETY: RuntimeContext is an opaque handle.
// Concurrent access to the underlying data is the host's responsibility.
unsafe impl Sync for RuntimeContext {}

impl RuntimeContext {
    /// Create a null context handle.
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
    use super::RuntimeContext;

    #[test]
    fn layout_runtime_context() {
        assert_eq!(size_of::<RuntimeContext>(), 8);
        assert_eq!(align_of::<RuntimeContext>(), 8);
    }

    #[test]
    fn null_context() {
        let ctx = RuntimeContext::null();
        assert!(ctx.is_null());
    }
}