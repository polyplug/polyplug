using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using Polyplug;

internal static class Program
{
    private const ulong TRANSFORMER_CONTRACT_ID = 0x3D53C682F3F5A9EFUL;
    private const ulong REPORTER_CONTRACT_ID = 0x81D41D43E511D297UL;

    private const ulong FNV_OFFSET = 0xCBF29CE484222325UL;
    private const ulong FNV_PRIME = 0x00000100000001B3UL;

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError AbiFn(IntPtr argsPtr, IntPtr outPtr);

    private sealed class DiscoveredBundle
    {
        public string Path { get; init; } = "";
        public string BundleName { get; init; } = "";
        public List<string> Provides { get; init; } = new();
    }

    private static int Main()
    {
        try
        {
            string pluginDir = ResolvePluginPath();
            ConfigureNativeResolver(pluginDir);
            Console.Error.WriteLine($"plugin directory: {pluginDir}");

            Runtime runtime = Runtime.Builder().Init();

            runtime.RegisterNativeLoader();
            runtime.RegisterDotnetLoader();
            runtime.RegisterPythonLoader();
            runtime.RegisterLuaLoader();
            runtime.RegisterJsLoader();

            List<DiscoveredBundle> bundles = ScanPluginDir(pluginDir);
            if (bundles.Count == 0)
            {
                Console.Error.WriteLine($"no plugins found in {pluginDir}. Run examples/build_all.sh first.");
                return 1;
            }

            Console.Error.WriteLine($"discovered {bundles.Count} bundles");

            foreach (DiscoveredBundle b in bundles)
            {
                runtime.LoadBundle(b.Path);
                Console.Error.WriteLine($"  loaded: {b.BundleName}");
            }

            foreach (DiscoveredBundle b in bundles)
            {
                ulong contractId = 0;
                string fnName = "";

                if (b.Provides.Contains("data.Transformer"))
                {
                    contractId = TRANSFORMER_CONTRACT_ID;
                    fnName = "transform";
                }
                else if (b.Provides.Contains("data.Reporter"))
                {
                    contractId = REPORTER_CONTRACT_ID;
                    fnName = "report";
                }
                else
                {
                    continue;
                }

                CallGuest(runtime, b.BundleName, contractId, fnName);
            }

            return 0;
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine($"error: {ex.Message}");
            return 1;
        }
    }

    private static unsafe void CallGuest(Runtime runtime, string bundleName, ulong contractId, string fnName)
    {
        ulong bundleId = BundleId(bundleName);
        ulong handle = runtime.FindByBundle(bundleId, contractId, 0u);
        if (handle == ulong.MaxValue)
        {
            throw new InvalidOperationException($"plugin not found: {bundleName}");
        }

        PluginGuard guard = runtime.ResolvePlugin(handle);
        try
        {
            IntPtr vtablePtr = guard.GetVTable();
            if (vtablePtr == IntPtr.Zero)
            {
                throw new InvalidOperationException($"null vtable: {bundleName}");
            }

            PluginVTable vtable = Marshal.PtrToStructure<PluginVTable>(vtablePtr);
            if (vtable.FunctionsPtr == IntPtr.Zero || vtable.FunctionCount == 0u)
            {
                throw new InvalidOperationException($"no functions: {bundleName}");
            }

            byte[] inputBytes = Encoding.UTF8.GetBytes("hello");
            GCHandle inputPin = GCHandle.Alloc(inputBytes, GCHandleType.Pinned);
            try
            {
                Polyplug.StringView inputSv = new Polyplug.StringView
                {
                    Ptr = inputPin.AddrOfPinnedObject(),
                    Len = (ulong)inputBytes.Length,
                };

                Polyplug.StringView outputSv = Polyplug.StringView.Empty;

                IntPtr fnPtr = Marshal.ReadIntPtr(vtable.FunctionsPtr, 0);
                AbiFn func = Marshal.GetDelegateForFunctionPointer<AbiFn>(fnPtr);
                AbiError err = func((IntPtr)(&inputSv), (IntPtr)(&outputSv));

                if (err.Code != AbiConstants.ABI_OK)
                {
                    throw new InvalidOperationException($"call failed for {bundleName}: code {err.Code}");
                }

                string result = outputSv.ToString();
                string label = $"[{bundleName}]";
                Console.WriteLine($"{label,-30} {fnName}(\"hello\") = \"{result}\"");
            }
            finally
            {
                if (inputPin.IsAllocated)
                {
                    inputPin.Free();
                }
            }
        }
        finally
        {
            guard.Dispose();
        }
    }

    private static ulong BundleId(string name)
    {
        byte[] data = Encoding.UTF8.GetBytes(name);
        ulong hash = FNV_OFFSET;
        foreach (byte b in data)
        {
            hash ^= b;
            hash *= FNV_PRIME;
        }

        return hash;
    }

