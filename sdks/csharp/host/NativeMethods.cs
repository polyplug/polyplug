using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

// All FFI struct types are imported from the auto-generated Abi.cs (Polyplug.Abi namespace).
// Per D-26/D-27: No hand-written [StructLayout] or [UnmanagedFunctionPointer] definitions remain.
using Polyplug.Abi;

namespace Polyplug.Host;

internal static partial class NativeMethods
{
    private const string NativeLib = "polyplug";

    // ─── FFI Entry Points ────────────────────────────────────────────────────

    /// <summary>
    /// Creates a new runtime instance.
    /// Pass a pointer to a <c>RuntimeConfig</c> struct to configure compatibility,
    /// hot-reload, and the reload callback, or <c>nint.Zero</c> to use defaults.
    /// The native side reads the config during the call; the caller may free it
    /// once the call returns.
    /// Returns a HostApi pointer that provides all runtime operations.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint PolyplugRuntimeCreate(nint config);

    /// <summary>
    /// Destroys a runtime instance.
    /// Takes HostApi pointer returned by polyplug_runtime_create.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void PolyplugRuntimeDestroy(nint host);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_begin_in_process_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static unsafe partial void PolyplugBeginInProcessBundle(
        nint host,
        byte* manifestBytes,
        nuint manifestLen,
        uint language,
        ulong* outBundleId,
        AbiError* outError);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_commit_in_process_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static unsafe partial void PolyplugCommitInProcessBundle(
        nint host,
        ulong bundleId,
        AbiError* outError);

    [LibraryImport(NativeLib, EntryPoint = "polyplug_abort_in_process_bundle")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void PolyplugAbortInProcessBundle(nint host, ulong bundleId);
}
