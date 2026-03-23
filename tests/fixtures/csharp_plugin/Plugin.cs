// C# test plugin for polyplug benchmarks and integration tests.
// Exposes a simple "test.add" contract with add, add_primitive, version, and reset functions.
//
// ABI types use LayoutKind.Explicit with FieldOffset for exact byte-level compatibility
// with Rust's #[repr(C)] layout.

using System.Runtime.InteropServices;

namespace CsharpPlugin;

// ABI types - must match polyplug_abi layout exactly

/// <summary>
/// Non-owning UTF-8 string view (16 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 16)]
public struct StringView
{
    [FieldOffset(0)]
    public nint Ptr;

    [FieldOffset(8)]
    public nuint Len;
}

/// <summary>
/// ABI error result (24 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 24)]
public struct AbiError
{
    [FieldOffset(0)]
    public uint Code;

    [FieldOffset(8)]
    public StringView Message;
}

/// <summary>
/// Opaque plugin handle (8 bytes, align 4).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 8)]
public struct PluginHandle
{
    [FieldOffset(0)]
    public uint Index;

    [FieldOffset(4)]
    public uint Generation;
}

/// <summary>
/// Host context passed to plugin functions (16 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 16)]
public struct HostContext
{
    [FieldOffset(0)]
    public nint Runtime;

    [FieldOffset(8)]
    public ulong BundleId;
}

/// <summary>
/// Dispatch mechanism type (4 bytes, align 4).
/// </summary>
public enum DispatchType : uint
{
    Native = 0,
    VirtualMachine = 1
}

/// <summary>
/// Native dispatch data (8 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 8)]
public struct NativeDispatch
{
    [FieldOffset(0)]
    public nint Functions;
}

/// <summary>
/// VM dispatch data (16 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 16)]
public struct VmDispatch
{
    [FieldOffset(0)]
    public nint Call;

    [FieldOffset(8)]
    public nint LoaderData;
}

/// <summary>
/// Union of dispatch mechanisms (16 bytes, align 8).
/// Access based on dispatch_type in PluginInterface.
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 16)]
public struct PluginDispatch
{
    [FieldOffset(0)]
    public NativeDispatch Native;

    [FieldOffset(0)]
    public VmDispatch Vm;
}

/// <summary>
/// Plugin interface (48 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 48)]
public struct PluginInterface
{
    [FieldOffset(0)]
    public nint RtCtx;

    [FieldOffset(8)]
    public ulong ContractId;

    [FieldOffset(16)]
    public uint ContractVersion;

    [FieldOffset(20)]
    public uint FunctionCount;

    [FieldOffset(24)]
    public DispatchType DispatchType;

    // Offset 28-31: padding (4 bytes)

    [FieldOffset(32)]
    public PluginDispatch Dispatch;
}

/// <summary>
/// Plugin descriptor (48 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 48)]
public struct PluginDescriptor
{
    [FieldOffset(0)]
    public StringView Name;

    [FieldOffset(16)]
    public StringView ContractName;

    [FieldOffset(32)]
    public uint VersionMajor;

    [FieldOffset(36)]
    public uint VersionMinor;

    [FieldOffset(40)]
    public uint VersionPatch;

    // Offset 44-47: padding (4 bytes)
}

/// <summary>
/// Context passed to polyplug_init (32 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 32)]
public struct PluginContext
{
    [FieldOffset(0)]
    public StringView BundlePath;

    [FieldOffset(16)]
    public uint HostAbiVersion;

    // Offset 20-23: padding (4 bytes)

    [FieldOffset(24)]
    public ulong BundleId;
}

/// <summary>
/// Host vtable (64 bytes, align 8).
/// </summary>
[StructLayout(LayoutKind.Explicit, Size = 64)]
public struct HostVTable
{
    [FieldOffset(0)]
    public nint RegisterPluginPtr;

    [FieldOffset(8)]
    public nint AllocPtr;

    [FieldOffset(16)]
    public nint FreePtr;

    [FieldOffset(24)]
    public nint FindByContractPtr;

    [FieldOffset(32)]
    public nint FindByBundlePtr;

    [FieldOffset(40)]
    public nint FindAllByContractPtr;

    [FieldOffset(48)]
    public nint ResolvePluginPtr;

    [FieldOffset(56)]
    public nint GetExtensionPtr;
}

