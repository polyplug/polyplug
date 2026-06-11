-- sdks/lua/host/polyplug.lua
-- LuaJIT FFI host library for polyplug.

local ffi = require('ffi')
local jit = require('jit')
local abi = require('polyplug_abi')
local runtime = require('polyplug.runtime')

local M = {}

local function get_platform_identifier()
    local os = jit.os
    local arch = jit.arch
    local os_map = {
        ["Linux"] = "linux",
        ["OSX"] = "darwin",
        ["Windows"] = "win32"
    }
    local arch_map = {
        ["x64"] = "x64",
        ["arm64"] = "arm64"
    }
    return (os_map[os] or "unknown") .. "-" .. (arch_map[arch] or "unknown")
end

local function get_script_dir()
    local source = debug.getinfo(1, "S").source
    if source:sub(1, 1) == "@" then
        return source:match("^@(.+)/") or "."
    end
    return "."
end

-- Resolution order (an explicit POLYPLUG_LIB always wins):
--   1. The POLYPLUG_LIB environment variable (path to the .so/.dylib/.dll).
--   2. Locally staged platform subdirectory (_native/<platform>/).
--   3. System library paths (plain library name via the OS loader).
local function auto_load_native_lib()
    local env_lib = os.getenv("POLYPLUG_LIB")
    if env_lib then
        return M.load_lib(env_lib)
    end

    local platform = get_platform_identifier()
    local script_dir = get_script_dir()
    local native_dir = script_dir .. "/_native/" .. platform

    local jit_os = jit.os
    local lib_name
    if jit_os == "Windows" then
        lib_name = "polyplug.dll"
    else
        lib_name = "libpolyplug.so"
    end

    local lib_path = native_dir .. "/" .. lib_name

    local f = io.open(lib_path, "r")
    if f then
        f:close()
        return M.load_lib(lib_path)
    end

    return M.load_lib(lib_name)
end

M.NULL_HANDLE_INDEX = runtime.NULL_HANDLE_INDEX
M.AbiErrorCode = abi.AbiErrorCode
M.LogLevel = runtime.LogLevel

M.bundle_id = abi.bundle_id
M.guest_contract_id = abi.guest_contract_id
M.host_contract_id = runtime.host_contract_id

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

M.get_platform_identifier = get_platform_identifier
M.get_script_dir = get_script_dir

auto_load_native_lib()

return M