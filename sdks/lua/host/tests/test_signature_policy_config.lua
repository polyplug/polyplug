-- sdks/lua/host/tests/test_signature_policy_config.lua
-- Asserts the Lua host SDK writes RuntimeConfig.signature_policy via the
-- opts.signature_policy mechanism, without loading the native library.
--
-- The ABI mirror (abi.lua) supplies the RuntimeConfig cdef and the
-- SignaturePolicy enum; this test builds the config exactly as Runtime.new does
-- (ffi.new("RuntimeConfig", { signature_policy = ... })) and reads the field
-- back. Full runtime-load coverage lives in test_reload_runtime.lua.
--
-- Run from repo root:
--   luajit sdks/lua/host/tests/test_signature_policy_config.lua

local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../abi/?.lua;"
           .. package.path

local ffi = require("ffi")
local abi = require("polyplug_abi")
local runtime = require("polyplug.runtime")

assert(abi ~= nil, "polyplug_abi must load (provides RuntimeConfig + SignaturePolicy cdefs)")

-- ─── M.SignaturePolicy table mirrors the ABI enum ─────────────────────────────
assert(runtime.SignaturePolicy.Off == 0, "SignaturePolicy.Off must be 0")
assert(runtime.SignaturePolicy.WarnOnly == 1, "SignaturePolicy.WarnOnly must be 1")
assert(runtime.SignaturePolicy.Required == 2, "SignaturePolicy.Required must be 2")

-- ─── RuntimeConfig layout: 72 bytes (signature_policy + trusted_keys) ─────────
assert(ffi.sizeof("RuntimeConfig") == 72,
    "RuntimeConfig must be 72 bytes, got " .. tostring(ffi.sizeof("RuntimeConfig")))

-- ─── Default (omitted) writes Off (0) ─────────────────────────────────────────
local default_config = ffi.new("RuntimeConfig", {
    compatibility = runtime.COMPATIBILITY_STRICT,
    hot_reload_enabled = 0,
    signature_policy = runtime.SignaturePolicy.Off,
})
assert(default_config.signature_policy == 0,
    "default signature_policy must be Off (0), got " .. tostring(default_config.signature_policy))

-- ─── Required writes 2 (the value Runtime.new builds from opts) ────────────────
local required_config = ffi.new("RuntimeConfig", {
    compatibility = runtime.COMPATIBILITY_STRICT,
    hot_reload_enabled = 0,
    signature_policy = runtime.SignaturePolicy.Required,
})
assert(required_config.signature_policy == 2,
    "Required signature_policy must be 2, got " .. tostring(required_config.signature_policy))

-- ─── WarnOnly writes 1 ────────────────────────────────────────────────────────
local warn_config = ffi.new("RuntimeConfig", {
    signature_policy = runtime.SignaturePolicy.WarnOnly,
})
assert(warn_config.signature_policy == 1,
    "WarnOnly signature_policy must be 1, got " .. tostring(warn_config.signature_policy))

print("All tests passed!")
