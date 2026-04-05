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
    /// FFI-safe representation of ReloadPhase (not a 'C suffix' type, but an FFI variant).
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct ReloadPhaseFfi
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
    /// FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes).
    /// Layout verified against Rust offset tests.
    /// </summary>
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    internal struct RuntimeConfig
    {
        /// <summary>
        /// Whether hot-reload is enabled (0=false, non-zero=true).
        /// Offset 0, 1 byte.
        /// </summary>
        public byte HotReloadEnabled;

        // Padding: 3 bytes (offset 1-3) from Pack=4

        /// <summary>
        /// Maximum retry attempts for hot-reload.
        /// Offset 4, 4 bytes.
        /// </summary>
        public uint HotReloadMaxRetries;

        /// <summary>
        /// Interval between retry attempts in milliseconds.
        /// Offset 8, 8 bytes.
        /// </summary>
        public ulong HotReloadRetryIntervalMs;

        /// <summary>
        /// Abort when max retries exhausted (0=false, non-zero=true).
        /// Offset 16, 1 byte.
        /// </summary>
        public byte HotReloadAbortOnMaxRetries;

        // Padding: 3 bytes (offset 17-19) from Pack=4

        /// <summary>
        /// Compatibility mode: 0=Strict, 1=Relaxed, 2=Yolo.
        /// Matches polyplug_abi::Compatibility #[repr(u32)].
        /// Offset 20, 4 bytes.
        /// </summary>
        public uint Compatibility;
    }

    /// <summary>
    /// Compatibility modes matching polyplug_abi::Compatibility #[repr(u32)].
    /// </summary>
    internal static class CompatibilityMode
    {
        /// <summary>Exact major match and minor >= required.</summary>
        public const uint Strict = 0;

        /// <summary>Same major, any minor.</summary>
        public const uint Relaxed = 1;

        /// <summary>Any version accepted.</summary>
        public const uint Yolo = 2;
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
    public static partial uint PolyplugRuntimeSetConfig(ref RuntimeConfig config);

    // Host contract registration
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_register_host_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeRegisterHostContract(nint rt, nint hostInterface);
}