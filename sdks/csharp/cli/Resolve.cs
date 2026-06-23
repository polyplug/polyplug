using System;
using System.IO;
using System.Runtime.InteropServices;

namespace Polyplug.Cli
{
    internal static class Resolve
    {
        /// <summary>
        /// Returns the path to the embedded polyplugc binary for the current RID,
        /// relative to <paramref name="baseDirectory"/>.
        /// Throws <see cref="PlatformNotSupportedException"/> when no binary is bundled
        /// for the detected platform/architecture combination.
        /// </summary>
        internal static string BinaryPath(string baseDirectory)
        {
            string rid = DetectRid();
            bool isWindows = RuntimeInformation.IsOSPlatform(OSPlatform.Windows);
            return BinaryPathForRid(baseDirectory, rid, isWindows);
        }

        /// <summary>
        /// Returns the path to the embedded polyplugc binary for the specified RID and OS flag.
        /// Pure function; directly unit-testable without platform detection.
        /// </summary>
        internal static string BinaryPathForRid(string baseDirectory, string rid, bool isWindows)
        {
            string fileName = isWindows ? "polyplugc.exe" : "polyplugc";
            return Path.Combine(baseDirectory, "bin-embed", rid, fileName);
        }

        /// <summary>
        /// Returns the RID string for the current platform.
        /// Throws <see cref="PlatformNotSupportedException"/> for unsupported combinations.
        /// </summary>
        internal static string DetectRid()
        {
            return DetectRidForPlatform(
                isLinux: RuntimeInformation.IsOSPlatform(OSPlatform.Linux),
                linuxArch: RuntimeInformation.OSArchitecture,
                isOsx: RuntimeInformation.IsOSPlatform(OSPlatform.OSX),
                osxArch: RuntimeInformation.OSArchitecture,
                isWindows: RuntimeInformation.IsOSPlatform(OSPlatform.Windows),
                winArch: RuntimeInformation.OSArchitecture,
                detectedOs: RuntimeInformation.IsOSPlatform(OSPlatform.Windows) ? "windows"
                    : RuntimeInformation.IsOSPlatform(OSPlatform.OSX) ? "macos"
                    : "linux",
                detectedArch: RuntimeInformation.OSArchitecture.ToString().ToLowerInvariant());
        }

        /// <summary>
        /// Platform-injectable RID detection. Pure function; directly unit-testable.
        /// </summary>
        internal static string DetectRidForPlatform(
            bool isLinux,
            Architecture linuxArch,
            bool isOsx,
            Architecture osxArch,
            bool isWindows,
            Architecture winArch,
            string detectedOs,
            string detectedArch)
        {
            if (isLinux && linuxArch == Architecture.X64)
            {
                return "linux-x64";
            }

            if (isOsx && osxArch == Architecture.Arm64)
            {
                return "macos-arm64";
            }

            if (isWindows && winArch == Architecture.X64)
            {
                return "win-x64";
            }

            throw new PlatformNotSupportedException(
                $"No bundled polyplugc binary for {detectedOs}-{detectedArch}. "
                + "Supported platforms: linux-x64, macos-arm64, win-x64. "
                + "Install from source: cargo install polyplugc, "
                + "or download from https://github.com/polyplug/polyplug/releases");
        }
    }
}
