// Plugin.cs — C# fixture plugin for polyplug integration tests.
// Implements the test.add@1.0 contract from tests/fixtures/test_api.toml.

using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace CsharpPlugin;

// The arg-pack struct for test.add::add (two u32 params)
[StructLayout(LayoutKind.Sequential)]
public struct TestAddContractAddArgs {
    public uint A;
    public uint B;
}

// The arg-pack struct for test.add::add_primitive (two u32 params)
[StructLayout(LayoutKind.Sequential)]
public struct TestAddContractAddPrimitiveArgs {
    public uint A;
    public uint B;
}

// Static impl — stores the registered implementation
public static unsafe class TestAddImpl {
    private static uint _counter = 0;

    private static readonly byte[] VERSION_BYTES = "1.0"u8.ToArray();

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static AbiError test_add_add_abi(void* args_ptr, void* out_ptr) {
        try {
            var args = *(TestAddContractAddArgs*)args_ptr;
            uint result = args.A + args.B;
            *(uint*)out_ptr = result;
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static AbiError test_add_add_primitive_abi(void* args_ptr, void* out_ptr) {
        try {
            var args = *(TestAddContractAddPrimitiveArgs*)args_ptr;
            uint result = args.A + args.B;
            *(uint*)out_ptr = result;
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static AbiError test_add_version_abi(void* _args, void* out_ptr) {
        try {
            fixed (byte* p = VERSION_BYTES) {
                *(StringView*)out_ptr = new StringView { Ptr = p, Len = 3 };
            }
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static AbiError test_add_reset_abi(void* _args, void* _out) {
        try { _counter = 0; return AbiError.Ok; }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    // Static vtable function pointer array
    private static readonly unsafe void*[] TEST_ADD_FNS = new void*[] {
        (void*)(delegate* unmanaged[Cdecl]<void*, void*, AbiError>)&test_add_add_abi,
        (void*)(delegate* unmanaged[Cdecl]<void*, void*, AbiError>)&test_add_add_primitive_abi,
        (void*)(delegate* unmanaged[Cdecl]<void*, void*, AbiError>)&test_add_version_abi,
        (void*)(delegate* unmanaged[Cdecl]<void*, void*, AbiError>)&test_add_reset_abi,
    };

    // Contract ID: FNV-1a of "test.add@1" = 0xCC4232FAB0410D2B = 14718382584480468267UL
    public static unsafe PluginVTable TEST_ADD_VTABLE;

    public static unsafe void InitVtable() {
        fixed (void** fns = TEST_ADD_FNS) {
            TEST_ADD_VTABLE = new PluginVTable {
                ContractId = 14718382584480468267UL,
                ContractVersion = 0u << 16 | 0u,
                FunctionCount = 4u,
                Functions = fns,
            };
        }
    }
}

// Plugin entry point
public static unsafe class Plugin {
    private static readonly byte[] _plugin_name = "test_add_plugin"u8.ToArray();
    private static readonly byte[] _contract_name = "test.add"u8.ToArray();

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static uint PolyplugInit(PluginRegistrar* registrar) {
        if (registrar == null) return AbiConstants.ABI_ERROR_GENERIC;
        try {
            TestAddImpl.InitVtable();
            fixed (byte* namePtr = _plugin_name)
            fixed (byte* contractPtr = _contract_name)
            fixed (PluginVTable* vtablePtr = &TestAddImpl.TEST_ADD_VTABLE) {
                var desc = new PluginDescriptor {
                    Name = new StringView { Ptr = namePtr, Len = (nuint)_plugin_name.Length },
                    ContractName = new StringView { Ptr = contractPtr, Len = (nuint)_contract_name.Length },
                    VersionMajor = 1u,
                    VersionMinor = 0u,
                    VersionPatch = 0u,
                };
                var err = registrar->RegisterPlugin(registrar, &desc, vtablePtr);
                return err.Code;
            }
        } catch { return AbiConstants.ABI_ERROR_PANIC; }
    }
}
