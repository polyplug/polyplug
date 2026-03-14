--- .NET loader registration for polyplug.
local ffi = require("ffi")

local ok = pcall(ffi.cdef, [[
    typedef struct { const uint8_t* ptr; size_t len; } PolyplugDotnetConfig;
    void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* cfg);
    void  polyplug_dotnet_loader_free(void* ptr);
    uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        _lib = ffi.load("polyplug_dotnet")
    end
    return _lib
end

local M = {}

function M.register(rt, min_framework)
    min_framework = min_framework or "10.0"
    local lib = get_lib()
    local cfg = ffi.new("PolyplugDotnetConfig", {
        ffi.cast("const uint8_t*", min_framework),
        #min_framework
    })
    local loader = lib.polyplug_dotnet_loader_create(cfg)
    if loader == nil then
        error("polyplug: dotnet loader create failed")
    end
    local polyplug_lib = ffi.load("polyplug")
    local err = polyplug_lib.polyplug_runtime_register_loader(rt, loader)
    if err ~= 0 then
        error("polyplug: dotnet loader register failed: " .. tostring(err))
    end
end

return M
