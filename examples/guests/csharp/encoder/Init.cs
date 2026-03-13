// Init.cs — csv_encoder ABI entry point. Contains isolated unsafe { } block.
// AllowUnsafeBlocks is kept in CsvEncoder.csproj for this file.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace CsvEncoder;

public static class Plugin
{
    private static readonly byte[] _plugin_name   = "csv_encoder_plugin"u8.ToArray();
    private static readonly byte[] _contract_name = "pipeline.encoder"u8.ToArray();

    // Output buffer: keep last encoded bytes pinned across ABI boundary.
    private static byte[]?  _lastOutput;
    private static GCHandle _lastOutputHandle;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError csv_encoder_encode_abi(IntPtr argsPtr, IntPtr outPtr)
    {
        try
        {
            DataRecord record;
            unsafe { record = System.Runtime.CompilerServices.Unsafe.AsRef<DataRecord>((void*)argsPtr); }

            string nameStr  = record.Name.ToString();
            string valueStr = record.Value.ToString();
            byte[] csvBytes = CsvEncoderImpl.EncodeToCsv(nameStr, valueStr, record.Count);

            if (_lastOutputHandle.IsAllocated) _lastOutputHandle.Free();
            _lastOutput       = csvBytes;
            _lastOutputHandle = GCHandle.Alloc(_lastOutput, GCHandleType.Pinned);

            var outBuf = new Polyplug.Guest.Buffer
            {
                Ptr = _lastOutputHandle.AddrOfPinnedObject(),
                Len = (ulong)csvBytes.Length,
                Cap = (ulong)csvBytes.Length,
            };
            unsafe { System.Runtime.CompilerServices.Unsafe.WriteUnaligned((void*)outPtr, outBuf); }
            return AbiError.Ok;
        }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    private static readonly IntPtr[] ENCODER_FNS;
    private static GCHandle _fnsPinHandle;
    public static PluginVTable ENCODER_VTABLE;

    static Plugin()
    {
        unsafe
        {
            ENCODER_FNS = new IntPtr[]
            {
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&csv_encoder_encode_abi,
            };
        }
        _fnsPinHandle = GCHandle.Alloc(ENCODER_FNS, GCHandleType.Pinned);
        ENCODER_VTABLE = new PluginVTable
        {
            ContractId      = CsvEncoderImpl.ENCODER_CONTRACT_ID,
            ContractVersion = 0u << 16 | 0u,
            FunctionCount   = 1u,
            FunctionsPtr    = _fnsPinHandle.AddrOfPinnedObject(),
        };
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static uint PolyplugInit(IntPtr registrarPtr, IntPtr ctxPtr)
    {
        if (registrarPtr == IntPtr.Zero) return AbiConstants.ABI_ERROR_GENERIC;
        System.Threading.Thread.BeginThreadAffinity();
        try
        {
            unsafe
            {
                var registrar = (PluginRegistrar*)registrarPtr;
                var ctx       = (PluginContext*)ctxPtr;
                _ = ctx;

                var registerFn = (delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*, PluginVTable*, AbiError>)
                    registrar->RegisterPluginPtr;

                // Cross-plugin dependency lookup: demonstrate decoder handle lookup.
                var findByContractFn = (delegate* unmanaged[Cdecl, SuppressGCTransition]<ulong, uint, PluginHandle>)
                    ((HostVTable*)registrar->HostPtr)->FindByContractPtr;
                PluginHandle decoderHandle = findByContractFn(CsvEncoderImpl.DECODER_CONTRACT_ID, 0u);
                _ = decoderHandle;

                var nameHandle     = GCHandle.Alloc(_plugin_name, GCHandleType.Pinned);
                var contractHandle = GCHandle.Alloc(_contract_name, GCHandleType.Pinned);
                try
                {
                    fixed (PluginVTable* vtablePtr = &ENCODER_VTABLE)
                    {
                        var desc = new PluginDescriptor
                        {
                            Name         = new StringView { Ptr = nameHandle.AddrOfPinnedObject(), Len = (ulong)_plugin_name.Length },
                            ContractName = new StringView { Ptr = contractHandle.AddrOfPinnedObject(), Len = (ulong)_contract_name.Length },
                            VersionMajor = 1u,
                            VersionMinor = 0u,
                            VersionPatch = 0u,
                        };
                        AbiError err = registerFn(registrar, &desc, vtablePtr);
                        if (err.Code != AbiConstants.ABI_OK) return err.Code;
                    }
                }
                finally
                {
                    nameHandle.Free();
                    contractHandle.Free();
                }
            }
            return AbiConstants.ABI_OK;
        }
        catch { return AbiConstants.ABI_ERROR_PANIC; }
        finally { System.Threading.Thread.EndThreadAffinity(); }
    }
}