// test.add contract ID = FNV-1a("test.add@1") = 0xCC4232FAB0410D2B
internal static class Constants
{
    public const ulong TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BUL;
    public const uint ABI_OK = 0;
}

[StructLayout(LayoutKind.Explicit, Size = 8)]
public struct AddArgs
{
    [FieldOffset(0)]
    public uint A;

    [FieldOffset(4)]
    public uint B;
}

public static class Plugin
{
    private static readonly nint[] s_functions = new nint[4];
    private static PluginInterface s_interface;
    private static readonly byte[] s_versionBytes = System.Text.Encoding.UTF8.GetBytes("1.0");
    private static readonly byte[] s_nameBytes = System.Text.Encoding.UTF8.GetBytes("csharp_test_adder");
    private static readonly byte[] s_contractBytes = System.Text.Encoding.UTF8.GetBytes("test.add");
    private static readonly PluginDescriptor s_descriptor;
    private static readonly nint s_functionsPtr;

    static Plugin()
    {
        unsafe
        {
            s_functions[0] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Add;
            s_functions[1] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&AddPrimitive;
            s_functions[2] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Version;
            s_functions[3] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Reset;

            var handle = System.Runtime.InteropServices.GCHandle.Alloc(s_functions, GCHandleType.Pinned);
            s_functionsPtr = handle.AddrOfPinnedObject();

            s_interface = new PluginInterface
            {
                RtCtx = nint.Zero,
                ContractId = Constants.TEST_ADD_CONTRACT_ID,
                ContractVersion = 1 << 16,
                FunctionCount = 4,
                DispatchType = DispatchType.Native,
                Dispatch = new PluginDispatch
                {
                    Native = new NativeDispatch { Functions = s_functionsPtr }
                }
            };

            fixed (byte* namePtr = s_nameBytes)
            fixed (byte* contractPtr = s_contractBytes)
            {
                s_descriptor = new PluginDescriptor
                {
                    Name = new StringView { Ptr = (nint)namePtr, Len = (nuint)s_nameBytes.Length },
                    ContractName = new StringView { Ptr = (nint)contractPtr, Len = (nuint)s_contractBytes.Length },
                    VersionMajor = 1,
                    VersionMinor = 0,
                    VersionPatch = 0
                };
            }
        }
    }

    [UnmanagedCallersOnly]
    public static AbiError Add(nint args, nint result)
    {
        unsafe
        {
            var addArgs = (AddArgs*)args;
            var outPtr = (uint*)result;
            *outPtr = addArgs->A + addArgs->B;
        }
        return new AbiError { Code = Constants.ABI_OK };
    }

    [UnmanagedCallersOnly]
    public static AbiError AddPrimitive(nint args, nint result)
    {
        unsafe
        {
            var addArgs = (AddArgs*)args;
            var outPtr = (uint*)result;
            *outPtr = addArgs->A + addArgs->B;
        }
        return new AbiError { Code = Constants.ABI_OK };
    }

    [UnmanagedCallersOnly]
    public static AbiError Version(nint args, nint result)
    {
        unsafe
        {
            var outPtr = (StringView*)result;
            fixed (byte* ptr = s_versionBytes)
            {
                *outPtr = new StringView { Ptr = (nint)ptr, Len = (nuint)s_versionBytes.Length };
            }
        }
        return new AbiError { Code = Constants.ABI_OK };
    }

    [UnmanagedCallersOnly]
    public static AbiError Reset(nint args, nint result)
    {
        return new AbiError { Code = Constants.ABI_OK };
    }

    [UnmanagedCallersOnly(EntryPoint = "PolyplugInit")]
    public static uint PolyplugInit(nint rtCtx, nint hostVTablePtr, nint ctxPtr)
    {
        unsafe
        {
            if (hostVTablePtr == nint.Zero)
                return 1;

            var hostVTable = (HostVTable*)hostVTablePtr;
            var registerPlugin = (delegate* unmanaged<nint, PluginDescriptor*, PluginInterface*, AbiError>)hostVTable->RegisterPluginPtr;

            s_interface.RtCtx = rtCtx;

            fixed (PluginDescriptor* descPtr = &s_descriptor)
            fixed (PluginInterface* ifacePtr = &s_interface)
            {
                var result = registerPlugin(rtCtx, descPtr, ifacePtr);
                return result.Code;
            }
        }
    }
}