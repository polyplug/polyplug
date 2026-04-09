-- sdks/lua/guest/polyplug_guest.lua
-- LuaJIT FFI guest library for polyplug plugins.
-- This module is loaded via require("polyplug_guest").
-- All ffi.cdef calls are at module load time and guarded with pcall
-- to prevent "already defined" errors when a second plugin calls require().

local ffi = require("ffi")
local abi = require("polyplug_abi")

local M = {}

M.AbiErrorCode = abi.AbiErrorCode

M.contract_id = abi.contract_id
M.bundle_id = abi.bundle_id
M.extension_id = abi.extension_id

local _host_interface_ptr = nil

function M.store_host_interface(ptr)
    _host_interface_ptr = ptr
end

function M.get_host_interface()
    return _host_interface_ptr
end

function M.cast_host_interface(ptr_int)
    return ffi.cast("HostInterface*", ffi.cast("uintptr_t", ptr_int))
end

function M.cast_context(ptr)
    return ffi.cast("PluginContext*", ptr)
end

function M.string_view(s)
    return ffi.new("StringView", { ptr = ffi.cast("const uint8_t*", s), len = #s })
end

function M.ok()
    return ffi.new("AbiError", { code = 0 })
end

function M.err(code, message)
    return ffi.new("AbiError", { code = code, message = M.string_view(message) })
end

function M.bundle_path_str(ctx)
    local sv = ctx.bundle_path
    return ffi.string(sv.ptr, sv.len)
end

return M