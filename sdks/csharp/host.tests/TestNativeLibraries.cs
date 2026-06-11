using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using Polyplug.Host;
using Polyplug.Loaders.Native;

namespace Polyplug.Host.Tests
{
    /// <summary>
    /// Shared native-library resolution for the host.tests suite.
    ///
    /// <see cref="NativeLibrary.SetDllImportResolver"/> may be installed at most
    /// ONCE per assembly, and xunit runs test classes in parallel — so the
    /// resolver install lives here behind a <see cref="Lazy{T}"/> and every test
    /// class calls <see cref="EnsureInstalled"/> from its static constructor.
    ///
    /// There is NO silent skip anywhere in this resolution: when the core cdylib
    /// cannot be found the suite fails loudly with instructions — a test run that
    /// quietly no-ops hides exactly the never-run breakage class these tests
    /// exist to catch.
    /// </summary>
    internal static class TestNativeLibraries
    {
        private static readonly Lazy<bool> Installer = new(Install, isThreadSafe: true);

        /// <summary>Repo root (the directory containing the workspace Cargo.toml).</summary>
        internal static string RepoRoot
        {
            get
            {
                DirectoryInfo? dir = new DirectoryInfo(AppContext.BaseDirectory);
                while (dir is not null && !File.Exists(Path.Combine(dir.FullName, "Cargo.toml")))
                {
                    dir = dir.Parent;
                }

                if (dir is null)
                {
                    throw new InvalidOperationException(
                        "workspace Cargo.toml not found above " + AppContext.BaseDirectory +
                        " — host.tests must run from a checkout of the polyplug repo.");
                }

                return dir.FullName;
            }
        }

        /// <summary>Install the resolver exactly once (thread-safe).</summary>
        internal static void EnsureInstalled()
        {
            _ = Installer.Value;
        }

        /// <summary>
        /// Platform cdylib file name for a cargo package: <c>&lt;name&gt;.dll</c> on
        /// Windows (no <c>lib</c> prefix), <c>lib&lt;name&gt;.dylib</c> on macOS,
        /// <c>lib&lt;name&gt;.so</c> on Linux.
        /// </summary>
        internal static string CdylibFileName(string packageName)
        {
            return OperatingSystem.IsWindows()
                ? $"{packageName}.dll"
                : OperatingSystem.IsMacOS()
                    ? $"lib{packageName}.dylib"
                    : $"lib{packageName}.so";
        }

        /// <summary>
        /// Resolve the polyplug core cdylib. POLYPLUG_LIB wins (so CI points the
        /// suite at the freshly built core); otherwise the workspace target
        /// directory is probed relative to the test assembly.
        /// </summary>
        private static string ResolveCoreLibrary()
        {
            string? fromEnv = Environment.GetEnvironmentVariable("POLYPLUG_LIB");
            if (!string.IsNullOrEmpty(fromEnv) && File.Exists(fromEnv))
            {
                return Path.GetFullPath(fromEnv);
            }

            string libFileName = CdylibFileName("polyplug");
            string root = RepoRoot;
            string[] candidates = new[]
            {
                Path.Combine(root, "target", "release", libFileName),
                Path.Combine(root, "target", "release", "deps", libFileName),
                Path.Combine(root, "target", "debug", libFileName),
                Path.Combine(root, "target", "debug", "deps", libFileName),
            };
            foreach (string candidate in candidates)
            {
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }

            throw new InvalidOperationException(
                $"{libFileName} not found. Set POLYPLUG_LIB to the built core cdylib " +
                $"(e.g. `export POLYPLUG_LIB=$PWD/target/release/{libFileName}` after " +
                "`cargo build --release -p polyplug`) or build the workspace so " +
                $"target/{{release,debug}}/{libFileName} exists.");
        }

        /// <summary>
        /// Install a DllImport resolver mapping every polyplug native library
        /// (the core and the loader cdylibs, e.g. <c>polyplug_native</c>) to the
        /// directory the core was resolved from. Installed for both the host SDK
        /// assembly and the native-loader assembly — each assembly resolves its
        /// own LibraryImports.
        /// </summary>
        private static bool Install()
        {
            string corePath = ResolveCoreLibrary();
            string? libDir = Path.GetDirectoryName(corePath);
            if (libDir is null)
            {
                throw new InvalidOperationException(
                    $"resolved core cdylib path has no directory: {corePath}");
            }

            DllImportResolver resolver = (string libraryName, Assembly assembly, DllImportSearchPath? searchPath) =>
            {
                string fileName = libraryName switch
                {
                    "polyplug" => Path.GetFileName(corePath),
                    _ => CdylibFileName(libraryName),
                };
                string candidate = Path.Combine(libDir, fileName);
                if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out nint handle))
                {
                    return handle;
                }
                return nint.Zero;
            };

            NativeLibrary.SetDllImportResolver(typeof(Runtime).Assembly, resolver);
            NativeLibrary.SetDllImportResolver(typeof(NativeLoaderExtensions).Assembly, resolver);
            return true;
        }
    }
}
