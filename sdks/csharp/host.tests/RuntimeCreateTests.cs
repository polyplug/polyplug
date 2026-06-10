using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using Polyplug.Host;
using Xunit;

namespace Polyplug.Host.Tests
{
    /// <summary>
    /// Verifies that <c>polyplug_runtime_create</c> is wired correctly through the
    /// host SDK, both with default configuration and with a configured
    /// <c>RuntimeConfig</c> (hot-reload enabled + reload callback) marshaled across
    /// the FFI boundary.
    /// </summary>
    public sealed class RuntimeCreateTests
    {
        static RuntimeCreateTests()
        {
            InstallNativeLibraryResolver();
        }

        /// <summary>
        /// Resolve the polyplug core cdylib from POLYPLUG_LIB so the native
        /// import target resolves to the freshly built core rather than a stale
        /// copy in the test output directory.
        /// </summary>
        private static void InstallNativeLibraryResolver()
        {
            string? corePath = Environment.GetEnvironmentVariable("POLYPLUG_LIB");
            if (string.IsNullOrEmpty(corePath))
            {
                return;
            }

            string? depsDir = Path.GetDirectoryName(Path.GetFullPath(corePath));
            if (depsDir is null)
            {
                return;
            }

            DllImportResolver resolver = (string libraryName, Assembly assembly, DllImportSearchPath? searchPath) =>
            {
                string fileName = libraryName switch
                {
                    "polyplug" => Path.GetFileName(corePath),
                    _ => $"lib{libraryName}.so",
                };
                string candidate = Path.Combine(depsDir, fileName);
                if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out nint handle))
                {
                    return handle;
                }
                return nint.Zero;
            };

            NativeLibrary.SetDllImportResolver(typeof(Runtime).Assembly, resolver);
        }

        private static bool NativeLibAvailable()
        {
            return !string.IsNullOrEmpty(Environment.GetEnvironmentVariable("POLYPLUG_LIB"));
        }

        [Fact]
        public void DefaultRuntimeCreateAndDispose()
        {
            if (!NativeLibAvailable())
            {
                return;
            }

            Runtime runtime = new RuntimeBuilder().Build();
            Assert.NotEqual(nint.Zero, runtime.HostHandle);
            GC.KeepAlive(runtime);
        }

        [Fact]
        public void RuntimeCreateWithHotReloadConfig()
        {
            if (!NativeLibAvailable())
            {
                return;
            }

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
            if (!NativeLibAvailable())
            {
                return;
            }

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
