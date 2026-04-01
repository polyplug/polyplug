/**
 *  Non-owning UTF-8 string view.
 * 
 *  OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
 *  of the call. Never freed by the receiver.
 */
export interface StringView {
    /**  UTF-8 bytes, NOT null-terminated. */
    ptr: bigint;
    /**  Byte count. */
    len: number;
}

/**
 *  Owning byte buffer.
 * 
 *  OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
 *  Owner calls `polyplug_host_free(ptr, cap, align)` when done.
 */
export interface Buffer {
    ptr: bigint;
    /**  Bytes currently used. */
    len: number;
    /**  Bytes allocated. */
    cap: number;
}

/**
 *  ABI error — returned by value from all ABI calls.
 * 
 *  OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
 *  via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
 *  after reading. If `code == AbiErrorCode::Ok`, `message.ptr` is NULL — no free needed.
 */
export interface AbiError {
    /**  0 = success, non-zero = error. */
    code: number;
    /**  Empty/NULL if success. UTF-8 message if non-zero code. */
    message: StringView;
}

/**
 *  Opaque handle to a loaded plugin — validated on use.
 * 
 *  INTERNAL STRUCTURE: index into registry array + generation counter.
 *  The generation counter detects use-after-unload.
 */
export interface PluginHandle {
    /**  Slot in the registry array. */
    index: number;
    /**  Incremented on unload — detects stale handles. */
    generation: number;
}

/**
 *  Opaque host context passed to plugin functions via rt_ctx parameter.
 * 
 *  Contains the runtime pointer and the bundle_id of the calling bundle.
 *  The actual implementation is in the polyplug crate; this definition
 *  establishes the ABI layout.
 * 
 *  OWNERSHIP: `'static`, lives as long as the runtime.
 */
export interface HostContext {
    /**  Opaque pointer to the Runtime. Never dereferenced by plugins. */
    runtime: bigint;
    /**  Bundle ID of the calling bundle for dependency enforcement. */
    bundle_id: bigint;
}

/**
 *  Native dispatch data — direct function pointer array.
 * 
 *  Used when `dispatch_type == DispatchType::Native`.
 *  The `functions` array contains `function_count` function pointers.
 */
export interface NativeDispatch {
    /**  Pointer to a static array of function pointers, indexed by function_id. */
    functions: bigint;
}

/**
 *  VM dispatch data — call through a dispatch function.
 * 
 *  Used when `dispatch_type == DispatchType::VirtualMachine`.
 *  The `call` function receives `loader_data` which contains VM-specific state.
 */
export interface VmDispatch {
    /**
     *  Dispatch function called for every VM function invocation.
     * 
     *  # Arguments
     *  - `loader_data`: VM-specific data (cast from `*mut c_void`)
     *  - `fn_id`: Function index within the contract
     *  - `args`: Pointer to packed arguments (ABI-specific layout)
     *  - `out`: Pointer to output buffer for return value
     */
    call: (loader_data: bigint, fn_id: number, args: bigint, out: bigint) => AbiError;
    /**
     *  Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
     *  Opaque to the host; interpreted by the dispatch function.
     */
    loader_data: bigint;
}

/**
 *  Plugin interface — one per contract implemented by a plugin.
 * 
 *  OWNERSHIP: Must be `'static` or intentionally leaked.
 *  Never stack-allocated. Never freed while runtime lives.
 * 
 *  # Dispatch
 *  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
 *  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
 */
export interface PluginInterface {
    /**
     *  Pointer to the host context for this plugin.
     *  Used for host function calls and dependency enforcement.
     */
    rt_ctx: bigint;
    /**  FNV-1a hash of "contract_name@major_version". */
    contract_id: bigint;
    /**  minor.patch encoded as `(minor << 16 | patch)`. */
    contract_version: number;
    /**  Number of valid entries in the dispatch array. */
    function_count: number;
    /**  Dispatch mechanism type (Native or VirtualMachine). */
    dispatch_type: DispatchType;
    /**  Union of dispatch mechanisms — access based on dispatch_type. */
    dispatch: PluginDispatch;
}

