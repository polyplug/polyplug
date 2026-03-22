// C# test plugin for polyplug benchmarks and integration tests.
// Exposes a simple "test.add" contract with add, add_primitive, version, and reset functions.

using System.Runtime.InteropServices;

namespace CsharpPlugin;

// ABI types - must match polyplug_abi layout exactly
[StructLayout(LayoutKind.Sequential)]
public struct StringView
{
    public nint Ptr;
    public nuint Len;
}

[StructLayout(LayoutKind.Sequential)]
public struct AbiError
{
    public uint Code;
    private uint _pad;
    public StringView Message;
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle
{
    public uint Index;
    public uint Generation;
}

[StructLayout(LayoutKind.Sequential)]
public struct HostContext
{
    public nint Runtime;
    public ulong BundleId;
}

public enum DispatchType : uint
{
    Native = 0,
    VirtualMachine = 1
}

[StructLayout(LayoutKind.Sequential)]
public struct NativeDispatch
{
    public nint Functions;
}

[StructLayout(LayoutKind.Explicit)]
public struct PluginDispatch
{
    [FieldOffset(0)]
    public NativeDispatch Native;
}

[StructLayout(LayoutKind.Sequential)]
public struct PluginInterface
{
    public nint RtCtx;
    public ulong ContractId;
    public uint ContractVersion;
    public uint FunctionCount;
    public DispatchType DispatchType;
    private uint _pad;
    public PluginDispatch Dispatch;
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
public struct PluginContext
{
    public StringView BundlePath;
    public uint HostAbiVersion;
    private uint _pad;
    public ulong BundleId;
}

[StructLayout(LayoutKind.Sequential)]
public struct HostVTable
{
    public nint RegisterPluginPtr;
    public nint AllocPtr;
    public nint FreePtr;
    public nint FindByContractPtr;
    public nint FindByBundlePtr;
    public nint FindAllByContractPtr;
    public nint ResolvePluginPtr;
    public nint GetExtensionPtr;
}

// test.add contract ID = FNV-1a("test.add@1") = 0xCC4232FAB0410D2B
internal static class Constants
{
    public const ulong TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BUL;
    public const uint ABI_OK = 0;
}

[StructLayout(LayoutKind.Sequential)]
public struct AddArgs
{
    public uint A;
    public uint B;
}

public static class Plugin
{
    private static readonly nint[] s_functions = new nint[4];
    private static readonly PluginInterface s_interface;
    private static readonly byte[] s_versionBytes = System.Text.Encoding.UTF8.GetBytes("1.0");
    private static readonly byte[] s_nameBytes = System.Text.Encoding.UTF8.GetBytes("csharp_test_adder");
    private static readonly byte[] s_contractBytes = System.Text.Encoding.UTF8.GetBytes("test.add");
    private static readonly PluginDescriptor s_descriptor;

    static Plugin()
    {
        unsafe
        {
            s_functions[0] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Add;
            s_functions[1] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&AddPrimitive;
            s_functions[2] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Version;
            s_functions[3] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Reset;

            var handle = System.Runtime.InteropServices.GCHandle.Alloc(s_functions, GCHandleType.Pinned);
            var functionsPtr = handle.AddrOfPinnedObject();

            s_interface = new PluginInterface
            {
                RtCtx = nint.Zero,
                ContractId = Constants.TEST_ADD_CONTRACT_ID,
                ContractVersion = 1 << 16,
                FunctionCount = 4,
                DispatchType = DispatchType.Native,
                Dispatch = new PluginDispatch
                {
                    Native = new NativeDispatch { Functions = functionsPtr }
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
        if (hostVTablePtr == nint.Zero)
            return 1;

        unsafe
        {
            var hostVTable = (HostVTable*)hostVTablePtr;
            var registerPlugin = (delegate* unmanaged<nint, PluginDescriptor*, PluginInterface*, AbiError>)hostVTable->RegisterPluginPtr;

            var iface = s_interface;
            iface.RtCtx = rtCtx;

            fixed (PluginDescriptor* descPtr = &s_descriptor)
            {
                var result = registerPlugin(rtCtx, descPtr, &iface);
                return result.Code;
            }
        }
    }
}