-- sdks/lua/loaders/js/polyplug/loaders/js.lua
-- JavaScript (QuickJS) loader registration for polyplug.

local ffi = require("ffi")

pcall(ffi.cdef, [[
    typedef struct { uint8_t _reserved; } PolyplugJsConfig;
    void* polyplug_js_loader_create(const PolyplugJsConfig* cfg);
    void  polyplug_js_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        -- POLYPLUG_JS_LIB (set by the test/CI harness) wins over the bare
        -- library name so the loader cdylib matches the freshly built core.
        _lib = ffi.load(os.getenv("POLYPLUG_JS_LIB") or "polyplug_js")
    end
    return _lib
end

local M = {}

--- Register the JavaScript (QuickJS) loader with a Runtime.
-- @param rt Runtime  The polyplug Runtime instance (exposes :register_loader).
function M.register(rt)
    local lib = get_lib()
    local cfg = ffi.new("PolyplugJsConfig", {0})
    local loader = lib.polyplug_js_loader_create(cfg)
    if loader == nil then
        error("polyplug: js loader create failed")
    end
    rt:register_loader(loader)
end

return M
