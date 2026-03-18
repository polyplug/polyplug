using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

internal static partial class NativeMethods
{
    private const string NativeLib = "polyplug";

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

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_vtable")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial nint PolyplugRuntimePluginVTable(nint guard);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_release")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl), typeof(CallConvSuppressGCTransition)])]
    public static partial void PolyplugRuntimePluginRelease(nint guard);

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
}