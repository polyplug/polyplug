// Unit tests for PolyplugHost.Log — the guest-side logging helper.
//
// The runtime is not involved: a fake HostApi is allocated in unmanaged memory
// with its Log field pointing at a managed capture callback. This validates the
// guest-side contract end to end — UTF-16 → UTF-8 transcoding, buffer pinning,
// StringView construction, the self-passing call convention, and the no-op
// paths before init / with a null Log pointer.

using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug.Abi;
using Xunit;

namespace Polyplug.Guest.Tests;

public unsafe class PolyplugHostLogTests : IDisposable
{
    private static int s_callCount;
    private static uint s_capturedLevel;
    private static string s_capturedScope = string.Empty;
    private static string s_capturedMessage = string.Empty;
    private static bool s_capturedScopePtrNull;
    private static bool s_capturedMessagePtrNull;
    private static IntPtr s_capturedSelf;

    private readonly HostApi* _host;

    public PolyplugHostLogTests()
    {
        s_callCount = 0;
        s_capturedLevel = 0;
        s_capturedScope = string.Empty;
        s_capturedMessage = string.Empty;
        s_capturedScopePtrNull = false;
        s_capturedMessagePtrNull = false;
        s_capturedSelf = IntPtr.Zero;

        _host = (HostApi*)NativeMemory.AllocZeroed((nuint)sizeof(HostApi));
        _host->Log = (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, uint, StringView, StringView, void>)&CaptureLog;
        RuntimeAbiStorage.StoreRuntimeAbi((IntPtr)_host);
    }

    public void Dispose()
    {
        // RuntimeAbiStorage is process-global; reset it so test order never matters.
        RuntimeAbiStorage.StoreRuntimeAbi(IntPtr.Zero);
        NativeMemory.Free(_host);
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static void CaptureLog(IntPtr self, uint level, StringView scope, StringView message)
    {
        s_callCount++;
        s_capturedSelf = self;
        s_capturedLevel = level;
        s_capturedScopePtrNull = scope.Ptr == IntPtr.Zero;
        s_capturedMessagePtrNull = message.Ptr == IntPtr.Zero;
        s_capturedScope = ReadView(scope);
        s_capturedMessage = ReadView(message);
    }

    private static string ReadView(StringView view)
    {
        if (view.Ptr == IntPtr.Zero || view.Len == 0)
            return string.Empty;
        return Encoding.UTF8.GetString((byte*)view.Ptr, checked((int)view.Len));
    }

    [Fact]
    public void Log_DeliversLevelScopeMessageVerbatim()
    {
        PolyplugHost.Log(LogLevel.Info, "guest.unit-test", "plain ascii message");

        Assert.Equal(1, s_callCount);
        Assert.Equal((IntPtr)_host, s_capturedSelf);
        Assert.Equal((uint)LogLevel.Info, s_capturedLevel);
        Assert.Equal("guest.unit-test", s_capturedScope);
        Assert.Equal("plain ascii message", s_capturedMessage);
    }

    [Fact]
    public void Log_TranscodesUtf16ToUtf8AtTheBoundary()
    {
        // Non-ASCII forces real transcoding: these code points are 2-4 UTF-8
        // bytes each, so a round-trip equality proves UTF-16 → UTF-8 → UTF-16.
        const string scope = "guest.plugin-ünïcode";
        const string message = "héllo ✓ 日本語 🦀";

        PolyplugHost.Log(LogLevel.Warn, scope, message);

        Assert.Equal(1, s_callCount);
        Assert.Equal((uint)LogLevel.Warn, s_capturedLevel);
        Assert.Equal(scope, s_capturedScope);
        Assert.Equal(message, s_capturedMessage);
    }

    [Fact]
    public void Log_EmptyStringsProduceLegalNullViews()
    {
        PolyplugHost.Log(LogLevel.Trace, string.Empty, string.Empty);

        Assert.Equal(1, s_callCount);
        Assert.Equal((uint)LogLevel.Trace, s_capturedLevel);
        Assert.True(s_capturedScopePtrNull, "empty scope must cross as a null view");
        Assert.True(s_capturedMessagePtrNull, "empty message must cross as a null view");
        Assert.Equal(string.Empty, s_capturedScope);
        Assert.Equal(string.Empty, s_capturedMessage);
    }

    [Fact]
    public void Log_NoOpBeforeInitStoresHost()
    {
        RuntimeAbiStorage.StoreRuntimeAbi(IntPtr.Zero);

        PolyplugHost.Log(LogLevel.Error, "guest.unit-test", "must not be delivered");

        Assert.Equal(0, s_callCount);
    }

    [Fact]
    public void Log_NoOpWhenHostLogPointerIsNull()
    {
        _host->Log = IntPtr.Zero;

        PolyplugHost.Log(LogLevel.Error, "guest.unit-test", "must not be delivered");

        Assert.Equal(0, s_callCount);
    }
}