/**  Host contract vtable header — metadata for a host-provided contract. */
export interface HostContractVTableHeader {
    /**  VTable format version (for future compatibility). */
    vtable_version: number;
    /**  FNV-1a hash of "contract_name@major_version". */
    contract_id: bigint;
    /**  Contract major version. */
    contract_major: number;
    /**  Contract minor version. */
    contract_minor: number;
    /**  Number of functions in this contract. */
    function_count: number;
    /**  Dispatch mechanism type (Native or VirtualMachine). */
    dispatch_type: DispatchType;
}

/**
 *  Native dispatch for host contracts — direct function pointer array.
 * 
 *  Used when `dispatch_type == DispatchType::Native`.
 *  The `functions` array contains `function_count` function pointers.
 */
export interface NativeHostContractDispatch {
    /**
     *  Pointer to the implementation (e.g., Box<dyn Trait> as *const c_void).
     *  This is passed as the first argument to all native dispatch functions.
     */
    impl_ptr: bigint;
    /**  Pointer to a static array of function pointers, indexed by function_id. */
    functions: bigint;
}

/**
 *  VM dispatch for host contracts — call through a dispatch function.
 * 
 *  Used when `dispatch_type == DispatchType::VirtualMachine`.
 *  The `call` function receives `bridge_data` which contains VM-specific state.
 */
export interface VmHostContractDispatch {
    /**
     *  Dispatch function called for every VM function invocation.
     * 
     *  # Arguments
     *  - `bridge_data`: VM-specific data (cast from `*mut c_void`)
     *  - `fn_id`: Function index within the contract
     *  - `args`: Pointer to packed arguments (ABI-specific layout)
     *  - `out`: Pointer to output buffer for return value
     */
    call: (bridge_data: bigint, fn_id: number, args: bigint, out: bigint) => AbiError;
    /**  VM-specific data (opaque to the host; interpreted by the dispatch function). */
    bridge_data: bigint;
}

/**
 *  Host contract vtable — complete interface for a host-provided contract.
 * 
 *  OWNERSHIP: Must be `'static` or intentionally leaked.
 *  Never stack-allocated. Never freed while runtime lives.
 * 
 *  # Dispatch
 *  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
 *  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
 */
export interface HostContractVTable {
    /**  Header containing contract metadata. */
    header: HostContractVTableHeader;
    /**  Union of dispatch mechanisms — access based on dispatch_type. */
    dispatch: HostContractDispatch;
}

/**
 *  Host capabilities passed to every plugin at init time.
 * 
 *  OWNERSHIP: `'static`, lives as long as the runtime.
 * 
 *  All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
 *  This allows each Runtime to have its own isolated state (no global registry).
 */
export interface HostVTable {
    register_plugin: (rt_ctx: bigint, descriptor: bigint, vtable: bigint) => AbiError;
    alloc: (rt_ctx: bigint, size: number, align: number) => bigint;
    free: (rt_ctx: bigint, ptr: bigint, size: number, align: number) => void;
    find_by_contract: (rt_ctx: bigint, contract_id: bigint, min_version: number) => PluginHandle;
    find_by_bundle: (rt_ctx: bigint, bundle_id: bigint, contract_id: bigint, min_version: number) => PluginHandle;
    find_all_by_contract: (rt_ctx: bigint, contract_id: bigint, min_version: number, out: bigint, out_cap: number) => number;
    resolve_plugin: (rt_ctx: bigint, handle: PluginHandle) => bigint;
    /**
     *  Get host contract vtable by contract_id and minimum version.
     *  Returns null if no host contract matches the criteria.
     */
    get_host_contract: (rt_ctx: bigint, contract_id: bigint, min_version: number) => bigint;
}

/**
 *  Metadata about a plugin within a bundle.
 * 
 *  OWNERSHIP: value type passed by pointer during init. The `name` and
 *  `contract_name` StringViews are borrowed from the plugin's static memory.
 *  The receiver must not free or outlive the plugin's library.
 */
export interface PluginDescriptor {
    /**  Human-readable plugin name. */
    name: StringView;
    /**  Full contract name for collision detection. */
    contract_name: StringView;
    version_major: number;
    version_minor: number;
    version_patch: number;
}

/**
 *  Context passed to every guest `polyplug_init()` function.
 *  The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
 *  **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
 */
export interface PluginContext {
    /**  Absolute canonical path to the directory containing the loaded bundle. */
    bundle_path: StringView;
    /**
     *  Host's supported ABI version for negotiation (Option C).
     *  Plugin can use this to determine available features.
     */
    host_abi_version: number;
    /**  Bundle ID for dependency enforcement during init. */
    bundle_id: bigint;
}

