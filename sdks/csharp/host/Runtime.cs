using System;
using System.Collections.Generic;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Text;

using Polyplug.Abi;

namespace Polyplug.Host;

public sealed class Runtime : IDisposable
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

    // Each entry owns all managed objects that back one successfully registered
    // internal plugin. The resident is released only after logical unload.
    private readonly object _internalPluginsGate = new();
    private readonly Dictionary<ulong, InternalPluginBundle> _internalPlugins = new();

    // Cached function pointer delegates (18-03)
    private LoadBundleDelegate? _loadBundleFn;
    private ReloadBundleDelegate? _reloadBundleFn;
    private UnloadBundleDelegate? _unloadBundleFn;
    private FindGuestContractDelegate? _findGuestContractFn;
    private ListBundlesDelegate? _listBundlesFn;
    private FindAllGuestContractsDelegate? _findAllFn;
    private ResolveGuestContractDelegate? _resolveFn;
    private GetLastErrorDelegate? _getLastErrorFn;
    private GetErrorLenDelegate? _getErrorLenFn;
    private RegisterHostContractDelegate? _registerHostContractFn;
    private RegisterLoaderDelegate? _registerLoaderFn;
    private FreeDelegate? _freeFn;
    private GetBundleDescriptorDelegate? _getBundleDescriptorFn;
    private ListRegisteredGuestContractsDelegate? _listRegisteredGuestContractsFn;
    private GetRegisteredContractDescriptorDelegate? _getRegisteredContractDescriptorFn;

    // ─── Function pointer delegate types (18-03) ─────────────────────────────────

    // Out-param ABI: these host calls return void and write their AbiError
    // through a trailing pointer. `out AbiError` marshals as that `*mut AbiError`
    // (AbiError is blittable), so no managed return value crosses the boundary.
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void LoadBundleDelegate(nint host, nint path, nuint pathLen, out AbiError outErr);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void ReloadBundleDelegate(nint host, nint path, nuint pathLen, out AbiError outErr);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void UnloadBundleDelegate(nint host, ulong bundleId, out AbiError outErr);

    // GuestContractHandle is `#[repr(C)] { index: u32, generation: u32 }` (8 bytes,
    // align 4). It crosses the C ABI by value as the 8-byte struct — its
    // [StructLayout(LayoutKind.Sequential, Size = 8)] in the generated ABI lays out
    // Index@0, Generation@4. The null handle is `{ index: u32::MAX, generation: 0 }`,
    // detected by `Index == uint.MaxValue` (0xFFFFFFFF).
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate GuestContractHandle FindGuestContractDelegate(nint host, ulong contractId, uint minVersion);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void FindAllGuestContractsDelegate(
        nint host, ulong contractId, uint minVersion, out Polyplug.Abi.Array handles);
    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    internal delegate void ListBundlesDelegate(nint host, out Polyplug.Abi.Array bundles);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nint ResolveGuestContractDelegate(nint host, GuestContractHandle handle);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint GetLastErrorDelegate(nint host, nint buf, nuint bufLen);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate nuint GetErrorLenDelegate(nint host);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void RegisterHostContractDelegate(nint host, nint interfacePtr, out AbiError outErr);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    private delegate void RegisterLoaderDelegate(nint host, nint loaderPtr, out AbiError outErr);


    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    internal delegate void FreeDelegate(nint host, nint ptr, nuint size, nuint align);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    internal delegate bool GetBundleDescriptorDelegate(
        nint host,
        ulong bundleId,
        out BundleDescriptorView descriptor);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    internal delegate void ListRegisteredGuestContractsDelegate(
        nint host, out Polyplug.Abi.Array handles);

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    internal delegate bool GetRegisteredContractDescriptorDelegate(
        nint host,
        GuestContractHandle handle,
        out RegisteredContractDescriptorView descriptor);
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
    /// <paramref name="phasePtr"/> points at the ABI <c>ReloadPhase</c>; the
    /// runtime guarantees it is non-null and valid only for the duration of the
    /// call, so the struct (and the strings inside it) is copied to managed
    /// memory before the callback runs.
    /// A managed exception must never unwind across the C ABI mid-reload, so the
    /// invocation is wrapped in a catch-all that logs and swallows.
    /// </summary>
    internal static void OnReloadNative(nint userData, nint phasePtr)
    {
        if (userData == nint.Zero || phasePtr == nint.Zero)
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
            Polyplug.Abi.ReloadPhase phase = Marshal.PtrToStructure<Polyplug.Abi.ReloadPhase>(phasePtr);
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

    private static string OwnedBytesToString(nint pointer, nuint length)
    {
        if (pointer == nint.Zero || length == nuint.Zero)
        {
            return string.Empty;
        }

        int byteCount = checked((int)length);
        byte[] buffer = new byte[byteCount];
        Marshal.Copy(pointer, buffer, 0, byteCount);
        return Encoding.UTF8.GetString(buffer);
    }

    private void FreeDescriptorBytes(nint pointer, nuint length, nuint alignment)
    {
        if (pointer != nint.Zero)
        {
            _freeFn!(_host, pointer, length, alignment);
        }
    }

    [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
    internal delegate void OnReloadTrampoline(nint userData, nint phasePtr);

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
        _listBundlesFn = Marshal.GetDelegateForFunctionPointer<ListBundlesDelegate>(_hostStruct.ListBundles);
        _freeFn = Marshal.GetDelegateForFunctionPointer<FreeDelegate>(_hostStruct.Free);

        if (_hostStruct.Reserved == nint.Zero)
        {
            return;
        }

        RuntimeIntrospection introspection = Marshal.PtrToStructure<RuntimeIntrospection>(_hostStruct.Reserved);
        if (introspection.GetBundleDescriptor != nint.Zero)
        {
            _getBundleDescriptorFn =
                Marshal.GetDelegateForFunctionPointer<GetBundleDescriptorDelegate>(introspection.GetBundleDescriptor);
        }
        if (introspection.ListRegisteredGuestContracts != nint.Zero)
        {
            _listRegisteredGuestContractsFn =
                Marshal.GetDelegateForFunctionPointer<ListRegisteredGuestContractsDelegate>(
                    introspection.ListRegisteredGuestContracts);
        }
        if (introspection.GetRegisteredContractDescriptor != nint.Zero)
        {
            _getRegisteredContractDescriptorFn =
                Marshal.GetDelegateForFunctionPointer<GetRegisteredContractDescriptorDelegate>(
                    introspection.GetRegisteredContractDescriptor);
        }
    }

    /// <summary>
    /// Destroys the native runtime and releases managed callback and resident state.
    /// </summary>
    public void Dispose()
    {
        if (!DestroyRuntime())
        {
            throw new InvalidOperationException("Failed to destroy runtime.");
        }

        GC.SuppressFinalize(this);
    }

    ~Runtime()
    {
        if (!DestroyRuntime())
        {
            GC.ReRegisterForFinalize(this);
        }
    }

    private bool DestroyRuntime()
    {
        if (_host != nint.Zero && !NativeMethods.PolyplugRuntimeDestroy(_host))
        {
            return false;
        }

        _host = nint.Zero;

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
        ReleaseInternalPlugins();
        return true;
    }

    private void ReleaseInternalPlugins()
    {
        List<InternalPluginBundle> plugins;
        lock (_internalPluginsGate)
        {
            plugins = new List<InternalPluginBundle>(_internalPlugins.Values);
            _internalPlugins.Clear();
        }

        foreach (InternalPluginBundle plugin in plugins)
        {
            plugin.Release();
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
            _loadBundleFn!(_host, ptr, len, out AbiError result);
            CheckAbiError(result, "Failed to load bundle.");
        });
    }

    public void ReloadBundle(string path)
    {
        EnsureHost();
        InvokeWithUtf8(path, (ptr, len) =>
        {
            _reloadBundleFn!(_host, ptr, len, out AbiError result);
            CheckAbiError(result, "Failed to reload bundle.");
        });
    }

    public void UnloadBundle(ulong bundleId)
    {
        EnsureHost();
        InternalPluginBundle? resident = null;
        lock (_internalPluginsGate)
        {
            _unloadBundleFn!(_host, bundleId, out AbiError result);
            CheckAbiError(result, "Failed to unload bundle.");
            if (_internalPlugins.Remove(bundleId, out InternalPluginBundle? plugin))
            {
                resident = plugin;
            }
        }
        resident?.Release();
    }

    /// <summary>
    /// Inspect an <see cref="AbiError"/> written through a host call's out-param.
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
        if (string.IsNullOrEmpty(message))
        {
            message = GetLastError();
        }
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

        // find_all_guest_contracts writes Array<GuestContractHandle> through an explicit
        // out parameter. The caller owns and frees its non-null buffer exactly once.
        _findAllFn!(_host, contractId, minVersion, out Polyplug.Abi.Array array);
        if (array.Items == nint.Zero)
        {
            return [];
        }

        int stride = Marshal.SizeOf<GuestContractHandle>();
        try
        {
            if (array.Len == nuint.Zero)
            {
                return [];
            }

            int count = checked((int)array.Len.ToUInt64());
            GuestContractHandle[] handles = new GuestContractHandle[count];
            for (int i = 0; i < count; i++)
            {
                handles[i] = Marshal.PtrToStructure<GuestContractHandle>(array.Items + i * stride);
            }

            return handles;
        }
        finally
        {
            _freeFn!(_host, array.Items, array.Len * (nuint)stride, array.Align);
        }
    }

    /// <summary>
    /// Snapshots loaded bundle metadata, copying borrowed ABI strings before return.
    /// Older runtimes that do not expose introspection return an empty snapshot.
    /// </summary>
    public IReadOnlyList<BundleDescriptor> GetBundleDescriptors()
    {
        EnsureHost();
        if (_getBundleDescriptorFn is null)
        {
            return System.Array.AsReadOnly(System.Array.Empty<BundleDescriptor>());
        }

        _listBundlesFn!(_host, out Polyplug.Abi.Array bundles);
        if (bundles.Items == nint.Zero)
        {
            return System.Array.AsReadOnly(System.Array.Empty<BundleDescriptor>());
        }

        int stride = sizeof(ulong);
        try
        {
            if (bundles.Len == nuint.Zero)
            {
                return System.Array.AsReadOnly(System.Array.Empty<BundleDescriptor>());
            }

            int count = checked((int)bundles.Len.ToUInt64());
            List<BundleDescriptor> descriptors = new(count);
            for (int index = 0; index < count; index++)
            {
                ulong bundleId = unchecked((ulong)Marshal.ReadInt64(bundles.Items + index * stride));
                if (!_getBundleDescriptorFn(_host, bundleId, out BundleDescriptorView view))
                {
                    continue;
                }

                try
                {
                    descriptors.Add(new BundleDescriptor(
                        view.Id,
                        OwnedBytesToString(view.Name, view.NameLen),
                        view.Version,
                        view.Runtime,
                        view.SourceKind));
                }
                finally
                {
                    FreeDescriptorBytes(view.Name, view.NameLen, view.NameAlign);
                }
            }

            return System.Array.AsReadOnly(descriptors.ToArray());
        }
        finally
        {
            _freeFn!(_host, bundles.Items, bundles.Len * (nuint)stride, bundles.Align);
        }
    }

    /// Snapshots live guest-contract ownership metadata, copying and releasing caller-owned ABI
    /// strings before return. Older runtimes that do not expose introspection return an empty
    /// snapshot.
    /// </summary>
    public IReadOnlyList<RegisteredContractDescriptor> GetRegisteredContractDescriptors()
    {
        EnsureHost();
        if (_listRegisteredGuestContractsFn is null || _getRegisteredContractDescriptorFn is null)
        {
            return System.Array.AsReadOnly(System.Array.Empty<RegisteredContractDescriptor>());
        }

        _listRegisteredGuestContractsFn(_host, out Polyplug.Abi.Array handles);
        if (handles.Items == nint.Zero)
        {
            return System.Array.AsReadOnly(System.Array.Empty<RegisteredContractDescriptor>());
        }

        int stride = Marshal.SizeOf<GuestContractHandle>();
        try
        {
            if (handles.Len == nuint.Zero)
            {
                return System.Array.AsReadOnly(System.Array.Empty<RegisteredContractDescriptor>());
            }

            int count = checked((int)handles.Len.ToUInt64());
            List<RegisteredContractDescriptor> descriptors = new(count);
            for (int index = 0; index < count; index++)
            {
                GuestContractHandle handle =
                    Marshal.PtrToStructure<GuestContractHandle>(handles.Items + index * stride);
                if (!_getRegisteredContractDescriptorFn(_host, handle, out RegisteredContractDescriptorView view))
                {
                    continue;
                }

                try
                {
                    descriptors.Add(new RegisteredContractDescriptor(
                        view.Handle,
                        view.BundleId,
                        view.ContractId,
                        OwnedBytesToString(view.Plugin.Name, view.Plugin.NameLen),
                        OwnedBytesToString(
                            view.Plugin.ContractName,
                            view.Plugin.ContractNameLen),
                        view.Plugin.Version));
                }
                finally
                {
                    FreeDescriptorBytes(view.Plugin.Name, view.Plugin.NameLen, view.Plugin.NameAlign);
                    FreeDescriptorBytes(
                        view.Plugin.ContractName,
                        view.Plugin.ContractNameLen,
                        view.Plugin.ContractNameAlign);
                }
            }

            return System.Array.AsReadOnly(descriptors.ToArray());
        }
        finally
        {
            _freeFn!(_host, handles.Items, handles.Len * (nuint)stride, handles.Align);
        }
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

        _registerHostContractFn!(_host, hostInterface, out AbiError result);
        CheckAbiError(result, "Failed to register host contract.");
    }

    /// <summary>
    /// Register a language loader with the runtime.
    /// The loader's runtime name comes from its own BundleLoader.runtime_name();
    /// it is not passed here.
    /// </summary>
    /// <param name="loaderPtr">Opaque loader pointer from the loader cdylib's create function.</param>
    /// <returns>Zero on success, non-zero AbiError code on failure.</returns>
    public uint RegisterLoader(nint loaderPtr)
    {
        EnsureHost();

        _registerLoaderFn!(_host, loaderPtr, out AbiError error);
        // AbiError.Message is static or runtime-owned; the receiver never frees it.
        return error.Code;
    }


    /// <summary>
    /// Registers generated internal plugin bindings through canonical manifest staging.
    /// An input is consumed for this attempt whether registration succeeds or fails;
    /// retrying requires a newly created generated input.
    /// </summary>
    /// <param name="plugin">Generated manifest, descriptor/interface registrar, and managed resident.</param>
    /// <param name="providerCount">Exact number of generated providers staged by the binding.</param>
    /// <returns>The canonical bundle identifier and exact committed handles in provider order.</returns>
    public unsafe InternalPluginRegistration RegisterInternalPlugin(
        InternalPluginBundle plugin,
        int providerCount)
    {
        EnsureHost();
        ArgumentNullException.ThrowIfNull(plugin);
        if (providerCount <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(providerCount));
        }
        if (!plugin.TryReserveTransfer())
        {
            throw new InvalidOperationException("Internal plugin input has already been consumed.");
        }

        ulong bundleId = 0;
        bool staged = false;
        try
        {
            lock (_internalPluginsGate)
            {
                _internalPlugins.EnsureCapacity(_internalPlugins.Count + 1);
                byte[] manifest = plugin.Manifest;
                AbiError beginError = default;
                fixed (byte* manifestBytes = manifest)
                {
                    NativeMethods.PolyplugBeginInternalPlugin(
                        _host,
                        manifestBytes,
                        (nuint)manifest.Length,
                        (uint)SupportedLanguage.Dotnet,
                        &bundleId,
                        &beginError);
                    CheckAbiError(beginError, "Failed to begin internal plugin registration.");
                }

                staged = true;
                CheckAbiError(plugin.RegisterContracts(_host), "Failed to register internal plugin guest contract.");
                GuestContractHandle[] handles = new GuestContractHandle[providerCount];
                AbiError commitError = default;
                nuint handleCount = 0;
                fixed (GuestContractHandle* handlePtr = handles)
                {
                    NativeMethods.PolyplugCommitInternalPluginWithHandles(
                        _host,
                        bundleId,
                        handlePtr,
                        (nuint)handles.Length,
                        &handleCount,
                        &commitError);
                }
                staged = false;
                CheckAbiError(commitError, "Failed to commit internal plugin registration.");
                _internalPlugins.Add(bundleId, plugin);
                return new InternalPluginRegistration(bundleId, handles);
            }
        }
        catch
        {
            if (staged)
            {
                NativeMethods.PolyplugAbortInternalPlugin(_host, bundleId);
            }

            plugin.Release();
            throw;
        }
    }

    internal int InternalPluginCount
    {
        get
        {
            lock (_internalPluginsGate)
            {
                return _internalPlugins.Count;
            }
        }
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

/// <summary>
/// An immutable snapshot of metadata for one loaded bundle.
/// </summary>
public sealed class BundleDescriptor
{
    internal BundleDescriptor(
        ulong id,
        string name,
        Polyplug.Abi.Version version,
        SupportedLanguage runtime,
        BundleSourceKind sourceKind)
    {
        Id = id;
        Name = name;
        Version = version;
        Runtime = runtime;
        SourceKind = sourceKind;
    }

    /// <summary>Stable bundle identity.</summary>
    public ulong Id { get; }

    /// <summary>Human-readable bundle name.</summary>
    public string Name { get; }

    /// <summary>Semantic bundle version.</summary>
    public Polyplug.Abi.Version Version { get; }

    /// <summary>Runtime language selected for the bundle.</summary>
    public SupportedLanguage Runtime { get; }

    /// <summary>Retained origin of the bundle.</summary>
    public BundleSourceKind SourceKind { get; }
}

/// <summary>
/// An immutable snapshot of one live guest-contract registration.
/// </summary>
public sealed class RegisteredContractDescriptor
{
    internal RegisteredContractDescriptor(
        GuestContractHandle handle,
        ulong bundleId,
        ulong contractId,
        string pluginName,
        string contractName,
        Polyplug.Abi.Version version)
    {
        Handle = handle;
        BundleId = bundleId;
        ContractId = contractId;
        PluginName = pluginName;
        ContractName = contractName;
        Version = version;
    }

    /// <summary>Stable handle for the live registration.</summary>
    public GuestContractHandle Handle { get; }

    /// <summary>Bundle that owns the registration.</summary>
    public ulong BundleId { get; }

    /// <summary>Canonical guest-contract identity.</summary>
    public ulong ContractId { get; }

    /// <summary>Human-readable provider name.</summary>
    public string PluginName { get; }

    /// <summary>Full canonical contract name.</summary>
    public string ContractName { get; }

    /// <summary>Provider version.</summary>
    public Polyplug.Abi.Version Version { get; }
}

/// <summary>
/// The canonical result of generated internal plugin registration.
/// </summary>
public sealed class InternalPluginRegistration
{
    internal InternalPluginRegistration(ulong bundleId, GuestContractHandle[] handles)
    {
        BundleId = bundleId;
        Handles = handles;
    }

    public ulong BundleId { get; }

    public GuestContractHandle[] Handles { get; }
}