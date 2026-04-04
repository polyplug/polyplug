/// Opaque handle to a loaded plugin.
///
/// The handle is just an index into the registry array.
/// Out-of-bounds indices return InvalidHandle error.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle {
    /// Slot in the registry array.
    pub index: u32,
}

impl PluginHandle {
    /// The null/invalid handle. Never returned by a successful lookup.
    pub const fn null() -> PluginHandle {
        PluginHandle { index: u32::MAX }
    }

    /// Returns true if this is the null handle.
    pub const fn is_null(&self) -> bool {
        self.index == u32::MAX
    }

    /// Pack the handle into a u64 for FFI calls.
    ///
    /// Used when passing the handle to FFI functions like
    /// `polyplug_runtime_resolve_plugin`.
    pub const fn pack(&self) -> u64 {
        if self.is_null() {
            u64::MAX
        } else {
            self.index as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::PluginHandle;

    #[test]
    fn test_plugin_handle_null() {
        let h: PluginHandle = PluginHandle::null();
        assert!(h.is_null());
        let valid: PluginHandle = PluginHandle { index: 0 };
        assert!(!valid.is_null());
    }

    #[test]
    fn layout_plugin_handle() {
        assert_eq!(size_of::<PluginHandle>(), 4);
        assert_eq!(align_of::<PluginHandle>(), 4);
        assert_eq!(offset_of!(PluginHandle, index), 0);
    }
}
