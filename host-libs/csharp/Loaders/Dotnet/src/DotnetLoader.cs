using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Loaders;

[StructLayout(LayoutKind.Sequential)]
internal struct DotnetLoaderConfig
{
    public StringView MinFramework;
}

public static partial class DotnetLoaderExtensions
{
    private const string NativeLoaderDotnetLib = "polyplug_dotnet";

    [LibraryImport(NativeLoaderDotnetLib, EntryPoint = "polyplug_dotnet_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugDotnetLoaderCreate(nint cfgPtr);

    public static void RegisterDotnetLoader(this Runtime runtime, string minFramework = "10.0")
    {
        byte[] bytes = Encoding.UTF8.GetBytes(minFramework);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try
        {
            var cfg = new DotnetLoaderConfig()
            {
                MinFramework = new StringView(stringHandle.AddrOfPinnedObject(), (ulong)bytes.Length)
            };
            nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<DotnetLoaderConfig>());
            try
            {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                nint loader = PolyplugDotnetLoaderCreate(cfgPtr);
                if (loader == nint.Zero)
                {
                    throw new InvalidOperationException("polyplug: dotnet loader create failed");
                }

                uint err = runtime.RegisterLoader(loader);
                if (err != 0)
                {
                    throw new InvalidOperationException($"polyplug: dotnet loader register failed ({err})");
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
