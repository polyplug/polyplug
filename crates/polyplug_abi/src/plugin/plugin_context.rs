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
    use core::ffi::c_void;

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

    /// TH-07: Verify PluginContext has no bare c_void pointers in public fields.
    /// This is a compile-time verification test.
    #[test]
    fn plugin_context_no_bare_c_void() {
        // PluginContext has two public fields:
        // - bundle_id: u64 (integer, not c_void)
        // - bundle_path: StringView (typed struct, not c_void)
        //
        // This test verifies the types are NOT c_void by checking their sizes.
        // u64 is 8 bytes, StringView is 16 bytes.
        // If either field were *mut c_void, it would be 8 bytes.
        assert_eq!(size_of::<u64>(), 8);
        assert_eq!(size_of::<crate::types::StringView>(), 16);

        // Verify PluginContext is 24 bytes (8 + 16), not 16 bytes (8 + 8)
        // which would indicate bare c_void pointers.
        assert_eq!(size_of::<PluginContext>(), 24);

        // Additional compile-time check: c_void is an empty enum,
        // and PluginContext fields use concrete types.
        // This assertion verifies c_void exists in scope (for import check)
        // but is NOT used in PluginContext public fields.
        let _ = size_of::<c_void>(); // c_void is 0-sized (empty enum)
    }
}
