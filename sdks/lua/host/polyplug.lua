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

local function auto_load_native_lib()
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
    
    local env_lib = os.getenv("POLYPLUG_LIB")
    if env_lib then
        return M.load_lib(env_lib)
    end
    
    return M.load_lib(lib_name)
end

M.NULL_HANDLE = runtime.NULL_HANDLE
M.ABI_OK = abi.ABI_OK
M.ABI_ERROR_GENERIC = abi.ABI_ERROR_GENERIC
M.ABI_ERROR_NOT_FOUND = abi.ABI_ERROR_NOT_FOUND
M.ABI_ERROR_STALE_HANDLE = abi.ABI_ERROR_STALE_HANDLE
M.ABI_FUNCTION_NOT_AVAIL = abi.ABI_FUNCTION_NOT_AVAIL

M.contract_id = abi.contract_id
M.bundle_id = abi.bundle_id
M.extension_id = abi.extension_id

M.Runtime = runtime.Runtime
M.Guard = runtime.Guard
M.on_reload = runtime.on_reload
M.set_config = runtime.set_config

function M.load_lib(so_path)
    runtime.load_lib(so_path)
    M._lib = runtime._lib
    return M._lib
end

function M.last_error(lib)
    return runtime.last_error(lib)
end

M.get_platform_identifier = get_platform_identifier
M.get_script_dir = get_script_dir

auto_load_native_lib()

return M