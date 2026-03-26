// THIS FILE IS PART OF polyplug — header-only C++ binding.
// Runtime configuration options for hot-reload behavior and other settings.

#pragma once

#include <chrono>
#include <cstdint>

namespace polyplug {

/// Configuration options for the Runtime.
///
/// This struct contains configurable parameters for hot-reload behavior
/// and other runtime settings. It is designed to be extensible for future options.
struct RuntimeConfig {
    /// Whether hot-reload is enabled for this runtime.
    /// Default: false. Must be true to use reload_bundle() or file watcher.
    bool hot_reload_enabled{false};

    /// Maximum number of retry attempts for hot-reload operations.
    /// Default: 3. Set to 0 for infinite retries (when abort_on_max_retries is false).
    uint32_t hot_reload_max_retries{3U};

    /// Interval between retry attempts for hot-reload operations.
    /// Default: 1 second.
    std::chrono::milliseconds hot_reload_retry_interval{1000};

    /// Whether to abort hot-reload after exhausting max_retries.
    /// If true (default): abort and fire Failed notification.
    /// If false: keep retrying forever.
    bool hot_reload_abort_on_max_retries{true};
};

}  // namespace polyplug