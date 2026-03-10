// Plugin.cs — C# showcase plugin for polyplug.
// Implements the pipeline.encoder@1.0 contract from showcase/api.toml.
// Demonstrates cross-plugin dependency lookup (pipeline.decoder@1).

using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace CsvEncoder;

// DataRecord ABI struct — mirrored inline per frozen ABI layout.
// name@0[16], value@16[16], count@32[4], _pad@36[4] = 40 bytes total.
[StructLayout(LayoutKind.Sequential)]
public unsafe struct DataRecord
{
    public StringView Name;
    public StringView Value;
    public uint Count;
    private uint _pad;
}

// Static impl — stores vtable and function pointers for pipeline.encoder
public static unsafe class CsvEncoderImpl
{
    // pipeline.encoder@1 contract ID: FNV-1a of "pipeline.encoder@1" = 0x12AD37F43386F752
    private const ulong ENCODER_CONTRACT_ID = 0x12AD37F43386F752UL;

    // pipeline.decoder@1 contract ID: FNV-1a of "pipeline.decoder@1" = 0x133E62ABD6E7D5BE
    // Used in PolyplugInit to demonstrate cross-plugin dependency lookup.
    private const ulong DECODER_CONTRACT_ID = 0x133E62ABD6E7D5BEUL;

    // Output: keeps the last encoded bytes alive across the ABI boundary.
    private static byte[]? _lastOutput;
    private static GCHandle _lastOutputHandle;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static AbiError csv_encoder_encode_abi(void* args_ptr, void* out_ptr)
    {
        try
        {
            DataRecord record = *(DataRecord*)args_ptr;

            // Decode name and value from their StringView pointers.
            string nameStr  = System.Text.Encoding.UTF8.GetString(record.Name.Ptr,  (int)record.Name.Len);
            string valueStr = System.Text.Encoding.UTF8.GetString(record.Value.Ptr, (int)record.Value.Len);

            // Encode CSV: header row + one data row.
            string csv      = $"name,value,count\n{nameStr},{valueStr},{record.Count}\n";
            byte[] csvBytes = System.Text.Encoding.UTF8.GetBytes(csv);

            // Release previously pinned output buffer if still alive.
            if (_lastOutputHandle.IsAllocated)
            {
                _lastOutputHandle.Free();
            }

            // Pin the output bytes so the GC cannot move them across the ABI boundary.
            _lastOutput       = csvBytes;
            _lastOutputHandle = GCHandle.Alloc(_lastOutput, GCHandleType.Pinned);
            byte* ptr         = (byte*)_lastOutputHandle.AddrOfPinnedObject();

            *(Polyplug.Guest.Buffer*)out_ptr = new Polyplug.Guest.Buffer
            {
                Ptr = ptr,
                Len = (nuint)csvBytes.Length,
                Cap = (nuint)csvBytes.Length,
            };

            return AbiError.Ok;
        }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    // Static vtable function pointer array — one entry for pipeline.encoder@1.
    private static readonly void*[] ENCODER_FNS = new void*[]
    {
        (void*)(delegate* unmanaged[Cdecl]<void*, void*, AbiError>)&csv_encoder_encode_abi,
    };

    public static PluginVTable ENCODER_VTABLE;

    public static void InitVtable()
    {
        fixed (void** fns = ENCODER_FNS)
        {
            ENCODER_VTABLE = new PluginVTable
            {
                ContractId      = ENCODER_CONTRACT_ID,
                ContractVersion = 0u << 16 | 0u,
                FunctionCount   = 1u,
                Functions       = fns,
            };
        }
    }

    public static ulong GetDecoderContractId() => DECODER_CONTRACT_ID;
}

// Plugin entry point
public static unsafe class Plugin
{
    private static readonly byte[] _plugin_name   = "csv_encoder_plugin"u8.ToArray();
    private static readonly byte[] _contract_name = "pipeline.encoder"u8.ToArray();

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    public static uint PolyplugInit(PluginRegistrar* registrar)
    {
        if (registrar == null) return AbiConstants.ABI_ERROR_GENERIC;
        try
        {
            CsvEncoderImpl.InitVtable();

            // Cross-plugin dependency lookup: find the pipeline.decoder plugin.
            // The [[dependency]] section in manifest.toml ensures the loader loads
            // csv_decoder before csv_encoder, so this handle is valid at runtime.
            // We do not call decode here — just demonstrate the lookup mechanism.
            PluginHandle decoderHandle = (*registrar->Host).FindByContract(
                CsvEncoderImpl.GetDecoderContractId(),
                0u
            );
            // Suppress unused-variable warning; handle would be stored/used in real code.
            _ = decoderHandle;

            fixed (byte* namePtr     = _plugin_name)
            fixed (byte* contractPtr = _contract_name)
            fixed (PluginVTable* vtablePtr = &CsvEncoderImpl.ENCODER_VTABLE)
            {
                PluginDescriptor desc = new PluginDescriptor
                {
                    Name         = new StringView { Ptr = namePtr,     Len = (nuint)_plugin_name.Length },
                    ContractName = new StringView { Ptr = contractPtr, Len = (nuint)_contract_name.Length },
                    VersionMajor = 1u,
                    VersionMinor = 0u,
                    VersionPatch = 0u,
                };
                AbiError err = registrar->RegisterPlugin(registrar, &desc, vtablePtr);
                return err.Code;
            }
        }
        catch { return AbiConstants.ABI_ERROR_PANIC; }
    }
}
