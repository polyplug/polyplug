using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

// All FFI struct types are imported from the auto-generated Abi.cs (Polyplug.Abi namespace).
// Per D-26/D-27: No hand-written [StructLayout] or [UnmanagedFunctionPointer] definitions remain.
using Polyplug.Abi;

namespace Polyplug.Host;

internal static partial class NativeMethods
{
    private const string NativeLib = "polyplug";

    // ─── FFI Entry Points (18-02: Only 2 exports) ─────────────────────────────────

    /// <summary>
    /// Creates a new runtime instance with default configuration.
    /// Returns a HostInterface pointer that provides all runtime operations.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint PolyplugRuntimeCreate();

    /// <summary>
    /// Creates a new runtime instance with the specified options.
    /// Returns a HostInterface pointer that provides all runtime operations.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create_with_options")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint PolyplugRuntimeCreateWithOptions(nint options);

    /// <summary>
    /// Destroys a runtime instance.
    /// Takes HostInterface pointer returned by polyplug_runtime_create.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void PolyplugRuntimeDestroy(nint host);

    // Hot-reload configuration (must be called before runtime creation)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_on_reload")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial uint PolyplugRuntimeOnReload(nint callback);
}
