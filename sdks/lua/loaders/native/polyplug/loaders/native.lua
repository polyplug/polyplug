-- sdks/lua/loaders/native/polyplug/loaders/native.lua
-- Native loader registration for polyplug.

local ffi = require("ffi")

pcall(ffi.cdef, [[
    typedef struct { uint8_t _reserved; } PolyplugNativeConfig;
    void* polyplug_native_loader_create(const PolyplugNativeConfig* cfg);
    void  polyplug_native_loader_free(void* ptr);
]])

local _lib = nil
local function get_lib()
    if not _lib then
        -- POLYPLUG_NATIVE_LIB (set by the test/CI harness) wins over the bare
        -- library name so the loader cdylib matches the freshly built core.
        _lib = ffi.load(os.getenv("POLYPLUG_NATIVE_LIB") or "polyplug_native")
    end
    return _lib
end

local M = {}

--- Register the native loader with a Runtime.
-- @param rt Runtime  The polyplug Runtime instance (exposes :register_loader).
function M.register(rt)
    local lib = get_lib()
    local cfg = ffi.new("PolyplugNativeConfig", {0})
    local loader = lib.polyplug_native_loader_create(cfg)
    if loader == nil then
        error("polyplug: native loader create failed")
    end
    rt:register_loader(loader)
end

return M
