using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

namespace Polyplug.Host;

public sealed class Runtime
{
    private static Action<ReloadPhase>? s_reloadCallback;
    private static GCHandle s_reloadCallbackHandle;
    private static readonly object s_lock = new();

    public nint Handle { get; private set; }

    public Runtime(nint handle)
    {
        Handle = handle;
    }

    /// <summary>
    /// Register a callback to be invoked during hot-reload operations.
    /// Must be called BEFORE creating a Runtime instance.
    /// </summary>
    public static void OnReload(Action<ReloadPhase> callback)
    {
        lock (s_lock)
        {
            if (s_reloadCallbackHandle.IsAllocated)
            {
                s_reloadCallbackHandle.Free();
            }

            s_reloadCallback = callback;

            if (callback is null)
            {
                uint result = NativeMethods.PolyplugRuntimeOnReload(nint.Zero);
                if (result != 0u)
                {
                    ThrowLastError("Failed to clear reload callback.");
                }
                return;
            }

            ReloadCallbackNative nativeCallback = OnReloadNative;
            s_reloadCallbackHandle = GCHandle.Alloc(nativeCallback);

            uint r = NativeMethods.PolyplugRuntimeOnReload(Marshal.GetFunctionPointerForDelegate(nativeCallback));
            if (r != 0u)
            {
                s_reloadCallbackHandle.Free();
                ThrowLastError("Failed to register reload callback.");
            }
        }
    }

    /// <summary>
    /// Set runtime configuration for subsequently created runtimes.
    /// Must be called BEFORE creating a Runtime instance.
    /// </summary>
    public static void SetConfig(HostRuntimeConfig config)
    {
        if (config is null)
        {
            throw new ArgumentNullException(nameof(config));
        }

        NativeMethods.RuntimeConfigC configC = new NativeMethods.RuntimeConfigC
        {
            HotReloadMaxRetries = config.HotReloadMaxRetries,
            HotReloadRetryIntervalMs = config.HotReloadRetryIntervalMs,
            HotReloadAbortOnMaxRetries = config.HotReloadAbortOnMaxRetries ? (byte)1 : (byte)0,
        };

        uint result = NativeMethods.PolyplugRuntimeSetConfig(ref configC);
        if (result != 0u)
        {
            ThrowLastError("Failed to set runtime config.");
        }
    }

    private static void OnReloadNative(NativeMethods.ReloadPhaseC phaseC)
    {
        Action<ReloadPhase>? cb = s_reloadCallback;
        if (cb is null)
        {
            return;
        }

        ReloadPhase phase = ConvertReloadPhase(phaseC);
        cb(phase);
    }

    private static ReloadPhase ConvertReloadPhase(NativeMethods.ReloadPhaseC phaseC)
    {
        ReloadPhaseType type = (ReloadPhaseType)phaseC.PhaseType;
        string bundleName = StringViewToString(phaseC.BundleName);
        string reason = StringViewToString(phaseC.Reason);

        return new ReloadPhase(type, phaseC.BundleId, bundleName, phaseC.RetryCount, reason);
    }

    private static string StringViewToString(NativeMethods.StringViewC sv)
    {
        if (sv.Ptr == nint.Zero || sv.Len == nuint.Zero)
        {
            return string.Empty;
        }

        int len = checked((int)sv.Len);
        byte[] buffer = new byte[len];
        Marshal.Copy(sv.Ptr, buffer, 0, len);
        return Encoding.UTF8.GetString(buffer);
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void ReloadCallbackNative(NativeMethods.ReloadPhaseC phase);

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

    public void RegisterHostContract(nint vtable)
    {
        EnsureHandle();

        uint result = NativeMethods.PolyplugRuntimeRegisterHostContract(Handle, vtable);
        if (result != 0u)
        {
            ThrowLastError("Failed to register host contract.");
        }
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

    /// <summary>
    /// Resolve a plugin handle to get the raw resolve handle.
    ///
    /// In the instance-based model (Phase 3), the host:
    /// 1. Gets resolve handle via ResolvePlugin (this method)
    /// 2. Calls create_instance on the GuestContractInterface
    /// 3. Makes dispatch calls with the instance
    /// 4. Calls destroy_instance before hot-reload (via ReloadPhase callback)
    ///
    /// The returned nint is a raw resolve handle. The caller must NOT
    /// cache this beyond hot-reload boundaries.
    /// </summary>
    /// <param name="packedHandle">Packed contract handle from FindByContract.</param>
    /// <returns>Raw resolve handle (nint.Zero if not found).</returns>
    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public nint ResolvePlugin(ulong packedHandle)
    {
        EnsureHandle();
        if (packedHandle == ulong.MaxValue)
        {
            return nint.Zero;
        }

        return NativeMethods.PolyplugRuntimeResolvePlugin(Handle, packedHandle);
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