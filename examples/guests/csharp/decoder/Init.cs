// Init.cs — csharp_decoder ABI entry point. Contains isolated unsafe { } block.
// AllowUnsafeBlocks is kept in CsvDecoder.csproj for this file.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace CsvDecoder;

public static class Plugin
{
    private static readonly byte[] _plugin_name   = "csharp_decoder"u8.ToArray();
    private static readonly byte[] _contract_name = "pipeline.Decoder"u8.ToArray();

    private static byte[]?  _lastOutput;
    private static GCHandle _lastOutputHandle;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError decoder_decode_abi(IntPtr argsPtr, IntPtr outPtr)
    {
        try
        {
            StringView input;
            unsafe { input = Unsafe.AsRef<StringView>((void*)argsPtr); }

            string inputStr = input.ToString();
            byte[] resultBytes = DecoderImpl.Decode(inputStr);

            if (_lastOutputHandle.IsAllocated) _lastOutputHandle.Free();
            _lastOutput       = resultBytes;
            _lastOutputHandle = GCHandle.Alloc(_lastOutput, GCHandleType.Pinned);

            var outView = new StringView
            {
                Ptr = _lastOutputHandle.AddrOfPinnedObject(),
                Len = (ulong)resultBytes.Length,
            };
            unsafe { Unsafe.WriteUnaligned((void*)outPtr, outView); }
            return AbiError.Ok;
        }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    private static readonly IntPtr[] DECODER_FNS;
    private static GCHandle _fnsPinHandle;
    public static PluginVTable DECODER_VTABLE;

    static Plugin()
    {
        unsafe
        {
            DECODER_FNS = new IntPtr[]
            {
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&decoder_decode_abi,
            };
        }
        _fnsPinHandle = GCHandle.Alloc(DECODER_FNS, GCHandleType.Pinned);
        DECODER_VTABLE = new PluginVTable
        {
            ContractId      = DecoderImpl.DECODER_CONTRACT_ID,
            ContractVersion = 0u << 16 | 0u,
            FunctionCount   = 1u,
            FunctionsPtr    = _fnsPinHandle.AddrOfPinnedObject(),
        };
    }

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) }, EntryPoint = "polyplug_init")]
    public static uint PolyplugInit(IntPtr registrarPtr, IntPtr ctxPtr)
    {
        if (registrarPtr == IntPtr.Zero || ctxPtr == IntPtr.Zero) return AbiConstants.ABI_ERROR_GENERIC;
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

                var nameHandle     = GCHandle.Alloc(_plugin_name, GCHandleType.Pinned);
                var contractHandle = GCHandle.Alloc(_contract_name, GCHandleType.Pinned);
                try
                {
                    fixed (PluginVTable* vtablePtr = &DECODER_VTABLE)
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
