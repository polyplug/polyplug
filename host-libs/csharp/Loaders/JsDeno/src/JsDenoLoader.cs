using System;
using System.Runtime.InteropServices;

namespace Polyplug.Loaders;

[StructLayout(LayoutKind.Sequential)]
internal struct JsDenoConfig
{
    public byte Reserved;
}

public static class JsDenoLoaderExtensions
{
    [DllImport("polyplug_js_deno", EntryPoint = "polyplug_js_deno_loader_create")]
    private static extern IntPtr CreateLoader(IntPtr cfgPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
    private static extern uint RegisterLoader(IntPtr rt, IntPtr loader);

    public static void RegisterJsDenoLoader(this Runtime runtime)
    {
        JsDenoConfig cfg = new JsDenoConfig { Reserved = 0 };
        var handle = GCHandle.Alloc(cfg, GCHandleType.Pinned);
        try {
            IntPtr loader = CreateLoader(handle.AddrOfPinnedObject());
            if (loader == IntPtr.Zero)
                throw new InvalidOperationException("polyplug: js_deno loader create failed");
            uint err = RegisterLoader(runtime.Handle, loader);
            if (err != 0)
                throw new InvalidOperationException($"polyplug: js_deno loader register failed ({err})");
        } finally {
            handle.Free();
        }
    }
}
