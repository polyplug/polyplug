local ffi = require("ffi")
local M = {}

    //  Non-owning UTF-8 string view.
    // 
    //  OWNERSHIP: borrowed reference. `ptr` must remain valid for the duration
    //  of the call. Never freed by the receiver.
    typedef struct StringView {
        //  UTF-8 bytes, NOT null-terminated.
        const uint8_t* ptr;
        //  Byte count.
        size_t len;
    } StringView;

    //  Owning byte buffer.
    // 
    //  OWNERSHIP: `ptr` is always allocated via `polyplug_host_alloc`.
    //  Owner calls `polyplug_host_free(ptr, cap, align)` when done.
    typedef struct Buffer {
        uint8_t* ptr;
        //  Bytes currently used.
        size_t len;
        //  Bytes allocated.
        size_t cap;
    } Buffer;

    //  ABI error — returned by value from all ABI calls.
    // 
    //  OWNERSHIP: `code` is a value type. `message.ptr` is allocated by the callee
    //  via `host_alloc`. Caller frees with `polyplug_host_free(message.ptr, message.len, 1)`
    //  after reading. If `code == ABI_OK`, `message.ptr` is NULL — no free needed.
    typedef struct AbiError {
        //  0 = success, non-zero = error.
        uint32_t code;
        //  Empty/NULL if success. UTF-8 message if non-zero code.
        StringView message;
    } AbiError;

    //  Opaque handle to a loaded plugin — validated on use.
    // 
    //  INTERNAL STRUCTURE: index into registry array + generation counter.
    //  The generation counter detects use-after-unload.
    typedef struct PluginHandle {
        //  Slot in the registry array.
        uint32_t index;
        //  Incremented on unload — detects stale handles.
        uint32_t generation;
    } PluginHandle;

    //  Opaque host context passed to plugin functions via rt_ctx parameter.
    // 
    //  Contains the runtime pointer and the bundle_id of the calling bundle.
    //  The actual implementation is in the polyplug crate; this definition
    //  establishes the ABI layout.
    // 
    //  OWNERSHIP: `'static`, lives as long as the runtime.
    typedef struct HostContext {
        //  Opaque pointer to the Runtime. Never dereferenced by plugins.
        void* runtime;
        //  Bundle ID of the calling bundle for dependency enforcement.
        uint64_t bundle_id;
    } HostContext;

    //  Native dispatch data — direct function pointer array.
    // 
    //  Used when `dispatch_type == DispatchType::Native`.
    //  The `functions` array contains `function_count` function pointers.
    typedef struct NativeDispatch {
        //  Pointer to a static array of function pointers, indexed by function_id.
        void* const* functions;
    } NativeDispatch;

    //  VM dispatch data — call through a dispatch function.
    // 
    //  Used when `dispatch_type == DispatchType::VirtualMachine`.
    //  The `call` function receives `loader_data` which contains VM-specific state.
    typedef struct VmDispatch {
        //  Dispatch function called for every VM function invocation.
        // 
        //  # Arguments
        //  - `loader_data`: VM-specific data (cast from `*mut c_void`)
        //  - `fn_id`: Function index within the contract
        //  - `args`: Pointer to packed arguments (ABI-specific layout)
        //  - `out`: Pointer to output buffer for return value
        AbiError(*)(void*, uint32_t, const void*, void*) call;
        //  Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
        //  Opaque to the host; interpreted by the dispatch function.
        void* loader_data;
    } VmDispatch;

    //  Plugin interface — one per contract implemented by a plugin.
    // 
    //  OWNERSHIP: Must be `'static` or intentionally leaked.
    //  Never stack-allocated. Never freed while runtime lives.
    // 
    //  # Dispatch
    //  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
    //  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
    typedef struct PluginInterface {
        //  Pointer to the host context for this plugin.
        //  Used for host function calls and dependency enforcement.
        const HostContext* rt_ctx;
        //  FNV-1a hash of "contract_name@major_version".
        uint64_t contract_id;
        //  minor.patch encoded as `(minor << 16 | patch)`.
        uint32_t contract_version;
        //  Number of valid entries in the dispatch array.
        uint32_t function_count;
        //  Dispatch mechanism type (Native or VirtualMachine).
        DispatchType dispatch_type;
        //  Union of dispatch mechanisms — access based on dispatch_type.
        PluginDispatch dispatch;
    } PluginInterface;

    //  Host contract vtable header — metadata for a host-provided contract.
    typedef struct HostContractVTableHeader {
        //  VTable format version (for future compatibility).
        uint32_t vtable_version;
        //  FNV-1a hash of "contract_name@major_version".
        uint64_t contract_id;
        //  Contract major version.
        uint32_t contract_major;
        //  Contract minor version.
        uint32_t contract_minor;
        //  Number of functions in this contract.
        uint32_t function_count;
        //  Dispatch mechanism type (Native or VirtualMachine).
        DispatchType dispatch_type;
    } HostContractVTableHeader;

    //  Native dispatch for host contracts — direct function pointer array.
    // 
    //  Used when `dispatch_type == DispatchType::Native`.
    //  The `functions` array contains `function_count` function pointers.
    typedef struct NativeHostContractDispatch {
        //  Pointer to the implementation (e.g., Box<dyn Trait> as *const c_void).
        //  This is passed as the first argument to all native dispatch functions.
        const void* impl_ptr;
        //  Pointer to a static array of function pointers, indexed by function_id.
        void* const* functions;
    } NativeHostContractDispatch;

    //  VM dispatch for host contracts — call through a dispatch function.
    // 
    //  Used when `dispatch_type == DispatchType::VirtualMachine`.
    //  The `call` function receives `bridge_data` which contains VM-specific state.
    typedef struct VmHostContractDispatch {
        //  Dispatch function called for every VM function invocation.
        // 
        //  # Arguments
        //  - `bridge_data`: VM-specific data (cast from `*mut c_void`)
        //  - `fn_id`: Function index within the contract
        //  - `args`: Pointer to packed arguments (ABI-specific layout)
        //  - `out`: Pointer to output buffer for return value
        AbiError(*)(void*, uint32_t, const void*, void*) call;
        //  VM-specific data (opaque to the host; interpreted by the dispatch function).
        void* bridge_data;
    } VmHostContractDispatch;

    //  Host contract vtable — complete interface for a host-provided contract.
    // 
    //  OWNERSHIP: Must be `'static` or intentionally leaked.
    //  Never stack-allocated. Never freed while runtime lives.
    // 
    //  # Dispatch
    //  - `dispatch_type == Native`: Call via `dispatch.native.functions[fn_id]`
    //  - `dispatch_type == VirtualMachine`: Call via `dispatch.vm.call(...)`
    typedef struct HostContractVTable {
        //  Header containing contract metadata.
        HostContractVTableHeader header;
        //  Union of dispatch mechanisms — access based on dispatch_type.
        HostContractDispatch dispatch;
    } HostContractVTable;

    //  Host capabilities passed to every plugin at init time.
    // 
    //  OWNERSHIP: `'static`, lives as long as the runtime.
    // 
    //  All functions take `rt_ctx` as first parameter - an opaque pointer to the Runtime.
    //  This allows each Runtime to have its own isolated state (no global registry).
    typedef struct HostVTable {
        AbiError(*)(void*, const PluginDescriptor*, const PluginInterface*) register_plugin;
        uint8_t*(*)(void*, size_t, size_t) alloc;
        void(*)(void*, uint8_t*, size_t, size_t, )) free;
        PluginHandle(*)(void*, uint64_t, uint32_t) find_by_contract;
        PluginHandle(*)(void*, uint64_t, uint64_t, uint32_t) find_by_bundle;
        size_t(*)(void*, uint64_t, uint32_t, PluginHandle*, size_t) find_all_by_contract;
        const PluginInterface*(*)(void*, PluginHandle) resolve_plugin;
        //  Get host contract vtable by contract_id and minimum version.
        //  Returns null if no host contract matches the criteria.
        const HostContractVTable*(*)(void*, uint64_t, uint32_t) get_host_contract;
    } HostVTable;

    //  Metadata about a plugin within a bundle.
    // 
    //  OWNERSHIP: value type passed by pointer during init. The `name` and
    //  `contract_name` StringViews are borrowed from the plugin's static memory.
    //  The receiver must not free or outlive the plugin's library.
    typedef struct PluginDescriptor {
        //  Human-readable plugin name.
        StringView name;
        //  Full contract name for collision detection.
        StringView contract_name;
        uint32_t version_major;
        uint32_t version_minor;
        uint32_t version_patch;
    } PluginDescriptor;

    //  Context passed to every guest `polyplug_init()` function.
    //  The `bundle_path` pointer is runtime-owned and valid for the lifetime of the `PluginRuntime`.
    //  **Plugin must not store the raw pointer** — copy the string value if persistence is needed.
    typedef struct PluginContext {
        //  Absolute canonical path to the directory containing the loaded bundle.
        StringView bundle_path;
        //  Host's supported ABI version for negotiation (Option C).
        //  Plugin can use this to determine available features.
        uint32_t host_abi_version;
        //  Bundle ID for dependency enforcement during init.
        uint64_t bundle_id;
    } PluginContext;

    //  Configuration passed to `polyplug_runtime_create` during runtime initialisation.
    // 
    //  OWNERSHIP: borrowed for the duration of the runtime build only.
    //  The caller may free all pointed-to memory after the build
    //  returns. The runtime copies any data it needs to retain.
    typedef struct RuntimeConfig {
        //  Plugin directories to scan (array of `plugin_dir_count` StringViews).
        const StringView* plugin_dirs;
        size_t plugin_dir_count;
        //  Compatibility mode: 0 = Strict (only mode implemented in MVP).
        uint32_t compatibility;
    } RuntimeConfig;

    //  Dispatch mechanism type — determines how function calls are routed.
    typedef enum DispatchType {
        //  Native dispatch: direct function pointer calls (zero overhead).
        DispatchType_Native = 0,
        //  VM dispatch: call through a dispatch function with loader_data.
        DispatchType_VirtualMachine = 1,
    } DispatchType;

    //  Host runtime type identifier — identifies the language/runtime hosting plugins.
    typedef enum HostRuntime {
        HostRuntime_Rust = 0,
        HostRuntime_Python = 1,
        HostRuntime_Lua = 2,
        HostRuntime_JavaScript = 3,
    } HostRuntime;

    //  Union of dispatch mechanisms — use based on `dispatch_type`.
    // 
    //  # Safety
    //  Access the correct variant based on `PluginInterface::dispatch_type`:
    //  - `dispatch_type == Native` → access `.native`
    //  - `dispatch_type == VirtualMachine` → access `.vm`
    typedef union PluginDispatch {
        NativeDispatch native;
        VmDispatch vm;
    } PluginDispatch;

    //  Union of host contract dispatch mechanisms — use based on `dispatch_type`.
    // 
    //  # Safety
    //  Access the correct variant based on `HostContractVTableHeader::dispatch_type`:
    //  - `dispatch_type == Native` → access `.native`
    //  - `dispatch_type == VirtualMachine` → access `.vm`
    typedef union HostContractDispatch {
        NativeHostContractDispatch native;
        VmHostContractDispatch vm;
    } HostContractDispatch;

