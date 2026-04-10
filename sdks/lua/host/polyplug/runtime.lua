-- sdks/lua/host/polyplug/runtime.lua
-- Runtime class with HostInterface-based API (18-04 refactor).

local ffi = require('ffi')
local abi = require('polyplug_abi')
local reload_phase = require('polyplug.reload_phase')

-- ─── HostInterface FFI Definition ───────────────────────────────────────────────
-- HostInterface struct (144 bytes, 18 pointer fields)
-- All operations are accessed through this struct, not via separate FFI functions.
ffi.cdef([[
    // HostInterface struct — 144 bytes, 18 pointer fields
    // Field order must match Rust #[repr(C)] layout exactly (ABI stability)
    typedef struct HostInterface {
        void* runtime;                          // offset 0
        void* register_contract;               // offset 8
        void* alloc;                           // offset 16
        void* free;                            // offset 24
        void* find_guest_contract;             // offset 32
        void* find_all_guest_contracts;        // offset 40
        void* resolve_guest_contract;          // offset 48
        void* call_guest_method;               // offset 56
        void* get_host_contract;               // offset 64
        void* resolve_host_contract_interface; // offset 72
        void* list_bundles;                    // offset 80
        void* get_dependencies;                // offset 88
        void* load_bundle;                     // offset 96
        void* reload_bundle;                   // offset 104
        void* register_host_contract;          // offset 112
        void* register_loader;                 // offset 120
        void* get_last_error;                  // offset 128
        void* get_error_len;                   // offset 136
    } HostInterface;

    typedef struct ResolveHandle ResolveHandle;

    // Only three FFI exports: create, create_with_options, destroy
    const HostInterface* polyplug_runtime_create(void);
    const HostInterface* polyplug_runtime_create_with_options(const void* options);
    void polyplug_runtime_destroy(const HostInterface* host);

    // RuntimeConfig for create_with_options (24 bytes)
    typedef struct {
        uint8_t hot_reload_enabled;        // offset 0
        uint8_t _pad1[3];                  // padding for alignment
        uint32_t hot_reload_max_retries;   // offset 4
        uint64_t hot_reload_retry_interval_ms; // offset 8
        uint8_t hot_reload_abort_on_max_retries; // offset 16
        uint8_t _pad2[3];                  // padding for alignment
        uint32_t compatibility;            // offset 20 (Compatibility enum: Strict=0, Relaxed=1, Yolo=2)
    } RuntimeConfig;

    typedef void (*ReloadPhaseCallback)(
        uint32_t phase_type,
        uint64_t bundle_id,
        const uint8_t* bundle_name,
        size_t bundle_name_len,
        uint32_t retry_count,
        const uint8_t* reason,
        size_t reason_len
    );

    typedef struct {
        const RuntimeConfig* config;
        ReloadPhaseCallback on_reload;
    } RuntimeCreateOptions;

    // Host Contract Interface types
    typedef struct HostContractInterfaceHeader {
        uint32_t vtable_version;
        uint64_t contract_id;
        uint32_t contract_major;
        uint32_t contract_minor;
        uint32_t function_count;
        DispatchType dispatch_type;
    } HostContractInterfaceHeader;

    typedef struct NativeHostContractDispatch {
        void* const* functions;
    } NativeHostContractDispatch;

    typedef struct VmHostContractDispatch {
        AbiError (*call)(void*, uint32_t, const void*, void*);
        void* bridge_data;
    } VmHostContractDispatch;

    typedef union HostContractDispatch {
        NativeHostContractDispatch native;
        VmHostContractDispatch vm;
    } HostContractDispatch;

    typedef struct HostContractInterface {
        HostContractInterfaceHeader header;
        HostContractDispatch dispatch;
    } HostContractInterface;
]])

local M = {}

local bit = require("bit")
local FNV_OFFSET = 0xcbf29ce484222325ULL
local FNV_PRIME = 0x00000100000001B3ULL

