--- Python loader registration for polyplug.
local ffi = require("ffi")

local ok = pcall(ffi.cdef, [[
    typedef struct { const uint8_t* ptr; size_t len; } PolyplugPythonConfig;
    void* polyplug_python_loader_create(const PolyplugPythonConfig* cfg);
    void  polyplug_python_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        _lib = ffi.load("polyplug_python")
    end
    return _lib
end

local M = {}

function M.register(rt, min_version)
    min_version = min_version or "3.11"
    local lib = get_lib()
    local cfg = ffi.new("PolyplugPythonConfig", {
        ffi.cast("const uint8_t*", min_version),
        #min_version
    })
    local loader = lib.polyplug_python_loader_create(cfg)
    if loader == nil then
        error("polyplug: python loader create failed")
    end
    local polyplug_lib = ffi.load("polyplug")
    local err = polyplug_lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then
        error("polyplug: python loader register failed: " .. tostring(err))
    end
end

return M