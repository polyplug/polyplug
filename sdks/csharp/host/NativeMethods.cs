using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Polyplug.Host;

internal static partial class NativeMethods
{
    private const string NativeLib = "polyplug";

    // ─── C-compatible types for hot-reload notification ───────────────────────────

    /// <summary>
    /// C-compatible string view for passing strings across the FFI boundary.
    /// The pointer must remain valid for the duration of the callback call.
    /// This is a borrowed view — the callback must NOT free the memory.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct StringViewC
    {
        /// <summary>
        /// Pointer to UTF-8 bytes.
        /// </summary>
        public nint Ptr;

        /// <summary>
        /// Length in bytes.
        /// </summary>
        public nuint Len;
    }

    /// <summary>
    /// C-compatible representation of ReloadPhase.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct ReloadPhaseC
    {
        /// <summary>
        /// The phase type (Preparing=0, Reloaded=1, or Failed=2).
        /// </summary>
        public uint PhaseType;

        /// <summary>
        /// Bundle ID (valid for all variants).
        /// </summary>
        public ulong BundleId;

        /// <summary>
        /// Bundle name (valid for all variants).
        /// </summary>
        public StringViewC BundleName;

        /// <summary>
        /// Retry count (valid only for Preparing variant).
        /// </summary>
        public uint RetryCount;

        /// <summary>
        /// Failure reason (valid only for Failed variant).
        /// </summary>
        public StringViewC Reason;
    }

    /// <summary>
    /// C-compatible configuration for hot-reload behavior.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct RuntimeConfigC
    {
        /// <summary>
        /// Whether hot-reload is enabled for this runtime.
        /// 0 = false (disabled), non-zero = true (enabled).
        /// </summary>
        public byte HotReloadEnabled;

        /// <summary>
        /// Maximum number of retry attempts for hot-reload operations.
        /// </summary>
        public uint HotReloadMaxRetries;

        /// <summary>
        /// Interval between hot-reload retry attempts, in milliseconds.
        /// </summary>
        public ulong HotReloadRetryIntervalMs;

        /// <summary>
        /// Whether to abort the runtime when max retries are exhausted.
        /// 0 = false (continue retrying), non-zero = true (abort).
        /// </summary>
        public byte HotReloadAbortOnMaxRetries;
    }

    // Lifecycle (init-time only, no SuppressGCTransition needed)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugRuntimeCreate();

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void PolyplugRuntimeDestroy(nint rt);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_load_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeLoadBundle(nint rt, nint path, nuint pathLen);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_reload_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeReloadBundle(nint rt, nint path, nuint pathLen);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_register_loader")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeRegisterLoader(nint rt, nint loaderPtr);

    // Hot path — SuppressGCTransition for zero-overhead
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial ulong PolyplugRuntimeFindByContract(nint rt, ulong contractId, uint minVersion);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial ulong PolyplugRuntimeFindByBundle(nint rt, ulong bundleId, ulong contractId, uint minVersion);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_all_by_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial nuint PolyplugRuntimeFindAllByContract(
        nint rt,
        ulong contractId,
        uint minVersion,
        nint outHandles,
        nuint outCap
    );

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_resolve_plugin")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial nint PolyplugRuntimeResolvePlugin(nint rt, ulong packedHandle);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_release_plugin")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial void PolyplugRuntimeReleasePlugin(nint resolveHandle);

    // Error handling (error path only, no SuppressGCTransition)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_last_error")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nuint PolyplugRuntimeLastError(nint buf, nuint bufLen);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_error_message_len")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nuint PolyplugRuntimeErrorMessageLen();

    // Memory — NO SuppressGCTransition (may trigger GC per PRD)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_host_free")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void PolyplugHostFree(nint ptr, nuint len, nuint align);

    // Hot-reload configuration (must be called before runtime creation)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_on_reload")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeOnReload(nint callback);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_set_config")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeSetConfig(ref RuntimeConfigC config);

    // Host contract registration
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_register_host_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeRegisterHostContract(nint rt, nint vtable);
}