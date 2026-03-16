using System;
using System.IO;
using System.Linq;
using Polyplug;
using Polyplug.Loader;
using Polyplug.Abi;

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

        rt.RegisterNativeLoader();

        var bundles = Scanner.ScanDir(pluginPath);
        if (bundles.Count == 0)
        {
            throw new Exception($"no plugins found in {pluginPath}");
        }

        Console.Error.WriteLine($"discovered {bundles.Count} bundles\n");

        foreach (var (path, manifest) in bundles)
        {
            rt.LoadBundle(path);
            Console.Error.WriteLine($"  loaded: {manifest.BundleName}");
        }

        Console.WriteLine("\n=== Pipeline Host (C#) ===\n");

        foreach (var (_, manifest) in bundles)
        {
            if (manifest.Provides.Any(c => c.StartsWith("pipeline.Decoder")))
            {
                var handle = rt.FindByBundle(manifest.BundleName, "pipeline.Decoder", 1);
                if (handle != IntPtr.Zero)
                {
                    Console.WriteLine($"[{manifest.BundleName}] decoder ready");
                }
            }
        }

        Console.WriteLine("\ndone.");
    }
}
