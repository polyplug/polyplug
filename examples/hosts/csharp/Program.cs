using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.RegularExpressions;
using Polyplug;

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

        var bundleInfos = bundles.Select(bundleDir => {
            rt.LoadBundle(bundleDir);
            var manifestPath = Path.Combine(bundleDir, "manifest.toml");
            var manifest = File.ReadAllText(manifestPath);
            var nameMatch = Regex.Match(manifest, @"bundle_name\s*=\s*""([^""]+)""");
            var providesMatch = Regex.Match(manifest, @"provides\s*=\s*\[([^\]]+)\]");
            var name = nameMatch.Success ? nameMatch.Groups[1].Value : "unknown";
            var provides = providesMatch.Success 
                ? Regex.Matches(providesMatch.Groups[1].Value, @"""([^""]+)""")
                    .Cast<Match>().Select(m => m.Groups[1].Value).ToList()
                : new System.Collections.Generic.List<string>();
            return new { Dir = bundleDir, Name = name, Provides = provides };
        }).ToList();

        foreach (var bundle in bundleInfos)
        {
            Console.Error.WriteLine($"  loaded: {bundle.Name}");
        }

        Console.WriteLine("\n=== Pipeline Host (C#) ===\n");

        var inputStr = "name,value,42";
        Console.WriteLine($"Input: \"{inputStr}\"\n");

        foreach (var bundle in bundleInfos)
        {
            var bid = ContractId.BundleId(bundle.Name);

            foreach (var contract in bundle.Provides)
            {
                var parts = contract.Split('@');
                if (parts.Length != 2) continue;
                var contractName = parts[0];
                var versionParts = parts[1].Split('.');
                var major = uint.Parse(versionParts[0].Split('-')[0]);

                var cid = ContractId.Compute(contractName, major);
                var handle = rt.FindByBundle(bid, cid, 0);

                if (handle == ulong.MaxValue) continue;

                using var guard = rt.ResolvePlugin(handle);

                if (contractName == "pipeline.Decoder")
                {
                    var result = guard.CallFunction(0, inputStr);
                    Console.WriteLine($"[{bundle.Name}] decode(\"{inputStr}\") = \"{result}\"");
                }
                else if (contractName == "data.Transformer")
                {
                    var decoded = $"DECODED:{inputStr.Replace(',', '|')}";
                    var result = guard.CallFunction(0, decoded);
                    Console.WriteLine($"[{bundle.Name}] transform(\"{decoded}\") = \"{result}\"");
                }
                else if (contractName == "pipeline.Encoder")
                {
                    var transformed = "TRANSFORMED:NAME|value (transformed)|43";
                    var result = guard.CallFunction(0, transformed);
                    Console.WriteLine($"[{bundle.Name}] encode(\"{transformed}\") = \"{result}\"");
                }
                else if (contractName == "data.Reporter")
                {
                    var transformed = "TRANSFORMED:NAME|value (transformed)|43";
                    var result = guard.CallFunction(0, transformed);
                    Console.WriteLine($"[{bundle.Name}] report(\"{transformed}\") = \"{result}\"");
                }
                else if (contractName == "pipeline.Validator")
                {
                    var decoded = $"DECODED:{inputStr.Replace(',', '|')}";
                    var result = guard.CallFunction(0, decoded);
                    Console.WriteLine($"[{bundle.Name}] validate(\"{decoded}\") = \"{result}\"");
                }
            }
        }

        Console.WriteLine("\ndone.");
    }
}
