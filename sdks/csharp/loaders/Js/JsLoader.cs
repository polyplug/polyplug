using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

using Polyplug.Host;

namespace Polyplug.Loaders.Js;

/// <summary>
/// Extension methods for registering the JavaScript (QuickJS) loader.
/// </summary>
public static partial class JsLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_js";

    [StructLayout(LayoutKind.Sequential)]
    private struct JsLoaderConfig
    {
        public byte Reserved;
    }

    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_js_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugJsLoaderCreate(nint cfgPtr);

    /// <summary>
    /// Registers the JavaScript (QuickJS) loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterJsLoader(this Runtime runtime)
    {
        if (runtime is null)
        {
            throw new ArgumentNullException(nameof(runtime));
        }

        JsLoaderConfig cfg = new JsLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<JsLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = PolyplugJsLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: js loader create failed");
            }

            uint err = runtime.RegisterLoader(loaderPtr);
            if (err != 0u)
            {
                Runtime.ThrowLastError($"polyplug: js loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }
}