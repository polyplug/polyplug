using System;
using System.Runtime.InteropServices;

namespace Polyplug.Loaders;

[StructLayout(LayoutKind.Sequential)]
internal struct LuaConfig
{
    public byte Reserved;
}

public static class LuaLoaderExtensions
{
    [DllImport("polyplug_lua", EntryPoint = "polyplug_lua_loader_create")]
    private static extern IntPtr CreateLoader(IntPtr cfgPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
    private static extern uint RegisterLoader(IntPtr rt, IntPtr loader);

    public static void RegisterLuaLoader(this Runtime runtime)
    {
        LuaConfig cfg = new LuaConfig { Reserved = 0 };
        var handle = GCHandle.Alloc(cfg, GCHandleType.Pinned);
        try {
            IntPtr loader = CreateLoader(handle.AddrOfPinnedObject());
            if (loader == IntPtr.Zero)
                throw new InvalidOperationException("polyplug: lua loader create failed");
            uint err = RegisterLoader(runtime.Handle, loader);
            if (err != 0)
                throw new InvalidOperationException($"polyplug: lua loader register failed ({err})");
        } finally {
            handle.Free();
        }
    }
}
