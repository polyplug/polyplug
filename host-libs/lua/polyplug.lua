-- host-libs/lua/polyplug.lua
-- LuaJIT FFI host library for polyplug.
-- Requires LuaJIT (not standard Lua) for the ffi module.

local ffi = require("ffi")
local M = {}

ffi.cdef([[
    // Opaque types (never dereferenced in Lua)
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct OpaqueGuard OpaqueGuard;

    OpaqueRuntime* polyplug_runtime_create(void);
    void polyplug_runtime_destroy(OpaqueRuntime* rt);
    uint32_t polyplug_runtime_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint32_t polyplug_runtime_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t path_len);
    uint64_t polyplug_runtime_find_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
    uint64_t polyplug_runtime_find_by_bundle(const OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t polyplug_runtime_find_all_by_contract(const OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
    OpaqueGuard* polyplug_runtime_resolve_plugin(const OpaqueRuntime* rt, uint64_t packed_handle);
    void polyplug_runtime_plugin_release(OpaqueGuard* guard);
    const void* polyplug_runtime_plugin_vtable(const OpaqueGuard* guard);
    size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
    size_t polyplug_runtime_error_message_len(void);
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
    local len = M._lib.polyplug_runtime_error_message_len()
    if len == 0 then return "" end
    local buf = ffi.new("uint8_t[?]", len)
    M._lib.polyplug_runtime_last_error(buf, len)
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
        M._lib.polyplug_runtime_plugin_release(self._ptr)
        self._ptr = nil
    end
end

-- Get the raw vtable pointer as an opaque void*.
-- Cast to your contract-specific vtable struct before calling functions.
function Guard:vtable()
    return M._lib.polyplug_runtime_plugin_vtable(self._ptr)
end

-- ─── Runtime ──────────────────────────────────────────────────────────────────
M.Runtime = {}
local Runtime = {}
Runtime.__index = Runtime

-- Construct a new Runtime. The returned object has automatic GC cleanup.
function M.Runtime.new()
    local ptr = M._lib.polyplug_runtime_create()
    if ptr == nil then
        error("polyplug_runtime_create failed: " .. M.last_error())
    end
    -- ffi.gc registers polyplug_runtime_destroy as the GC finalizer for this pointer.
    -- This is the ONLY safe pattern for pointer cdata (not the metatype gc metamethod).
    local managed = ffi.gc(ptr, M._lib.polyplug_runtime_destroy)
    return setmetatable({ _ptr = managed }, Runtime)
end

-- Explicitly free the runtime before GC. Disarms the finalizer to prevent double-free.
function Runtime:free()
    if self._ptr ~= nil then
        ffi.gc(self._ptr, nil)  -- disarm GC finalizer
        M._lib.polyplug_runtime_destroy(self._ptr)
        self._ptr = nil
    end
end

-- Load a bundle from the given directory path.
-- Returns true on success, raises error on failure.
function Runtime:load_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_runtime_load_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("load_bundle failed: " .. M.last_error())
    end
    return true
end

-- Reload a bundle. Returns true on success.
function Runtime:reload_bundle(path)
    local path_bytes = ffi.cast("const uint8_t*", path)
    local result = M._lib.polyplug_runtime_reload_bundle(self._ptr, path_bytes, #path)
    if result ~= 0 then
        error("reload_bundle failed: " .. M.last_error())
    end
    return true
end

-- Find the first plugin providing contract_id (a uint64_t cdata or Lua number).
-- min_version: encoded as (minor << 16 | patch), pass 0 to accept any version.
-- Returns packed handle (uint64_t cdata), or M.NULL_HANDLE if not found.
function Runtime:find_by_contract(contract_id, min_version)
    return M._lib.polyplug_runtime_find_by_contract(self._ptr, contract_id, min_version or 0)
end

-- Find first plugin providing contract_id from a specific bundle.
function Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    return M._lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version or 0)
end

-- Find all plugins providing contract_id. Returns a Lua table of packed handles.
function Runtime:find_all_by_contract(contract_id, min_version, cap)
    cap = cap or 64
    local out = ffi.new("uint64_t[?]", cap)
    local count = M._lib.polyplug_runtime_find_all_by_contract(self._ptr, contract_id, min_version or 0, out, cap)
    local result = {}
    for i = 0, math.min(count, cap) - 1 do
        result[i + 1] = out[i]
    end
    return result, count
end

-- Resolve a packed handle to a Guard object. Returns nil + error string on failure.
function Runtime:resolve_plugin(packed_handle)
    local ptr = M._lib.polyplug_runtime_resolve_plugin(self._ptr, packed_handle)
    if ptr == nil then
        return nil, M.last_error()
    end
    -- Register GC finalizer for the guard pointer.
    local managed = ffi.gc(ptr, M._lib.polyplug_runtime_plugin_release)
    return setmetatable({ _ptr = managed }, Guard)
end

-- ─── Loader Registration ──────────────────────────────────────────────────────
-- Declare loader FFI types once at module load time, guarded with pcall to
-- handle the case where this file is loaded multiple times in the same process.
pcall(ffi.cdef, [[
    typedef struct { const uint8_t* ptr; size_t len; } PolyplugDotnetCfg;
    void* polyplug_dotnet_loader_create(const PolyplugDotnetCfg* cfg);

    typedef struct { const uint8_t* ptr; size_t len; } PolyplugPythonCfg;
    void* polyplug_python_loader_create(const PolyplugPythonCfg* cfg);

    typedef struct { uint8_t _reserved; } PolyplugLuaCfg;
    void* polyplug_lua_loader_create(const PolyplugLuaCfg* cfg);

    typedef struct { uint8_t _reserved; } PolyplugJsCfg;
    void* polyplug_js_loader_create(const PolyplugJsCfg* cfg);

    typedef struct { uint8_t _reserved; } PolyplugNativeConfig;
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);

    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
]])

-- Lazy-loaded loader library handles, keyed by library name.
local _loader_libs = {}

local function get_loader_lib(name)
    if not _loader_libs[name] then
        _loader_libs[name] = ffi.load(name)
    end
    return _loader_libs[name]
end

-- Register a .NET loader with the runtime.
-- rt: an OpaqueRuntime* cdata (from M._lib.polyplug_runtime_create).
-- opts: optional table, may contain opts.min_framework (string, default "10.0").
function M.register_dotnet_loader(rt, opts)
    opts = opts or {}
    local min_fw = opts.min_framework or "10.0"
    local lib = get_loader_lib("polyplug_dotnet")
    local fw_bytes = ffi.new("uint8_t[?]", #min_fw, min_fw)
    local cfg = ffi.new("PolyplugDotnetCfg", fw_bytes, #min_fw)
    local loader = lib.polyplug_dotnet_loader_create(cfg)
    if loader == nil then error("polyplug: dotnet loader create failed") end
    local err = M._lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then error("polyplug: dotnet loader register failed: " .. err) end
end

-- Register a Python loader with the runtime.
-- rt: an OpaqueRuntime* cdata.
-- opts: optional table, may contain opts.min_version (string, default "3.11").
function M.register_python_loader(rt, opts)
    opts = opts or {}
    local min_ver = opts.min_version or "3.11"
    local lib = get_loader_lib("polyplug_python")
    local ver_bytes = ffi.new("uint8_t[?]", #min_ver, min_ver)
    local cfg = ffi.new("PolyplugPythonCfg", ver_bytes, #min_ver)
    local loader = lib.polyplug_python_loader_create(cfg)
    if loader == nil then error("polyplug: python loader create failed") end
    local err = M._lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then error("polyplug: python loader register failed: " .. err) end
end

-- Register a Lua loader with the runtime.
-- rt: an OpaqueRuntime* cdata.
function M.register_lua_loader(rt)
    local lib = get_loader_lib("polyplug_lua")
    local cfg = ffi.new("PolyplugLuaCfg", 0)
    local loader = lib.polyplug_lua_loader_create(cfg)
    if loader == nil then error("polyplug: lua loader create failed") end
    local err = M._lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then error("polyplug: lua loader register failed: " .. err) end
end

-- Register a JS (QuickJS) loader with the runtime.
-- rt: an OpaqueRuntime* cdata.
function M.register_js_loader(rt)
    local lib = get_loader_lib("polyplug_js")
    local cfg = ffi.new("PolyplugJsCfg", 0)
    local loader = lib.polyplug_js_loader_create(cfg)
    if loader == nil then error("polyplug: js loader create failed") end
    local err = M._lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then error("polyplug: js loader register failed: " .. err) end
end

-- Register a native loader with the runtime.
-- rt: an OpaqueRuntime* cdata.
function M.register_native_loader(rt)
    local cfg = ffi.new("PolyplugNativeConfig", 0)
    local lib = get_loader_lib("polyplug_native")
    local loader = lib.polyplug_native_loader_create(cfg)
    if loader == nil then error("polyplug: native loader create failed") end
    local err = M._lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then error("polyplug: native loader register failed: " .. err) end
end

return M

--- Convert StringView to Lua string.
-- @param sv StringView cdata
-- @return Lua string
local function to_str(sv)
    if not sv.ptr or sv.len == 0 then
        return ""
    end
    return ffi.string(sv.ptr, sv.len)
end

--- Create StringView from Lua string (borrowed).
-- Warning: StringView only valid while Lua string exists.
-- @param s Lua string
-- @return StringView cdata
local function str_as_view(s)
    return M.string_view(s)
end

M.to_str = to_str
M.to_string = to_str
M.str_as_view = str_as_view
