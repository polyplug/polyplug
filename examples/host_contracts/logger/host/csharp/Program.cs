using System;
using System.IO;
using Polyplug.Host;
using Polyplug.Abi;
using Polyplug.Generated;

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

        if (string.IsNullOrEmpty(pluginPath))
        {
            var dir = Directory.GetCurrentDirectory();
            var candidates = new[] {
                Path.Combine(dir, "examples", "host_contracts", "logger", "plugins"),
                Path.Combine(dir, "..", "..", "..", "..", "plugins"),
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

        var rt = new RuntimeBuilder()
            .PluginDir(pluginPath)
            .Build();

        var vtable = VTableFactories.CreateHostLoggerVTable(new ConsoleLogger());
        rt.RegisterHostContract(HostLoggerConstants.IHOSTLOGGER_CONTRACT_ID, vtable);

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

        Console.WriteLine("\n=== Logger Host (C#) ===\n");

        var inputStr = "hello world";
        Console.WriteLine($"Input: \"{inputStr}\"\n");

        if (ExampleWorkerContractCaller.Create(rt) is { } worker)
        {
            using (worker)
            using (var input = new PinnedStringView(inputStr))
            {
                var result = worker.DoWork(input.View);
                Console.WriteLine($"[host] do_work(\"{inputStr}\") = \"{StringHelpers.ToString(result)}\"");
            }
        }

        Console.WriteLine("\ndone.");
    }
}

class ConsoleLogger : IHostLogger
{
    public void Log(string message)
    {
        Console.WriteLine($"[PLUGIN LOG] {message}");
    }
}