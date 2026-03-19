using System;
using Xunit;

namespace Polyplug.Tests;

/// <summary>
/// Unit tests for reload notification types and Runtime configuration.
/// </summary>
public class ReloadPhaseTests
{
    #region ReloadPhaseType Enum Tests

    [Fact]
    public void ReloadPhaseType_Preparing_HasValueZero()
    {
        Assert.Equal(0u, (uint)ReloadPhaseType.Preparing);
    }

    [Fact]
    public void ReloadPhaseType_Reloaded_HasValueOne()
    {
        Assert.Equal(1u, (uint)ReloadPhaseType.Reloaded);
    }

    [Fact]
    public void ReloadPhaseType_Failed_HasValueTwo()
    {
        Assert.Equal(2u, (uint)ReloadPhaseType.Failed);
    }

    #endregion

    #region ReloadPhase Constructor Tests

    [Fact]
    public void ReloadPhase_Constructor_SetsAllProperties()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Preparing,
            bundleId: 12345ul,
            bundleName: "TestBundle",
            retryCount: 2u,
            reason: "Test reason"
        );

        Assert.Equal(ReloadPhaseType.Preparing, phase.Type);
        Assert.Equal(12345ul, phase.BundleId);
        Assert.Equal("TestBundle", phase.BundleName);
        Assert.Equal(2u, phase.RetryCount);
        Assert.Equal("Test reason", phase.Reason);
    }

    [Fact]
    public void ReloadPhase_Constructor_UsesDefaultValues()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Reloaded,
            bundleId: 999ul,
            bundleName: "MyBundle"
        );

        Assert.Equal(ReloadPhaseType.Reloaded, phase.Type);
        Assert.Equal(999ul, phase.BundleId);
        Assert.Equal("MyBundle", phase.BundleName);
        Assert.Equal(0u, phase.RetryCount);
        Assert.Equal(string.Empty, phase.Reason);
    }

    [Fact]
    public void ReloadPhase_Constructor_HandlesNullBundleName()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Failed,
            bundleId: 1ul,
            bundleName: null!,
            retryCount: 0u,
            reason: "Error"
        );

        Assert.Equal(string.Empty, phase.BundleName);
    }

    [Fact]
    public void ReloadPhase_Constructor_HandlesNullReason()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Failed,
            bundleId: 1ul,
            bundleName: "Bundle",
            retryCount: 0u,
            reason: null!
        );

        Assert.Equal(string.Empty, phase.Reason);
    }

    #endregion

    #region ReloadPhase Helper Method Tests

    [Fact]
    public void IsPreparing_ReturnsTrue_WhenTypeIsPreparing()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1ul, "Bundle");
        Assert.True(phase.IsPreparing());
    }

    [Fact]
    public void IsPreparing_ReturnsFalse_WhenTypeIsNotPreparing()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Reloaded, 1ul, "Bundle");
        Assert.False(phase.IsPreparing());
    }

    [Fact]
    public void IsReloaded_ReturnsTrue_WhenTypeIsReloaded()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Reloaded, 1ul, "Bundle");
        Assert.True(phase.IsReloaded());
    }

    [Fact]
    public void IsReloaded_ReturnsFalse_WhenTypeIsNotReloaded()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1ul, "Bundle");
        Assert.False(phase.IsReloaded());
    }

    [Fact]
    public void IsFailed_ReturnsTrue_WhenTypeIsFailed()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Failed, 1ul, "Bundle");
        Assert.True(phase.IsFailed());
    }

    [Fact]
    public void IsFailed_ReturnsFalse_WhenTypeIsNotFailed()
    {
        var phase = new ReloadPhase(ReloadPhaseType.Preparing, 1ul, "Bundle");
        Assert.False(phase.IsFailed());
    }

    #endregion

    #region ReloadPhase ToString Tests

    [Fact]
    public void ToString_Preparing_IncludesAllRelevantFields()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Preparing,
            bundleId: 42ul,
            bundleName: "TestBundle",
            retryCount: 3u,
            reason: "Ignored"
        );

        var result = phase.ToString();

        Assert.Contains("Preparing", result);
        Assert.Contains("BundleId=42", result);
        Assert.Contains("TestBundle", result);
        Assert.Contains("RetryCount=3", result);
    }

    [Fact]
    public void ToString_Reloaded_IncludesBundleInfo()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Reloaded,
            bundleId: 99ul,
            bundleName: "MyPlugin"
        );

        var result = phase.ToString();

        Assert.Contains("Reloaded", result);
        Assert.Contains("BundleId=99", result);
        Assert.Contains("MyPlugin", result);
    }

    [Fact]
    public void ToString_Failed_IncludesReason()
    {
        var phase = new ReloadPhase(
            ReloadPhaseType.Failed,
            bundleId: 1ul,
            bundleName: "FailedBundle",
            retryCount: 0u,
            reason: "Connection timeout"
        );

        var result = phase.ToString();

        Assert.Contains("Failed", result);
        Assert.Contains("BundleId=1", result);
        Assert.Contains("FailedBundle", result);
        Assert.Contains("Connection timeout", result);
    }

    #endregion
}