local function string_view_from_static(&'static[u8] bytes) end

local function string_view_null() end

local function string_view_as_str(&StringView sv) end

local function string_view_to_string_owned(&StringView sv) end

local function buffer_as_slice(&Buffer buf) end

local function buffer_as_mut_slice(&mutBuffer buf) end

local function abi_error_ok() end

local function abi_error_panic_caught() end

local function abi_error_is_ok(&AbiError err) end

local function plugin_handle_null() end

local function plugin_handle_is_null(&PluginHandle handle) end

M.POLYPLUG_ABI_VERSION = ffi.cast("uint32_t", 1)
M.ABI_OK = ffi.cast("uint32_t", 0)
M.ABI_ERROR_GENERIC = ffi.cast("uint32_t", 1)
M.ABI_BUFFER_TOO_SMALL = ffi.cast("uint32_t", 2)
M.ABI_ERROR_PANIC = ffi.cast("uint32_t", 3)
M.ABI_ERROR_NOT_FOUND = ffi.cast("uint32_t", 4)
M.ABI_ERROR_STALE_HANDLE = ffi.cast("uint32_t", 5)
M.ABI_FUNCTION_NOT_AVAIL = ffi.cast("uint32_t", 6)
M.ABI_ERROR_DUPLICATE_PROVIDER = ffi.cast("uint32_t", 7)
M.ABI_ERROR_INVALID_POINTER = ffi.cast("uint32_t", 8)
M.ABI_HOST_CONTRACT_NOT_FOUND = ffi.cast("uint32_t", 100)
M.ABI_HOST_CONTRACT_VERSION_MISMATCH = ffi.cast("uint32_t", 101)
M.ABI_HOST_CONTRACT_CALL_FAILED = ffi.cast("uint32_t", 102)
local function fnv1a_64(&[u8] data) end

local function contract_id(&str name, uint32_t major) end

local function bundle_id(&str name) end

local function host_contract_id(&str name, uint32_t major) end

local function plugin_contract_id(&str name, uint32_t major) end

return M
