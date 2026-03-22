// THIS FILE IS HAND-WRITTEN (not generated). Part of the Polyplug.Guest library.
// REQUIREMENT: All [UnmanagedCallersOnly] methods in guest plugins MUST declare CallConvs = new[] { typeof(CallConvCdecl) }
using System.Runtime.InteropServices;

namespace Polyplug.Guest;

/// <summary>Non-owning UTF-8 string view. ptr(8) + len(8) = 16 bytes. Matches Rust StringView.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct StringView
{
    public IntPtr Ptr;  // 8 bytes — UTF-8 bytes, NOT null-terminated (IntPtr = pointer-sized = 8 bytes on 64-bit)
    public ulong  Len;  // 8 bytes — byte count — total: 16 bytes

    public static readonly StringView Empty = default;
    public bool IsEmpty => Len == 0;

    public override string ToString() =>
        Ptr == IntPtr.Zero ? string.Empty
        : Marshal.PtrToStringUTF8(Ptr, (int)Len) ?? string.Empty;

    public static explicit operator string(StringView sv) => sv.ToString();
}

/// <summary>Context passed to every guest PolyplugInit function.</summary>
/// <remarks>BundlePath.Ptr is runtime-owned. Do not store the raw pointer — copy the string.</remarks>
[StructLayout(LayoutKind.Sequential)]
public struct PluginContext
{
    public StringView BundlePath;
}

/// <summary>Owning byte buffer. ptr(8) + len(8) + cap(8) = 24 bytes. Matches Rust Buffer.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct Buffer
{
    public IntPtr Ptr;  // 8 bytes — always host-allocated
    public ulong  Len;  // 8 bytes — bytes currently used
    public ulong  Cap;  // 8 bytes — bytes allocated — total: 24 bytes

    public bool IsEmpty => Len == 0;
}

/// <summary>
/// ABI error returned by value from all ABI calls.
/// code(4) + _pad(4) + message(StringView=16) = 24 bytes. Matches Rust AbiError.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct AbiError
{
    public uint Code;       // 4 bytes — 0 = success, non-zero = error
    private uint _pad;      // 4 bytes explicit padding — aligns message to offset 8, matches Rust layout
    public StringView Message;  // 16 bytes — total: 24 bytes

    public static AbiError Ok => new AbiError { Code = 0, Message = StringView.Empty };

    public static AbiError FromException(Exception ex) =>
        new AbiError { Code = AbiConstants.ABI_ERROR_PANIC, Message = StringView.Empty };
}

/// <summary>Opaque handle to a loaded plugin. index(4) + generation(4) = 8 bytes. Matches Rust PluginHandle.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle
{
    public uint Index;       // 4 bytes — slot in the registry array
    public uint Generation;  // 4 bytes — generation counter — total: 8 bytes

    public static readonly PluginHandle Null =
        new PluginHandle { Index = uint.MaxValue, Generation = 0 };
    public bool IsNull => Index == uint.MaxValue;
}

/// <summary>Dispatch mechanism type — determines how function calls are routed.</summary>
[StructLayout(LayoutKind.Sequential)]
public enum DispatchType : uint
{
    /// <summary>Native dispatch: direct function pointer calls (zero overhead).</summary>
    Native = 0,
    /// <summary>VM dispatch: call through a dispatch function with loader_data.</summary>
    VirtualMachine = 1,
}

/// <summary>Native dispatch data — direct function pointer array.</summary>
/// <remarks>Used when <see cref="DispatchType"/> == <see cref="DispatchType.Native"/>.</remarks>
[StructLayout(LayoutKind.Sequential)]
public struct NativeDispatch
{
    /// <summary>Pointer to a static array of function pointers, indexed by function_id.</summary>
    public IntPtr Functions;  // 8 bytes — *const *const ()
}

/// <summary>VM dispatch data — call through a dispatch function.</summary>
/// <remarks>Used when <see cref="DispatchType"/> == <see cref="DispatchType.VirtualMachine"/>.</remarks>
[StructLayout(LayoutKind.Sequential)]
public struct VmDispatch
{
    /// <summary>Dispatch function called for every VM function invocation.</summary>
    public IntPtr Call;       // 8 bytes — function pointer
    /// <summary>Loader-specific data (e.g., LuaLoaderData, JsLoaderData).</summary>
    public IntPtr LoaderData; // 8 bytes — opaque loader data
}