M.NULL_HANDLE = ffi.cast("uint64_t", "0xFFFFFFFFFFFFFFFF")
M.AbiErrorCode = abi.AbiErrorCode

M.contract_id = abi.contract_id
M.bundle_id = abi.bundle_id
M.extension_id = abi.extension_id

-- Compatibility modes matching polyplug_abi::Compatibility #[repr(u32)]
M.COMPATIBILITY_STRICT = 0
M.COMPATIBILITY_RELAXED = 1
M.COMPATIBILITY_YOLO = 2

--- Compute the host contract ID for "host_contract:name@major_version" using FNV-1a 64-bit.
-- @param name string         The host contract name (must start with "host.").
-- @param major_version number The major version.
-- @return number             The host contract ID.
function M.host_contract_id(name, major_version)
    local s = "host_contract:" .. name .. "@" .. tostring(major_version)
    local h = FNV_OFFSET
    for i = 1, #s do
        local b = s:byte(i)
        h = bit.bxor(h, b)
        h = h * FNV_PRIME
    end
    return h
end

local function get_lib()
    if not M._lib then
        error("polyplug: library not loaded. Call load_lib() first.")
    end
    return M._lib
end

function M.load_lib(so_path)
    M._lib = ffi.load(so_path)
    return M._lib
end

--- Get last error message from HostInterface.
-- @param host HostInterface*  The host interface pointer.
-- @param lib                  The library handle.
-- @return string              The error message, or empty string.
function M.last_error(host, lib)
    lib = lib or get_lib()
    -- Cast and call through HostInterface.get_error_len field
    local get_error_len_fn = ffi.cast("size_t(*)(const HostInterface*)", host.get_error_len)
    local len = get_error_len_fn(host)
    if len == 0 then
        return ""
    end
    local buf = ffi.new("uint8_t[?]", len)
    -- Cast and call through HostInterface.get_last_error field
    local get_last_error_fn = ffi.cast("size_t(*)(const HostInterface*, uint8_t*, size_t)", host.get_last_error)
    get_last_error_fn(host, buf, len)
    return ffi.string(buf, len)
end

M.Runtime = {}
M.Runtime.__index = M.Runtime

M._pending_reload_callback = nil
M._pending_config = nil

function M.on_reload(callback)
    M._pending_reload_callback = callback
end

function M.set_config(config)
    M._pending_config = config
end

--- Create a new Runtime instance.
-- Uses HostInterface-based API: polyplug_runtime_create returns HostInterface*.
-- @return Runtime             The runtime instance.
function M.Runtime.new()
    local lib = get_lib()
    local host_ptr

    if M._pending_config or M._pending_reload_callback then
        local options = ffi.new("RuntimeCreateOptions")
        local config_c

        if M._pending_config then
            config_c = ffi.new("RuntimeConfig", {
                hot_reload_enabled = M._pending_config.hot_reload_enabled and 1 or 0,
                _pad1 = {0, 0, 0},
                hot_reload_max_retries = M._pending_config.hot_reload_max_retries,
                hot_reload_retry_interval_ms = M._pending_config.hot_reload_retry_interval_ms,
                hot_reload_abort_on_max_retries = M._pending_config.hot_reload_abort_on_max_retries and 1 or 0,
                _pad2 = {0, 0, 0},
                compatibility = M._pending_config.compatibility or M.COMPATIBILITY_STRICT,
            })
            options.config = config_c
        end

        if M._pending_reload_callback then
            local callback = M._pending_reload_callback
            M._ffi_reload_callback = ffi.cast("ReloadPhaseCallback", function(
                phase_type, bundle_id, bundle_name_ptr, bundle_name_len,
                retry_count, reason_ptr, reason_len
            )
                local bundle_name = ""
                if bundle_name_ptr ~= nil and bundle_name_len > 0 then
                    bundle_name = ffi.string(bundle_name_ptr, bundle_name_len)
                end
                local reason = ""
                if reason_ptr ~= nil and reason_len > 0 then
                    reason = ffi.string(reason_ptr, reason_len)
                end
                local phase = reload_phase.new(
                    phase_type, bundle_id, bundle_name, retry_count, reason
                )
                callback(phase)
            end)
            options.on_reload = M._ffi_reload_callback
        end

        host_ptr = lib.polyplug_runtime_create_with_options(options)
    else
        host_ptr = lib.polyplug_runtime_create()
    end

    if host_ptr == nil then
        error("polyplug_runtime_create failed: returned null HostInterface")
    end

    -- Dereference HostInterface struct from pointer
    local host = host_ptr[0]
    local self = {
        _host = host_ptr,      -- HostInterface* pointer (for FFI calls)
        _host_struct = host,   -- Dereferenced struct (for field access)
        _lib = lib,
        _destroyed = false
    }
    local obj = setmetatable(self, M.Runtime)

    -- Finalizer: destroy HostInterface when GC collects
    ffi.gc(host_ptr, function(ptr)
        if not self._destroyed and ptr ~= nil then
            lib.polyplug_runtime_destroy(ptr)
        end
    end)
    return obj
