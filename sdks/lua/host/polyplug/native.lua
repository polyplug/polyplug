-- sdks/lua/host/polyplug/native.lua
-- Shared native-library resolution helper for the polyplug Lua SDK.
-- Resolves a named library path in three-tier order:
--   1. Explicit env-var override (wins unconditionally).
--   2. Co-located staged directory: <host-root>/_native/<platform>/<filename>.
--   3. Bare system name (passed to the OS loader as-is).
-- All loaders and the host module route through this single source of truth.

local ffi = require("ffi")
local jit = require("jit")

local M = {}

-- Maps jit.os → the platform directory segment used under _native/.
local OS_PLATFORM = {
    ["Linux"]   = "linux",
    ["OSX"]     = "macos",
    ["Windows"] = "windows",
}

-- Maps jit.arch → the architecture segment used in the platform directory.
local ARCH_SUFFIX = {
    ["x64"]   = "x64",
    ["arm64"] = "arm64",
}

-- Returns the per-OS library filename for a given (os_name, base) pair.
-- os_name matches jit.os values: "Linux", "OSX", "Windows".
-- base: e.g. "polyplug", "polyplug_native", "polyplug_lua".
-- Naming conventions (matching the release workflow):
--   Linux:   lib<base>.so
--   macOS:   lib<base>.dylib
--   Windows: <base>.dll  (no "lib" prefix)
function M.lib_filename_for_os(os_name, base)
    if os_name == "Linux" then
        return "lib" .. base .. ".so"
    elseif os_name == "OSX" then
        return "lib" .. base .. ".dylib"
    else
        -- Windows and any unrecognised OS: no lib prefix, .dll extension.
        return base .. ".dll"
    end
end

-- Returns the per-OS library filename for a given base library name,
-- using the current platform.
function M.lib_filename(base)
    return M.lib_filename_for_os(jit.os, base)
end

-- Returns the platform directory name for a given (os_name, arch_name) pair,
-- matching the staging layout: linux-x64, macos-x64, macos-arm64, windows-x64.
function M.platform_for(os_name, arch_name)
    local os_part   = OS_PLATFORM[os_name]   or "unknown"
    local arch_part = ARCH_SUFFIX[arch_name] or "unknown"
    return os_part .. "-" .. arch_part
end

-- Returns the platform directory name for the current platform.
function M.platform()
    return M.platform_for(jit.os, jit.arch)
end

-- The host root directory (_native/ is staged here).
-- This file lives at <host-root>/polyplug/native.lua, so the host root
-- is one directory up from this file's own directory.
-- Exposed for tests and diagnostics.
function M.host_root()
    local src = debug.getinfo(1, "S").source
    if src:sub(1, 1) ~= "@" then
        return "."
    end
    -- src is "@/absolute/path/to/polyplug/native.lua"
    -- Strip the filename to get the containing directory.
    local dir = src:match("^@(.+)/[^/]+$") or "."
    -- dir is "<host-root>/polyplug" — go up one level.
    local root = dir:match("^(.+)/[^/]+$") or dir
    return root
end

-- Resolves the path or name for a native library without loading it.
-- Returns the path or bare name that should be passed to ffi.load.
-- @param env_var  string  Name of the env var that may carry an explicit path.
-- @param base     string  Library base name (e.g. "polyplug", "polyplug_native").
-- @return string          Path or bare system name.
function M.resolve(env_var, base)
    local explicit = os.getenv(env_var)
    if explicit then
        return explicit
    end

    local staged = M.host_root() .. "/_native/" .. M.platform() .. "/" .. M.lib_filename(base)
    local f = io.open(staged, "r")
    if f then
        f:close()
        return staged
    end

    return base
end

-- Resolves and ffi.loads a native library.
-- @param env_var  string  Name of the env var that may carry an explicit path.
-- @param base     string  Library base name (e.g. "polyplug", "polyplug_native").
-- @return clib            The loaded ffi library handle.
function M.load(env_var, base)
    return ffi.load(M.resolve(env_var, base))
end

return M
