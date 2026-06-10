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

local phase_all = reload_phase.new(reload_phase.TYPE_PREPARING, 12345, "TestBundle", "Test reason")
assert_equals(reload_phase.TYPE_PREPARING, phase_all.type,        "phase.type should be TYPE_PREPARING")
assert_equals(12345,                       phase_all.bundle_id,   "phase.bundle_id should be 12345")
assert_equals("TestBundle",               phase_all.bundle_name, "phase.bundle_name should be TestBundle")
assert_equals("Test reason",              phase_all.reason,      "phase.reason should be Test reason")

-- Optional fields default to empty string when nil is passed
local phase_defaults = reload_phase.new(reload_phase.TYPE_RELOADED, 999, "MyBundle")
assert_equals(reload_phase.TYPE_RELOADED, phase_defaults.type,        "defaults: type should be TYPE_RELOADED")
assert_equals(999,                         phase_defaults.bundle_id,   "defaults: bundle_id should be 999")
assert_equals("MyBundle",                 phase_defaults.bundle_name, "defaults: bundle_name should be MyBundle")
assert_equals("",                          phase_defaults.reason,      "defaults: reason should default to empty string")

-- nil bundle_name defaults to empty string
local phase_nil_name = reload_phase.new(reload_phase.TYPE_FAILED, 1, nil, "Error")
assert_equals("", phase_nil_name.bundle_name, "nil bundle_name should default to empty string")

-- nil reason defaults to empty string
local phase_nil_reason = reload_phase.new(reload_phase.TYPE_FAILED, 1, "Bundle", nil)
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

-- ─── Runtime module: per-instance config (Rule 12) ───────────────────────────
print("\n=== Runtime per-instance config ===")

-- Configuration is passed per-instance to Runtime.new(opts) — the module must
-- hold NO pending-config / pending-callback statics shared across runtimes.
local runtime = require("polyplug.runtime")

assert_equals(nil, runtime._pending_config,          "module must not hold a _pending_config static")
assert_equals(nil, runtime._pending_reload_callback, "module must not hold a _pending_reload_callback static")
assert_equals(nil, runtime.set_config,               "module-level set_config is gone (per-instance opts)")
assert_equals(nil, runtime.on_reload,                "module-level on_reload is gone (per-instance opts)")

-- ─── find_all_guest_contracts ABI signature (defect: sret corruption) ────────
print("\n=== find_all_guest_contracts ABI layout ===")

local ffi = require("ffi")

-- The ABI returns Array BY VALUE: { items ptr, len size_t, align size_t } = 24
-- bytes, align 8. The host binding must declare exactly this layout — a
-- narrower struct return makes the SysV sret write past LuaJIT's return slot.
assert_equals(24, ffi.sizeof("Array"),  "ABI Array struct is 24 bytes")
assert_equals(8,  ffi.alignof("Array"), "ABI Array struct is 8-byte aligned")
assert_equals("Array(*)(const HostApi*, uint64_t, uint32_t)",
    runtime.FIND_ALL_FN_SIGNATURE,
    "find_all cast signature returns the by-value Array struct")
-- The cast signature must be constructible against the generated cdef.
local fn_ctype_ok = pcall(ffi.typeof, runtime.FIND_ALL_FN_SIGNATURE)
assert_true(fn_ctype_ok, "FIND_ALL_FN_SIGNATURE is a valid ctype against the generated ABI")

-- ─── Runtime exercise (real native lib, when available) ──────────────────────
print("\n=== find_all_guest_contracts runtime exercise ===")

-- Exercise the real 24-byte sret path when a native libpolyplug is reachable
-- (POLYPLUG_LIB or the staged _native/ copy). The polyplug entry module
-- auto-loads the library; if none is available this section is skipped (the
-- layout assertions above remain the minimum bar).
package.path = script_dir .. "../../?.lua;" .. package.path
local polyplug_ok, polyplug = pcall(require, "polyplug")
if polyplug_ok then
    local rt = polyplug.Runtime.new({
        config = { compatibility = 0, hot_reload_enabled = true },
    })
    assert_true(rt ~= nil, "Runtime.new with per-instance config succeeds")

    -- LuaJIT FFI cannot create callbacks with struct-by-value parameters, so
    -- requesting on_reload must fail LOUDLY with the documented message (not a
    -- cryptic cast error, and never silently).
    local cb_ok, cb_err = pcall(polyplug.Runtime.new, {
        on_reload = function(_) end,
    })
    assert_false(cb_ok, "on_reload request fails (LuaJIT struct-by-value callback limit)")
    assert_true(tostring(cb_err):find("on_reload is not supported", 1, true) ~= nil,
        "on_reload failure carries the documented diagnostic")

    -- A second runtime must not inherit or clobber the first's options.
    local rt2 = polyplug.Runtime.new()
    assert_equals(nil, rt2._reload_cb_cdata, "second runtime owns no callback (no cross-instance leak)")

    -- No bundles loaded: find_all must return an empty table — and the 24-byte
    -- sret must not corrupt adjacent memory (verified by the host surviving
    -- further calls).
    local handles = rt:find_all_guest_contracts(0xDEADBEEFULL, 0)
    assert_equals(0, #handles, "find_all on empty runtime returns no handles")
    local handle = rt:find_guest_contract(0xDEADBEEFULL, 0)
    assert_equals(polyplug.NULL_HANDLE_INDEX, handle.index, "host still functional after find_all sret")

    rt2:destroy()
    rt:destroy()
else
    print("  SKIP: native libpolyplug unavailable (" .. tostring(polyplug) .. ")")
end

-- ─── Results ─────────────────────────────────────────────────────────────────
print("\n=== Results ===")
print(string.format("Tests passed: %d", tests_passed))
print(string.format("Tests failed: %d", tests_failed))

if tests_failed > 0 then
    os.exit(1)
end
