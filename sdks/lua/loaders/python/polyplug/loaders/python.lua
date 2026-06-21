-- sdks/lua/loaders/python/polyplug/loaders/python.lua
-- Python loader registration for polyplug.

local ffi    = require("ffi")
local native = require("polyplug.native")

pcall(ffi.cdef, [[
    typedef struct {
        const uint8_t* min_version_ptr;
        size_t         min_version_len;
    } PolyplugPythonConfig;
    void* polyplug_python_loader_create(const PolyplugPythonConfig* cfg);
    void  polyplug_python_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        _lib = native.load("POLYPLUG_PYTHON_LIB", "polyplug_python")
    end
    return _lib
end

local M = {}

--- Register the Python loader with a Runtime.
-- @param rt Runtime         The polyplug Runtime instance (exposes :register_loader).
-- @param min_version string Minimum CPython version (e.g. "3.11"). Defaults to "3.11".
function M.register(rt, min_version)
    min_version = min_version or "3.11"
    local lib = get_lib()
    local version_bytes = ffi.new("uint8_t[?]", #min_version, min_version)
    local cfg = ffi.new("PolyplugPythonConfig")
    cfg.min_version_ptr = version_bytes
    cfg.min_version_len = #min_version
    local loader = lib.polyplug_python_loader_create(cfg)
    if loader == nil then
        error("polyplug: python loader create failed")
    end
    rt:register_loader(loader)
end

return M
