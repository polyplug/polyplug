using System;
using System.IO;
using System.Linq;
using Polyplug;

class Program
{
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
        
        // Try to find plugins relative to workspace
        if (string.IsNullOrEmpty(pluginPath))
        {
            var dir = Directory.GetCurrentDirectory();
            // Try various paths
            var candidates = new[] {
                Path.Combine(dir, "examples", "plugins"),
                Path.Combine(dir, "..", "..", "..", "examples", "plugins"),
                Path.Combine(dir, "..", "..", "plugins"),
                "/mnt/data/Projects/Utils/polyplug/examples/plugins"
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

        var rt = Runtime.Builder()
            .PluginDir(pluginPath)
            .Init();

        // Scan for manifest.toml files
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
            var manifestPath = Path.Combine(bundleDir, "manifest.toml");
            var manifest = File.ReadAllText(manifestPath);
            var name = manifest.Split('\n').FirstOrDefault(line => line.StartsWith("bundle_name"))?
                .Split('=').LastOrDefault()?.Trim().Trim('"') ?? "unknown";
            Console.Error.WriteLine($"  loaded: {name}");
        }

        Console.WriteLine("\n=== Pipeline Host (C#) ===\n");
        Console.WriteLine("C# host loaded all plugins successfully!");
        Console.WriteLine("\ndone.");
    }
}
