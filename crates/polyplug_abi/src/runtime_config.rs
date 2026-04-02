use crate::{compatibility::Compatibility, types::{array::Array, string_view::StringView}};

/// Configuration passed to `polyplug_runtime_create` during runtime initialisation.
///
/// # OWNERSHIP
/// borrowed for the duration of the runtime build only.
/// The caller may free all pointed-to memory after the build
/// returns. The runtime copies any data it needs to retain.
#[repr(C)]
pub struct RuntimeConfig {
    /// Plugin directories to scan (array of `plugin_dir_count` StringViews).
    pub plugin_dirs: Array<StringView>,
    /// Compatibility mode.
    pub compatibility: Compatibility,
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use crate::runtime_config::RuntimeConfig;

    #[test]
    fn layout_runtime_config() {
        assert_eq!(size_of::<RuntimeConfig>(), 24);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, plugin_dirs), 0);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 16);
    }
}