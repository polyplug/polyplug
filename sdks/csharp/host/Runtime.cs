using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;

namespace Polyplug.Host;

public sealed class Runtime
{
    private static Action<ReloadPhase>? s_reloadCallback;
    private static GCHandle s_reloadCallbackHandle;
    private static readonly object s_lock = new();

    // HostInterface pointer and loaded struct (18-03)
    private nint _host;
    private HostInterface _hostStruct;

    // Cached function pointer delegates (18-03)
    private LoadBundleDelegate? _loadBundleFn;
    private ReloadBundleDelegate? _reloadBundleFn;
    private FindGuestContractDelegate? _findGuestContractFn;
    private FindAllGuestContractsDelegate? _findAllFn;
    private ResolveGuestContractDelegate? _resolveFn;
    private GetLastErrorDelegate? _getLastErrorFn;
    private GetErrorLenDelegate? _getErrorLenFn;
    private RegisterHostContractDelegate? _registerHostContractFn;
    private RegisterLoaderDelegate? _registerLoaderFn;
    private FreeDelegate? _freeFn;

    // ─── Function pointer delegate types (18-03) ─────────────────────────────────

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError LoadBundleDelegate(nint host, nint path, nuint pathLen);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError ReloadBundleDelegate(nint host, nint path, nuint pathLen);

    // GuestContractHandle is `#[repr(C)] { index: u32 }` (4 bytes). A single-field
    // 4-byte repr(C) struct crosses the C ABI as a `uint`, so the handle is marshaled
    // as `uint`, not `ulong`. The null handle is `index == u32::MAX` (0xFFFFFFFF).
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate uint FindGuestContractDelegate(nint host, ulong contractId, uint minVersion);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate Polyplug.Abi.Array FindAllGuestContractsDelegate(nint host, ulong contractId, uint minVersion);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nint ResolveGuestContractDelegate(nint host, uint handle);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint GetLastErrorDelegate(nint host, nint buf, nuint bufLen);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint GetErrorLenDelegate(nint host);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError RegisterHostContractDelegate(nint host, nint interfacePtr);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError RegisterLoaderDelegate(nint host, StringView runtimeName, nint loaderPtr);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void FreeDelegate(nint host, nint ptr, nuint size, nuint align);

    /// <summary>
    /// Create a new Runtime instance with default configuration.
    /// Gets HostInterface pointer from FFI and caches struct fields.
    /// </summary>
    public Runtime()
    {
        _host = CreateNative();
        if (_host == nint.Zero)
        {
            ThrowLastError("Failed to create runtime.");
        }
        _hostStruct = Marshal.PtrToStructure<HostInterface>(_host);
        CacheFunctionPointers();
    }

    /// <summary>
    /// Create a Runtime from an existing HostInterface pointer.
    /// Used by RuntimeBuilder after creating the HostInterface.
    /// </summary>
    /// <param name="hostInterfacePtr">HostInterface pointer from polyplug_runtime_create.</param>
    internal Runtime(nint hostInterfacePtr)
    {
        if (hostInterfacePtr == nint.Zero)
        {
            throw new InvalidOperationException("HostInterface pointer is null.");
        }
        _host = hostInterfacePtr;
        _hostStruct = Marshal.PtrToStructure<HostInterface>(_host);
        CacheFunctionPointers();
    }

