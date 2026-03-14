using System;
using System.Runtime.InteropServices;

namespace Polyplug.Loaders;

public static class NativeLoaderExtensions
{
    [DllImport("polyplug_native", EntryPoint = "polyplug_native_loader_create")]
    private static extern IntPtr CreateLoader(IntPtr cfgPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
    private static extern uint RegisterLoader(IntPtr rt, IntPtr loader);

    public static void RegisterNativeLoader(this Runtime runtime)
    {
        byte cfg = 0;
        var handle = GCHandle.Alloc(cfg, GCHandleType.Pinned);
        try {
            IntPtr loader = CreateLoader(handle.AddrOfPinnedObject());
            if (loader == IntPtr.Zero)
                throw new InvalidOperationException("polyplug: native loader create failed");
            uint err = RegisterLoader(runtime.Handle, loader);
            if (err != 0)
                throw new InvalidOperationException($"polyplug: register loader failed ({err})");
        } finally {
            handle.Free();
        }
    }
}
