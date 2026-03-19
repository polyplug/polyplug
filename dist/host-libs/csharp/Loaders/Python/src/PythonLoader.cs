using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Loaders;

/// <summary>
/// Extension methods for registering the Python loader.
/// </summary>
public static partial class PythonLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_python";

    [StructLayout(LayoutKind.Sequential)]
    private struct PythonLoaderConfig
    {
        public nint MinVersionPtr;
        public nuint MinVersionLen;
    }

    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_python_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugPythonLoaderCreate(nint cfgPtr);

    /// <summary>
    /// Registers the Python loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <param name="minVersion">Minimum Python version required (default: "3.11").</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterPythonLoader(this Runtime runtime, string minVersion = "3.11")
    {
        if (runtime is null)
        {
            throw new ArgumentNullException(nameof(runtime));
        }

        byte[] bytes = Encoding.UTF8.GetBytes(minVersion);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try
        {
            PythonLoaderConfig cfg = new PythonLoaderConfig
            {
                MinVersionPtr = stringHandle.AddrOfPinnedObject(),
                MinVersionLen = (nuint)bytes.Length,
            };
            nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<PythonLoaderConfig>());
            try
            {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                nint loaderPtr = PolyplugPythonLoaderCreate(cfgPtr);
                if (loaderPtr == nint.Zero)
                {
                    throw new InvalidOperationException("polyplug: python loader create failed");
                }

                uint err = runtime.RegisterLoader(loaderPtr);
                if (err != 0u)
                {
                    Runtime.ThrowLastError($"polyplug: python loader register failed: {err}");
                }
            }
            finally
            {
                Marshal.FreeHGlobal(cfgPtr);
            }
        }
        finally
        {
            stringHandle.Free();
        }
    }
}