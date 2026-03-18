using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Polyplug.Loaders;

/// <summary>
/// Extension methods for registering the native library loader.
/// </summary>
public static partial class NativeLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_native";

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeLoaderConfig
    {
        public byte Reserved;
    }

    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_native_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugNativeLoaderCreate(nint cfgPtr);

    /// <summary>
    /// Registers the native library loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterNativeLoader(this Runtime runtime)
    {
        if (runtime is null)
        {
            throw new ArgumentNullException(nameof(runtime));
        }

        NativeLoaderConfig cfg = new NativeLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<NativeLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = PolyplugNativeLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: native loader create failed");
            }

            uint err = runtime.RegisterLoader(loaderPtr);
            if (err != 0u)
            {
                Runtime.ThrowLastError($"polyplug: native loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }
}