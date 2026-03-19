using System;

namespace Polyplug;

/// <summary>
/// Phase type for hot-reload notifications.
/// </summary>
public enum ReloadPhaseType : uint
{
    /// <summary>
    /// Before vtable swap - host should destroy instances.
    /// </summary>
    Preparing = 0,

    /// <summary>
    /// After vtable swap - instances can be re-resolved.
    /// </summary>
    Reloaded = 1,

    /// <summary>
    /// Reload aborted after max retries.
    /// </summary>
    Failed = 2,
}

/// <summary>
/// Notification phase for hot-reload operations.
///
/// Used by the reload callback to notify the host about reload progress.
/// Mirrors the C ABI callback parameters for hot-reload notifications.
/// </summary>
public sealed class ReloadPhase
{
    /// <summary>
    /// Phase type (Preparing, Reloaded, or Failed).
    /// </summary>
    public ReloadPhaseType Type { get; }

    /// <summary>
    /// FNV-1a 64-bit hash of the bundle name.
    /// </summary>
    public ulong BundleId { get; }

    /// <summary>
    /// Human-readable bundle name.
    /// </summary>
    public string BundleName { get; }

    /// <summary>
    /// Current retry attempt (0-indexed, only for Preparing).
    /// </summary>
    public uint RetryCount { get; }

    /// <summary>
    /// Error reason (only for Failed phase).
    /// </summary>
    public string Reason { get; }

    /// <summary>
    /// Creates a new ReloadPhase instance.
    /// </summary>
    public ReloadPhase(
        ReloadPhaseType type,
        ulong bundleId,
        string bundleName,
        uint retryCount = 0u,
        string reason = "")
    {
        Type = type;
        BundleId = bundleId;
        BundleName = bundleName ?? string.Empty;
        RetryCount = retryCount;
        Reason = reason ?? string.Empty;
    }

    /// <summary>
    /// Returns true if this is a Preparing phase.
    /// </summary>
    public bool IsPreparing() => Type == ReloadPhaseType.Preparing;

    /// <summary>
    /// Returns true if this is a Reloaded phase.
    /// </summary>
    public bool IsReloaded() => Type == ReloadPhaseType.Reloaded;

    /// <summary>
    /// Returns true if this is a Failed phase.
    /// </summary>
    public bool IsFailed() => Type == ReloadPhaseType.Failed;

    /// <inheritdoc />
    public override string ToString()
    {
        return Type switch
        {
            ReloadPhaseType.Preparing => $"ReloadPhase.Preparing(BundleId={BundleId}, BundleName=\"{BundleName}\", RetryCount={RetryCount})",
            ReloadPhaseType.Reloaded => $"ReloadPhase.Reloaded(BundleId={BundleId}, BundleName=\"{BundleName}\")",
            ReloadPhaseType.Failed => $"ReloadPhase.Failed(BundleId={BundleId}, BundleName=\"{BundleName}\", Reason=\"{Reason}\")",
            _ => $"ReloadPhase.Unknown(Type={Type}, BundleId={BundleId})"
        };
    }
}