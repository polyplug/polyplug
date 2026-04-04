use crate::types::{StringView, Version};

/// Metadata about a plugin within a bundle.
///
/// # OWNERSHIP
/// value type passed by pointer during init. The `name` and
/// `contract_name` StringViews are borrowed from the plugin's static memory.
/// The receiver must not free or outlive the plugin's library.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PluginDescriptor {
    /// Human-readable plugin name.
    pub name: StringView,
    /// Full contract name for collision detection.
    pub contract_name: StringView,
    /// Plugin version
    pub version: Version,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::plugin::plugin_descriptor::PluginDescriptor;

    #[test]
    fn layout_plugin_descriptor() {
        assert_eq!(size_of::<PluginDescriptor>(), 48);
        assert_eq!(align_of::<PluginDescriptor>(), 8);
        assert_eq!(offset_of!(PluginDescriptor, name), 0);
        assert_eq!(offset_of!(PluginDescriptor, contract_name), 16);
        assert_eq!(offset_of!(PluginDescriptor, version), 32);
    }
}
