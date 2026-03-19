using System;

namespace Polyplug;

/// <summary>
/// Configuration options for the Runtime.
///
/// This class contains configurable parameters for hot-reload behavior
/// and other runtime settings. It is designed to be extensible for future options.
/// </summary>
public sealed class RuntimeConfig
{
    /// <summary>
    /// Maximum number of retry attempts for hot-reload operations.
    /// Default: 3. Set to 0 for infinite retries (when AbortOnMaxRetries is false).
    /// </summary>
    public uint HotReloadMaxRetries { get; set; } = 3u;

    /// <summary>
    /// Interval between retry attempts for hot-reload operations in milliseconds.
    /// Default: 1000 (1 second).
    /// </summary>
    public ulong HotReloadRetryIntervalMs { get; set; } = 1000ul;

    /// <summary>
    /// Whether to abort hot-reload after exhausting MaxRetries.
    /// If true (default): abort and fire Failed notification.
    /// If false: keep retrying forever.
    /// </summary>
    public bool HotReloadAbortOnMaxRetries { get; set; } = true;

    /// <summary>
    /// Creates a new RuntimeConfig with default values.
    /// </summary>
    public RuntimeConfig() { }

    /// <summary>
    /// Creates a new RuntimeConfig with specified values.
    /// </summary>
    public RuntimeConfig(uint maxRetries, ulong retryIntervalMs, bool abortOnMaxRetries)
    {
        HotReloadMaxRetries = maxRetries;
        HotReloadRetryIntervalMs = retryIntervalMs;
        HotReloadAbortOnMaxRetries = abortOnMaxRetries;
    }
}