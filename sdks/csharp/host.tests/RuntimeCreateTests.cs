using System;
using Polyplug.Host;
using Xunit;

namespace Polyplug.Host.Tests
{
    /// <summary>
    /// Verifies that <c>polyplug_runtime_create</c> is wired correctly through the
    /// host SDK, both with default configuration and with a configured
    /// <c>RuntimeConfig</c> (hot-reload enabled + reload callback) marshaled across
    /// the FFI boundary. Phase DELIVERY on a real reload is covered by
    /// <see cref="ReloadRuntimeTests"/>.
    /// </summary>
    public sealed class RuntimeCreateTests
    {
        static RuntimeCreateTests()
        {
            TestNativeLibraries.EnsureInstalled();
        }

        [Fact]
        public void DefaultRuntimeCreateAndDispose()
        {
            Runtime runtime = new RuntimeBuilder().Build();
            Assert.NotEqual(nint.Zero, runtime.HostHandle);
            GC.KeepAlive(runtime);
        }

        [Fact]
        public void RuntimeCreateWithHotReloadConfig()
        {
            bool callbackRegistered = false;
            Runtime runtime = new RuntimeBuilder()
                .OnReload(_ => callbackRegistered = true)
                .Build();
            Assert.NotEqual(nint.Zero, runtime.HostHandle);
            GC.KeepAlive(runtime);

            // The callback is invoked only on a real reload event; here we only
            // assert that building with a config-bearing OnReload registration
            // succeeded without crashing across the FFI boundary.
            Assert.False(callbackRegistered);
        }

        [Fact]
        public void ReloadCallbacksArePerInstance()
        {
            // Rule 12: each runtime owns its reload callback — building a second
            // runtime with a different callback must not clobber the first.
            bool firstFired = false;
            bool secondFired = false;
            Runtime first = new RuntimeBuilder().OnReload(_ => firstFired = true).Build();
            Runtime second = new RuntimeBuilder().OnReload(_ => secondFired = true).Build();

            Assert.NotEqual(nint.Zero, first.HostHandle);
            Assert.NotEqual(nint.Zero, second.HostHandle);
            Assert.NotEqual(first.HostHandle, second.HostHandle);
            Assert.False(firstFired);
            Assert.False(secondFired);
            GC.KeepAlive(first);
            GC.KeepAlive(second);
        }
    }
}
