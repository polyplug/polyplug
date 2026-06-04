-- sdks/lua/host/tests/test_reload_notification.lua
-- Unit tests for ReloadPhase and RuntimeConfig types (SDK-side only, no native runtime).
--
-- Run from repo root:
--   cd sdks/lua/host/tests && luajit test_reload_notification.lua
--
-- The justfile already does this via:
--   cd {{sdks_dir}}/lua/host/tests && luajit test_reload_notification.lua

-- ─── Path setup ──────────────────────────────────────────────────────────────
-- The working directory when this test runs is sdks/lua/host/tests/.
-- Append the host lib parent and the ABI dir so require() resolves both
-- "polyplug.reload_phase" (under ../polyplug/) and "abi"/"polyplug_abi"
-- (under ../../../../sdks/lua/abi/).
local script_dir = debug.getinfo(1, "S").source:match("^@(.+/)[^/]+$") or "./"
package.path = script_dir .. "../?.lua;"
           .. script_dir .. "../../../../sdks/lua/abi/?.lua;"
           .. package.path

-- ─── Load the module under test ──────────────────────────────────────────────
local reload_phase = require("polyplug.reload_phase")

-- ─── Test harness ────────────────────────────────────────────────────────────
local tests_passed = 0
local tests_failed = 0

local function assert_equals(expected, actual, message)
    if expected == actual then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        print("    Expected: " .. tostring(expected))
        print("    Actual:   " .. tostring(actual))
        tests_failed = tests_failed + 1
    end
end

local function assert_true(value, message)
    if value == true then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        print("    Expected: true")
        print("    Actual:   " .. tostring(value))
        tests_failed = tests_failed + 1
    end
end

local function assert_false(value, message)
    if value == false then
        print("  PASS: " .. message)
        tests_passed = tests_passed + 1
    else
        print("  FAIL: " .. message)
        print("    Expected: false")
        print("    Actual:   " .. tostring(value))
        tests_failed = tests_failed + 1
    end
end

-- ─── ReloadPhase Type Constants ───────────────────────────────────────────────
print("=== ReloadPhase Type Constants ===")

assert_equals(0, reload_phase.TYPE_PREPARING, "TYPE_PREPARING should be 0")
assert_equals(1, reload_phase.TYPE_RELOADED,  "TYPE_RELOADED should be 1")
assert_equals(2, reload_phase.TYPE_FAILED,    "TYPE_FAILED should be 2")

-- ─── ReloadPhase Constructor ──────────────────────────────────────────────────
print("\n=== ReloadPhase Constructor ===")

local phase_all = reload_phase.new(reload_phase.TYPE_PREPARING, 12345, "TestBundle", 2, "Test reason")
assert_equals(reload_phase.TYPE_PREPARING, phase_all.type,        "phase.type should be TYPE_PREPARING")
assert_equals(12345,                       phase_all.bundle_id,   "phase.bundle_id should be 12345")
assert_equals("TestBundle",               phase_all.bundle_name, "phase.bundle_name should be TestBundle")
assert_equals(2,                           phase_all.retry_count, "phase.retry_count should be 2")
assert_equals("Test reason",              phase_all.reason,      "phase.reason should be Test reason")

-- Optional fields default to empty string / 0 when nil is passed
local phase_defaults = reload_phase.new(reload_phase.TYPE_RELOADED, 999, "MyBundle")
assert_equals(reload_phase.TYPE_RELOADED, phase_defaults.type,        "defaults: type should be TYPE_RELOADED")
assert_equals(999,                         phase_defaults.bundle_id,   "defaults: bundle_id should be 999")
assert_equals("MyBundle",                 phase_defaults.bundle_name, "defaults: bundle_name should be MyBundle")
assert_equals(0,                           phase_defaults.retry_count, "defaults: retry_count should default to 0")
assert_equals("",                          phase_defaults.reason,      "defaults: reason should default to empty string")

-- nil bundle_name defaults to empty string
local phase_nil_name = reload_phase.new(reload_phase.TYPE_FAILED, 1, nil, 0, "Error")
assert_equals("", phase_nil_name.bundle_name, "nil bundle_name should default to empty string")