/// <summary>Union of dispatch mechanisms — use based on <see cref="PluginInterface.DispatchType"/>.</summary>
[StructLayout(LayoutKind.Explicit)]
public struct PluginDispatch
{
    /// <summary>Native dispatch data (when DispatchType == Native).</summary>
    [FieldOffset(0)]
    public NativeDispatch Native;
    /// <summary>VM dispatch data (when DispatchType == VirtualMachine).</summary>
    [FieldOffset(0)]
    public VmDispatch Vm;
}

/// <summary>
/// Plugin interface — one per contract implemented by a plugin.
/// rt_ctx(8) + contract_id(8) + contract_version(4) + function_count(4) + dispatch_type(4) + _pad(4) + dispatch(16) = 48 bytes.
/// Matches Rust PluginInterface.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct PluginInterface
{
    /// <summary>Pointer to the host context for this plugin.</summary>
    public IntPtr RtCtx;            // 8 bytes — *const HostContext
    /// <summary>FNV-1a hash of "contract_name@major_version".</summary>
    public ulong  ContractId;       // 8 bytes
    /// <summary>minor.patch encoded as (minor &lt;&lt; 16 | patch).</summary>
    public uint   ContractVersion;  // 4 bytes
    /// <summary>Number of valid entries in the dispatch array.</summary>
    public uint   FunctionCount;    // 4 bytes
    /// <summary>Dispatch mechanism type (Native or VirtualMachine).</summary>
    public DispatchType DispatchType; // 4 bytes
    /// <summary>Padding for alignment.</summary>
    private uint  _pad;             // 4 bytes explicit padding
    /// <summary>Union of dispatch mechanisms — access based on DispatchType.</summary>
    public PluginDispatch Dispatch; // 16 bytes — total: 48 bytes
}

/// <summary>
/// Backward-compatible alias for PluginInterface.
/// </summary>
public struct PluginVTable
{
    /// <summary>Use the PluginInterface fields directly.</summary>
    public PluginInterface Interface;
}

/// <summary>
/// Host capabilities passed to every plugin at init time.
/// 7 × IntPtr(8) = 56 bytes. Matches Rust HostVTable.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct HostVTable
{
    public IntPtr AllocPtr;             // delegate* unmanaged[Cdecl]<nuint,nuint,byte*> — host_alloc(size, align) → ptr
    public IntPtr FreePtr;              // delegate* unmanaged[Cdecl]<byte*,nuint,nuint,void> — host_free(ptr, size, align)
    // SuppressGCTransition: safe because these are short non-blocking Rust calls that never call back into managed code
    public IntPtr FindByContractPtr;    // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>
    public IntPtr FindByBundlePtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, ulong, uint, PluginHandle>
    public IntPtr FindAllByContractPtr; // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle*, nuint, nuint>
    public IntPtr ResolvePluginPtr;     // delegate* unmanaged[Cdecl, SuppressGCTransition]<PluginHandle, PluginVTable*>
    public IntPtr GetExtensionPtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<uint, void*>
}

/// <summary>
/// Plugin metadata passed during init.
/// name(16) + contract_name(16) + version_major(4) + version_minor(4) + version_patch(4) + _pad(4) = 48 bytes.
/// Matches Rust PluginDescriptor.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct PluginDescriptor
{
    public StringView Name;          // 16 bytes — human-readable plugin name
    public StringView ContractName;  // 16 bytes — full contract name for collision detection
    public uint       VersionMajor;  // 4 bytes
    public uint       VersionMinor;  // 4 bytes
    public uint       VersionPatch;  // 4 bytes
    private uint      _pad;          // 4 bytes explicit tail padding — total: 48 bytes, matches Rust layout
}

/// <summary>
/// Bridge used during polyplug_init only — not stored long-term.
/// register_plugin fn-ptr(8) + host ptr(8) = 16 bytes. Matches Rust PluginRegistrar.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public struct PluginRegistrar
{
    public IntPtr RegisterPluginPtr; // delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError> — 8 bytes
    public IntPtr HostPtr;           // HostVTable* — 8 bytes — total: 16 bytes
}

/// <summary>ABI error code constants. Must match Rust ABI constants exactly.</summary>
public static class AbiConstants
{
    public const uint ABI_OK               = 0;
    public const uint ABI_ERROR_GENERIC    = 1;
    public const uint ABI_BUFFER_TOO_SMALL = 2;
    public const uint ABI_ERROR_PANIC      = 3;
    public const uint ABI_ERROR_NOT_FOUND  = 4;
    public const uint ABI_ERROR_STALE_HANDLE = 5;
    public const uint ABI_FUNCTION_NOT_AVAIL = 6;
}
