using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace Polyplug.Host;

internal static partial class NativeMethods
{
    private const string NativeLib = "polyplug";

    // ─── C-compatible types for hot-reload notification ───────────────────────────

    /// <summary>
    /// C-compatible string view for passing strings across the FFI boundary.
    /// The pointer must remain valid for the duration of the callback call.
    /// This is a borrowed view — the callback must NOT free the memory.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct StringViewC
    {
        /// <summary>
        /// Pointer to UTF-8 bytes.
        /// </summary>
        public nint Ptr;

        /// <summary>
        /// Length in bytes.
        /// </summary>
        public nuint Len;
    }

    /// <summary>
    /// FFI-safe representation of ReloadPhase (not a 'C suffix' type, but an FFI variant).
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct ReloadPhaseFfi
    {
        /// <summary>
        /// The phase type (Preparing=0, Reloaded=1, or Failed=2).
        /// </summary>
        public uint PhaseType;

        /// <summary>
        /// Bundle ID (valid for all variants).
        /// </summary>
        public ulong BundleId;

        /// <summary>
        /// Bundle name (valid for all variants).
        /// </summary>
        public StringViewC BundleName;

        /// <summary>
        /// Retry count (valid only for Preparing variant).
        /// </summary>
        public uint RetryCount;

        /// <summary>
        /// Failure reason (valid only for Failed variant).
        /// </summary>
        public StringViewC Reason;
    }

    /// <summary>
    /// FFI RuntimeConfig matching polyplug_abi::RuntimeConfig (24 bytes).
    /// Layout verified against Rust offset tests.
    /// </summary>
    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    internal struct RuntimeConfig
    {
        /// <summary>
        /// Whether hot-reload is enabled (0=false, non-zero=true).
        /// Offset 0, 1 byte.
        /// </summary>
        public byte HotReloadEnabled;

        // Padding: 3 bytes (offset 1-3) from Pack=4

        /// <summary>
        /// Maximum retry attempts for hot-reload.
        /// Offset 4, 4 bytes.
        /// </summary>
        public uint HotReloadMaxRetries;

        /// <summary>
        /// Interval between retry attempts in milliseconds.
        /// Offset 8, 8 bytes.
        /// </summary>
        public ulong HotReloadRetryIntervalMs;

        /// <summary>
        /// Abort when max retries exhausted (0=false, non-zero=true).
        /// Offset 16, 1 byte.
        /// </summary>
        public byte HotReloadAbortOnMaxRetries;

        // Padding: 3 bytes (offset 17-19) from Pack=4

        /// <summary>
        /// Compatibility mode: 0=Strict, 1=Relaxed, 2=Yolo.
        /// Matches polyplug_abi::Compatibility #[repr(u32)].
        /// Offset 20, 4 bytes.
        /// </summary>
        public uint Compatibility;
    }

    /// <summary>
    /// Compatibility modes matching polyplug_abi::Compatibility #[repr(u32)].
    /// </summary>
    internal static class CompatibilityMode
    {
        /// <summary>Exact major match and minor >= required.</summary>
        public const uint Strict = 0;

        /// <summary>Same major, any minor.</summary>
        public const uint Relaxed = 1;

        /// <summary>Any version accepted.</summary>
        public const uint Yolo = 2;
    }

    /// <summary>
    /// Options for creating a runtime instance.
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct RuntimeCreateOptions
    {
        /// <summary>
        /// Pointer to RuntimeConfig, or null for default config.
        /// </summary>
        public nint Config;

        /// <summary>
        /// Reload callback function pointer, or null for no callback.
        /// </summary>
        public nint OnReload;
    }

    // ─── HostInterface Structure (18-03) ─────────────────────────────────────────
    // FFI HostInterface matching polyplug_abi::HostInterface (144 bytes)
    // Layout verified in polyplug_abi/tests: offset_of checks

    /// <summary>
    /// HostInterface struct matching polyplug_abi::HostInterface (144 bytes).
    /// Contains runtime pointer and 18 function pointer fields.
    /// All function pointers use self-passing pattern (receive HostInterface* as first param).
    /// </summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct HostInterface
    {
        // offset 0: runtime (*mut c_void)
        public nint Runtime;

        // offset 8: register_contract
        public nint RegisterContract;

        // offset 16: alloc
        public nint Alloc;

        // offset 24: free
        public nint Free;

        // offset 32: find_guest_contract
        public nint FindGuestContract;

        // offset 40: find_all_guest_contracts
        public nint FindAllGuestContracts;

        // offset 48: resolve_guest_contract
        public nint ResolveGuestContract;

        // offset 56: call_guest_method
        public nint CallGuestMethod;

        // offset 64: get_host_contract
        public nint GetHostContract;

        // offset 72: resolve_host_contract_interface
        public nint ResolveHostContractInterface;

        // offset 80: list_bundles
        public nint ListBundles;

        // offset 88: get_dependencies
        public nint GetDependencies;

        // offset 96: load_bundle
        public nint LoadBundle;

        // offset 104: reload_bundle
        public nint ReloadBundle;

        // offset 112: register_host_contract
        public nint RegisterHostContract;

        // offset 120: register_loader
        public nint RegisterLoader;

        // offset 128: get_last_error
        public nint GetLastError;

        // offset 136: get_error_len
        public nint GetErrorLen;
    }

    // ─── FFI Entry Points (18-02: Only 2 exports) ─────────────────────────────────

    /// <summary>
    /// Creates a new runtime instance with default configuration.
    /// Returns a HostInterface pointer that provides all runtime operations.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint PolyplugRuntimeCreate();

    /// <summary>
    /// Creates a new runtime instance with the specified options.
    /// Returns a HostInterface pointer that provides all runtime operations.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_create_with_options")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial nint PolyplugRuntimeCreateWithOptions(nint options);

    /// <summary>
    /// Destroys a runtime instance.
    /// Takes HostInterface pointer returned by polyplug_runtime_create.
    /// </summary>
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_destroy")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial void PolyplugRuntimeDestroy(nint host);

    // Hot-reload configuration (must be called before runtime creation)
    [LibraryImport(NativeLib, EntryPoint = "polyplug_runtime_on_reload")]
    [UnmanagedCallConv(CallConvs = [typeof(CallConvCdecl)])]
    internal static partial uint PolyplugRuntimeOnReload(nint callback);
}