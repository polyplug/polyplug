using System;
using System.Collections.Generic;

namespace Polyplug;

public sealed class RuntimeBuilder
{
    private readonly List<string> _pluginDirs;

    public RuntimeBuilder()
    {
        _pluginDirs = [];
    }

    public RuntimeBuilder PluginDir(string path)
    {
        if (path is null)
        {
            throw new ArgumentNullException(nameof(path));
        }

        _pluginDirs.Add(path);
        return this;
    }

    public Runtime Build()
    {
        nint handle = NativeMethods.PolyplugRuntimeCreate();
        if (handle == nint.Zero)
        {
            Runtime.ThrowLastError("Failed to create runtime.");
        }

        return new Runtime(handle);
    }
}
