--- Lua loader registration for polyplug.
local ffi = require("ffi")

pcall(ffi.cdef, [[
    typedef struct { uint8_t _reserved; } PolyplugLuaConfig;
    void* polyplug_lua_loader_create(const PolyplugLuaConfig* cfg);
    void  polyplug_lua_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        -- POLYPLUG_LUA_LIB (set by the test/CI harness) wins over the bare
        -- library name so the loader cdylib matches the freshly built core.
        _lib = ffi.load(os.getenv("POLYPLUG_LUA_LIB") or "polyplug_lua")
    end
    return _lib
end

local M = {}

--- Register the Lua loader with a Runtime.
-- @param rt Runtime  The polyplug Runtime instance (exposes :register_loader).
function M.register(rt)
    local lib = get_lib()
    local cfg = ffi.new("PolyplugLuaConfig", {0})
    local loader = lib.polyplug_lua_loader_create(cfg)
    if loader == nil then
        error("polyplug: lua loader create failed")
    end
    rt:register_loader("lua", loader)
end

--- Handle to the lua loader cdylib for the host-contract bridge.
-- The generated host interface factories (host/interface_factories.lua) need
-- the `polyplug_lua_host_*` trampolines exported by this cdylib because LuaJIT
-- callbacks cannot return structs by value. The trampoline cdefs live in the
-- generated factories file; this accessor only hands out the clib.
-- @return clib  ffi.load handle for libpolyplug_lua.
function M.bridge_lib()
    return get_lib()
end

return M
