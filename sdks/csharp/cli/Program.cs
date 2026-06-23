using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;

namespace Polyplug.Cli
{
    internal static class Program
    {
        internal static int Main(string[] args)
        {
            string baseDirectory = AppContext.BaseDirectory;
            string binaryPath = Resolve.BinaryPath(baseDirectory);

            if (!File.Exists(binaryPath))
            {
                Console.Error.WriteLine(
                    $"polyplugc binary not found at: {binaryPath}{Environment.NewLine}"
                    + "The tool package may be incomplete. "
                    + "Install from source: cargo install polyplugc, "
                    + "or download from https://github.com/polyplug/polyplug/releases");
                return 1;
            }

            EnsureExecutable(binaryPath);

            ProcessStartInfo startInfo = new ProcessStartInfo
            {
                FileName = binaryPath,
                UseShellExecute = false,
            };

            foreach (string arg in args)
            {
                startInfo.ArgumentList.Add(arg);
            }

            Process? process = Process.Start(startInfo);
            if (process is null)
            {
                Console.Error.WriteLine($"Failed to start polyplugc at: {binaryPath}");
                return 1;
            }

            process.WaitForExit();
            return process.ExitCode;
        }

        private static void EnsureExecutable(string path)
        {
            if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            {
                return;
            }

            UnixFileMode current = File.GetUnixFileMode(path);
            UnixFileMode withExec = current
                | UnixFileMode.UserExecute
                | UnixFileMode.GroupExecute
                | UnixFileMode.OtherExecute;

            if (current != withExec)
            {
                File.SetUnixFileMode(path, withExec);
            }
        }
    }
}
