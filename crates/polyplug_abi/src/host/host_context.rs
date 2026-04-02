/// Opaque host context passed to plugin functions via rt_ctx parameter.
///
/// Contains the runtime pointer and the bundle_id of the calling bundle.
/// The actual implementation is in the polyplug crate; this definition
/// establishes the ABI layout.
///
/// # OWNERSHIP
/// `'static`, lives as long as the runtime.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct HostContext {
    /// Opaque pointer to the Runtime. Never dereferenced by plugins.
    pub runtime: *mut core::ffi::c_void,
    /// Bundle ID of the calling bundle for dependency enforcement.
    pub bundle_id: u64,
    /// Host's supported ABI version for negotiation.
    /// Plugin can use this to determine available features.
    pub host_abi_version: u32,
}

// SAFETY: HostContext contains a raw pointer (which is Send+Sync as raw ptr)
// and a u64. The pointer is only dereferenced by the host runtime.
unsafe impl Send for HostContext {}

// SAFETY: HostContext contains only a raw pointer and a u64.
// Concurrent reads are safe — no mutation occurs through shared references.
unsafe impl Sync for HostContext {}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::host::host_context::HostContext;

    #[test]
    fn layout_host_context() {
        assert_eq!(size_of::<HostContext>(), 16);
        assert_eq!(align_of::<HostContext>(), 8);
        assert_eq!(offset_of!(HostContext, runtime), 0);
        assert_eq!(offset_of!(HostContext, bundle_id), 8);
    }
}