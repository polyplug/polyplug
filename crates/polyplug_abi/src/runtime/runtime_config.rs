//! Runtime configuration.

use crate::runtime::Compatibility;

/// Configuration for the polyplug runtime passed to `polyplug_runtime_create`.
///
/// # OWNERSHIP
/// Borrowed for the duration of the runtime build only.
/// The runtime copies any data it needs to retain.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Whether hot-reload is enabled.
    pub hot_reload_enabled: bool,
    /// Maximum retry attempts for hot-reload.
    pub hot_reload_max_retries: u32,
    /// Interval between retries in milliseconds.
    pub hot_reload_retry_interval_ms: u64,
    /// Abort runtime when max retries exhausted.
    pub hot_reload_abort_on_max_retries: bool,
    /// Compatibility mode for version resolution.
    pub compatibility: Compatibility,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            hot_reload_enabled: false,
            hot_reload_max_retries: 3,
            hot_reload_retry_interval_ms: 3000,
            hot_reload_abort_on_max_retries: true,
            compatibility: Compatibility::Strict,
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
        assert_eq!(size_of::<RuntimeConfig>(), 24);
        assert_eq!(align_of::<RuntimeConfig>(), 8);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_enabled), 0x0);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_max_retries), 0x4);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_retry_interval_ms), 0x8);
        assert_eq!(offset_of!(RuntimeConfig, hot_reload_abort_on_max_retries), 0x10);
        assert_eq!(offset_of!(RuntimeConfig, compatibility), 0x14);
    }

    #[test]
    fn default_runtime_config() {
        let config = RuntimeConfig::default();
        assert!(!config.hot_reload_enabled);
        assert_eq!(config.hot_reload_max_retries, 3);
        assert_eq!(config.hot_reload_retry_interval_ms, 3000);
        assert!(config.hot_reload_abort_on_max_retries);
        assert_eq!(config.compatibility, Compatibility::Strict);
    }
}