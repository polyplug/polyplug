using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

using Polyplug.Host;

namespace Polyplug.Loaders.Lua;

/// <summary>
/// Extension methods for registering the Lua loader.
/// </summary>
public static partial class LuaLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_lua";


    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_lua_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugLuaLoaderCreate();

    /// <summary>
    /// Registers the Lua loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterLuaLoader(this Runtime runtime)
    {
        if (runtime is null)
        {
            throw new ArgumentNullException(nameof(runtime));
        }

        nint loaderPtr = PolyplugLuaLoaderCreate();
        if (loaderPtr == nint.Zero)
        {
            throw new InvalidOperationException("polyplug: lua loader create failed");
        }

        uint err = runtime.RegisterLoader(loaderPtr);
        if (err != 0u)
        {
            Runtime.ThrowLastError($"polyplug: lua loader register failed: {err}");
        }
    }
}