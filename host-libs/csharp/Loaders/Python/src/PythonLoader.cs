using System;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Loaders;

[StructLayout(LayoutKind.Sequential)]
internal struct PythonConfig
{
    public IntPtr MinVersionPtr;
    public UIntPtr MinVersionLen;
}

public static class PythonLoaderExtensions
{
    [DllImport("polyplug_python", EntryPoint = "polyplug_python_loader_create")]
    private static extern IntPtr CreateLoader(IntPtr cfgPtr);

    [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
    private static extern uint RegisterLoader(IntPtr rt, IntPtr loader);

    public static void RegisterPythonLoader(this Runtime runtime, string minVersion = "3.11")
    {
        byte[] bytes = Encoding.UTF8.GetBytes(minVersion);
        var stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try {
            PythonConfig cfg = new PythonConfig {
                MinVersionPtr = stringHandle.AddrOfPinnedObject(),
                MinVersionLen = (UIntPtr)bytes.Length
            };
            var cfgHandle = GCHandle.Alloc(cfg, GCHandleType.Pinned);
            try {
                IntPtr loader = CreateLoader(cfgHandle.AddrOfPinnedObject());
                if (loader == IntPtr.Zero)
                    throw new InvalidOperationException("polyplug: python loader create failed");
                uint err = RegisterLoader(runtime.Handle, loader);
                if (err != 0)
                    throw new InvalidOperationException($"polyplug: python loader register failed ({err})");
            } finally {
                cfgHandle.Free();
            }
        } finally {
            stringHandle.Free();
        }
    }
}
