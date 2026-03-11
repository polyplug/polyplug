// Init.cs — C# fixture: ABI entry point and vtable registration.
// Equivalent to what polyplugc generates. Contains isolated unsafe { } blocks.
// <AllowUnsafeBlocks>true</AllowUnsafeBlocks> is set in CsharpPlugin.csproj.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace CsharpPlugin;

public static class Plugin {
    private static readonly byte[] _plugin_name = "test_add_plugin"u8.ToArray();
    private static readonly byte[] _contract_name = "test.add"u8.ToArray();

    // Contract ID: FNV-1a of "test.add@1" = 14718382584480468267UL
    private const ulong TEST_ADD_CONTRACT_ID = 14718382584480468267UL;

    // ABI shim methods: IntPtr parameters, isolated unsafe block for pointer ops.
    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError test_add_add_abi(IntPtr argsPtr, IntPtr outPtr) {
        try {
            unsafe {
                var args = System.Runtime.CompilerServices.Unsafe.AsRef<TestAddContractAddArgs>((void*)argsPtr);
                uint result = TestAddImpl.Add(args.A, args.B);
                System.Runtime.CompilerServices.Unsafe.WriteUnaligned((void*)outPtr, result);
            }
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError test_add_add_primitive_abi(IntPtr argsPtr, IntPtr outPtr) {
        try {
            unsafe {
                var args = System.Runtime.CompilerServices.Unsafe.AsRef<TestAddContractAddPrimitiveArgs>((void*)argsPtr);
                uint result = TestAddImpl.AddPrimitive(args.A, args.B);
                System.Runtime.CompilerServices.Unsafe.WriteUnaligned((void*)outPtr, result);
            }
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError test_add_version_abi(IntPtr _argsPtr, IntPtr outPtr) {
        try {
            byte[] vb = TestAddImpl.GetVersionBytes();
            var handle = GCHandle.Alloc(vb, GCHandleType.Pinned);
            try {
                unsafe {
                    System.Runtime.CompilerServices.Unsafe.WriteUnaligned(
                        (void*)outPtr,
                        new StringView { Ptr = handle.AddrOfPinnedObject(), Len = (ulong)vb.Length }
                    );
                }
            } finally { handle.Free(); }
            return AbiError.Ok;
        } catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError test_add_reset_abi(IntPtr _argsPtr, IntPtr _outPtr) {
        try { TestAddImpl.Reset(); return AbiError.Ok; }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    // Function pointer array pinned permanently.
    private static readonly IntPtr[] TEST_ADD_FNS;
    private static GCHandle _fnsPinHandle;
    public static PluginVTable TEST_ADD_VTABLE;

    static Plugin() {
        unsafe {
            TEST_ADD_FNS = new IntPtr[] {
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&test_add_add_abi,
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&test_add_add_primitive_abi,
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&test_add_version_abi,
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&test_add_reset_abi,
            };
        }
        _fnsPinHandle = GCHandle.Alloc(TEST_ADD_FNS, GCHandleType.Pinned);
        TEST_ADD_VTABLE = new PluginVTable {
            ContractId = TEST_ADD_CONTRACT_ID,
            ContractVersion = 0u << 16 | 0u,
            FunctionCount = 4u,
            FunctionsPtr = _fnsPinHandle.AddrOfPinnedObject(),
        };
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static uint PolyplugInit(IntPtr registrarPtr, IntPtr ctxPtr) {
        if (registrarPtr == IntPtr.Zero || ctxPtr == IntPtr.Zero) return AbiConstants.ABI_ERROR_GENERIC;
        System.Threading.Thread.BeginThreadAffinity();
        try {
            unsafe {
                var registrar = (PluginRegistrar*)registrarPtr;
                var ctx = (PluginContext*)ctxPtr;
                _ = ctx; // ctx available for bundle path if needed

                var registerFn = (delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError>)
                    registrar->RegisterPluginPtr;

                var nameHandle = GCHandle.Alloc(_plugin_name, GCHandleType.Pinned);
                var contractHandle = GCHandle.Alloc(_contract_name, GCHandleType.Pinned);
                try {
                    fixed (PluginVTable* vtablePtr = &TEST_ADD_VTABLE) {
                        var desc = new PluginDescriptor {
                            Name = new StringView { Ptr = nameHandle.AddrOfPinnedObject(), Len = (ulong)_plugin_name.Length },
                            ContractName = new StringView { Ptr = contractHandle.AddrOfPinnedObject(), Len = (ulong)_contract_name.Length },
                            VersionMajor = 1u,
                            VersionMinor = 0u,
                            VersionPatch = 0u,
                        };
                        AbiError err = registerFn(registrar, &desc, vtablePtr);
                        if (err.Code != AbiConstants.ABI_OK) return err.Code;
                    }
                } finally {
                    nameHandle.Free();
                    contractHandle.Free();
                }
            }
            return AbiConstants.ABI_OK;
        } catch { return AbiConstants.ABI_ERROR_PANIC; }
        finally { System.Threading.Thread.EndThreadAffinity(); }
    }
}