    private static string ResolvePluginPath()
    {
        string? envPath = Environment.GetEnvironmentVariable("POLYPLUG_PLUGIN_PATH");
        if (!string.IsNullOrWhiteSpace(envPath) && Directory.Exists(envPath))
        {
            return envPath;
        }

        string[] seeds = { AppContext.BaseDirectory, Directory.GetCurrentDirectory() };
        foreach (string seed in seeds)
        {
            string dir = Path.GetFullPath(seed);
            for (int i = 0; i < 8; i++)
            {
                string pluginsPath = Path.Combine(dir, "examples", "plugins");
                if (Directory.Exists(pluginsPath))
                {
                    return pluginsPath;
                }
                DirectoryInfo? parent = Directory.GetParent(dir);
                if (parent == null)
                {
                    break;
                }
                dir = parent.FullName;
            }
        }

        return Path.Combine(Directory.GetCurrentDirectory(), "examples", "plugins");
    }

    private static List<DiscoveredBundle> ScanPluginDir(string dir)
    {
        List<DiscoveredBundle> bundles = new();
        if (!Directory.Exists(dir))
        {
            return bundles;
        }

        foreach (string entry in Directory.GetDirectories(dir))
        {
            string manifestPath = Path.Combine(entry, "manifest.toml");
            if (!File.Exists(manifestPath))
            {
                continue;
            }

            string content = File.ReadAllText(manifestPath);
            string bundleName = "";
            List<string> provides = new();

            foreach (string line in content.Split('\n'))
            {
                string trimmed = line.Trim();
                if (trimmed.StartsWith("bundle_name"))
                {
                    int eq = trimmed.IndexOf('=');
                    if (eq >= 0)
                    {
                        string val = trimmed.Substring(eq + 1).Trim().Trim('"');
                        bundleName = val;
                    }
                }
                else if (trimmed.StartsWith("provides"))
                {
                    int start = trimmed.IndexOf('[');
                    int end = trimmed.IndexOf(']');
                    if (start >= 0 && end >= 0)
                    {
                        string items = trimmed.Substring(start + 1, end - start - 1);
                        foreach (string item in items.Split(','))
                        {
                            string contract = item.Trim().Trim('"');
                            if (!string.IsNullOrEmpty(contract))
                            {
                                provides.Add(contract);
                            }
                        }
                    }
                }
            }

            if (!string.IsNullOrEmpty(bundleName))
            {
                bundles.Add(new DiscoveredBundle
                {
                    Path = entry,
                    BundleName = bundleName,
                    Provides = provides,
                });
            }
        }

        bundles.Sort((a, b) => string.Compare(a.BundleName, b.BundleName, StringComparison.Ordinal));
        return bundles;
    }

    private static void ConfigureNativeResolver(string pluginDir)
    {
        string repoRoot = FindRepoRoot();

        DllImportResolver resolver = (string name, System.Reflection.Assembly assembly, DllImportSearchPath? path) =>
        {
            string[] knownLibs = { "polyplug", "polyplug_native", "polyplug_dotnet", "polyplug_python", "polyplug_lua", "polyplug_js" };
            bool isKnown = false;
            foreach (string known in knownLibs)
            {
                if (string.Equals(name, known, StringComparison.Ordinal))
                {
                    isKnown = true;
                    break;
                }
            }

            if (!isKnown)
            {
                return IntPtr.Zero;
            }

            string? envSoPath = Environment.GetEnvironmentVariable("POLYPLUG_SO");
            if (string.Equals(name, "polyplug", StringComparison.Ordinal)
                && !string.IsNullOrWhiteSpace(envSoPath) && File.Exists(envSoPath))
            {
                return NativeLibrary.Load(envSoPath);
            }

            string soName = $"lib{name}.so";
            string debugPath = Path.Combine(repoRoot, "target", "debug", soName);
            if (File.Exists(debugPath))
            {
                return NativeLibrary.Load(debugPath);
            }

            return NativeLibrary.Load(soName);
        };

        NativeLibrary.SetDllImportResolver(typeof(Runtime).Assembly, resolver);
        NativeLibrary.SetDllImportResolver(System.Reflection.Assembly.GetExecutingAssembly(), resolver);
    }

    private static string FindRepoRoot()
    {
        string[] seeds = { AppContext.BaseDirectory, Directory.GetCurrentDirectory() };
        foreach (string seed in seeds)
        {
            string dir = Path.GetFullPath(seed);
            for (int i = 0; i < 8; i++)
            {
                string examplesPath = Path.Combine(dir, "examples", "guests");
                if (Directory.Exists(examplesPath))
                {
                    return dir;
                }
                DirectoryInfo? parent = Directory.GetParent(dir);
                if (parent == null)
                {
                    break;
                }
                dir = parent.FullName;
            }
        }

        return Directory.GetCurrentDirectory();
    }
}
