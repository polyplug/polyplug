-- THIS FILE IS MANUALLY MAINTAINED TO MATCH polyplug_abi
-- DO NOT MODIFY FIELD ORDER OR SIZES — these must match the host runtime exactly.

--- ABI constants and types for the polyplug plugin runtime.
-- This module contains the frozen ABI types that match the Rust ABI exactly.
-- DO NOT modify field order or sizes — these must match the host runtime.

local ffi = require("ffi")
local M = {}

-- ─── ABI Constants ────────────────────────────────────────────────────────────

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

-- ─── ABI Enums ────────────────────────────────────────────────────────────────

ffi.cdef[[
    //  Dispatch mechanism type — determines how function calls are routed.
    typedef enum DispatchType {
        //  Native dispatch: direct function pointer calls (zero overhead).
        DispatchType_Native = 0,
        //  VM dispatch: call through a dispatch function with loader_data.
        DispatchType_VirtualMachine = 1,
    } DispatchType;

]]

-- ─── ABI Structs ──────────────────────────────────────────────────────────────

ffi.cdef[[
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

    //  Semantic version (major, minor, patch).
    typedef struct Version {
        uint32_t major;
        uint32_t minor;
        uint32_t patch;
    } Version;

    //  Opaque handle to a loaded guest contract — validated on use.
    //
    //  The handle is just an index into the registry array.
    //  Out-of-bounds indices return InvalidHandle error.
    typedef struct PluginHandle {
        //  Slot in the registry array.
        uint32_t index;
    } PluginHandle;

    //  Opaque handle to a guest contract instance.
    //  Created by GuestContractInterface.create_instance, destroyed by destroy_instance.
    typedef struct GuestContractInstance {
        void* data;
        uint64_t contract_id;
    } GuestContractInstance;

    //  Opaque handle to a host contract instance.
    typedef struct HostContractInstance {
        void* data;
    } HostContractInstance;

    //  Opaque handle to VM loader state.
    typedef struct VmLoaderData {
        void* data;
    } VmLoaderData;

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

    //  Native dispatch data — direct function pointer array.
    //
    //  Used when `dispatch_type == DispatchType::Native`.
    typedef struct NativeDispatch {
        //  Pointer to a static array of function pointers, indexed by function_id.
        void* const* functions;
    } NativeDispatch;

    //  VM dispatch data — call through a dispatch function.
    //
    //  Used when `dispatch_type == DispatchType::VirtualMachine`.
    typedef struct VmDispatch {
        AbiError (*call )(void*, GuestContractInstance, uint32_t, const void*, void*);
        //  Loader-specific data (e.g., LuaLoaderData, JsLoaderData).
        void* loader_data;
    } VmDispatch;

]]

-- ─── ABI Unions ───────────────────────────────────────────────────────────────

ffi.cdef[[
    //  Union of dispatch mechanisms — use based on `dispatch_type`.
    //
    //  # Safety
    //  Access the correct variant based on `GuestContractInterface::dispatch_type`:
    //  - `dispatch_type == Native` → access `.native`
    //  - `dispatch_type == VirtualMachine` → access `.vm`
    typedef union DispatchMechanisms {
        //  Native dispatch data (when dispatch_type == Native).
        NativeDispatch native;
        //  VM dispatch data (when dispatch_type == VirtualMachine).
        VmDispatch vm;
    } DispatchMechanisms;

]]

-- ─── ABI Structs (after unions) ──────────────────────────────────────────────