/**
 *  Configuration passed to `polyplug_runtime_create` during runtime initialisation.
 * 
 *  OWNERSHIP: borrowed for the duration of the runtime build only.
 *  The caller may free all pointed-to memory after the build
 *  returns. The runtime copies any data it needs to retain.
 */
export interface RuntimeConfig {
    /**  Plugin directories to scan (array of `plugin_dir_count` StringViews). */
    plugin_dirs: bigint;
    plugin_dir_count: number;
    /**  Compatibility mode: 0 = Strict (only mode implemented in MVP). */
    compatibility: number;
}

/**
 *  ABI error codes (reserved: 0-255 runtime, 256+ plugin-defined).
 * 
 *  These codes are returned by all ABI functions to indicate success or failure.
 *  The `code` field of `AbiError` uses these values.
 */
export const enum AbiErrorCode {
    /**  Success — no error. */
    Ok = 0,
    /**  Generic error — unspecified failure. */
    Generic = 1,
    /**  Buffer too small — caller must reallocate (see Buffer protocol). */
    BufferTooSmall = 2,
    /**  Panic — plugin panicked (caught by catch_unwind). */
    Panic = 3,
    /**  Not found — plugin/contract not found. */
    NotFound = 4,
    /**  Stale handle — PluginHandle generation mismatch. */
    StaleHandle = 5,
    /**  Function not available — function_id >= function_count. */
    FunctionNotAvailable = 6,
    /**  Duplicate provider — same bundle already provides this contract. */
    DuplicateProvider = 7,
    /**  Invalid pointer — null or invalid pointer passed to ABI function. */
    InvalidPointer = 8,
    /**  Host contract not found — no host contract matches contract_id. */
    HostContractNotFound = 100,
    /**  Host contract version mismatch — host contract version does not match. */
    HostContractVersionMismatch = 101,
    /**  Host contract call failed — host contract function call failed. */
    HostContractCallFailed = 102,
}

/**  Dispatch mechanism type — determines how function calls are routed. */
export const enum DispatchType {
    /**  Native dispatch: direct function pointer calls (zero overhead). */
    Native = 0,
    /**  VM dispatch: call through a dispatch function with loader_data. */
    VirtualMachine = 1,
}

/**  Host runtime type identifier — identifies the language/runtime hosting plugins. */
export const enum HostRuntime {
    Rust = 0,
    Python = 1,
    Lua = 2,
    JavaScript = 3,
}

/**
 *  Union of dispatch mechanisms — use based on `dispatch_type`.
 * 
 *  # Safety
 *  Access the correct variant based on `PluginInterface::dispatch_type`:
 *  - `dispatch_type == Native` → access `.native`
 *  - `dispatch_type == VirtualMachine` → access `.vm`
 */
export type PluginDispatch =
    | { native: NativeDispatch }
    | { vm: VmDispatch }
;

/**
 *  Union of host contract dispatch mechanisms — use based on `dispatch_type`.
 * 
 *  # Safety
 *  Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
 *  - `dispatch_type == Native` → access `.native`
 *  - `dispatch_type == VirtualMachine` → access `.vm`
 */
export type HostContractDispatch =
    | { native: NativeHostContractDispatch }
    | { vm: VmHostContractDispatch }
;

export function string_view_from_static(bytes: &'static[u8]): StringView {}

export function string_view_null(): StringView {}

export function string_view_as_str(sv: &StringView): &str {}

export function string_view_to_string_owned(sv: &StringView): String {}

export function buffer_as_slice(buf: &Buffer): &[u8] {}

export function buffer_as_mut_slice(buf: &mutBuffer): &mut[u8] {}

export function abi_error_ok(): AbiError {}

export function abi_error_panic_caught(): AbiError {}

export function abi_error_is_ok(err: &AbiError): boolean {}

export function plugin_handle_null(): PluginHandle {}

export function plugin_handle_is_null(handle: &PluginHandle): boolean {}

export const POLYPLUG_ABI_VERSION: number = 1;

export function fnv1a_64(data: &[u8]): bigint {}

export function contract_id(name: &str, major: number): bigint {}

export function bundle_id(name: &str): bigint {}

export function host_contract_id(name: &str, major: number): bigint {}

export function plugin_contract_id(name: &str, major: number): bigint {}

