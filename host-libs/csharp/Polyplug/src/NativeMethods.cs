using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

internal static partial class NativeMethods
{
    internal const string NativeLib = "polyplug";
    internal const string NativeLoaderNativeLib = "polyplug_native";
    internal const string NativeLoaderDotnetLib = "polyplug_dotnet";
    internal const string NativeLoaderPythonLib = "polyplug_python";
    internal const string NativeLoaderLuaLib = "polyplug_lua";
    internal const string NativeLoaderJsLib = "polyplug_js";
    
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

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial ulong PolyplugRuntimeFindByContract(nint rt, ulong contractId, uint minVersion);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_by_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial ulong PolyplugRuntimeFindByBundle(nint rt, ulong bundleId, ulong contractId, uint minVersion);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_find_all_by_contract")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nuint PolyplugRuntimeFindAllByContract(
        nint rt,
        ulong contractId,
        uint minVersion,
        nint outHandles,
        nuint outCap
    );

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_resolve_plugin")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugRuntimeResolvePlugin(nint rt, ulong packedHandle);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_vtable")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugRuntimePluginVTable(nint guard);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_plugin_release")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial void PolyplugRuntimePluginRelease(nint guard);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_last_error")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nuint PolyplugRuntimeLastError(nint buf, nuint bufLen);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_error_message_len")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nuint PolyplugRuntimeErrorMessageLen();

    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_register_loader")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial uint PolyplugRuntimeRegisterLoader(nint rt, nint loaderPtr);

    [LibraryImport(NativeLoaderNativeLib, EntryPoint = "polyplug_native_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugNativeLoaderCreate(nint cfgPtr);

    [LibraryImport(NativeLoaderDotnetLib, EntryPoint = "polyplug_dotnet_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugDotnetLoaderCreate(nint cfgPtr);

    [LibraryImport(NativeLoaderPythonLib, EntryPoint = "polyplug_python_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugPythonLoaderCreate(nint cfgPtr);

    [LibraryImport(NativeLoaderLuaLib, EntryPoint = "polyplug_lua_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugLuaLoaderCreate(nint cfgPtr);

    [LibraryImport(NativeLoaderJsLib, EntryPoint = "polyplug_js_loader_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    public static partial nint PolyplugJsLoaderCreate(nint cfgPtr);
}