public class RuntimeConfigTests
{
    #region Default Values Tests

    [Fact]
    public void RuntimeConfig_DefaultConstructor_SetsDefaultValues()
    {
        var config = new RuntimeConfig();

        Assert.Equal(3u, config.HotReloadMaxRetries);
        Assert.Equal(1000ul, config.HotReloadRetryIntervalMs);
        Assert.True(config.HotReloadAbortOnMaxRetries);
    }

    [Fact]
    public void RuntimeConfig_ParameterlessConstructor_CreatesValidInstance()
    {
        var config = new RuntimeConfig();

        Assert.NotNull(config);
        Assert.IsType<RuntimeConfig>(config);
    }

    #endregion

    #region Custom Values Tests

    [Fact]
    public void RuntimeConfig_ParameterizedConstructor_SetsCustomValues()
    {
        var config = new RuntimeConfig(
            maxRetries: 10u,
            retryIntervalMs: 5000ul,
            abortOnMaxRetries: false
        );

        Assert.Equal(10u, config.HotReloadMaxRetries);
        Assert.Equal(5000ul, config.HotReloadRetryIntervalMs);
        Assert.False(config.HotReloadAbortOnMaxRetries);
    }

    [Fact]
    public void RuntimeConfig_Properties_CanBeModified()
    {
        var config = new RuntimeConfig
        {
            HotReloadMaxRetries = 5u,
            HotReloadRetryIntervalMs = 2000ul,
            HotReloadAbortOnMaxRetries = false
        };

        Assert.Equal(5u, config.HotReloadMaxRetries);
        Assert.Equal(2000ul, config.HotReloadRetryIntervalMs);
        Assert.False(config.HotReloadAbortOnMaxRetries);
    }

    [Fact]
    public void RuntimeConfig_AllowsZeroRetries()
    {
        var config = new RuntimeConfig
        {
            HotReloadMaxRetries = 0u
        };

        Assert.Equal(0u, config.HotReloadMaxRetries);
    }

    [Fact]
    public void RuntimeConfig_AllowsLargeRetryInterval()
    {
        var config = new RuntimeConfig
        {
            HotReloadRetryIntervalMs = ulong.MaxValue
        };

        Assert.Equal(ulong.MaxValue, config.HotReloadRetryIntervalMs);
    }

    #endregion
}

public class RuntimeStaticMethodTests
{
    #region OnReload Argument Validation Tests

    [Fact]
    public void OnReload_ThrowsArgumentNullException_WhenCallbackIsNull()
    {
        Assert.Throws<ArgumentNullException>(() => Runtime.OnReload(null!));
    }

    #endregion

    #region SetConfig Argument Validation Tests

    [Fact]
    public void SetConfig_ThrowsArgumentNullException_WhenConfigIsNull()
    {
        Assert.Throws<ArgumentNullException>(() => Runtime.SetConfig(null!));
    }

    #endregion
}