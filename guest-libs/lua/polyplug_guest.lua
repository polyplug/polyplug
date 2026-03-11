-- polyplug_guest.lua
-- LuaJIT FFI guest library for polyplug plugins.
-- This module is loaded via require("polyplug_guest").
-- All ffi.cdef calls are at module load time and guarded with pcall
-- to prevent "already defined" errors when a second plugin calls require().

local ffi = require("ffi")

-- ABI struct declarations.
-- Guards prevent "already defined" errors on second require().
local function cdef_guarded(decl)
    local ok, err = pcall(ffi.cdef, decl)
    if not ok and not string.find(err, "already defined", 1, true) then
        error(err, 2)
    end
end

cdef_guarded([[
    typedef struct { const uint8_t* ptr; size_t len; } StringView;
    typedef struct { uint8_t* ptr; size_t len; size_t cap; } Buffer;
    typedef struct { uint32_t code; uint32_t _pad; StringView message; } AbiError;
    typedef struct { uint32_t index; uint32_t generation; } PluginHandle;
    typedef struct {
        uint64_t contract_id;
        uint32_t contract_version;
        uint32_t function_count;
        void* const* functions;
    } PluginVTable;
    typedef struct {
        StringView name;
        StringView contract_name;
        uint32_t version_major;
        uint32_t version_minor;
        uint32_t version_patch;
        uint32_t _tail_pad;
    } PluginDescriptor;
    typedef AbiError (*register_plugin_fn_t)(void*, const PluginDescriptor*, const PluginVTable*);
    typedef struct {
        register_plugin_fn_t register_plugin;
        const void* host;
    } PluginRegistrar;
    typedef struct { StringView bundle_path; } PluginContext;
]])

local M = {}

--- Reconstruct a PluginRegistrar pointer from the integer passed by the host.
--- The host passes the pointer as an i64 to avoid Lua double precision loss.
--- @param ptr_int number  The registrar pointer as a LuaJIT integer (int64_t).
--- @return cdata          A typed PluginRegistrar pointer.
function M.cast_registrar(ptr_int)
    -- PRECISION: ptr_int is a LuaJIT int64_t, not a double.
    -- ffi.cast via uintptr_t preserves all 64 bits.
    return ffi.cast("PluginRegistrar*", ffi.cast("uintptr_t", ptr_int))
end

--- Create a StringView from a Lua string.
--- The string data is owned by Lua and must remain alive for the duration of the call.
--- @param s string  A Lua string.
--- @return cdata    A StringView cdata pointing into the Lua string.
function M.string_view(s)
    return ffi.new("StringView", { ptr = ffi.cast("const uint8_t*", s), len = #s })
end

--- Create a zero AbiError (success).
--- @return cdata  An AbiError with code=0.
function M.ok()
    return ffi.new("AbiError", { code = 0 })
end

--- Create an AbiError with a given code and message.
--- @param code    number  Error code (non-zero).
--- @param message string  Error message (Lua string).
--- @return cdata          An AbiError cdata.
function M.err(code, message)
    return ffi.new("AbiError", { code = code, message = M.string_view(message) })
end


--- Extract the bundle path from a PluginContext as a Lua string.
--- @param ctx cdata  A PluginContext pointer.
--- @return string    The bundle path as a UTF-8 Lua string.
function M.bundle_path_str(ctx)
    local sv = ctx.bundle_path
    return ffi.string(sv.ptr, sv.len)
end

--- Reconstruct a PluginContext pointer from the integer passed by the host.
--- @param ptr number  The context pointer as a LuaJIT integer (int64_t).
--- @return cdata      A typed PluginContext pointer.
function M.cast_context(ptr)
    return ffi.cast("PluginContext*", ptr)
end

return M
