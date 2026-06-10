using System;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;

namespace Polyplug.Host;

public sealed class Runtime
{
    // HostApi pointer and loaded struct (18-03)
    private nint _host;
    private HostApi _hostStruct;

    // ─── Per-instance reload callback storage (Rule 12: no statics) ──────────────
    //
    // Mirrors the C++ reference pattern (sdks/cpp/host/polyplug/runtime.hpp):
    // the user callback lives in a heap-allocated state object owned by THIS
    // Runtime instance. A GCHandle to that state is passed to the native side as
    // `RuntimeConfig.OnReloadUserData` and recovered by the static (stateless)
    // trampoline on every invocation. The trampoline delegate itself is also
    // pinned per-instance so the function pointer stays valid for the runtime's
    // lifetime.
    private GCHandle _reloadStateHandle;
    private GCHandle _reloadTrampolineHandle;

    // Cached function pointer delegates (18-03)
    private LoadBundleDelegate? _loadBundleFn;
    private ReloadBundleDelegate? _reloadBundleFn;
    private UnloadBundleDelegate? _unloadBundleFn;
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

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate AbiError UnloadBundleDelegate(nint host, ulong bundleId);

    // GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes,
    // align 4). It crosses the C ABI by value as the 8-byte struct — its
    // [StructLayout(LayoutKind.Sequential, Size = 8)] in the generated ABI lays out
    // Index@0, Generation@4. The null handle is `{ index: u32::MAX, generation: 0 }`,
    // detected by `Index == uint.MaxValue` (0xFFFFFFFF).
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate GuestContractHandle FindGuestContractDelegate(nint host, ulong contractId, uint minVersion);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate Polyplug.Abi.Array FindAllGuestContractsDelegate(nint host, ulong contractId, uint minVersion);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nint ResolveGuestContractDelegate(nint host, GuestContractHandle handle);

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
    /// Gets HostApi pointer from FFI and caches struct fields.
    /// </summary>
    public Runtime()
    {
        _host = CreateNative();
        if (_host == nint.Zero)
        {
            ThrowLastError("Failed to create runtime.");
        }
        _hostStruct = Marshal.PtrToStructure<HostApi>(_host);
        CacheFunctionPointers();
    }

    /// <summary>
    /// Create a Runtime from an existing HostApi pointer.
    /// Used by RuntimeBuilder after creating the HostApi.
    /// </summary>
    /// <param name="hostInterfacePtr">HostApi pointer from polyplug_runtime_create.</param>
    /// <param name="reloadStateHandle">GCHandle on the per-instance <see cref="ReloadCallbackState"/> (or default).</param>
    /// <param name="reloadTrampolineHandle">GCHandle keeping the trampoline delegate alive (or default).</param>
    internal Runtime(nint hostInterfacePtr, GCHandle reloadStateHandle, GCHandle reloadTrampolineHandle)
    {
        if (hostInterfacePtr == nint.Zero)
        {
            throw new InvalidOperationException("HostApi pointer is null.");
        }
        _host = hostInterfacePtr;
        _reloadStateHandle = reloadStateHandle;
        _reloadTrampolineHandle = reloadTrampolineHandle;
        if (_reloadStateHandle.IsAllocated && _reloadStateHandle.Target is ReloadCallbackState state)
        {
            state.Owner = this;
        }
        _hostStruct = Marshal.PtrToStructure<HostApi>(_host);
        CacheFunctionPointers();
    }

    /// <summary>
    /// Per-instance reload callback state recovered from
    /// <c>RuntimeConfig.OnReloadUserData</c> by <see cref="OnReloadNative"/>.
    /// Owned by the Runtime instance via <see cref="_reloadStateHandle"/>.
    /// </summary>
    internal sealed class ReloadCallbackState
    {
        internal ReloadCallbackState(Action<ReloadPhase> callback)
        {
            Callback = callback;
        }

        internal Action<ReloadPhase> Callback { get; }

        /// <summary>The owning runtime, set once construction completes; used for failure logging.</summary>
        internal Runtime? Owner { get; set; }
    }

