--- Lua loader registration for polyplug.
local ffi    = require("ffi")
local native = require("polyplug.native")

pcall(ffi.cdef, [[
    void* polyplug_lua_loader_create(void);
    void  polyplug_lua_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        _lib = native.load("POLYPLUG_LUA_LIB", "polyplug_lua")
    end
    return _lib
end

local M = {}
local internal_bridge = nil

function M.internal_plugin_bridge()
    if internal_bridge == nil then
        local open = assert(
            package.loadlib(
                native.resolve("POLYPLUG_LUA_LIB", "polyplug_lua"),
                "luaopen_polyplug_lua_bridge"
            )
        )
        internal_bridge = open()
    end
    return internal_bridge
end


--- Register the Lua loader with a Runtime.
-- @param rt Runtime  The polyplug Runtime instance (exposes :register_loader).
function M.register(rt)
    local lib = get_lib()
    local loader = lib.polyplug_lua_loader_create()
    if loader == nil then
        error("polyplug: lua loader create failed")
    end
    rt:register_loader(loader)
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
