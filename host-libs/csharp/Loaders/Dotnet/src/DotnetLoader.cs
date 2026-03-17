using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Loaders;

[StructLayout(LayoutKind.Sequential)]
internal struct DotnetConfig
{
    public StringView MinFramework;
}

public static class DotnetLoaderExtensions
{
    [DllImport("polyplug_dotnet", EntryPoint = "polyplug_dotnet_loader_create")]
    private static extern IntPtr CreateLoader(IntPtr cfgPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
    private static extern uint RegisterLoader(IntPtr rt, IntPtr loader);

    public static void RegisterDotnetLoader(this Runtime runtime, string minFramework = "10.0")
    {
        StringView
        byte[] bytes = Encoding.UTF8.GetBytes(minFramework);
        var stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try {
            DotnetConfig cfg = new DotnetConfig {
                MinFrameworkPtr = stringHandle.AddrOfPinnedObject(),
                MinFrameworkLen = (UIntPtr)bytes.Length
            };
            var cfgHandle = GCHandle.Alloc(cfg, GCHandleType.Pinned);
            try {
                IntPtr loader = CreateLoader(cfgHandle.AddrOfPinnedObject());
                if (loader == IntPtr.Zero)
                    throw new InvalidOperationException("polyplug: dotnet loader create failed");
                uint err = RegisterLoader(runtime.Handle, loader);
                if (err != 0)
                    throw new InvalidOperationException($"polyplug: dotnet loader register failed ({err})");
            } finally {
                cfgHandle.Free();
            }
        } finally {
            stringHandle.Free();
        }
    }
}
