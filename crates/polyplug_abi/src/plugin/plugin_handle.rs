/// Opaque handle to a loaded plugin — validated on use.
///
/// INTERNAL STRUCTURE: index into registry array + generation counter.
/// The generation counter detects use-after-unload.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginHandle {
    /// Slot in the registry array.
    pub index: u32,
    /// Incremented on unload — detects stale handles.
    pub generation: u32,
}

impl PluginHandle {
    /// The null/invalid handle. Never returned by a successful lookup.
    pub const fn null() -> PluginHandle {
        PluginHandle {
            index: u32::MAX,
            generation: 0,
        }
    }

    /// Returns true if this is the null handle.
    pub const fn is_null(&self) -> bool {
        self.index == u32::MAX
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
        let valid: PluginHandle = PluginHandle {
            index: 0,
            generation: 1,
        };
        assert!(!valid.is_null());
    }

    #[test]
    fn layout_plugin_handle() {
        assert_eq!(size_of::<PluginHandle>(), 8);
        assert_eq!(align_of::<PluginHandle>(), 4);
        assert_eq!(offset_of!(PluginHandle, index), 0);
        assert_eq!(offset_of!(PluginHandle, generation), 4);
    }
}
