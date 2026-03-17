using System;
using System.Runtime.InteropServices;

namespace Polyplug;

[StructLayout(LayoutKind.Sequential)]
public struct StringView
{
    public static readonly StringView Empty = default;

    public nint Ptr;    // 8 bytes — ABI-identical to byte*
    public ulong Len;   // 8 bytes

    public StringView(nint ptr, ulong len)
    {
        Ptr = ptr;
        Len = len;
    }

    public readonly bool IsEmpty()
    {
        return Len == 0;
    }

    public override readonly string ToString()
    {
        return Ptr == nint.Zero ? string.Empty : Marshal.PtrToStringUTF8(Ptr, (int)Len) ?? string.Empty;
    }

    public static explicit operator string(StringView sv)
    {
        return sv.ToString();
    }
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginContext
{
    public StringView BundlePath;
}

[StructLayout(LayoutKind.Sequential)]
public struct Buffer
{
    public nint Ptr;   // 8 bytes — always host-allocated
    public ulong Len;   // 8 bytes — bytes currently used
    public ulong Cap;   // 8 bytes — bytes allocated — total: 24 bytes

    public readonly bool IsEmpty()
    {
        return Len == 0;
    }
}

[StructLayout(LayoutKind.Sequential)]
public struct AbiError
{
    public uint Code;
    private uint _pad;
    public StringView Message;

    public static AbiError Ok()
    {
        return new AbiError { Code = 0, Message = StringView.Empty };
    }

    public static AbiError FromException(Exception ex)
    {
        return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC, Message = StringView.Empty };
    }
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle
{
    public static readonly PluginHandle Null = new() { Index = uint.MaxValue, Generation = 0 };

    public uint Index;
    public uint Generation;

    public readonly bool IsNull()
    {
        return Index == uint.MaxValue;
    }
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginVTable
{
    public ulong ContractId;
    public uint ContractVersion;
    public uint FunctionCount;
    public nint FunctionsPtr;     // void** → nint
}

[StructLayout(LayoutKind.Sequential)]
public struct HostVTable
{
    public nint AllocPtr;             // delegate* unmanaged[Cdecl]<nuint, nuint, byte*>
    public nint FreePtr;              // delegate* unmanaged[Cdecl]<byte*, nuint, nuint, void>
    // SuppressGCTransition: safe because these are short non-blocking Rust calls that never call back into managed code:
    public nint FindByContractPtr;    // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>
    public nint FindByBundlePtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, ulong, uint, PluginHandle>
    public nint FindAllByContractPtr; // delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle*, nuint, nuint>
    public nint ResolvePluginPtr;     // delegate* unmanaged[Cdecl, SuppressGCTransition]<PluginHandle, PluginVTable*>
    public nint GetExtensionPtr;      // delegate* unmanaged[Cdecl, SuppressGCTransition]<uint, void*>
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginDescriptor
{
    public StringView Name;
    public StringView ContractName;
    public uint VersionMajor;
    public uint VersionMinor;
    public uint VersionPatch;
    private readonly uint _pad;
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginRegistrar
{
    [MarshalAs(UnmanagedType.FunctionPtr)]
    public nint RegisterPluginPtr;  // delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError>

    [MarshalAs(UnmanagedType.LPStruct)]
    public HostVTable HostPtr;            // HostVTable*
}

public static class AbiConstants
{
    public const uint ABI_OK = 0;
    public const uint ABI_ERROR_GENERIC = 1;
    public const uint ABI_BUFFER_TOO_SMALL = 2;
    public const uint ABI_ERROR_PANIC = 3;
}

public static class ContractId
{
    private const ulong FNV_OFFSET = 0xcbf29ce484222325UL;
    private const ulong FNV_PRIME = 0x00000100000001B3UL;

    public static ulong Compute(string name, uint majorVersion)
    {
        var s = $"{name}@{majorVersion}";
        var bytes = System.Text.Encoding.UTF8.GetBytes(s);
        ulong h = FNV_OFFSET;
        foreach (var b in bytes)
        {
            h ^= b;
            h = checked(h * FNV_PRIME);
        }
        return h;
    }

    public static ulong BundleId(string name)
    {
        var bytes = System.Text.Encoding.UTF8.GetBytes(name);
        ulong h = FNV_OFFSET;
        foreach (var b in bytes)
        {
            h ^= b;
            h = checked(h * FNV_PRIME);
        }
        return h;
    }
}
