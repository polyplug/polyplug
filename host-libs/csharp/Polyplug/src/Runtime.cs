using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug;

public sealed class Runtime
{
    public nint Handle { get; private set; }

    public Runtime(nint handle)
    {
        Handle = handle;
    }

    ~Runtime()
    {
        if (Handle != nint.Zero)
        {
            NativeMethods.PolyplugRuntimeDestroy(Handle);
            Handle = nint.Zero;
        }
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private void EnsureHandle()
    {
        if (Handle != nint.Zero)
        {
            return;
        }

        throw new ObjectDisposedException(nameof(Runtime));
    }

    public void LoadBundle(string path)
    {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) =>
        {
            uint result = NativeMethods.PolyplugRuntimeLoadBundle(Handle, ptr, (nuint)len);
            if (result != 0u)
            {
                ThrowLastError("Failed to load bundle.");
            }
        });
    }

    public void ReloadBundle(string path)
    {
        EnsureHandle();
        InvokeWithUtf8(path, (ptr, len) =>
        {
            uint result = NativeMethods.PolyplugRuntimeReloadBundle(Handle, ptr, len);
            if (result != 0u)
            {
                ThrowLastError("Failed to reload bundle.");
            }
        });
    }

    public ulong FindByContract(ulong contractId, uint minVersion)
    {
        EnsureHandle();
        ulong packed = NativeMethods.PolyplugRuntimeFindByContract(Handle, contractId, minVersion);
        return packed;
    }

    public ulong FindByBundle(ulong bundleId, ulong contractId, uint minVersion)
    {
        EnsureHandle();
        ulong packed = NativeMethods.PolyplugRuntimeFindByBundle(Handle, bundleId, contractId, minVersion);
        return packed;
    }

    public ulong[] FindAllByContract(ulong contractId, uint minVersion)
    {
        EnsureHandle();

        int capacity = 16;
        while (true)
        {
            var handles = new ulong[capacity];
            GCHandle pinned = GCHandle.Alloc(handles, GCHandleType.Pinned);
            try
            {
                nint outPtr = pinned.AddrOfPinnedObject();
                nuint outCap = (nuint)handles.Length;
                nuint written = NativeMethods.PolyplugRuntimeFindAllByContract(
                    Handle,
                    contractId,
                    minVersion,
                    outPtr,
                    outCap
                );
                ulong count = written.ToUInt64();
                if (count == 0ul)
                {
                    return [];
                }
                if (count < (ulong)handles.Length)
                {
                    ulong[] result = new ulong[count];
                    Array.Copy(handles, result, (long)count);
                    return result;
                }
            }
            finally
            {
                pinned.Free();
            }
            capacity = checked(capacity * 2);
        }
    }

    public PluginGuard ResolvePlugin(ulong packedHandle)
    {
        EnsureHandle();
        if (packedHandle == ulong.MaxValue)
        {
            return new PluginGuard(nint.Zero);
        }

        nint guard = NativeMethods.PolyplugRuntimeResolvePlugin(Handle, packedHandle);
        if (guard == nint.Zero)
        {
            ThrowLastError("Failed to resolve plugin.");
        }

        return new PluginGuard(guard);
    }

    private static void InvokeWithUtf8(string value, Action<nint, nuint> action)
    {
        if (value is null)
        {
            throw new ArgumentNullException(nameof(value));
        }

        byte[] bytes = Encoding.UTF8.GetBytes(value);
        int length = bytes.Length;
        int allocSize = length == 0 ? 1 : length;
        nint ptr = Marshal.AllocHGlobal(allocSize);
        try
        {
            if (length > 0)
            {
                Marshal.Copy(bytes, 0, ptr, length);
            }
            action(ptr, (nuint)length);
        }
        finally
        {
            Marshal.FreeHGlobal(ptr);
        }
    }

    internal static void ThrowLastError(string fallbackMessage)
    {
        string message = GetLastError();
        if (string.IsNullOrEmpty(message))
        {
            message = fallbackMessage;
        }
        throw new InvalidOperationException(message);
    }

    private static string GetLastError()
    {
        nuint len = NativeMethods.PolyplugRuntimeErrorMessageLen();
        ulong length = len.ToUInt64();
        if (length == 0ul)
        {
            return string.Empty;
        }
        if (length > int.MaxValue)
        {
            return "polyplug error message too large";
        }
        byte[] buffer = new byte[(int)length];
        GCHandle pinned = GCHandle.Alloc(buffer, GCHandleType.Pinned);
        try
        {
            nuint written = NativeMethods.PolyplugRuntimeLastError(pinned.AddrOfPinnedObject(), (nuint)buffer.Length);
            int count = (int)written.ToUInt64();
            if (count == 0)
            {
                return string.Empty;
            }
            return Encoding.UTF8.GetString(buffer, 0, count);
        }
        finally
        {
            pinned.Free();
        }
    }

    internal static nint GetVTablePtr(nint guard) => NativeMethods.PolyplugRuntimePluginVTable(guard);

    internal static void ReleaseGuard(nint guard)
    {
        if (guard != nint.Zero)
        {
            NativeMethods.PolyplugRuntimePluginRelease(guard);
        }
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct DotnetLoaderConfig
    {
        public nint MinFrameworkPtr;
        public nuint MinFrameworkLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PythonLoaderConfig
    {
        public nint MinVersionPtr;
        public nuint MinVersionLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct EmptyLoaderConfig
    {
        public byte Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct NativeLoaderConfig
    {
        public byte Reserved;
    }

    public void RegisterDotnetLoader(string minFramework = "10.0")
    {
        EnsureHandle();
        byte[] bytes = Encoding.UTF8.GetBytes(minFramework);
        GCHandle stringHandle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        try
        {
            DotnetLoaderConfig cfg = new DotnetLoaderConfig
            {
                MinFrameworkPtr = stringHandle.AddrOfPinnedObject(),
                MinFrameworkLen = (nuint)bytes.Length,
            };
            nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<DotnetLoaderConfig>());
            try
            {
                Marshal.StructureToPtr(cfg, cfgPtr, false);
                nint loaderPtr = NativeMethods.PolyplugDotnetLoaderCreate(cfgPtr);
                if (loaderPtr == nint.Zero)
                {
                    throw new InvalidOperationException("polyplug: dotnet loader create failed");
                }
                uint err = NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
                if (err != 0u)
                {
                    ThrowLastError($"polyplug: dotnet loader register failed: {err}");
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

    public void RegisterPythonLoader(string minVersion = "3.11")
    {
        EnsureHandle();
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
                nint loaderPtr = NativeMethods.PolyplugPythonLoaderCreate(cfgPtr);
                if (loaderPtr == nint.Zero)
                {
                    throw new InvalidOperationException("polyplug: python loader create failed");
                }
                uint err = NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
                if (err != 0u)
                {
                    ThrowLastError($"polyplug: python loader register failed: {err}");
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

    public void RegisterLuaLoader()
    {
        EnsureHandle();
        EmptyLoaderConfig cfg = new EmptyLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<EmptyLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = NativeMethods.PolyplugLuaLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: lua loader create failed");
            }
            uint err = NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
            if (err != 0u)
            {
                ThrowLastError($"polyplug: lua loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }

    public void RegisterJsLoader()
    {
        EnsureHandle();
        EmptyLoaderConfig cfg = new EmptyLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<EmptyLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = NativeMethods.PolyplugJsLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: js loader create failed");
            }
            uint err = NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
            if (err != 0u)
            {
                ThrowLastError($"polyplug: js loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }

    public void RegisterNativeLoader()
    {
        EnsureHandle();
        var cfg = new NativeLoaderConfig { Reserved = 0 };
        nint cfgPtr = Marshal.AllocHGlobal(Marshal.SizeOf<NativeLoaderConfig>());
        try
        {
            Marshal.StructureToPtr(cfg, cfgPtr, false);
            nint loaderPtr = NativeMethods.PolyplugNativeLoaderCreate(cfgPtr);
            if (loaderPtr == nint.Zero)
            {
                throw new InvalidOperationException("polyplug: native loader create failed");
            }

            uint err = NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
            if (err != 0u)
            {
                ThrowLastError($"polyplug: native loader register failed: {err}");
            }
        }
        finally
        {
            Marshal.FreeHGlobal(cfgPtr);
        }
    }
}
