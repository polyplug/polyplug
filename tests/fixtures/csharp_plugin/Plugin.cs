// C# test plugin for polyplug benchmarks and integration tests.
// Exposes a simple "test.add" contract with add, add_primitive, version, and reset functions.
// reset additionally logs through the guest SDK's PolyplugHost.Log so the host-side
// integration suite can prove a real .NET guest log reaches a host-installed logger.

using System.Runtime.InteropServices;
using Polyplug.Abi;
using Polyplug.Guest;

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
    // test.add contract ID = FNV-1a("guest_contract:test.add@1") = 0x40244DF59FCBECB6
    // Must match polyplug_utils::guest_contract_id("test.add", 1).
    private const ulong TEST_ADD_CONTRACT_ID = 0x40244DF59FCBECB6UL;

    private static readonly nint[] s_functions = new nint[4];
    private static GuestContractInterface s_interface;
    private static readonly byte[] s_versionBytes = System.Text.Encoding.UTF8.GetBytes("1.0");
    private static readonly byte[] s_nameBytes = System.Text.Encoding.UTF8.GetBytes("csharp_test_adder");
    private static readonly byte[] s_contractBytes = System.Text.Encoding.UTF8.GetBytes("test.add");
    private static readonly PluginDescriptor s_descriptor;
    private static readonly nint s_functionsPtr;

    // Pinned for the process lifetime: every pointer that escapes to the host
    // (descriptor StringViews, the version StringView returned by Version, the
    // dispatch function array) must come from a pinned object, mirroring the
    // GCHandle.Alloc(..., Pinned) pattern in the generated C# guest code.
    // Pointers taken inside a `fixed` block become invalid the moment the
    // block exits — the GC may move the array afterwards.
    private static readonly GCHandle s_functionsPin;
    private static readonly GCHandle s_versionPin;
    private static readonly GCHandle s_namePin;
    private static readonly GCHandle s_contractPin;

    // create_instance / destroy_instance are REQUIRED ABI fields — the runtime
    // rejects a null fn pointer at registration. This contract is stateless, so
    // the stubs mirror the generated C# guest code: create returns a null-data
    // instance stamped with the contract id, destroy is a no-op.
    [UnmanagedCallersOnly]
    private static GuestContractInstance CreateInstanceStub(nint host, nint args)
    {
        return new GuestContractInstance { Data = nint.Zero, ContractId = TEST_ADD_CONTRACT_ID };
    }

    [UnmanagedCallersOnly]
    private static void DestroyInstanceStub(nint host, GuestContractInstance instance)
    {
        // Stateless contract — nothing to clean up.
    }

    static Plugin()
    {
        unsafe
        {
            s_functions[0] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Add;
            s_functions[1] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&AddPrimitive;
            s_functions[2] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Version;
            s_functions[3] = (IntPtr)(delegate* unmanaged<nint, nint, AbiError>)&Reset;

            s_functionsPin = GCHandle.Alloc(s_functions, GCHandleType.Pinned);
            s_functionsPtr = s_functionsPin.AddrOfPinnedObject();

            s_versionPin = GCHandle.Alloc(s_versionBytes, GCHandleType.Pinned);
            s_namePin = GCHandle.Alloc(s_nameBytes, GCHandleType.Pinned);
            s_contractPin = GCHandle.Alloc(s_contractBytes, GCHandleType.Pinned);

            s_interface = new GuestContractInterface
            {
                ContractId = TEST_ADD_CONTRACT_ID,
                ContractVersion = new Polyplug.Abi.Version { Major = 1, Minor = 0, Patch = 0 },
                DispatchType = DispatchType.Native,
                CreateInstance = (IntPtr)(delegate* unmanaged<nint, nint, GuestContractInstance>)&CreateInstanceStub,
                DestroyInstance = (IntPtr)(delegate* unmanaged<nint, GuestContractInstance, void>)&DestroyInstanceStub,
                Dispatch = new DispatchMechanisms
                {
                    Native = new NativeDispatch { FunctionCount = 4, Functions = s_functionsPtr }
                }
            };

            s_descriptor = new PluginDescriptor
            {
                Name = new StringView { Ptr = s_namePin.AddrOfPinnedObject(), Len = (nuint)s_nameBytes.Length },
                ContractName = new StringView { Ptr = s_contractPin.AddrOfPinnedObject(), Len = (nuint)s_contractBytes.Length },
                Version = new Polyplug.Abi.Version { Major = 1, Minor = 0, Patch = 0 }
            };
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
        return new AbiError { Code = (uint)AbiErrorCode.Ok };
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
        return new AbiError { Code = (uint)AbiErrorCode.Ok };
    }

    [UnmanagedCallersOnly]
    public static AbiError Version(nint args, nint result)
    {
        unsafe
        {
            var outPtr = (StringView*)result;
            // The pointer escapes this call (the host reads the StringView after
            // we return), so it must come from the process-lifetime pin — never
            // from a `fixed` block, whose pin ends at the block's closing brace.
            *outPtr = new StringView { Ptr = s_versionPin.AddrOfPinnedObject(), Len = (nuint)s_versionBytes.Length };
        }
        return new AbiError { Code = (uint)AbiErrorCode.Ok };
    }

    [UnmanagedCallersOnly]
    public static AbiError Reset(nint args, nint result)
    {
        // Deterministic guest→host log probe: routes through HostApi.Log via the
        // host pointer stored by PolyplugInit. Non-ASCII characters prove the
        // UTF-16 → UTF-8 boundary transcode. A no-op when no host is stored.
        PolyplugHost.Log(LogLevel.Info, "guest.csharp_test_adder", "héllo from .NET ✓");
        return new AbiError { Code = (uint)AbiErrorCode.Ok };
    }

    [UnmanagedCallersOnly(EntryPoint = "PolyplugInit")]
    public static uint PolyplugInit(nint hostPtr, nint ctxPtr)
    {
        unsafe
        {
            if (hostPtr == nint.Zero)
                return 1;

            RuntimeAbiStorage.StoreRuntimeAbi(hostPtr);

            var host = (HostApi*)hostPtr;

            fixed (PluginDescriptor* descPtr = &s_descriptor)
            fixed (GuestContractInterface* ifacePtr = &s_interface)
            {
                var registerContract =
                    (delegate* unmanaged[Cdecl]<nint, nint, nint, AbiError>)host->RegisterGuestContract;
                AbiError result = registerContract((nint)host, (nint)descPtr, (nint)ifacePtr);
                return (uint)result.Code;
            }
        }
    }
}