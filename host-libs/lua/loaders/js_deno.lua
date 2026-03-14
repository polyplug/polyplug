--- JS Deno (V8) loader registration for polyplug.
local ffi = require("ffi")

local ok = pcall(ffi.cdef, [[
    typedef struct { uint8_t _reserved; } PolyplugJsDenoConfig;
    void* polyplug_js_deno_loader_create(const PolyplugJsDenoConfig* cfg);
    void  polyplug_js_deno_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        _lib = ffi.load("polyplug_js_deno")
    end
    return _lib
end

local M = {}

function M.register(rt)
    local lib = get_lib()
    local cfg = ffi.new("PolyplugJsDenoConfig", {0})
    local loader = lib.polyplug_js_deno_loader_create(cfg)
    if loader == nil then
        error("polyplug: js_deno loader create failed")
    end
    local polyplug_lib = ffi.load("polyplug")
    local err = polyplug_lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then
        error("polyplug: js_deno loader register failed: " .. tostring(err))
    end
end

return M
