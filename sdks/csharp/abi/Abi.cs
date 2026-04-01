using System.Runtime.InteropServices;

namespace Polyplug.Abi {

///  Non-owning UTF-8 string view.
///
///  OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
///  of the call. Never freed by the receiver.
[StructLayout(LayoutKind.Sequential)]
public struct StringView
{
    ///  UTF-8 bytes, NOT null-terminated.
    public IntPtr Ptr;
    ///  Byte count.
    public nuint Len;
}

///  Owning byte buffer.
///
///  OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
///  Owner calls `polyplug_host_free(ptr, cap, align)` when done.
[StructLayout(LayoutKind.Sequential)]
public struct Buffer
{
    public IntPtr Ptr;
    ///  Bytes currently used.
    public nuint Len;
    ///  Bytes allocated.
    public nuint Cap;
}

///  ABI error — returned by value from all ABI calls.
///
///  OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
///  via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
///  after reading. If `code == AbiErrorCode::Ok`, `message.ptr` is NULL — no free needed.
[StructLayout(LayoutKind.Sequential)]
public struct AbiError
{
    ///  0 = success, non-zero = error.
    public uint Code;
    ///  Empty/NULL if success. UTF-8 message if non-zero code.
    public StringView Message;
}

///  Opaque handle to a loaded plugin — validated on use.
///
///  INTERNAL STRUCTURE: index into registry array + generation counter.
///  The generation counter detects use-after-unload.
[StructLayout(LayoutKind.Sequential)]
public struct PluginHandle
{
    ///  Slot in the registry array.
    public uint Index;
    ///  Incremented on unload — detects stale handles.
    public uint Generation;
}

///  Opaque host context passed to plugin functions via rt_ctx parameter.
///
///  Contains the runtime pointer and the bundle_id of the calling bundle.
///  The actual implementation is in the polyplug crate; this definition
///  establishes the ABI layout.
///
///  OWNERSHIP: `'static`, lives as long as the runtime.
[StructLayout(LayoutKind.Sequential)]
public struct HostContext
{
    ///  Opaque pointer to the Runtime. Never dereferenced by plugins.
    public IntPtr Runtime;
    ///  Bundle ID of the calling bundle for dependency enforcement.
    public ulong BundleId;
}

///  Native dispatch data — direct function pointer array.
///
///  Used when `dispatch_type == DispatchType::Native`.
///  The `functions` array contains `function_count` function pointers.
[StructLayout(LayoutKind.Sequential)]
public struct NativeDispatch
{
    ///  Pointer to a static array of function pointers, indexed by function_id.
    public IntPtr Functions;
}

///  VM dispatch data — call through a dispatch function.
///
///  Used when `dispatch_type == DispatchType::VirtualMachine`.
///  The `call` function receives `loader_data` which contains VM-specific state.
[StructLayout(LayoutKind.Sequential)]
public struct VmDispatch
{
    ///  Dispatch function called for every VM function invocation.
    ///
    ///  # Arguments
    ///  - `loader_data`: VM-specific data (cast from `*mut c_void`)
    ///  - `fn_id`: Function index within the contract
    ///  - `args`: Pointer to packed arguments (ABI-specific layout)
    ///  - `out`: Pointer to output buffer for return value
    public IntPtr Call;
    ///  Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
    ///  Opaque to the host; interpreted by the dispatch function.
    public IntPtr LoaderData;
}

///  Plugin interface — one per contract implemented by a plugin.
///
///  OWNERSHIP: Must be `'static` or intentionally leaked.
///  Never stack-allocated. Never freed while runtime lives.
///
///  # Dispatch
///  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
///  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
[StructLayout(LayoutKind.Sequential)]
public struct PluginInterface
{
    ///  Pointer to the host context for this plugin.
    ///  Used for host function calls and dependency enforcement.
    public IntPtr RtCtx;
    ///  FNV-1a hash of "contract_name@major_version".
    public ulong ContractId;
    ///  minor.patch encoded as `(minor << 16 | patch)`.
    public uint ContractVersion;
    ///  Number of valid entries in the dispatch array.
    public uint FunctionCount;
    ///  Dispatch mechanism type (Native or VirtualMachine).
    public DispatchType DispatchType;
    ///  Union of dispatch mechanisms — access based on dispatch_type.
    public PluginDispatch Dispatch;
}

///  Host contract vtable header — metadata for a host-provided contract.
[StructLayout(LayoutKind.Sequential)]
public struct HostContractVTableHeader
{
    ///  VTable format version (for future compatibility).
    public uint VtableVersion;
    ///  FNV-1a hash of "contract_name@major_version".
    public ulong ContractId;
    ///  Contract major version.
    public uint ContractMajor;
    ///  Contract minor version.
    public uint ContractMinor;
    ///  Number of functions in this contract.
    public uint FunctionCount;
    ///  Dispatch mechanism type (Native or VirtualMachine).
    public DispatchType DispatchType;
}

///  Native dispatch for host contracts — direct function pointer array.
///
///  Used when `dispatch_type == DispatchType::Native`.
///  The `functions` array contains `function_count` function pointers.
[StructLayout(LayoutKind.Sequential)]
public struct NativeHostContractDispatch
{
    ///  Pointer to the implementation (e.g., Box<dyn Trait> as *const c_void).
    ///  This is passed as the first argument to all native dispatch functions.
    public IntPtr ImplPtr;
    ///  Pointer to a static array of function pointers, indexed by function_id.
    public IntPtr Functions;
}

///  VM dispatch for host contracts — call through a dispatch function.
///
///  Used when `dispatch_type == DispatchType::VirtualMachine`.
///  The `call` function receives `bridge_data` which contains VM-specific state.
[StructLayout(LayoutKind.Sequential)]
public struct VmHostContractDispatch
{
    ///  Dispatch function called for every VM function invocation.
    ///
    ///  # Arguments
    ///  - `bridge_data`: VM-specific data (cast from `*mut c_void`)
    ///  - `fn_id`: Function index within the contract
    ///  - `args`: Pointer to packed arguments (ABI-specific layout)
    ///  - `out`: Pointer to output buffer for return value
    public IntPtr Call;
    ///  VM-specific data (opaque to the host; interpreted by the dispatch function).
    public IntPtr BridgeData;
}

///  Host contract vtable — complete interface for a host-provided contract.
///
///  OWNERSHIP: Must be `'static` or intentionally leaked.
///  Never stack-allocated. Never freed while runtime lives.
///
///  # Dispatch
///  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
///  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
[StructLayout(LayoutKind.Sequential)]
public struct HostContractVTable
{
    ///  Header containing contract metadata.
    public HostContractVTableHeader Header;
    ///  Union of dispatch mechanisms — access based on dispatch_type.
    public HostContractDispatch Dispatch;
}

///  Host capabilities passed to every plugin at init time.
///
///  OWNERSHIP: `'static`, lives as long as the runtime.
///
///  All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
///  This allows each Runtime to have its own isolated state (no global registry).
[StructLayout(LayoutKind.Sequential)]
public struct HostVTable
{
    public IntPtr RegisterPlugin;
    public IntPtr Alloc;
    public IntPtr Free;
    public IntPtr FindByContract;
    public IntPtr FindByBundle;
    public IntPtr FindAllByContract;
    public IntPtr ResolvePlugin;
    ///  Get host contract vtable by contract_id and minimum version.
    ///  Returns null if no host contract matches the criteria.
    public IntPtr GetHostContract;
}

///  Metadata about a plugin within a bundle.
///
///  OWNERSHIP: value type passed by pointer during init. The `name` and
///  `contract_name` StringViews are borrowed from the plugin's static memory.
///  The receiver must not free or outlive the plugin's library.
[StructLayout(LayoutKind.Sequential)]
public struct PluginDescriptor
{
    ///  Human-readable plugin name.
    public StringView Name;
    ///  Full contract name for collision detection.
    public StringView ContractName;
    public uint VersionMajor;
    public uint VersionMinor;
    public uint VersionPatch;
}

///  Context passed to every guest `polyplug_init()` function.
///  The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
///  **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
[StructLayout(LayoutKind.Sequential)]
public struct PluginContext
{
    ///  Absolute canonical path to the directory containing the loaded bundle.
    public StringView BundlePath;
    ///  Host's supported ABI version for negotiation (Option C).
    ///  Plugin can use this to determine available features.
    public uint HostAbiVersion;
    ///  Bundle ID for dependency enforcement during init.
    public ulong BundleId;
}

///  Configuration passed to `polyplug_runtime_create` during runtime initialisation.
///
///  OWNERSHIP: borrowed for the duration of the runtime build only.
///  The caller may free all pointed-to memory after the build
///  returns. The runtime copies any data it needs to retain.
[StructLayout(LayoutKind.Sequential)]
public struct RuntimeConfig
{
    ///  Plugin directories to scan (array of `plugin_dir_count` StringViews).
    public IntPtr PluginDirs;
    public nuint PluginDirCount;
    ///  Compatibility mode: 0 = Strict (only mode implemented in MVP).
    public uint Compatibility;
}

///  ABI error codes (reserved: 0-255 runtime, 256+ plugin-defined).
///
///  These codes are returned by all ABI functions to indicate success or failure.
///  The `code` field of `AbiError` uses these values.
public enum AbiErrorCode : uint
{
    ///  Success — no error.
    Ok = 0,
    ///  Generic error — unspecified failure.
    Generic = 1,
    ///  Buffer too small — caller must reallocate (see Buffer protocol).
    BufferTooSmall = 2,
    ///  Panic — plugin panicked (caught by catch_unwind).
    Panic = 3,
    ///  Not found — plugin/contract not found.
    NotFound = 4,
    ///  Stale handle — PluginHandle generation mismatch.
    StaleHandle = 5,
    ///  Function not available — function_id >= function_count.
    FunctionNotAvailable = 6,
    ///  Duplicate provider — same bundle already provides this contract.
    DuplicateProvider = 7,
    ///  Invalid pointer — null or invalid pointer passed to ABI function.
    InvalidPointer = 8,
    ///  Host contract not found — no host contract matches contract_id.
    HostContractNotFound = 100,
    ///  Host contract version mismatch — host contract version does not match.
    HostContractVersionMismatch = 101,
    ///  Host contract call failed — host contract function call failed.
    HostContractCallFailed = 102,
}

///  Dispatch mechanism type — determines how function calls are routed.
public enum DispatchType : uint
{
    ///  Native dispatch: direct function pointer calls (zero overhead).
    Native = 0,
    ///  VM dispatch: call through a dispatch function with loader_data.
    VirtualMachine = 1,
}

///  Host runtime type identifier — identifies the language/runtime hosting plugins.
public enum HostRuntime : uint
{
    Rust = 0,
    Python = 1,
    Lua = 2,
    JavaScript = 3,
}

///  Union of dispatch mechanisms — use based on `dispatch_type`.
///
///  # Safety
///  Access the correct variant based on `PluginInterface::dispatch_type`:
///  - `dispatch_type == Native` → access `.native`
///  - `dispatch_type == VirtualMachine` → access `.vm`
[StructLayout(LayoutKind.Explicit)]
public struct PluginDispatch
{
    [FieldOffset(0)]
    public NativeDispatch Native;
    [FieldOffset(0)]
    public VmDispatch Vm;
}

///  Union of host contract dispatch mechanisms — use based on `dispatch_type`.
///
///  # Safety
///  Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
///  - `dispatch_type == Native` → access `.native`
///  - `dispatch_type == VirtualMachine` → access `.vm`
[StructLayout(LayoutKind.Explicit)]
public struct HostContractDispatch
{
    [FieldOffset(0)]
    public NativeHostContractDispatch Native;
    [FieldOffset(0)]
    public VmHostContractDispatch Vm;
}


/// ABI constants for polyplug.
public static class AbiConstants
{
    public const uint ABI_OK = 0u;
    public const uint ABI_ERROR_GENERIC = 1u;
    public const uint ABI_ERROR_BUFFER_TOO_SMALL = 2u;
    public const uint ABI_ERROR_PANIC = 3u;
    public const uint ABI_ERROR_NOT_FOUND = 4u;
    public const uint ABI_ERROR_STALE_HANDLE = 5u;
    public const uint ABI_ERROR_FUNCTION_NOT_AVAILABLE = 6u;
    public const uint ABI_ERROR_DUPLICATE_PROVIDER = 7u;
    public const uint ABI_ERROR_INVALID_POINTER = 8u;
    public const uint ABI_HOST_CONTRACT_NOT_FOUND = 100u;
    public const uint ABI_HOST_CONTRACT_VERSION_MISMATCH = 101u;
    public const uint ABI_HOST_CONTRACT_CALL_FAILED = 102u;
    public const uint POLYPLUG_ABI_VERSION = 1u;
}
}
