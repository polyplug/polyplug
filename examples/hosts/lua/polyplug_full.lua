-- examples/hosts/lua/polyplug_full.lua
-- Local wrapper around the polyplug Lua host-lib.
--
-- Uses the companion libpolyplug_lua_host.so which provides
-- polyplug_runtime_new_full() — a Runtime built with ALL language loaders
-- (native, Lua, Python, JS-QuickJS, .NET). This enables the Lua host to
-- load all 12 guest plugins across every supported language.
--
-- See host-libs/lua/polyplug.lua for the base implementation.
-- This file copies the base and overrides Runtime.new() to call
-- polyplug_runtime_new_full() instead of polyplug_runtime_new().

local ffi = require("ffi")
local M = {}

ffi.cdef([[
    // Opaque types (never dereferenced in Lua)
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct OpaqueGuard OpaqueGuard;

    OpaqueRuntime* polyplug_runtime_new(void);
    OpaqueRuntime* polyplug_runtime_new_full(void);
    void polyplug_runtime_free(OpaqueRuntime* rt);
    uint32_t polyplug_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_rt_find_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_rt_find_by_bundle(const OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_rt_find_all_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    OpaqueGuard* polyplug_rt_resolve_plugin(const OpaqueRuntime* rt, uint64_t packed_handle);
    void polyplug_guard_free(OpaqueGuard* guard);
    const void* polyplug_get_vtable(const OpaqueGuard* guard);
    size_t polyplug_last_error(uint8_t* buf, size_t buf_len);
    size_t polyplug_error_message_len(void);
]])

-- NULL_HANDLE sentinel: u64::MAX = 0xFFFFFFFFFFFFFFFF
M.NULL_HANDLE = ffi.cast("uint64_t", 0xFFFFFFFFFFFFFFFFULL)

-- Load the companion shared library (must use full path).
-- Use libpolyplug_lua_host.so for all-loader support.
function M.load_lib(so_path)
    M._lib = ffi.load(so_path, true)  -- true = RTLD_GLOBAL: exposes symbols to subsequently loaded guest .so files
    return M._lib
end

-- Helper: get last error string from the runtime.
function M.last_error()
    local len = M._lib.polyplug_error_message_len()
    if len == 0 then return "" end
    local buf = ffi.new("uint8_t[?]", len)
    M._lib.polyplug_last_error(buf, len)
    return ffi.string(buf, len)
end

-- ─── Guard (forward declaration) ──────────────────────────────────────────────
local Guard = {}
Guard.__index = Guard

function Guard:free()
    if self._ptr ~= nil then
        ffi.gc(self._ptr, nil)
        M._lib.polyplug_guard_free(self._ptr)
        self._ptr = nil
    end
end

function Guard:vtable()
    return M._lib.polyplug_get_vtable(self._ptr)
end

-- ─── Runtime ──────────────────────────────────────────────────────────────────
M.Runtime = {}
local Runtime = {}
Runtime.__index = Runtime

-- Construct a new Runtime with ALL language loaders registered.
-- Uses polyplug_runtime_new_full() from the companion cdylib.
function M.Runtime.new()
    local ptr = M._lib.polyplug_runtime_new_full()
    if ptr == nil then
        error("polyplug_runtime_new_full failed: " .. M.last_error())
    end
    local managed = ffi.gc(ptr, M._lib.polyplug_runtime_free)
    return setmetatable({ _ptr = managed }, Runtime)
end

function Runtime:free()
    if self._ptr ~= nil then
        ffi.gc(self._ptr, nil)
        M._lib.polyplug_runtime_free(self._ptr)
        self._ptr = nil
    end
end

function Runtime:load_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_load_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("load_bundle failed: " .. M.last_error())
    end
    return true
end

function Runtime:reload_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_reload_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("reload_bundle failed: " .. M.last_error())
    end
    return true
end

function Runtime:find_by_contract(contract_id, min_version)
    return M._lib.polyplug_rt_find_by_contract(self._ptr, contract_id, min_version or 0)
end

function Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    return M._lib.polyplug_rt_find_by_bundle(self._ptr, bundle_id, contract_id, min_version or 0)
end

function Runtime:find_all_by_contract(contract_id, min_version, cap)
    cap = cap or 64
    local out = ffi.new("uint64_t[?]", cap)
    local count = M._lib.polyplug_rt_find_all_by_contract(self._ptr, contract_id, min_version or 0, out, cap)
    local result = {}
    for i = 0, math.min(count, cap) - 1 do
        result[i + 1] = out[i]
    end
    return result, count
end

function Runtime:resolve_plugin(packed_handle)
    local ptr = M._lib.polyplug_rt_resolve_plugin(self._ptr, packed_handle)
    if ptr == nil then
        return nil, M.last_error()
    end
    local managed = ffi.gc(ptr, M._lib.polyplug_guard_free)
    return setmetatable({ _ptr = managed }, Guard)
end

return M
