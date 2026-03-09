// THIS FILE IS HAND-WRITTEN (not generated). Part of the Polyplug.Guest library.
// REQUIREMENT: All [UnmanagedCallersOnly] methods in guest plugins MUST declare CallConvs = new[] { typeof(CallConvCdecl) }
using System.Runtime.InteropServices;

namespace Polyplug.Guest;

/// <summary>Non-owning UTF-8 string view. ptr(8) + len(8) = 16 bytes. Matches Rust StringView.</summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct StringView
{
    public byte* Ptr;   // 8 bytes — UTF-8 bytes, NOT null-terminated
    public nuint Len;   // 8 bytes — byte count — total: 16 bytes

    public static StringView Null => new StringView { Ptr = null, Len = 0 };
}

/// <summary>Owning byte buffer. ptr(8) + len(8) + cap(8) = 24 bytes. Matches Rust Buffer.</summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct Buffer
{
    public byte* Ptr;   // 8 bytes — always host-allocated
    public nuint Len;   // 8 bytes — bytes currently used
    public nuint Cap;   // 8 bytes — bytes allocated — total: 24 bytes
}

/// <summary>
/// ABI error returned by value from all ABI calls.
/// code(4) + _pad(4) + message(StringView=16) = 24 bytes. Matches Rust AbiError.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct AbiError
{
    public uint Code;       // 4 bytes — 0 = success, non-zero = error
    private uint _pad;      // 4 bytes explicit padding — aligns message to offset 8, matches Rust layout
    public StringView Message;  // 16 bytes — total: 24 bytes

    public static AbiError Ok => new AbiError { Code = 0, Message = StringView.Null };
}

/// <summary>Opaque handle to a loaded plugin. index(4) + generation(4) = 8 bytes. Matches Rust PluginHandle.</summary>
[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle
{
    public uint Index;       // 4 bytes — slot in the registry array
    public uint Generation;  // 4 bytes — generation counter — total: 8 bytes
}

/// <summary>
/// Plugin VTable — one per contract implemented by a plugin.
/// contract_id(8) + contract_version(4) + function_count(4) + functions(8) = 24 bytes.
/// Matches Rust PluginVTable.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct PluginVTable
{
    public ulong ContractId;       // 8 bytes — FNV-1a hash of "contract_name@major_version"
    public uint  ContractVersion;  // 4 bytes — minor.patch encoded as (minor << 16 | patch)
    public uint  FunctionCount;    // 4 bytes — number of valid entries in Functions
    public void** Functions;       // 8 bytes — pointer to static array of fn ptrs — total: 24 bytes
}

/// <summary>
/// Host capabilities passed to every plugin at init time.
/// 7 × fn-ptr(8) = 56 bytes. Matches Rust HostVTable.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct HostVTable
{
    public delegate* unmanaged[Cdecl]<nuint, nuint, byte*>                                                     Alloc;            // host_alloc(size, align) → ptr
    public delegate* unmanaged[Cdecl]<byte*, nuint, nuint, void>                                               Free;             // host_free(ptr, size, align)
    // SuppressGCTransition: safe because these are short non-blocking Rust calls that never call back into managed code
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>                        FindByContract;    // find_by_contract(contract_id, min_version) → handle
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, ulong, uint, PluginHandle>                 FindByBundle;      // find_by_bundle(bundle_id, contract_id, min_version) → handle
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle*, nuint, nuint>         FindAllByContract; // find_all_by_contract(contract_id, min_version, out, out_cap) → count
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<PluginHandle, PluginVTable*>                      ResolvePlugin;     // resolve_plugin(handle) → vtable ptr
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<uint, void*>                                      GetExtension;      // get_extension(extension_id) → vtable ptr
}

/// <summary>
/// Plugin metadata passed during init.
/// name(16) + contract_name(16) + version_major(4) + version_minor(4) + version_patch(4) + _pad(4) = 48 bytes.
/// Matches Rust PluginDescriptor.
/// </summary>
[StructLayout(LayoutKind.Sequential)]
public unsafe struct PluginDescriptor
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
public unsafe struct PluginRegistrar
{
    public delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError> RegisterPlugin; // 8 bytes
    public HostVTable* Host;  // 8 bytes — total: 16 bytes
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
