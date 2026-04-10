-- sdks/lua/host/polyplug/runtime.lua
-- Runtime class with hot-reload notification support.

local ffi = require('ffi')
local abi = require('polyplug_abi')
local reload_phase = require('polyplug.reload_phase')

ffi.cdef([[
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct ResolveHandle ResolveHandle;

    OpaqueRuntime* polyplug_runtime_create(void);
    void polyplug_runtime_destroy(OpaqueRuntime* rt);
    uint32_t polyplug_runtime_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_runtime_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_runtime_find_guest_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_runtime_find_by_bundle(const OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_runtime_find_all_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    const ResolveHandle* polyplug_runtime_resolve_guest_contract(const OpaqueRuntime* rt, uint64_t packed_handle);
    void polyplug_runtime_release_plugin(const ResolveHandle* handle);
    size_t polyplug_runtime_error_message_len(void);
    void polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
    void polyplug_host_free(void* ptr, size_t size, size_t align);

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
        uint8_t hot_reload_enabled;        // offset 0
        uint8_t _pad1[3];                  // padding for alignment
        uint32_t hot_reload_max_retries;   // offset 4
        uint64_t hot_reload_retry_interval_ms; // offset 8
        uint8_t hot_reload_abort_on_max_retries; // offset 16
        uint8_t _pad2[3];                  // padding for alignment
        uint32_t compatibility;            // offset 20 (Compatibility enum: Strict=0, Relaxed=1, Yolo=2)
    } RuntimeConfig;

    typedef struct {
        const RuntimeConfig* config;
        ReloadPhaseCallback on_reload;
    } RuntimeCreateOptions;

    OpaqueRuntime* polyplug_runtime_create_with_options(const RuntimeCreateOptions* options);

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

    uint32_t polyplug_runtime_register_host_contract(OpaqueRuntime* rt, const HostContractInterface* interface);
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

function M.last_error(lib)
    lib = lib or get_lib()
    local len = lib.polyplug_runtime_error_message_len()
    if len == 0 then
        return ""
    end
    local buf = ffi.new("uint8_t[?]", len)
    lib.polyplug_runtime_last_error(buf, len)
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

function M.Runtime.new()
    local lib = get_lib()
    local rt_ptr

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

        rt_ptr = lib.polyplug_runtime_create_with_options(options)
    else
        rt_ptr = lib.polyplug_runtime_create()
    end

    if rt_ptr == nil then
        error("polyplug_runtime_create failed")
    end
    local self = { _ptr = rt_ptr, _lib = lib, _destroyed = false }
    local obj = setmetatable(self, M.Runtime)
    ffi.gc(rt_ptr, function(ptr)
        if not self._destroyed and ptr ~= nil then
            lib.polyplug_runtime_destroy(ptr)
        end
    end)
    return obj
end

function M.Runtime:load_bundle(path)
    local lib = self._lib
    local path_str = tostring(path)
    local result = lib.polyplug_runtime_load_bundle(self._ptr, path_str, #path_str)
    if result ~= 0 then
        error("polyplug_runtime_load_bundle failed: " .. result)
    end
end

function M.Runtime:reload_bundle(path)
    local lib = self._lib
    local path_str = tostring(path)
    local result = lib.polyplug_runtime_reload_bundle(self._ptr, path_str, #path_str)
    if result ~= 0 then
        error("polyplug_runtime_reload_bundle failed: " .. result)
    end
end

function M.Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    local lib = self._lib
    return lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)
end

function M.Runtime:find_guest_contract(contract_id, min_version)
    local lib = self._lib
    return lib.polyplug_runtime_find_guest_contract(self._ptr, contract_id, min_version)
end

function M.Runtime:find_all_by_contract(contract_id, min_version, cap)
    cap = cap or 64
    local lib = self._lib
    local out = ffi.new("uint64_t[?]", cap)
    local count = lib.polyplug_runtime_find_all_by_contract(self._ptr, contract_id, min_version, out, cap)
    local result = {}
    for i = 0, math.min(count, cap) - 1 do
        table.insert(result, out[i])
    end
    return result
end

function M.Runtime:register_host_contract(interface)
    local lib = self._lib
    local result = lib.polyplug_runtime_register_host_contract(self._ptr, interface)
    if result == 1 then
        error("polyplug_runtime_register_host_contract: null runtime or interface pointer")
    elseif result == 2 then
        error("polyplug_runtime_register_host_contract: duplicate contract registration")
    elseif result == 3 then
        local err_msg = M.last_error(lib)
        error("polyplug_runtime_register_host_contract failed: " .. err_msg)
    end
end

function M.Runtime:resolve_guest_contract(packed_handle)
    -- Instance-based model: returns raw resolve_handle (cdata) for host to use directly.
    -- Host should:
    --   1. Get resolve_handle from resolve_plugin
    --   2. Access GuestContractInterface via FFI (ResolveHandle first field)
    --   3. Call create_instance on interface
    --   4. Make dispatch calls with instance
    --   5. Call destroy_instance before hot-reload
    if packed_handle == M.NULL_HANDLE then
        return nil, "null handle"
    end
    local lib = self._lib
    local resolve_handle = lib.polyplug_runtime_resolve_guest_contract(self._ptr, packed_handle)
    if resolve_handle == nil then
        return nil, M.last_error(lib)
    end
    return resolve_handle
end

function M.Runtime:destroy()
    if self._ptr ~= nil and not self._destroyed then
        self._lib.polyplug_runtime_destroy(self._ptr)
        self._destroyed = true
        self._ptr = nil
    end
end

return M