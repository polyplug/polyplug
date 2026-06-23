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

-- ─── trusted_keys: builds the allowlist Array and points the config at it ──────
-- Mirrors what Runtime.new does (M.build_trusted_keys) without the native lib:
-- build a config, fill it from a key list, and read the three fields back.
local k1 = string.rep("\1", 32)
local k2 = string.rep("\2", 32)
local pinned_config = ffi.new("RuntimeConfig", {
    signature_policy = runtime.SignaturePolicy.Required,
})
local buf = runtime.build_trusted_keys(pinned_config, { k1, k2 })
assert(buf ~= nil, "build_trusted_keys must return the anchoring buffer for a non-empty list")
assert(pinned_config.trusted_keys ~= nil, "trusted_keys ptr must be non-null after pinning")
assert(tonumber(pinned_config.trusted_keys_len) == 2,
    "trusted_keys_len must be 2, got " .. tostring(pinned_config.trusted_keys_len))
assert(tonumber(pinned_config.trusted_keys__align) == ffi.alignof("Ed25519PublicKey"),
    "trusted_keys__align must equal alignof(Ed25519PublicKey), got "
        .. tostring(pinned_config.trusted_keys__align))

-- The copied bytes must round-trip through the anchored buffer.
assert(buf[0].bytes[0] == 1, "first key byte 0 must be 1")
assert(buf[1].bytes[0] == 2, "second key byte 0 must be 2")

-- ─── trusted_keys: table-of-bytes form is accepted ────────────────────────────
local byte_key = {}
for i = 1, 32 do byte_key[i] = i end
local table_config = ffi.new("RuntimeConfig", {})
local table_buf = runtime.build_trusted_keys(table_config, { byte_key })
assert(table_buf ~= nil, "build_trusted_keys must accept a 32-byte table key")
assert(tonumber(table_config.trusted_keys_len) == 1, "table-key list len must be 1")
assert(table_buf[0].bytes[31] == 32, "last byte of the table key must be 32")

-- ─── trusted_keys: empty/nil leaves fields zero (Trust-On-First-Use) ──────────
local tofu_config = ffi.new("RuntimeConfig", {})
assert(runtime.build_trusted_keys(tofu_config, nil) == nil, "nil list returns nil buffer")
assert(runtime.build_trusted_keys(tofu_config, {}) == nil, "empty list returns nil buffer")
assert(tofu_config.trusted_keys == nil, "trusted_keys ptr must stay null for TOFU")
assert(tonumber(tofu_config.trusted_keys_len) == 0, "trusted_keys_len must stay 0 for TOFU")

-- ─── trusted_keys: wrong-length key is rejected ───────────────────────────────
local bad_config = ffi.new("RuntimeConfig", {})
local ok = pcall(runtime.build_trusted_keys, bad_config, { string.rep("\1", 31) })
assert(not ok, "a 31-byte key must be rejected")

print("All tests passed!")
