using System;
using System.Collections.Generic;
using System.IO;
using Polyplug;
using Polyplug.Guest;
using Polyplug.Generated;

class Program
{
    private static readonly Dictionary<ulong, List<IDisposable>> _instances = new();

    static int Main(string[] args)
    {
        try
        {
            Run();
            return 0;
        }
        catch (Exception e)
        {
            Console.Error.WriteLine($"error: {e.Message}");
            return 1;
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

        Runtime.SetConfig(new RuntimeConfig
        {
            HotReloadMaxRetries = 5,
            HotReloadRetryIntervalMs = 200,
            HotReloadAbortOnMaxRetries = false
        });

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
                Console.WriteLine($"[decoder] decode(\"{inputStr}\") = \"{(string)result}\"");
            }
        }

        var decoded = $"DECODED:{inputStr.Replace(',', '|')}";
        if (DataTransformerContractCaller.Create(rt) is { } transformer)
        {
            using (transformer)
            using (var input = new PinnedStringView(decoded))
            {
                var result = transformer.Transform(input.View);
                Console.WriteLine($"[transformer] transform(\"{decoded}\") = \"{(string)result}\"");
            }
        }

        var transformed = "TRANSFORMED:NAME|value (transformed)|43";
        if (PipelineEncoderContractCaller.Create(rt) is { } encoder)
        {
            using (encoder)
            using (var input = new PinnedStringView(transformed))
            {
                var result = encoder.Encode(input.View);
                Console.WriteLine($"[encoder] encode(\"{transformed}\") = \"{(string)result}\"");
            }
        }

        if (DataReporterContractCaller.Create(rt) is { } reporter)
        {
            using (reporter)
            using (var input = new PinnedStringView(transformed))
            {
                var result = reporter.Report(input.View);
                Console.WriteLine($"[reporter] report(\"{transformed}\") = \"{(string)result}\"");
            }
        }

        if (PipelineValidatorContractCaller.Create(rt) is { } validator)
        {
            using (validator)
            using (var input = new PinnedStringView(decoded))
            {
                var result = validator.Validate(input.View);
                Console.WriteLine($"[validator] validate(\"{decoded}\") = \"{(string)result}\"");
            }
        }

        Console.WriteLine("\ndone.");
    }
}