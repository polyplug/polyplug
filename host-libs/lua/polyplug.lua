-- host-libs/lua/polyplug.lua
-- LuaJIT FFI host library for polyplug.
-- Requires LuaJIT (not standard Lua) for the ffi module.

local ffi = require("ffi")
local M = {}

ffi.cdef([[
    // Opaque types (never dereferenced in Lua)
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct OpaqueGuard OpaqueGuard;

    OpaqueRuntime* polyplug_runtime_new(void);
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
-- Use ULL suffix (LuaJIT cdata literal syntax)
M.NULL_HANDLE = ffi.cast("uint64_t", 0xFFFFFFFFFFFFFFFFULL)

-- Load the shared library. Caller must pass the full path to libpolyplug.so.
-- Returns the loaded ffi library object (stored on M for re-use).
function M.load_lib(so_path)
    M._lib = ffi.load(so_path)
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
-- Guard MUST be defined before Runtime:resolve_plugin uses it as a metatable
local Guard = {}
Guard.__index = Guard

-- Explicitly free the guard. Disarms the GC finalizer.
function Guard:free()
    if self._ptr ~= nil then
        ffi.gc(self._ptr, nil)  -- disarm GC finalizer to prevent double-free
        M._lib.polyplug_guard_free(self._ptr)
        self._ptr = nil
    end
end

-- Get the raw vtable pointer as an opaque void*.
-- Cast to your contract-specific vtable struct before calling functions.
function Guard:vtable()
    return M._lib.polyplug_get_vtable(self._ptr)
end

-- ─── Runtime ──────────────────────────────────────────────────────────────────
M.Runtime = {}
local Runtime = {}
Runtime.__index = Runtime

-- Construct a new Runtime. The returned object has automatic GC cleanup.
function M.Runtime.new()
    local ptr = M._lib.polyplug_runtime_new()
    if ptr == nil then
        error("polyplug_runtime_new failed: " .. M.last_error())
    end
    -- ffi.gc registers polyplug_runtime_free as the GC finalizer for this pointer.
    -- This is the ONLY safe pattern for pointer cdata (not the metatype gc metamethod).
    local managed = ffi.gc(ptr, M._lib.polyplug_runtime_free)
    return setmetatable({ _ptr = managed }, Runtime)
end

-- Explicitly free the runtime before GC. Disarms the finalizer to prevent double-free.
function Runtime:free()
    if self._ptr ~= nil then
        ffi.gc(self._ptr, nil)  -- disarm GC finalizer
        M._lib.polyplug_runtime_free(self._ptr)
        self._ptr = nil
    end
end

-- Load a bundle from the given directory path.
-- Returns true on success, raises error on failure.
function Runtime:load_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_load_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("load_bundle failed: " .. M.last_error())
    end
    return true
end

-- Reload a bundle. Returns true on success.
function Runtime:reload_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_reload_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("reload_bundle failed: " .. M.last_error())
    end
    return true
end

-- Find the first plugin providing contract_id (a uint64_t cdata or Lua number).
-- min_version: encoded as (minor << 16 | patch), pass 0 to accept any version.
-- Returns packed handle (uint64_t cdata), or M.NULL_HANDLE if not found.
function Runtime:find_by_contract(contract_id, min_version)
    return M._lib.polyplug_rt_find_by_contract(self._ptr, contract_id, min_version or 0)
end

-- Find first plugin providing contract_id from a specific bundle.
function Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    return M._lib.polyplug_rt_find_by_bundle(self._ptr, bundle_id, contract_id, min_version or 0)
end

-- Find all plugins providing contract_id. Returns a Lua table of packed handles.
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

-- Resolve a packed handle to a Guard object. Returns nil + error string on failure.
function Runtime:resolve_plugin(packed_handle)
    local ptr = M._lib.polyplug_rt_resolve_plugin(self._ptr, packed_handle)
    if ptr == nil then
        return nil, M.last_error()
    end
    -- Register GC finalizer for the guard pointer.
    local managed = ffi.gc(ptr, M._lib.polyplug_guard_free)
    return setmetatable({ _ptr = managed }, Guard)
end

return M
