//! Runtime configuration.

use crate::runtime::Compatibility;
use crate::runtime::ReloadPhase;

/// Configuration for the polyplug runtime passed to `polyplug_runtime_create`.
///
/// # OWNERSHIP
/// Borrowed for the duration of the runtime build only.
/// The runtime copies any data it needs to retain.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Compatibility mode for version resolution.
    pub compatibility: Compatibility,
    /// Whether hot-reload is enabled.
    pub hot_reload_enabled: bool,
    /// Optional hot-reload callback, or null for no callback.
    pub on_reload: Option<unsafe extern "C" fn(ReloadPhase)>,
}

// SAFETY: RuntimeConfig contains a function pointer and no shared mutable state.
// Function pointers are Send. The struct is read-only after construction.
unsafe impl Send for RuntimeConfig {}
// SAFETY: No interior mutability — all fields are plain values or function pointers.
unsafe impl Sync for RuntimeConfig {}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            compatibility: Compatibility::Strict,
            hot_reload_enabled: false,
            on_reload: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use core::mem::{align_of, offset_of, size_of};

    use super::RuntimeConfig;
    use crate::runtime::Compatibility;

    #[test]
    fn layout_runtime_config() {
        // compatibility: 4 bytes (u32) at 0x00
        // hot_reload_enabled: 1 byte (bool) at 0x04
        // padding: 3 bytes (0x05-0x07)
        // on_reload: 8 bytes (fn pointer) at 0x08
        // Total: 16 bytes, alignment 8
        assert_eq!(size_of::<RuntimeConfig>(), 16);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 0x0);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_enabled), 0x4);
        assert_eq!(offset_of!(RuntimeConfig, on_reload), 0x8);
    }

    #[test]
    fn default_runtime_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.compatibility, Compatibility::Strict);
        assert!(!config.hot_reload_enabled);
        assert!(config.on_reload.is_none());
    }
}
