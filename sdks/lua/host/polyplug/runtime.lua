-- sdks/lua/host/polyplug/runtime.lua
-- Runtime class with HostInterface-based API (18-04 refactor).
-- All ABI struct types are imported from the auto-generated abi.lua (per D-26).

local ffi = require('ffi')
local abi = require('polyplug_abi')
local reload_phase = require('polyplug.reload_phase')

-- ─── Host-specific FFI definitions ──────────────────────────────────────────────
-- Only FFI function declarations live here. All struct types (RuntimeConfig,
-- ReloadPhase, HostInterface, etc.) come from the auto-generated abi.lua.
ffi.cdef([[
    // Forward declaration for resolve handle
    typedef struct ResolveHandle ResolveHandle;

    // Only three FFI exports: create, create_with_options, destroy
    const HostInterface* polyplug_runtime_create(void);
    const HostInterface* polyplug_runtime_create_with_options(const void* options);
    void polyplug_runtime_destroy(const HostInterface* host);

    // Host-side options wrapper: references the ABI RuntimeConfig (16 bytes)
    // and the ABI RuntimeConfig_on_reload_fn callback type (both from abi.lua).
    typedef struct {
        const RuntimeConfig* config;
        RuntimeConfig_on_reload_fn on_reload;
    } RuntimeCreateOptions;
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
                compatibility = M._pending_config.compatibility or M.COMPATIBILITY_STRICT,
                hot_reload_enabled = M._pending_config.hot_reload_enabled and 1 or 0,
                on_reload = nil,  -- set separately below if callback provided
            })
            options.config = config_c
        end

        if M._pending_reload_callback then
            local callback = M._pending_reload_callback
            M._ffi_reload_callback = ffi.cast("RuntimeConfig_on_reload_fn", function(
                phase_struct
            )
                -- Extract fields from the ABI ReloadPhase struct
                local phase = reload_phase.new(
                    phase_struct.phase_type,
                    phase_struct.bundle_id,
                    abi.to_str(phase_struct.bundle_name),
                    nil,       -- retry_count removed from ABI
                    abi.to_str(phase_struct.reason)
                )
                callback(phase)
            end)
            options.on_reload = M._ffi_reload_callback
            -- If no config was provided but we have a callback, create a
            -- default config so the on_reload pointer is paired with a config.
            if not config_c then
                config_c = ffi.new("RuntimeConfig", {
                    compatibility = M.COMPATIBILITY_STRICT,
                    hot_reload_enabled = 0,
                    on_reload = nil,
                })
                options.config = config_c
            end
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
