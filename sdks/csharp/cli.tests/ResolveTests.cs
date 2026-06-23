using System;
using System.IO;
using System.Runtime.InteropServices;
using Polyplug.Cli;
using Xunit;

namespace Polyplug.Cli.Tests
{
    public sealed class ResolveTests
    {
        [Fact]
        public void DetectRid_ReturnsLinuxX64_OnLinuxX64()
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.Linux)
                || RuntimeInformation.OSArchitecture != Architecture.X64)
            {
                return;
            }

            string rid = Resolve.DetectRid();
            Assert.Equal("linux-x64", rid);
        }

        [Fact]
        public void DetectRid_ReturnsMacosArm64_OnMacosArm64()
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.OSX)
                || RuntimeInformation.OSArchitecture != Architecture.Arm64)
            {
                return;
            }

            string rid = Resolve.DetectRid();
            Assert.Equal("macos-arm64", rid);
        }

        [Fact]
        public void DetectRid_ReturnsWinX64_OnWindowsX64()
        {
            if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                || RuntimeInformation.OSArchitecture != Architecture.X64)
            {
                return;
            }

            string rid = Resolve.DetectRid();
            Assert.Equal("win-x64", rid);
        }

        [Fact]
        public void BinaryPath_LinuxX64_ReturnsCorrectRelativePath()
        {
            string baseDir = "/fake/tool/dir";
            string result = Resolve.BinaryPathForRid(baseDir, "linux-x64", isWindows: false);
            string expected = Path.Combine(baseDir, "bin-embed", "linux-x64", "polyplugc");
            Assert.Equal(expected, result);
        }

        [Fact]
        public void BinaryPath_MacosArm64_ReturnsCorrectRelativePath()
        {
            string baseDir = "/fake/tool/dir";
            string result = Resolve.BinaryPathForRid(baseDir, "macos-arm64", isWindows: false);
            string expected = Path.Combine(baseDir, "bin-embed", "macos-arm64", "polyplugc");
            Assert.Equal(expected, result);
        }

        [Fact]
        public void BinaryPath_WinX64_ReturnsExeSuffix()
        {
            string baseDir = @"C:\fake\tool\dir";
            string result = Resolve.BinaryPathForRid(baseDir, "win-x64", isWindows: true);
            string expected = Path.Combine(baseDir, "bin-embed", "win-x64", "polyplugc.exe");
            Assert.Equal(expected, result);
        }

        [Fact]
        public void BinaryPath_CurrentPlatform_PathContainsBinEmbed()
        {
            string baseDir = AppContext.BaseDirectory;
            string result = Resolve.BinaryPath(baseDir);
            Assert.Contains("bin-embed", result);
        }

        [Fact]
        public void DetectRid_ThrowsPlatformNotSupportedException_OnUnsupportedPlatform()
        {
            // This test only runs on an unsupported combination (e.g. linux-arm64, macos-x64).
            bool isLinuxX64 = RuntimeInformation.IsOSPlatform(OSPlatform.Linux)
                && RuntimeInformation.OSArchitecture == Architecture.X64;
            bool isMacArm64 = RuntimeInformation.IsOSPlatform(OSPlatform.OSX)
                && RuntimeInformation.OSArchitecture == Architecture.Arm64;
            bool isWinX64 = RuntimeInformation.IsOSPlatform(OSPlatform.Windows)
                && RuntimeInformation.OSArchitecture == Architecture.X64;

            if (isLinuxX64 || isMacArm64 || isWinX64)
            {
                // Current platform is supported — test the helper directly with a fake combo.
                Assert.Throws<PlatformNotSupportedException>(
                    () => Resolve.DetectRidForPlatform(
                        isLinux: false,
                        linuxArch: Architecture.X64,
                        isOsx: false,
                        osxArch: Architecture.Arm64,
                        isWindows: false,
                        winArch: Architecture.X64,
                        detectedOs: "freebsd",
                        detectedArch: "arm64"));
            }
            else
            {
                Assert.Throws<PlatformNotSupportedException>(() => Resolve.DetectRid());
            }
        }
    }
}
