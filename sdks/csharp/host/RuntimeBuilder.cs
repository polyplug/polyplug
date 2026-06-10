using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

using Polyplug.Abi;

namespace Polyplug.Host;

/// <summary>
/// Fluent builder for <see cref="Runtime"/> instances.
///
/// Mirrors the Rust <c>RuntimeBuilder</c> semantics: plugin directories added
/// via <see cref="PluginDir"/> are scanned during <see cref="Build"/> and every
/// bundle found (a subdirectory containing <c>manifest.toml</c>) is loaded, in
/// sorted order. Note that bundles requiring a language loader can only load
/// after that loader is registered — hosts that register loaders after
/// <c>Build()</c> must load such bundles explicitly instead of using
/// <see cref="PluginDir"/>.
/// </summary>
public sealed class RuntimeBuilder
{
    private readonly List<string> _pluginDirs;
    private Action<ReloadPhase>? _onReload;

    public RuntimeBuilder()
    {
        _pluginDirs = [];
        _onReload = null;
    }

    /// <summary>Add a directory to scan for plugin bundles during <see cref="Build"/>.</summary>
    public RuntimeBuilder PluginDir(string path)
    {
        if (path is null)
        {
            throw new ArgumentNullException(nameof(path));
        }

        _pluginDirs.Add(path);
        return this;
    }

    /// <summary>
    /// Register a callback invoked on hot-reload phase transitions.
    /// The callback is owned by the built <see cref="Runtime"/> instance
    /// (per-instance, no process-global state) and stays alive until the
    /// runtime is destroyed.
    /// </summary>
    public RuntimeBuilder OnReload(Action<ReloadPhase> callback)
    {
        if (callback is null)
        {
            throw new ArgumentNullException(nameof(callback));
        }

        _onReload = callback;
        return this;
    }

    public Runtime Build()
    {
        Runtime runtime = _onReload is null ? BuildDefault() : BuildWithReloadCallback(_onReload);
        LoadPluginDirs(runtime);
        return runtime;
    }

    private static Runtime BuildDefault()
    {
        nint handle = Runtime.CreateNative();
        if (handle == nint.Zero)
        {
            Runtime.ThrowLastError("Failed to create runtime.");
        }

        return new Runtime(handle, default, default);
    }

    /// <summary>
    /// Create the runtime with a marshaled <see cref="RuntimeConfig"/> carrying
    /// the reload trampoline. The callback state GCHandle travels as
    /// <c>OnReloadUserData</c> (recovered per-invocation by the static
    /// trampoline); ownership of both GCHandles transfers to the Runtime.
    /// The native core copies the config during the call, so the unmanaged
    /// copy is freed once create returns.
    /// </summary>
    private static Runtime BuildWithReloadCallback(Action<ReloadPhase> callback)
    {
        Runtime.ReloadCallbackState state = new Runtime.ReloadCallbackState(callback);
        Runtime.OnReloadTrampoline trampoline = Runtime.OnReloadNative;
        GCHandle stateHandle = GCHandle.Alloc(state);
        GCHandle trampolineHandle = GCHandle.Alloc(trampoline);

        try
        {
            RuntimeConfig config = new RuntimeConfig
            {
                Compatibility = Compatibility.Strict,
                HotReloadEnabled = true,
                OnReload = Marshal.GetFunctionPointerForDelegate(trampoline),
                OnReloadUserData = GCHandle.ToIntPtr(stateHandle),
            };

            nint configPtr = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeConfig>());
            nint handle;
            try
            {
                Marshal.StructureToPtr(config, configPtr, fDeleteOld: false);
                handle = NativeMethods.PolyplugRuntimeCreate(configPtr);
            }
            finally
            {
                Marshal.FreeHGlobal(configPtr);
            }

            if (handle == nint.Zero)
            {
                Runtime.ThrowLastError("Failed to create runtime.");
            }

            return new Runtime(handle, stateHandle, trampolineHandle);
        }
        catch
        {
            // Creation failed before a Runtime took ownership — release here.
            stateHandle.Free();
            trampolineHandle.Free();
            throw;
        }
    }

    /// <summary>
    /// Scan each stored plugin directory for bundle subdirectories (containing
    /// <c>manifest.toml</c>) and load them in sorted order — mirroring the Rust
    /// builder's scan-and-load-at-build semantics.
    /// </summary>
    private void LoadPluginDirs(Runtime runtime)
    {
        foreach (string dir in _pluginDirs)
        {
            if (!Directory.Exists(dir))
            {
                continue;
            }

            string[] bundleDirs = Directory.GetDirectories(dir);
            // System.Array spelled out: `using Polyplug.Abi` brings the ABI `Array` struct into scope.
            System.Array.Sort(bundleDirs, StringComparer.Ordinal);
            foreach (string bundleDir in bundleDirs)
            {
                if (File.Exists(Path.Combine(bundleDir, "manifest.toml")))
                {
                    runtime.LoadBundle(bundleDir);
                }
            }
        }
    }
}
