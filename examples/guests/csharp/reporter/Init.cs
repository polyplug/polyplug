// Init.cs — reporter ABI entry point. Contains isolated unsafe { } block.
// AllowUnsafeBlocks is kept in Reporter.csproj for this file.
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Polyplug.Guest;

namespace Reporter;

public static class Plugin
{
    private static readonly byte[] _plugin_name   = "reporter_plugin"u8.ToArray();
    private static readonly byte[] _contract_name = "pipeline.reporter"u8.ToArray();

    // Output buffer: keep last report bytes pinned across ABI boundary.
    private static byte[]?  _lastOutput;
    private static GCHandle _lastOutputHandle;

    [UnmanagedCallersOnly(CallConvs = new[] { typeof(CallConvCdecl) })]
    private static AbiError reporter_report_abi(IntPtr argsPtr, IntPtr outPtr)
    {
        try
        {
            DataRecord record;
            unsafe { record = Unsafe.AsRef<DataRecord>((void*)argsPtr); }

            string nameStr  = record.Name.ToString();
            string valueStr = record.Value.ToString();
            byte[] reportBytes = ReporterImpl.BuildReport(nameStr, valueStr, record.Count);

            if (_lastOutputHandle.IsAllocated) _lastOutputHandle.Free();
            _lastOutput       = reportBytes;
            _lastOutputHandle = GCHandle.Alloc(_lastOutput, GCHandleType.Pinned);

            var outView = new StringView
            {
                Ptr = _lastOutputHandle.AddrOfPinnedObject(),
                Len = (ulong)reportBytes.Length,
            };
            unsafe { Unsafe.WriteUnaligned((void*)outPtr, outView); }
            return AbiError.Ok;
        }
        catch { return new AbiError { Code = AbiConstants.ABI_ERROR_PANIC }; }
    }

    private static readonly IntPtr[] REPORTER_FNS;
    private static GCHandle _fnsPinHandle;
    public static PluginVTable REPORTER_VTABLE;

    static Plugin()
    {
        unsafe
        {
            REPORTER_FNS = new IntPtr[]
            {
                (IntPtr)(delegate* unmanaged[Cdecl]<IntPtr, IntPtr, AbiError>)&reporter_report_abi,
            };
        }
        _fnsPinHandle = GCHandle.Alloc(REPORTER_FNS, GCHandleType.Pinned);
        REPORTER_VTABLE = new PluginVTable
        {
            ContractId      = ReporterImpl.REPORTER_CONTRACT_ID,
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

                var nameHandle     = GCHandle.Alloc(_plugin_name, GCHandleType.Pinned);
                var contractHandle = GCHandle.Alloc(_contract_name, GCHandleType.Pinned);
                try
                {
                    fixed (PluginVTable* vtablePtr = &REPORTER_VTABLE)
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
