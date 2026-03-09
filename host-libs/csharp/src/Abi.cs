// THIS FILE IS HAND-WRITTEN (not generated). Part of the Polyplug host library.
using System.Runtime.InteropServices;

namespace Polyplug;

[StructLayout(LayoutKind.Sequential)]
public unsafe struct StringView {
    public byte* Ptr;
    public nuint Len;
    public static StringView Null => new StringView { Ptr = null, Len = 0 };
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct Buffer {
    public byte* Ptr;
    public nuint Len;
    public nuint Cap;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct AbiError {
    public uint  Code;
    private uint _pad;
    public StringView Message;
    public static AbiError Ok => new AbiError { Code = 0, Message = StringView.Null };
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle {
    public uint Index;
    public uint Generation;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct PluginVTable {
    public ulong ContractId;
    public uint  ContractVersion;
    public uint  FunctionCount;
    public void** Functions;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct HostVTable {
    public delegate* unmanaged[Cdecl]<nuint, nuint, byte*> Alloc;
    public delegate* unmanaged[Cdecl]<byte*, nuint, nuint, void> Free;
    // SuppressGCTransition: safe because these are short non-blocking Rust calls that never call back into managed code:
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>                                    FindByContract;    // find_by_contract(contract_id, min_version) → handle
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, ulong, uint, PluginHandle>                             FindByBundle;      // find_by_bundle(bundle_id, contract_id, min_version) → handle
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle*, nuint, nuint>                     FindAllByContract; // find_all_by_contract(contract_id, min_version, out, out_cap) → count
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<PluginHandle, PluginVTable*>                                  ResolvePlugin;     // resolve_plugin(handle) → vtable ptr
    public delegate* unmanaged[Cdecl, SuppressGCTransition]<uint, void*>                                                  GetExtension;      // get_extension(extension_id) → vtable ptr
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct PluginDescriptor {
    public StringView Name;
    public StringView ContractName;
    public uint       VersionMajor;
    public uint       VersionMinor;
    public uint       VersionPatch;
    private uint      _pad;
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct PluginRegistrar {
    public delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError> RegisterPlugin;
    public HostVTable* Host;
}

public static class AbiConstants {
    public const uint ABI_OK              = 0;
    public const uint ABI_ERROR_GENERIC   = 1;
    public const uint ABI_BUFFER_TOO_SMALL = 2;
    public const uint ABI_ERROR_PANIC     = 3;
}