ffi.cdef[[
    // Forward declarations
    typedef struct HostInterface HostInterface;

    //  Metadata about a plugin within a bundle.
    //
    //  OWNERSHIP: value type passed by pointer during init. The `name` and
    //  `contract_name` StringViews are borrowed from the plugin's static memory.
    //  The receiver must not free or outlive the plugin's library.
    typedef struct PluginDescriptor {
        StringView name;
        StringView contract_name;
        uint32_t version_major;
        uint32_t version_minor;
        uint32_t version_patch;
    } PluginDescriptor;

    //  Guest Contract Interface — one per contract implemented by a guest (plugin).
    //
    //  OWNERSHIP: Must be `'static` or intentionally leaked.
    //  Never stack-allocated. Never freed while runtime lives.
    //
    //  Layout (56 bytes):
    //  - contract_id (u64): 8 bytes @ 0
    //  - contract_version (Version): 12 bytes @ 8
    //  - dispatch_type (u32): 4 bytes @ 20
    //  - padding: 4 bytes
    //  - create_instance (fn ptr): 8 bytes @ 24
    //  - destroy_instance (fn ptr): 8 bytes @ 32
    //  - dispatch (union): 16 bytes @ 40
    typedef struct GuestContractInterface {
        uint64_t contract_id;
        Version contract_version;
        DispatchType dispatch_type;
        // 4 bytes padding
        GuestContractInstance (*create_instance)(const HostInterface*, const void*);
        void (*destroy_instance)(const HostInterface*, GuestContractInstance);
        DispatchMechanisms dispatch;
    } GuestContractInterface;

    //  Host Interface — function table passed to guests during initialization.
    //
    //  OWNERSHIP: `'static`, lives as long as the runtime.
    //
    //  Layout (88 bytes):
    //  - runtime (*mut c_void): 8 bytes @ 0
    //  - register_contract: 8 bytes @ 8
    //  - alloc: 8 bytes @ 16
    //  - free: 8 bytes @ 24
    //  - find_by_contract: 8 bytes @ 32
    //  - find_all_by_contract: 8 bytes @ 40
    //  - resolve_contract: 8 bytes @ 48
    //  - get_host_contract: 8 bytes @ 56
    //  - get_last_error: 8 bytes @ 64
    //  - list_bundles: 8 bytes @ 72
    //  - get_dependencies: 8 bytes @ 80
    typedef struct HostInterface {
        void* runtime;
        AbiError (*register_contract)(const HostInterface*, const PluginDescriptor*, const GuestContractInterface*);
        uint8_t* (*alloc)(const HostInterface*, size_t, size_t);
        void (*free)(const HostInterface*, uint8_t*, size_t, size_t);
        PluginHandle (*find_by_contract)(const HostInterface*, uint64_t, uint32_t);
        struct { void* items; size_t len; size_t align; } (*find_all_by_contract)(const HostInterface*, uint64_t, uint32_t);
        const GuestContractInterface* (*resolve_contract)(const HostInterface*, PluginHandle);
        HostContractInstance (*get_host_contract)(const HostInterface*, uint64_t, uint32_t);
        StringView (*get_last_error)(const HostInterface*);
        struct { void* items; size_t len; size_t align; } (*list_bundles)(const HostInterface*);
        struct { void* items; size_t len; size_t align; } (*get_dependencies)(const HostInterface*);
    } HostInterface;

    //  Context passed to every guest `polyplug_init()` function.
    typedef struct PluginContext {
        uint64_t bundle_id;
        StringView bundle_path;
    } PluginContext;

    //  Configuration passed to `polyplug_runtime_create` during runtime initialisation.
    typedef struct RuntimeConfig {
        const StringView* plugin_dirs;
        size_t plugin_dir_count;
        uint32_t compatibility;
    } RuntimeConfig;

    ]]

-- ─── FNV-1a Hash Helpers ──────────────────────────────────────────────────────

local bit = require("bit")
local FNV_OFFSET = 0xcbf29ce484222325ULL
local FNV_PRIME = 0x00000100000001B3ULL

--- Compute FNV-1a 64-bit hash of a string.
local function fnv1a_64(str)
    local h = FNV_OFFSET
    for i = 1, #str do
        local b = str:byte(i)
        h = bit.bxor(h, b)
        h = h * FNV_PRIME
    end
    return h
end

--- Compute the contract ID for "name@major_version" using FNV-1a 64-bit.
function M.contract_id(name, major_version)
    local s = name .. '@' .. tostring(major_version)
    return fnv1a_64(s)
end

--- Compute a bundle ID from its name using FNV-1a 64-bit hash.
function M.bundle_id(name)
    return fnv1a_64(name)
end

--- Calculate guest contract ID from name and major version.
function M.guest_contract_id(name, major_version)
    local s = 'guest_contract:' .. name .. '@' .. tostring(major_version)
    return fnv1a_64(s)
end

--- Calculate host contract ID from name and major version.
function M.host_contract_id(name, major_version)
    local s = 'host_contract:' .. name .. '@' .. tostring(major_version)
    return fnv1a_64(s)
end

--- Legacy: Calculate plugin contract ID. Use guest_contract_id instead.
function M.plugin_contract_id(name, major_version)
    return M.guest_contract_id(name, major_version)
end

return M