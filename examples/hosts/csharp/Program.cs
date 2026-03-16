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
        var pluginPath = Environment.GetEnvironmentVariable("POLYPLUG_PLUGIN_PATH") 
            ?? Path.Combine(Directory.GetCurrentDirectory(), "examples", "plugins");

        Console.Error.WriteLine($"loading plugins from: {pluginPath}\n");

        var rt = Runtime.Builder()
            .PluginDir(pluginPath)
            .Init();

        var bundles = Scanner.ScanDir(pluginPath);
        if (bundles.Count == 0)
        {
            throw new Exception($"no plugins found in {pluginPath}");
        }

        Console.Error.WriteLine($"discovered {bundles.Count} bundles\n");

        foreach (var bundle in bundles)
        {
            rt.LoadBundle(bundle.Item1);
            Console.Error.WriteLine($"  loaded: {bundle.Item2.BundleName}");
        }

        Console.WriteLine("\n=== Pipeline Host (C#) ===\n");
        Console.WriteLine("C# host loaded all plugins successfully!");
        Console.WriteLine("\ndone.");
    }
}
