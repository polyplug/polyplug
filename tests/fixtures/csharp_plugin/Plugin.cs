// C# test plugin for polyplug benchmarks and integration tests.
// Exposes a simple "test.add" contract with add, add_primitive, version, and reset functions.

using System.Runtime.InteropServices;
using Polyplug.Abi;
using static Polyplug.Abi.AbiConstants;

namespace CsharpPlugin;

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
    // test.add contract ID = FNV-1a("test.add@1") = 0xCC4232FAB0410D2B
    private const ulong TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BUL;

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
                ContractId = TEST_ADD_CONTRACT_ID,
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
        return new AbiError { Code = ABI_OK };
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
        return new AbiError { Code = ABI_OK };
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
        return new AbiError { Code = ABI_OK };
    }

    [UnmanagedCallersOnly]
    public static AbiError Reset(nint args, nint result)
    {
        return new AbiError { Code = ABI_OK };
    }

    [UnmanagedCallersOnly(EntryPoint = "PolyplugInit")]
    public static uint PolyplugInit(nint rtCtx, nint hostVTablePtr, nint ctxPtr)
    {
        unsafe
        {
            if (hostVTablePtr == nint.Zero)
                return 1;

            var hostVTable = (HostVTable*)hostVTablePtr;
            var registerPlugin = (delegate* unmanaged<nint, PluginDescriptor*, PluginInterface*, AbiError>)hostVTable->RegisterPlugin;

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