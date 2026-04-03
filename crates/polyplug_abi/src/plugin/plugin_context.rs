use crate::types::StringView;

/// Context passed to every guest `polyplug_init()` function.
///
/// # OWNERSHIP
/// The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
/// **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginContext {
    /// Bundle ID for dependency enforcement during init.
    pub bundle_id: u64,
    /// Absolute canonical path to the directory containing the loaded bundle.
    pub bundle_path: StringView,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::plugin::plugin_context::PluginContext;

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn plugin_context_layout() {
        // PluginContext: u64 (8) + StringView (16) = 24 bytes
        assert_eq!(size_of::<PluginContext>(), 24);
        assert_eq!(align_of::<PluginContext>(), 8);
        assert_eq!(offset_of!(PluginContext, bundle_id), 0);
        assert_eq!(offset_of!(PluginContext, bundle_path), 8);
    }
}
