-- sdks/lua/host/polyplug.lua
-- LuaJIT FFI host library for polyplug.

local abi     = require('polyplug_abi')
local runtime = require('polyplug.runtime')
local native  = require('polyplug.native')

local M = {}

M.NULL_HANDLE_INDEX = runtime.NULL_HANDLE_INDEX
M.AbiErrorCode      = abi.AbiErrorCode
M.LogLevel          = runtime.LogLevel

M.bundle_id        = abi.bundle_id
M.guest_contract_id = abi.guest_contract_id
M.host_contract_id  = runtime.host_contract_id

M.Runtime = runtime.Runtime

function M.load_lib(so_path)
    runtime.load_lib(so_path)
    M._lib = runtime._lib
    return M._lib
end

--- Get last error message from a HostApi pointer.
-- Forwards BOTH arguments: runtime.last_error(host, lib) requires the host
-- pointer first; lib is optional (defaults to the loaded library).
-- @param host HostApi*  The host interface pointer.
-- @param lib            Optional library handle.
-- @return string        The error message, or empty string.
function M.last_error(host, lib)
    return runtime.last_error(host, lib)
end

M.load_lib(native.resolve("POLYPLUG_LIB", "polyplug"))

return M