end

--- Load a plugin bundle from path.
-- Calls through HostInterface.load_bundle field.
-- @param path string  Path to bundle directory.
function M.Runtime:load_bundle(path)
    local path_str = tostring(path)
    local path_bytes = ffi.new("uint8_t[?]", #path_str, path_str)
    -- Cast function pointer and call with self-passing pattern
    local fn = ffi.cast("uint32_t(*)(const HostInterface*, const uint8_t*, size_t)", self._host_struct.load_bundle)
    local result = fn(self._host, path_bytes, #path_str)
    if result ~= 0 then
        error("load_bundle failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Reload a plugin bundle (hot-reload).
-- Calls through HostInterface.reload_bundle field.
-- @param path string  Path to bundle directory.
function M.Runtime:reload_bundle(path)
    local path_str = tostring(path)
    local path_bytes = ffi.new("uint8_t[?]", #path_str, path_str)
    -- Cast function pointer and call with self-passing pattern
    local fn = ffi.cast("uint32_t(*)(const HostInterface*, const uint8_t*, size_t)", self._host_struct.reload_bundle)
    local result = fn(self._host, path_bytes, #path_str)
    if result ~= 0 then
        error("reload_bundle failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Find guest contract by contract_id and minimum version.
-- Calls through HostInterface.find_guest_contract field.
-- @param contract_id number  Contract identifier hash.
-- @param min_version number  Minimum version required.
-- @return number             Packed handle, or NULL_HANDLE if not found.
function M.Runtime:find_guest_contract(contract_id, min_version)
    -- Cast function pointer and call with self-passing pattern
    local fn = ffi.cast("uint64_t(*)(const HostInterface*, uint64_t, uint32_t)", self._host_struct.find_guest_contract)
    return fn(self._host, contract_id, min_version)
end

--- Find guest contract by bundle_id and contract_id.
-- Note: This method is NOT in HostInterface (removed from FFI surface).
-- It's a convenience wrapper that would require different backend support.
-- For now, returns NULL_HANDLE to indicate unimplemented.
-- @param bundle_id number    Bundle identifier.
-- @param contract_id number  Contract identifier hash.
-- @param min_version number  Minimum version required.
-- @return number             Packed handle, or NULL_HANDLE.
function M.Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    -- Note: find_by_bundle is not in HostInterface (18-02 removed it from FFI surface)
    -- This method is deprecated and returns NULL_HANDLE
    -- TODO: Implement via list_bundles + find_guest_contract if needed
    return M.NULL_HANDLE
end

--- Find all guest contracts matching contract_id and minimum version.
-- Calls through HostInterface.find_all_guest_contracts field.
-- @param contract_id number  Contract identifier hash.
-- @param min_version number  Minimum version required.
-- @param cap number          Maximum results to return (default 64).
-- @return table              Array of packed handles.
function M.Runtime:find_all_guest_contracts(contract_id, min_version, cap)
    cap = cap or 64
    -- Cast function pointer and call with self-passing pattern
    -- Returns Array<GuestContractHandle> struct with ptr and len
    local fn = ffi.cast("struct { uint64_t* ptr; size_t len; }(*)(const HostInterface*, uint64_t, uint32_t)", self._host_struct.find_all_guest_contracts)
    local arr = fn(self._host, contract_id, min_version)
    local result = {}
    for i = 0, math.min(arr.len, cap) - 1 do
        table.insert(result, arr.ptr[i])
    end
    -- Free the array via HostInterface.free
    if arr.ptr ~= nil and arr.len > 0 then
        local free_fn = ffi.cast("void(*)(const HostInterface*, void*, size_t, size_t)", self._host_struct.free)
        free_fn(self._host, arr.ptr, arr.len * 8, 8)
    end
    return result
end

--- Resolve a packed handle to a resolve_handle pointer.
-- Calls through HostInterface.resolve_guest_contract field.
-- @param packed_handle number  Packed handle from find_guest_contract.
-- @return cdata                ResolveHandle* pointer, or nil if invalid.
function M.Runtime:resolve_guest_contract(packed_handle)
    if packed_handle == M.NULL_HANDLE then
        return nil, "null handle"
    end
    -- Cast function pointer and call with self-passing pattern
    local fn = ffi.cast("const ResolveHandle*(*)(const HostInterface*, uint64_t)", self._host_struct.resolve_guest_contract)
    local resolve_handle = fn(self._host, packed_handle)
    if resolve_handle == nil then
        return nil, M.last_error(self._host, self._lib)
    end
    return resolve_handle
end

--- Register a host contract interface with the runtime.
-- Calls through HostInterface.register_host_contract field.
-- @param interface HostContractInterface*  Pointer to host contract interface.
function M.Runtime:register_host_contract(interface)
    if interface == nil then
        error("register_host_contract: null interface pointer")
    end
    -- Cast function pointer and call with self-passing pattern
    local fn = ffi.cast("uint32_t(*)(const HostInterface*, const HostContractInterface*)", self._host_struct.register_host_contract)
    local result = fn(self._host, interface)
    if result == 1 then
        error("register_host_contract: null interface pointer")
    elseif result == 2 then
        error("register_host_contract: duplicate contract registration")
    elseif result ~= 0 then
        error("register_host_contract failed: " .. M.last_error(self._host, self._lib))
    end
end

--- Get the HostInterface pointer.
-- @return HostInterface*  The host interface pointer.
function M.Runtime:host()
    return self._host
end

--- Destroy the runtime explicitly.
-- Call polyplug_runtime_destroy on the HostInterface.
function M.Runtime:destroy()
    if self._host ~= nil and not self._destroyed then
        self._lib.polyplug_runtime_destroy(self._host)
        self._destroyed = true
        self._host = nil
    end
end

-- ─── Backward Compatibility Aliases ─────────────────────────────────────────────
-- These aliases allow old code to work with the new HostInterface-based API.
-- Deprecated: Use find_guest_contract, find_all_guest_contracts, resolve_guest_contract instead.

--- Alias for find_guest_contract (deprecated).
function M.Runtime:find(contract_id, min_version)
    return self:find_guest_contract(contract_id, min_version)
end

--- Alias for find_all_guest_contracts (deprecated).
function M.Runtime:find_all_by_contract(contract_id, min_version, cap)
    return self:find_all_guest_contracts(contract_id, min_version, cap)
end

--- Alias for resolve_guest_contract (deprecated).
function M.Runtime:resolve_plugin(packed_handle)
    return self:resolve_guest_contract(packed_handle)
end

return M