-- nil reason defaults to empty string
local phase_nil_reason = reload_phase.new(reload_phase.TYPE_FAILED, 1, "Bundle", 0, nil)
assert_equals("", phase_nil_reason.reason, "nil reason should default to empty string")

-- ─── ReloadPhase Helper Methods ───────────────────────────────────────────────
print("\n=== ReloadPhase Helper Methods ===")

local preparing = reload_phase.new(reload_phase.TYPE_PREPARING, 1, "Bundle")
local reloaded  = reload_phase.new(reload_phase.TYPE_RELOADED,  1, "Bundle")
local failed    = reload_phase.new(reload_phase.TYPE_FAILED,    1, "Bundle")

assert_true(reload_phase.is_preparing(preparing),  "is_preparing: true for Preparing")
assert_false(reload_phase.is_preparing(reloaded),  "is_preparing: false for Reloaded")
assert_false(reload_phase.is_preparing(failed),    "is_preparing: false for Failed")

assert_false(reload_phase.is_reloaded(preparing),  "is_reloaded: false for Preparing")
assert_true(reload_phase.is_reloaded(reloaded),    "is_reloaded: true for Reloaded")
assert_false(reload_phase.is_reloaded(failed),     "is_reloaded: false for Failed")

assert_false(reload_phase.is_failed(preparing),    "is_failed: false for Preparing")
assert_false(reload_phase.is_failed(reloaded),     "is_failed: false for Reloaded")
assert_true(reload_phase.is_failed(failed),        "is_failed: true for Failed")

-- ─── phase_type_name ─────────────────────────────────────────────────────────
print("\n=== phase_type_name ===")

assert_equals("Preparing", reload_phase.phase_type_name(reload_phase.TYPE_PREPARING), "name for TYPE_PREPARING")
assert_equals("Reloaded",  reload_phase.phase_type_name(reload_phase.TYPE_RELOADED),  "name for TYPE_RELOADED")
assert_equals("Failed",    reload_phase.phase_type_name(reload_phase.TYPE_FAILED),    "name for TYPE_FAILED")
assert_equals("Unknown(99)", reload_phase.phase_type_name(99), "name for unknown type")

-- ─── RuntimeConfig (pure-Lua table, no native runtime) ───────────────────────
print("\n=== RuntimeConfig Defaults ===")

-- The Lua SDK represents RuntimeConfig as a plain table passed to set_config().
-- Canonical 3-field ABI struct: compatibility, hot_reload_enabled, on_reload.
-- Verify the runtime module exposes the expected defaults / field names.
local runtime = require("polyplug.runtime")

-- set_config stores the table in M._pending_config; verify round-trip
runtime._pending_config = nil  -- ensure clean state

local config = { compatibility = 0, hot_reload_enabled = false, on_reload = nil }
runtime.set_config(config)
assert_equals(0,     runtime._pending_config.compatibility,    "config.compatibility default is 0")
assert_false(runtime._pending_config.hot_reload_enabled,       "config.hot_reload_enabled default is false")
assert_equals(nil,   runtime._pending_config.on_reload,        "config.on_reload default is nil")

-- hot_reload_enabled can be set to true
local config_enabled = { compatibility = 0, hot_reload_enabled = true, on_reload = nil }
runtime.set_config(config_enabled)
assert_true(runtime._pending_config.hot_reload_enabled, "config.hot_reload_enabled can be set to true")

-- on_reload callback can be set
local cb_called = false
local config_with_cb = { compatibility = 0, hot_reload_enabled = true, on_reload = function() cb_called = true end }
runtime.set_config(config_with_cb)
assert_equals("function", type(runtime._pending_config.on_reload), "config.on_reload should be a function when set")

-- Clean up module state so tests are hermetic
runtime._pending_config = nil

-- ─── Results ─────────────────────────────────────────────────────────────────
print("\n=== Results ===")
print(string.format("Tests passed: %d", tests_passed))
print(string.format("Tests failed: %d", tests_failed))

if tests_failed > 0 then
    os.exit(1)
end
