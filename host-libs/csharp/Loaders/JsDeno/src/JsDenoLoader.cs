using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Polyplug.Loaders;

/// <summary>
/// Extension methods for registering the JavaScript (Deno) loader.
/// </summary>
public static partial class JsDenoLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_js_deno";

    [StructLayout(LayoutKind.Sequential)]
    private struct JsDenoLoaderConfig
    {
        public byte Reserved;
    }

    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_js_deno_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugJsDenoLoaderCreate(nint cfgPtr);

    /// <summary>
    /// Registers the JavaScript (Deno) loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterJsDenoLoader(this Runtime runtime)
    {
        if (runtime is null)
        {
            throw new ArgumentNullException(nameof(runtime));
        }

        JsDenoLoaderConfig cfg = new JsDenoLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<JsDenoLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = PolyplugJsDenoLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: js_deno loader create failed");
            }

            uint err = runtime.RegisterLoader(loaderPtr);
            if (err != 0u)
            {
                Runtime.ThrowLastError($"polyplug: js_deno loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }
}