    /// <summary>
    /// Register a callback to be invoked during hot-reload operations.
    /// Must be called BEFORE creating a Runtime instance.
    /// The callback is stored statically so the C ABI trampoline can reference it.
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
                s_reloadTrampoline = null;
                return;
            }

            OnReloadTrampoline trampoline = OnReloadNative;
            s_reloadCallbackHandle = GCHandle.Alloc(trampoline);
            s_reloadTrampoline = trampoline;
        }
    }

    /// <summary>
    /// C ABI trampoline for the reload callback. Stored as a static so the
    /// delegate is not garbage-collected while the runtime holds the pointer.
    /// </summary>
    private static OnReloadTrampoline? s_reloadTrampoline;

    private static void OnReloadNative(Polyplug.Abi.ReloadPhase phase)
    {
        Action<ReloadPhase>? cb = s_reloadCallback;
        if (cb is null)
        {
            return;
        }

        ReloadPhaseType type = (ReloadPhaseType)phase.PhaseType;
        string bundleName = StringViewToString(phase.BundleName);
        string reason = StringViewToString(phase.Reason);

        // Polyplug.Abi.ReloadPhase has no RetryCount field; use 0 as default.
        cb(new ReloadPhase(type, (ulong)phase.BundleId, bundleName, 0u, reason));
    }

    private static string StringViewToString(StringView sv)
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
    private delegate void OnReloadTrampoline(Polyplug.Abi.ReloadPhase phase);

    /// <summary>
    /// Build a RuntimeConfig struct that includes the stored reload callback.
    /// Returns null if no callback has been set via OnReload().
    /// </summary>
    internal static RuntimeConfig? BuildRuntimeConfig()
    {
        lock (s_lock)
        {
            if (s_reloadTrampoline is null)
            {
                return null;
            }

            return new RuntimeConfig
            {
                Compatibility = Compatibility.Strict,
                HotReloadEnabled = true,
                OnReload = Marshal.GetFunctionPointerForDelegate(s_reloadTrampoline),
            };
        }
    }

    /// <summary>
    /// Calls <c>polyplug_runtime_create</c>, marshaling a configured
    /// <see cref="RuntimeConfig"/> when one was set via <see cref="OnReload"/>.
    /// The native core copies the config during the call, so the unmanaged copy
    /// is freed once create returns. The reload trampoline delegate is kept alive
    /// for the process lifetime by <c>s_reloadCallbackHandle</c> / <c>s_reloadTrampoline</c>.
    /// </summary>
    internal static nint CreateNative()
    {
        RuntimeConfig? config = BuildRuntimeConfig();
        if (config is null)
        {
            return NativeMethods.PolyplugRuntimeCreate(nint.Zero);
        }

        nint configPtr = Marshal.AllocHGlobal(Marshal.SizeOf<RuntimeConfig>());
        try
        {
            Marshal.StructureToPtr(config.Value, configPtr, fDeleteOld: false);
            return NativeMethods.PolyplugRuntimeCreate(configPtr);
        }
        finally
        {
            Marshal.FreeHGlobal(configPtr);
        }
    }

    private void CacheFunctionPointers()
    {
        _loadBundleFn = Marshal.GetDelegateForFunctionPointer<LoadBundleDelegate>(_hostStruct.LoadBundle);
        _reloadBundleFn = Marshal.GetDelegateForFunctionPointer<ReloadBundleDelegate>(_hostStruct.ReloadBundle);
        _findGuestContractFn = Marshal.GetDelegateForFunctionPointer<FindGuestContractDelegate>(_hostStruct.FindGuestContract);
        _findAllFn = Marshal.GetDelegateForFunctionPointer<FindAllGuestContractsDelegate>(_hostStruct.FindAllGuestContracts);
        _resolveFn = Marshal.GetDelegateForFunctionPointer<ResolveGuestContractDelegate>(_hostStruct.ResolveGuestContract);
        _getLastErrorFn = Marshal.GetDelegateForFunctionPointer<GetLastErrorDelegate>(_hostStruct.GetLastError);
        _getErrorLenFn = Marshal.GetDelegateForFunctionPointer<GetErrorLenDelegate>(_hostStruct.GetErrorLen);
        _registerHostContractFn = Marshal.GetDelegateForFunctionPointer<RegisterHostContractDelegate>(_hostStruct.RegisterHostContract);
        _registerLoaderFn = Marshal.GetDelegateForFunctionPointer<RegisterLoaderDelegate>(_hostStruct.RegisterLoader);
        _freeFn = Marshal.GetDelegateForFunctionPointer<FreeDelegate>(_hostStruct.Free);
    }

    ~Runtime()
    {
        if (_host != nint.Zero)
        {
            NativeMethods.PolyplugRuntimeDestroy(_host);
            _host = nint.Zero;
        }
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    private void EnsureHost()
    {
        if (_host != nint.Zero)
        {
            return;
        }

        throw new ObjectDisposedException(nameof(Runtime));
    }

    private string GetLastError()
    {
        EnsureHost();
        nuint len = _getErrorLenFn!(_host);
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
            nuint written = _getLastErrorFn!(_host, pinned.AddrOfPinnedObject(), (nuint)buffer.Length);
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

    public static void ThrowLastError(string fallbackMessage)
    {
        // Create a temporary HostInterface with default config to read the error.
        nint tempHost = NativeMethods.PolyplugRuntimeCreate(nint.Zero);
        if (tempHost == nint.Zero)
        {
            throw new InvalidOperationException(fallbackMessage);
        }

        try
        {
            HostInterface tempStruct = Marshal.PtrToStructure<HostInterface>(tempHost);
            GetErrorLenDelegate getLen = Marshal.GetDelegateForFunctionPointer<GetErrorLenDelegate>(tempStruct.GetErrorLen);
            GetLastErrorDelegate getErr = Marshal.GetDelegateForFunctionPointer<GetLastErrorDelegate>(tempStruct.GetLastError);

            nuint len = getLen(tempHost);
            ulong length = len.ToUInt64();
            if (length == 0ul)
            {
                throw new InvalidOperationException(fallbackMessage);
            }

            if (length > int.MaxValue)
            {
                throw new InvalidOperationException("polyplug error message too large");
            }

            byte[] buffer = new byte[(int)length];
            GCHandle pinned = GCHandle.Alloc(buffer, GCHandleType.Pinned);
            try
            {
                nuint written = getErr(tempHost, pinned.AddrOfPinnedObject(), (nuint)buffer.Length);
                int count = (int)written.ToUInt64();
                if (count == 0)
                {
                    throw new InvalidOperationException(fallbackMessage);
                }
                string message = Encoding.UTF8.GetString(buffer, 0, count);
                throw new InvalidOperationException(string.IsNullOrEmpty(message) ? fallbackMessage : message);
            }
            finally
            {
                pinned.Free();
            }
        }
        finally
        {
            NativeMethods.PolyplugRuntimeDestroy(tempHost);
        }
    }

    public void LoadBundle(string path)
    {
        EnsureHost();
        InvokeWithUtf8(path, (ptr, len) =>
        {
            AbiError result = _loadBundleFn!(_host, ptr, len);
            CheckAbiError(result, "Failed to load bundle.");
        });
    }

    public void ReloadBundle(string path)
    {
        EnsureHost();
        InvokeWithUtf8(path, (ptr, len) =>
        {
            AbiError result = _reloadBundleFn!(_host, ptr, len);
            CheckAbiError(result, "Failed to reload bundle.");
        });
    }

    /// <summary>
    /// Inspect an <see cref="AbiError"/> returned by value from a host call.
    /// On a non-Ok code, frees the error message (allocated by the callee via
    /// the host allocator) after copying it, then throws.
    /// </summary>
    private void CheckAbiError(AbiError error, string fallbackMessage)
    {
        if (error.Code == AbiErrorCode.Ok)
        {
            return;
        }

        string message = StringViewToString(error.Message);
        if (error.Message.Ptr != nint.Zero && error.Message.Len != nuint.Zero)
        {
            _freeFn!(_host, error.Message.Ptr, error.Message.Len, 1);
        }

        throw new InvalidOperationException(string.IsNullOrEmpty(message) ? fallbackMessage : message);
    }

    /// <summary>
    /// The runtime's <c>HostInterface*</c> pointer, passed to guest contract
    /// factory functions (create_instance / destroy_instance) for host allocation.
    /// </summary>
    public nint HostHandle
    {
        get
        {
            EnsureHost();
            return _host;
        }
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public uint FindGuestContract(ulong contractId, uint minVersion)
    {
        EnsureHost();
        return _findGuestContractFn!(_host, contractId, minVersion);
    }

    public uint[] FindAllByContract(ulong contractId, uint minVersion)
    {
        EnsureHost();

        // find_all_guest_contracts returns Array<GuestContractHandle> by value
        // (items pointer, len, align). The caller owns the items buffer and must
        // free it via host->free using the returned size and alignment.
        Polyplug.Abi.Array array = _findAllFn!(_host, contractId, minVersion);
        if (array.Items == nint.Zero || array.Len == nuint.Zero)
        {
            return [];
        }

        // Each GuestContractHandle is `#[repr(C)] { index: u32 }` = 4 bytes, so the
        // array has a 4-byte element stride and each element is read as a uint.
        int count = checked((int)array.Len.ToUInt64());
        uint[] handles = new uint[count];
        for (int i = 0; i < count; i++)
        {
            handles[i] = (uint)Marshal.ReadInt32(array.Items + i * 4);
        }

        _freeFn!(_host, array.Items, (nuint)(count * 4), array.Align);

        return handles;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public nint ResolveGuestContract(uint handle)
    {
        EnsureHost();
        // Null handle sentinel is index == u32::MAX (0xFFFFFFFF).
        if (handle == uint.MaxValue)
        {
            return nint.Zero;
        }

        return _resolveFn!(_host, handle);
    }

    public void RegisterHostContract(nint hostInterface)
    {
        EnsureHost();

        AbiError result = _registerHostContractFn!(_host, hostInterface);
        CheckAbiError(result, "Failed to register host contract.");
    }

    /// <summary>
    /// Register a language loader with the runtime.
    /// </summary>
    /// <param name="runtimeName">Runtime name the loader handles (e.g. "native", "python").</param>
    /// <param name="loaderPtr">Opaque loader pointer from the loader cdylib's create function.</param>
    /// <returns>Zero on success, non-zero AbiError code on failure.</returns>
    public uint RegisterLoader(string runtimeName, nint loaderPtr)
    {
        EnsureHost();

        uint result = 0u;
        InvokeWithUtf8(runtimeName, (ptr, len) =>
        {
            StringView name = new StringView { Ptr = ptr, Len = len };
            AbiError error = _registerLoaderFn!(_host, name, loaderPtr);
            result = (uint)error.Code;
            if (error.Code != AbiErrorCode.Ok && error.Message.Ptr != nint.Zero && error.Message.Len != nuint.Zero)
            {
                _freeFn!(_host, error.Message.Ptr, error.Message.Len, 1);
            }
        });
        return result;
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
}