    /// <summary>
    /// C ABI trampoline for the reload callback. Stateless: the per-instance
    /// callback is recovered from <paramref name="userData"/>, which carries the
    /// GCHandle of the owning runtime's <see cref="ReloadCallbackState"/>.
    /// A managed exception must never unwind across the C ABI mid-reload, so the
    /// invocation is wrapped in a catch-all that logs and swallows.
    /// </summary>
    internal static void OnReloadNative(nint userData, Polyplug.Abi.ReloadPhase phase)
    {
        if (userData == nint.Zero)
        {
            return;
        }

        ReloadCallbackState? state = GCHandle.FromIntPtr(userData).Target as ReloadCallbackState;
        if (state is null)
        {
            return;
        }

        try
        {
            ReloadPhaseType type = (ReloadPhaseType)phase.PhaseType;
            string bundleName = StringViewToString(phase.BundleName);
            string reason = StringViewToString(phase.Reason);
            state.Callback(new ReloadPhase(type, (ulong)phase.BundleId, bundleName, reason));
        }
        catch (Exception e)
        {
            // Swallow: unwinding a managed exception across the C ABI mid-reload
            // is undefined behavior. Funnel the failure through the host logging
            // path when the owning runtime is available, else stderr.
            state.Owner?.TryLogReloadCallbackFailure(e);
            if (state.Owner is null)
            {
                Console.Error.WriteLine($"polyplug: reload callback threw: {e}");
            }
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void LogDelegate(nint host, uint level, StringView scope, StringView message);

    /// <summary>
    /// Best-effort funnel of a reload-callback failure through <c>HostApi.log</c>
    /// (level 1 = Error); falls back to stderr if the host is unavailable or the
    /// log call itself fails.
    /// </summary>
    private void TryLogReloadCallbackFailure(Exception e)
    {
        try
        {
            if (_host == nint.Zero || _hostStruct.Log == nint.Zero)
            {
                Console.Error.WriteLine($"polyplug: reload callback threw: {e}");
                return;
            }

            LogDelegate logFn = Marshal.GetDelegateForFunctionPointer<LogDelegate>(_hostStruct.Log);
            byte[] scopeBytes = Encoding.UTF8.GetBytes("host.reload_callback");
            byte[] messageBytes = Encoding.UTF8.GetBytes($"reload callback threw: {e.Message}");
            GCHandle scopePin = GCHandle.Alloc(scopeBytes, GCHandleType.Pinned);
            GCHandle messagePin = GCHandle.Alloc(messageBytes, GCHandleType.Pinned);
            try
            {
                StringView scopeView = new StringView { Ptr = scopePin.AddrOfPinnedObject(), Len = (nuint)scopeBytes.Length };
                StringView messageView = new StringView { Ptr = messagePin.AddrOfPinnedObject(), Len = (nuint)messageBytes.Length };
                logFn(_host, (uint)LogLevel.Error, scopeView, messageView);
            }
            finally
            {
                scopePin.Free();
                messagePin.Free();
            }
        }
        catch (Exception logError)
        {
            Console.Error.WriteLine($"polyplug: reload callback threw: {e} (log funnel also failed: {logError.Message})");
        }
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
    internal delegate void OnReloadTrampoline(nint userData, Polyplug.Abi.ReloadPhase phase);

    /// <summary>
    /// Calls <c>polyplug_runtime_create</c> with default configuration
    /// (no <see cref="RuntimeConfig"/>; the native side applies its defaults).
    /// </summary>
    internal static nint CreateNative()
    {
        return NativeMethods.PolyplugRuntimeCreate(nint.Zero);
    }

    private void CacheFunctionPointers()
    {
        _loadBundleFn = Marshal.GetDelegateForFunctionPointer<LoadBundleDelegate>(_hostStruct.LoadBundle);
        _reloadBundleFn = Marshal.GetDelegateForFunctionPointer<ReloadBundleDelegate>(_hostStruct.ReloadBundle);
        _unloadBundleFn = Marshal.GetDelegateForFunctionPointer<UnloadBundleDelegate>(_hostStruct.UnloadBundle);
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

        // Release the per-instance reload callback storage only after the native
        // runtime is destroyed — the native side may invoke the trampoline (and
        // dereference the state GCHandle) up until polyplug_runtime_destroy returns.
        if (_reloadStateHandle.IsAllocated)
        {
            _reloadStateHandle.Free();
        }
        if (_reloadTrampolineHandle.IsAllocated)
        {
            _reloadTrampolineHandle.Free();
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

    /// <summary>
    /// Throw an <see cref="InvalidOperationException"/> with the given message.
    /// Used on runtime create / loader register failure paths where no live
    /// <c>HostApi</c> is available to read <c>get_last_error</c> from, so
    /// the supplied fallback message is thrown directly.
    /// </summary>
    public static void ThrowLastError(string fallbackMessage)
    {
        throw new InvalidOperationException(fallbackMessage);
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

    public void UnloadBundle(ulong bundleId)
    {
        EnsureHost();
        AbiError result = _unloadBundleFn!(_host, bundleId);
        CheckAbiError(result, "Failed to unload bundle.");
    }

    /// <summary>
    /// Inspect an <see cref="AbiError"/> returned by value from a host call.
    /// On a non-Ok code, copy the message into a managed string and throw.
    /// </summary>
    /// <remarks>
    /// Per the ABI ownership contract, <see cref="AbiError.Message"/> is always a
    /// static or runtime-owned string; the receiver MUST NEVER free it. Freeing a
    /// static <c>.rodata</c> pointer through the host allocator corrupts the heap.
    /// Rich detail is available via <c>get_last_error</c>.
    /// </remarks>
    private void CheckAbiError(AbiError error, string fallbackMessage)
    {
        if (error.Code == (uint)AbiErrorCode.Ok)
        {
            return;
        }

        string message = StringViewToString(error.Message);
        throw new InvalidOperationException(string.IsNullOrEmpty(message) ? fallbackMessage : message);
    }

    /// <summary>
    /// The runtime's <c>HostApi*</c> pointer, passed to guest contract
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
    public GuestContractHandle FindGuestContract(ulong contractId, uint minVersion)
    {
        EnsureHost();
        return _findGuestContractFn!(_host, contractId, minVersion);
    }

    public GuestContractHandle[] FindAllByContract(ulong contractId, uint minVersion)
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

        // Each GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }`
        // = 8 bytes, so the array element stride is sizeof(GuestContractHandle) and
        // each element is marshaled as the full struct.
        int count = checked((int)array.Len.ToUInt64());
        int stride = Marshal.SizeOf<GuestContractHandle>();
        GuestContractHandle[] handles = new GuestContractHandle[count];
        for (int i = 0; i < count; i++)
        {
            handles[i] = Marshal.PtrToStructure<GuestContractHandle>(array.Items + i * stride);
        }

        _freeFn!(_host, array.Items, (nuint)(count * stride), array.Align);

        return handles;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public nint ResolveGuestContract(GuestContractHandle handle)
    {
        EnsureHost();
        // Null handle sentinel is { index: u32::MAX, generation: 0 }; the index
        // alone identifies the null handle.
        if (handle.Index == uint.MaxValue)
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
            // AbiError.Message is static or runtime-owned; the receiver never frees it.
            result = error.Code;
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