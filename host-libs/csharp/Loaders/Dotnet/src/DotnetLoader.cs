using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Loaders;

/// <summary>
/// Extension methods for registering the .NET loader.
/// </summary>
public static partial class DotnetLoaderExtensions
{
    private const string NativeLoaderLib = "polyplug_dotnet";

    [StructLayout(LayoutKind.Sequential)]
    private struct DotnetLoaderConfig
    {
        public StringView MinFramework;
    }

    [LibraryImport(NativeLoaderLib, EntryPoint = "polyplug_dotnet_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    private static partial nint PolyplugDotnetLoaderCreate(nint cfgPtr);

    /// <summary>
    /// Registers the .NET loader with the runtime.
    /// </summary>
    /// <param name="runtime">The runtime to register the loader with.</param>
    /// <param name="minFramework">Minimum .NET framework version required (default: "10.0").</param>
    /// <exception cref="InvalidOperationException">Thrown if loader creation or registration fails.</exception>
    public static void RegisterDotnetLoader(this Runtime runtime, string minFramework = "10.0")
    {
        ArgumentNullException.ThrowIfNull(runtime);

        byte[] bytes = Encoding.UTF8.GetBytes(minFramework);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try
        {
            var cfg = new DotnetLoaderConfig
            {
                MinFramework = new StringView(stringHandle.AddrOfPinnedObject(), (ulong)bytes.Length),
            };
            nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<DotnetLoaderConfig>());
            try
            {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                nint loaderPtr = PolyplugDotnetLoaderCreate(cfgPtr);
                if (loaderPtr == nint.Zero)
                {
                    throw new InvalidOperationException("polyplug: dotnet loader create failed");
                }

                uint err = runtime.RegisterLoader(loaderPtr);
                if (err != 0u)
                {
                    Runtime.ThrowLastError($"polyplug: dotnet loader register failed: {err}");
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