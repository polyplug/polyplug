use crate::compatibility::Compatibility;

/// Configuration for the polyplug runtime passed to `polyplug_runtime_create`
/// during runtime initialisation.
///
/// # OWNERSHIP
/// borrowed for the duration of the runtime build only.
/// The caller may free all pointed-to memory after the build
/// returns. The runtime copies any data it needs to retain.
#[repr(C)]
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Whether hot-reload is enabled for this runtime.
    /// When disabled, reload_bundle() returns ReloadDisabled error.
    pub hot_reload_enabled: bool,
    /// Maximum number of retry attempts for hot-reload operations.
    pub hot_reload_max_retries: u32,
    /// Interval between hot-reload retry attempts in seconds.
    pub hot_reload_retry_interval_ms: u64,
    /// Whether to abort the runtime when max retries are exhausted.
    pub hot_reload_abort_on_max_retries: bool,
    /// Compatibility mode.
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

    use crate::runtime_config::RuntimeConfig;

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
}