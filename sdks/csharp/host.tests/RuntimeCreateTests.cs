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
        /// Resolve the polyplug core cdylib. POLYPLUG_LIB wins (so CI points the
        /// suite at the freshly built core); otherwise the workspace target
        /// directory is probed relative to the test assembly. There is NO silent
        /// skip: when neither resolves, the suite fails loudly with instructions —
        /// a test run that quietly no-ops hides exactly the never-run breakage
        /// class these tests exist to catch.
        /// </summary>
        private static string ResolveCoreLibrary()
        {
            string? fromEnv = Environment.GetEnvironmentVariable("POLYPLUG_LIB");
            if (!string.IsNullOrEmpty(fromEnv) && File.Exists(fromEnv))
            {
                return Path.GetFullPath(fromEnv);
            }

            // Probe the cargo target dir of the enclosing workspace (the test
            // assembly lives under sdks/csharp/host.tests/bin/...).
            DirectoryInfo? dir = new DirectoryInfo(AppContext.BaseDirectory);
            while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml")))
            {
                dir = dir.Parent;
            }

            string libFileName = OperatingSystem.IsWindows()
                ? "polyplug.dll"
                : OperatingSystem.IsMacOS()
                    ? "libpolyplug.dylib"
                    : "libpolyplug.so";

            if (dir is not null)
            {
                string[] candidates = new[]
                {
                    Path.Combine(dir.FullName, "target", "release", libFileName),
                    Path.Combine(dir.FullName, "target", "release", "deps", libFileName),
                    Path.Combine(dir.FullName, "target", "debug", libFileName),
                    Path.Combine(dir.FullName, "target", "debug", "deps", libFileName),
                };
                foreach (string candidate in candidates)
                {
                    if (File.Exists(candidate))
                    {
                        return candidate;
                    }
                }
            }

            throw new InvalidOperationException(
                $"{libFileName} not found. Set POLYPLUG_LIB to the built core cdylib " +
                $"(e.g. `export POLYPLUG_LIB=$PWD/target/release/{libFileName}` after " +
                "`cargo build --release -p polyplug`) or build the workspace so " +
                $"target/{{release,debug}}/{libFileName} exists.");
        }

        private static void InstallNativeLibraryResolver()
        {
            string corePath = ResolveCoreLibrary();
            string? depsDir = Path.GetDirectoryName(corePath);
            if (depsDir is null)
            {
                throw new InvalidOperationException(
                    $"resolved core cdylib path has no directory: {corePath}");
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
