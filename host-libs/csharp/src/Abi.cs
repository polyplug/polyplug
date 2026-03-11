// THIS FILE IS HAND-WRITTEN (not generated). Part of the Polyplug host library.
using System.Runtime.InteropServices;

namespace Polyplug;

[StructLayout(LayoutKind.Sequential)]
public struct StringView {
    public IntPtr Ptr;   // 8 bytes — IntPtr is pointer-sized, ABI-identical to byte*
    public ulong  Len;   // 8 bytes — ulong matches Rust usize on 64-bit
    public static readonly StringView Empty = default;
    public bool IsEmpty => Len == 0;
    public override string ToString() =>
        Ptr == IntPtr.Zero ? string.Empty
        : Marshal.PtrToStringUTF8(Ptr, (int)Len) ?? string.Empty;
    public static explicit operator string(StringView sv) => sv.ToString();
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginContext {
    public StringView BundlePath;
}

[StructLayout(LayoutKind.Sequential)]
public struct Buffer {
    public IntPtr Ptr;   // 8 bytes — always host-allocated
    public ulong  Len;   // 8 bytes — bytes currently used
    public ulong  Cap;   // 8 bytes — bytes allocated — total: 24 bytes
    public bool IsEmpty => Len == 0;
}

[StructLayout(LayoutKind.Sequential)]
public struct AbiError {
    public uint  Code;
    private uint _pad;
    public StringView Message;
    public static AbiError Ok => new AbiError { Code = 0, Message = StringView.Empty };
    public static AbiError FromException(Exception ex) =>
        new AbiError { Code = AbiConstants.ABI_ERROR_PANIC, Message = StringView.Empty };
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle {
    public uint Index;
    public uint Generation;
    public static readonly PluginHandle Null =
        new PluginHandle { Index = uint.MaxValue, Generation = 0 };
    public bool IsNull => Index == uint.MaxValue;
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginVTable {
    public ulong  ContractId;
    public uint   ContractVersion;
    public uint   FunctionCount;
    public IntPtr FunctionsPtr;     // void** → IntPtr
}

[StructLayout(LayoutKind.Sequential)]
public struct HostVTable {
    public IntPtr AllocPtr;             // delegate* unmanaged[Cdecl]<nuint, nuint, byte*>
    public IntPtr FreePtr;              // delegate* unmanaged[Cdecl]<byte*, nuint, nuint, void>
    // SuppressGCTransition: safe because these are short non-blocking Rust calls that never call back into managed code:
    public IntPtr FindByContractPtr;    // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>
    public IntPtr FindByBundlePtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, ulong, uint, PluginHandle>
    public IntPtr FindAllByContractPtr; // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle*, nuint, nuint>
    public IntPtr ResolvePluginPtr;     // delegate* unmanaged[Cdecl, SuppressGCTransition]<PluginHandle, PluginVTable*>
    public IntPtr GetExtensionPtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<uint, void*>
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginDescriptor {
    public StringView Name;
    public StringView ContractName;
    public uint       VersionMajor;
    public uint       VersionMinor;
    public uint       VersionPatch;
    private uint      _pad;
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginRegistrar {
    public IntPtr RegisterPluginPtr;  // delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError>
    public IntPtr HostPtr;            // HostVTable*
}

public static class AbiConstants {
    public const uint ABI_OK              = 0;
    public const uint ABI_ERROR_GENERIC   = 1;
    public const uint ABI_BUFFER_TOO_SMALL = 2;
    public const uint ABI_ERROR_PANIC     = 3;
}
