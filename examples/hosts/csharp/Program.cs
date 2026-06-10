using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using Polyplug.Host;
using Polyplug.Guest;
using Polyplug.Abi;
using Polyplug.Generated;
using Polyplug.Loaders.Native;
using Polyplug.Loaders.Python;
using Polyplug.Loaders.Lua;
using Polyplug.Loaders.Js;
using Polyplug.Loaders.Dotnet;

class Program
{
    private static readonly Dictionary<ulong, List<IDisposable>> _instances = new();

    static int Main(string[] args)
    {
        try
        {
            InstallNativeLibraryResolver();
            Run();
            return 0;
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
        }
    }

    /// <summary>
    /// Resolve polyplug core + loader cdylibs from the cargo target directory.
    /// The verify_hosts.sh harness exports POLYPLUG_LIB (core) and
    /// POLYPLUG_NATIVE_LIB (native loader); every other loader cdylib lives
    /// in the same directory. Without this, the default OS loader can pick up a
    /// stale libpolyplug.so left in the assembly output directory.
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

        foreach (Assembly assembly in new[]
        {
            typeof(Runtime).Assembly,
            typeof(NativeLoaderExtensions).Assembly,
            typeof(PythonLoaderExtensions).Assembly,
            typeof(LuaLoaderExtensions).Assembly,
            typeof(JsLoaderExtensions).Assembly,
            typeof(DotnetLoaderExtensions).Assembly,
        })
        {
            NativeLibrary.SetDllImportResolver(assembly, resolver);
        }
    }

    static void Run()
    {
        var pluginPath = Environment.GetEnvironmentVariable("POLYPLUG_PLUGIN_PATH");

        if (string.IsNullOrEmpty(pluginPath))
        {
            var dir = Directory.GetCurrentDirectory();
            var candidates = new[] {
                Path.Combine(dir, "examples", "plugins"),
                Path.Combine(dir, "..", "..", "..", "examples", "plugins"),
                Path.Combine(dir, "..", "..", "plugins"),
            };

            foreach (var candidate in candidates)
            {
                if (Directory.Exists(candidate))
                {
                    pluginPath = candidate;
                    break;
                }
            }
        }

        if (string.IsNullOrEmpty(pluginPath) || !Directory.Exists(pluginPath))
        {
            throw new Exception($"plugins directory not found");
        }

        Console.Error.WriteLine($"loading plugins from: {pluginPath}\n");

        Runtime.OnReload(phase =>
        {
            if (phase.IsPreparing())
            {
                Console.Error.WriteLine($"[HOT-RELOAD] Preparing: {phase.BundleName} (bundle_id=0x{phase.BundleId:X16}, retry {phase.RetryCount})");
                if (_instances.Remove(phase.BundleId, out var instances))
                {
                    foreach (var instance in instances)
                    {
                        instance.Dispose();
                    }
                    Console.Error.WriteLine($"[HOT-RELOAD] Cleared instances for bundle {phase.BundleName}");
                }
            }
            else if (phase.IsReloaded())
            {
                Console.Error.WriteLine($"[HOT-RELOAD] Reloaded: {phase.BundleName} (bundle_id=0x{phase.BundleId:X16})");
            }
            else if (phase.IsFailed())
            {
                Console.Error.WriteLine($"[HOT-RELOAD] Failed: {phase.BundleName} (bundle_id=0x{phase.BundleId:X16}) - {phase.Reason}");
            }
        });

        var rt = new RuntimeBuilder()
            .PluginDir(pluginPath)
            .Build();

        rt.RegisterNativeLoader();
        rt.RegisterPythonLoader();
        rt.RegisterLuaLoader();
        rt.RegisterJsLoader();
        rt.RegisterDotnetLoader();

        var bundles = Directory.GetDirectories(pluginPath)
            .Where(dir => File.Exists(Path.Combine(dir, "manifest.toml")))
            .ToList();

        if (bundles.Count == 0)
        {
            throw new Exception($"no plugins found in {pluginPath}");
        }

        Console.Error.WriteLine($"discovered {bundles.Count} bundles\n");

        foreach (var bundleDir in bundles)
        {
            rt.LoadBundle(bundleDir);
            Console.Error.WriteLine($"  loaded: {Path.GetFileName(bundleDir)}");
        }

        Console.WriteLine("\n=== Pipeline Host (C#) ===\n");

        var inputStr = "name,value,42";
        Console.WriteLine($"Input: \"{inputStr}\"\n");

        // Use generated contract callers - no manifest parsing needed
        if (PipelineDecoderContractCaller.Create(rt) is { } decoder)
        {
            using (decoder)
            using (var input = new PinnedStringView(inputStr))
            {
                var result = decoder.Decode(input.View);
                Console.WriteLine($"[decoder] decode(\"{inputStr}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        var decoded = $"DECODED:{inputStr.Replace(',', '|')}";
        if (DataTransformerContractCaller.Create(rt) is { } transformer)
        {
            using (transformer)
            using (var input = new PinnedStringView(decoded))
            {
                var result = transformer.Transform(input.View);
                Console.WriteLine($"[transformer] transform(\"{decoded}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        var transformed = "TRANSFORMED:NAME|value (transformed)|43";
        if (PipelineEncoderContractCaller.Create(rt) is { } encoder)
        {
            using (encoder)
            using (var input = new PinnedStringView(transformed))
            {
                var result = encoder.Encode(input.View);
                Console.WriteLine($"[encoder] encode(\"{transformed}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        if (DataReporterContractCaller.Create(rt) is { } reporter)
        {
            using (reporter)
            using (var input = new PinnedStringView(transformed))
            {
                var result = reporter.Report(input.View);
                Console.WriteLine($"[reporter] report(\"{transformed}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        if (PipelineValidatorContractCaller.Create(rt) is { } validator)
        {
            using (validator)
            using (var input = new PinnedStringView(decoded))
            {
                var result = validator.Validate(input.View);
                Console.WriteLine($"[validator] validate(\"{decoded}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        // Round-trip micro-benchmark (opt-in via POLYPLUG_BENCH_ITERS): times the full
        // host → runtime → native guest → return path (C# host calling the native
        // decoder plugin and getting a StringView back). Point POLYPLUG_PLUGIN_PATH at
        // native guests only so the resolved decoder is the native cdylib.
        var benchIters = Environment.GetEnvironmentVariable("POLYPLUG_BENCH_ITERS");
        if (benchIters != null && int.TryParse(benchIters, out var iters) && iters > 0
            && PipelineDecoderContractCaller.Create(rt) is { } benchDecoder)
        {
            using (benchDecoder)
            using (var input = new PinnedStringView(inputStr))
            {
                int warmup = Math.Min(iters, 10000);
                for (int i = 0; i < warmup; i++) benchDecoder.Decode(input.View);
                var sw = System.Diagnostics.Stopwatch.StartNew();
                for (int i = 0; i < iters; i++) benchDecoder.Decode(input.View);
                sw.Stop();
                double ns = sw.Elapsed.TotalNanoseconds / iters;
                Console.WriteLine($"ROUNDTRIP_NS={ns:F2} LANG=csharp");
            }
        }

        Console.WriteLine("\ndone.");
    }
}