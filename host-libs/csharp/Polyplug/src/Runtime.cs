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

    public uint RegisterLoader(nint loaderPtr)
    {
        EnsureHandle();

        return NativeMethods.PolyplugRuntimeRegisterLoader(Handle, loaderPtr);
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

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public ulong FindByContract(ulong contractId, uint minVersion)
    {
        EnsureHandle();
        return NativeMethods.PolyplugRuntimeFindByContract(Handle, contractId, minVersion);
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public ulong FindByBundle(ulong bundleId, ulong contractId, uint minVersion)
    {
        EnsureHandle();
        return NativeMethods.PolyplugRuntimeFindByBundle(Handle, bundleId, contractId, minVersion);
    }

    public ulong[] FindAllByContract(ulong contractId, uint minVersion)
    {
        EnsureHandle();

        int capacity = 16;
        while (true)
        {
            ulong[] handles = new ulong[capacity];
            GCHandle pinned = GCHandle.Alloc(handles, GCHandleType.Pinned);
            try
            {
                nint outPtr = pinned.AddrOfPinnedObject();
                nuint written = NativeMethods.PolyplugRuntimeFindAllByContract(
                    Handle,
                    contractId,
                    minVersion,
                    outPtr,
                    (nuint)handles.Length
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
            return new PluginGuard(nint.Zero, nint.Zero);
        }

        nint guardHandle = NativeMethods.PolyplugRuntimeResolvePlugin(Handle, packedHandle);
        if (guardHandle == nint.Zero)
        {
            ThrowLastError("Failed to resolve plugin.");
        }

        nint vtablePtr = NativeMethods.PolyplugRuntimePluginVTable(guardHandle);
        return new PluginGuard(guardHandle, vtablePtr);
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

    public static void ThrowLastError(string fallbackMessage)
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
}