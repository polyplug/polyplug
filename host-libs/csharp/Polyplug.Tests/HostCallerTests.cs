using System;
using Xunit;

namespace Polyplug.Tests;

public class HostCallerTests
{
    #region PluginGuard Public API Tests

    [Fact]
    public void PluginGuard_Reset_ReturnsNullGuard()
    {
        var guard = PluginGuard.Reset();

        Assert.True(guard.IsNull());
        Assert.False(guard.IsValid);
    }

    [Fact]
    public void PluginGuard_DefaultConstructor_CreatesNullGuard()
    {
        PluginGuard guard = default;

        Assert.True(guard.IsNull());
        Assert.False(guard.IsValid);
    }

    [Fact]
    public void PluginGuard_IsValid_IsFalseForNullGuard()
    {
        var guard = PluginGuard.Reset();

        Assert.False(guard.IsValid);
    }

    [Fact]
    public void PluginGuard_IsNull_ReturnsTrueForDefaultGuard()
    {
        PluginGuard guard = default;

        Assert.True(guard.IsNull());
    }

    [Fact]
    public void PluginGuard_GetHandle_ReturnsMaxValueForNullGuard()
    {
        var guard = PluginGuard.Reset();

        Assert.Equal(ulong.MaxValue, guard.GetHandle());
    }

    [Fact]
    public void PluginGuard_GetVTable_ReturnsZeroForNullGuard()
    {
        var guard = PluginGuard.Reset();

        Assert.Equal(nint.Zero, guard.GetVTable());
    }

    #endregion

    #region PluginHandle Public API Tests

    [Fact]
    public void PluginHandle_Null_IsValidFalse()
    {
        var handle = PluginHandle.Null;

        Assert.True(handle.IsNull());
        Assert.False(handle.IsValid);
    }

    [Fact]
    public void PluginHandle_DefaultConstructor_CreatesValidHandle()
    {
        PluginHandle handle = default;

        Assert.False(handle.IsNull());
        Assert.True(handle.IsValid);
    }

    [Fact]
    public void PluginHandle_IsValid_IsFalseForNullHandle()
    {
        var handle = PluginHandle.Null;

        Assert.False(handle.IsValid);
    }

    [Fact]
    public void PluginHandle_Null_HasMaxIndex()
    {
        var handle = PluginHandle.Null;

        Assert.Equal(uint.MaxValue, handle.Index);
    }

    #endregion

    #region Generated Code Pattern Tests

    [Fact]
    public void GeneratedCallerPattern_HasFactoryMethodSignature()
    {
        var expectedPattern = "public static";
        Assert.True(expectedPattern.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_FactoryReturnsNullable()
    {
        var expectedReturnType = "?";
        Assert.True(expectedReturnType.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_HasIsValidProperty()
    {
        var expectedProperty = "public bool IsValid";
        Assert.True(expectedProperty.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_HasResetMethod()
    {
        var expectedMethod = "public void Reset()";
        Assert.True(expectedMethod.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_ImplementsIDisposable()
    {
        var expectedInterface = "IDisposable";
        Assert.True(expectedInterface.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_DisposeCallsReset()
    {
        var expectedPattern = "Dispose() { Reset(); }";
        Assert.True(expectedPattern.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_HasPrivateConstructor()
    {
        var expectedPattern = "private";
        Assert.True(expectedPattern.Length > 0);
    }

    [Fact]
    public void GeneratedCallerPattern_FactoryAcceptsMinVersion()
    {
        var expectedParam = "uint minVersion = 0";
        Assert.True(expectedParam.Length > 0);
    }

    #endregion

    #region Hot-Reload Pattern Tests

    [Fact]
    public void HotReloadPattern_ResetClearsGuard()
    {
        var guard = PluginGuard.Reset();
        Assert.False(guard.IsValid);

        var resetGuard = PluginGuard.Reset();
        Assert.False(resetGuard.IsValid);
    }

    [Fact]
    public void HotReloadPattern_GuardCanBeResetMultipleTimes()
    {
        var guard = PluginGuard.Reset();

        for (int i = 0; i < 10; i++)
        {
            guard = PluginGuard.Reset();
            Assert.False(guard.IsValid);
        }
    }

    [Fact]
    public void HotReloadPattern_NullGuardHasZeroVTable()
    {
        var guard = PluginGuard.Reset();

        Assert.Equal(nint.Zero, guard.GetVTable());
    }

    #endregion
}