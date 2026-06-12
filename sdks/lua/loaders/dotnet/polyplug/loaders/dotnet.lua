-- sdks/lua/loaders/dotnet/polyplug/loaders/dotnet.lua
-- .NET loader registration for polyplug.

local ffi = require("ffi")

pcall(ffi.cdef, [[
    typedef struct {
        const uint8_t* min_framework_ptr;
        size_t         min_framework_len;
    } PolyplugDotnetConfig;
    void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* cfg);
    void  polyplug_dotnet_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        -- POLYPLUG_DOTNET_LIB (set by the test/CI harness) wins over the bare
        -- library name so the loader cdylib matches the freshly built core.
        _lib = ffi.load(os.getenv("POLYPLUG_DOTNET_LIB") or "polyplug_dotnet")
    end
    return _lib
end

local M = {}

--- Register the .NET loader with a Runtime.
-- @param rt Runtime           The polyplug Runtime instance (exposes :register_loader).
-- @param min_framework string Minimum .NET framework version (e.g. "10.0"). Defaults to "10.0".
function M.register(rt, min_framework)
    min_framework = min_framework or "10.0"
    local lib = get_lib()
    local framework_bytes = ffi.new("uint8_t[?]", #min_framework, min_framework)
    local cfg = ffi.new("PolyplugDotnetConfig")
    cfg.min_framework_ptr = framework_bytes
    cfg.min_framework_len = #min_framework
    local loader = lib.polyplug_dotnet_loader_create(cfg)
    if loader == nil then
        error("polyplug: dotnet loader create failed")
    end
    rt:register_loader(loader)
end

return M
