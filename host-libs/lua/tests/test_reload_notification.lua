-- host-libs/lua/tests/test_reload_notification.lua
-- Unit tests for ReloadPhase and RuntimeConfig types.

-- Set up package path to find polyplug modules
package.path = "../?.lua;" .. package.path

local reload_phase = require('polyplug.reload_phase')
local runtime_config = require('polyplug.runtime_config')

local tests_passed = 0
local tests_failed = 0

local function assert_equals(expected, actual, message)
    if expected == actual then
        tests_passed = tests_passed + 1
    else
        tests_failed = tests_failed + 1
        print("FAIL: " .. (message or "assertion failed"))
        print("  Expected: " .. tostring(expected))
        print("  Actual: " .. tostring(actual))
    end
end

local function assert_true(value, message)
    if value == true then
        tests_passed = tests_passed + 1
    else
        tests_failed = tests_failed + 1
        print("FAIL: " .. (message or "expected true"))
        print("  Actual: " .. tostring(value))
    end
end

local function assert_false(value, message)
    if value == false then
        tests_passed = tests_passed + 1
    else
        tests_failed = tests_failed + 1
        print("FAIL: " .. (message or "expected false"))
        print("  Actual: " .. tostring(value))
    end
end

-- ============================================================================
-- ReloadPhase Type Constants Tests
-- ============================================================================

print("=== ReloadPhase Type Constants ===")

assert_equals(0, reload_phase.TYPE_PREPARING, "TYPE_PREPARING should be 0")
assert_equals(1, reload_phase.TYPE_RELOADED, "TYPE_RELOADED should be 1")
assert_equals(2, reload_phase.TYPE_FAILED, "TYPE_FAILED should be 2")

-- ============================================================================
-- ReloadPhase.new() Constructor Tests
-- ============================================================================

print("\n=== ReloadPhase.new() Constructor ===")

-- Test constructor sets all properties
local phase = reload_phase.new(
    reload_phase.TYPE_PREPARING,
    12345,
    "TestBundle",
    2,
    "Test reason"
)
assert_equals(reload_phase.TYPE_PREPARING, phase.type, "phase.type should be TYPE_PREPARING")
assert_equals(12345, phase.bundle_id, "phase.bundle_id should be 12345")
assert_equals("TestBundle", phase.bundle_name, "phase.bundle_name should be TestBundle")
assert_equals(2, phase.retry_count, "phase.retry_count should be 2")
assert_equals("Test reason", phase.reason, "phase.reason should be Test reason")

-- Test constructor uses default values
local phase_defaults = reload_phase.new(
    reload_phase.TYPE_RELOADED,
    999,
    "MyBundle"
)
assert_equals(reload_phase.TYPE_RELOADED, phase_defaults.type, "phase_defaults.type should be TYPE_RELOADED")
assert_equals(999, phase_defaults.bundle_id, "phase_defaults.bundle_id should be 999")
assert_equals("MyBundle", phase_defaults.bundle_name, "phase_defaults.bundle_name should be MyBundle")
assert_equals(0, phase_defaults.retry_count, "phase_defaults.retry_count should default to 0")
assert_equals("", phase_defaults.reason, "phase_defaults.reason should default to empty string")

-- Test constructor handles nil bundle_name
local phase_nil_name = reload_phase.new(
    reload_phase.TYPE_FAILED,
    1,
    nil,
    0,
    "Error"
)
assert_equals("", phase_nil_name.bundle_name, "nil bundle_name should default to empty string")

-- Test constructor handles nil reason
local phase_nil_reason = reload_phase.new(
    reload_phase.TYPE_FAILED,
    1,
    "Bundle",
    0,
    nil
)
assert_equals("", phase_nil_reason.reason, "nil reason should default to empty string")

-- ============================================================================
-- ReloadPhase Helper Method Tests
-- ============================================================================

print("\n=== ReloadPhase Helper Methods ===")

-- Test is_preparing
local preparing_phase = reload_phase.new(reload_phase.TYPE_PREPARING, 1, "Bundle")
assert_true(reload_phase.is_preparing(preparing_phase), "is_preparing should return true for TYPE_PREPARING")
assert_false(reload_phase.is_reloaded(preparing_phase), "is_reloaded should return false for TYPE_PREPARING")
assert_false(reload_phase.is_failed(preparing_phase), "is_failed should return false for TYPE_PREPARING")

-- Test is_reloaded
local reloaded_phase = reload_phase.new(reload_phase.TYPE_RELOADED, 1, "Bundle")
assert_false(reload_phase.is_preparing(reloaded_phase), "is_preparing should return false for TYPE_RELOADED")
assert_true(reload_phase.is_reloaded(reloaded_phase), "is_reloaded should return true for TYPE_RELOADED")
assert_false(reload_phase.is_failed(reloaded_phase), "is_failed should return false for TYPE_RELOADED")

-- Test is_failed
local failed_phase = reload_phase.new(reload_phase.TYPE_FAILED, 1, "Bundle", 0, "Error occurred")
assert_false(reload_phase.is_preparing(failed_phase), "is_preparing should return false for TYPE_FAILED")
assert_false(reload_phase.is_reloaded(failed_phase), "is_reloaded should return false for TYPE_FAILED")
assert_true(reload_phase.is_failed(failed_phase), "is_failed should return true for TYPE_FAILED")

-- ============================================================================
-- ReloadPhase phase_type_name Tests
-- ============================================================================

print("\n=== ReloadPhase phase_type_name ===")

assert_equals("Preparing", reload_phase.phase_type_name(reload_phase.TYPE_PREPARING), "phase_type_name(TYPE_PREPARING) should be Preparing")
assert_equals("Reloaded", reload_phase.phase_type_name(reload_phase.TYPE_RELOADED), "phase_type_name(TYPE_RELOADED) should be Reloaded")
assert_equals("Failed", reload_phase.phase_type_name(reload_phase.TYPE_FAILED), "phase_type_name(TYPE_FAILED) should be Failed")
assert_equals("Unknown(99)", reload_phase.phase_type_name(99), "phase_type_name(99) should be Unknown(99)")

-- ============================================================================
-- RuntimeConfig Default Values Tests
-- ============================================================================

print("\n=== RuntimeConfig Default Values ===")

local default_config = runtime_config.new()
assert_equals(3, default_config.hot_reload_max_retries, "default hot_reload_max_retries should be 3")
assert_equals(1000, default_config.hot_reload_retry_interval_ms, "default hot_reload_retry_interval_ms should be 1000")
assert_true(default_config.hot_reload_abort_on_max_retries, "default hot_reload_abort_on_max_retries should be true")

-- ============================================================================
-- RuntimeConfig Custom Values Tests
-- ============================================================================

print("\n=== RuntimeConfig Custom Values ===")

local custom_config = runtime_config.new({
    hot_reload_max_retries = 10,
    hot_reload_retry_interval_ms = 5000,
})
assert_equals(10, custom_config.hot_reload_max_retries, "custom hot_reload_max_retries should be 10")
assert_equals(5000, custom_config.hot_reload_retry_interval_ms, "custom hot_reload_retry_interval_ms should be 5000")
assert_true(custom_config.hot_reload_abort_on_max_retries, "custom hot_reload_abort_on_max_retries should use default true")

-- Test partial override (other values should use defaults)
local partial_config = runtime_config.new({
    hot_reload_max_retries = 5,
})
assert_equals(5, partial_config.hot_reload_max_retries, "partial hot_reload_max_retries should be 5")
assert_equals(1000, partial_config.hot_reload_retry_interval_ms, "partial hot_reload_retry_interval_ms should use default 1000")
assert_true(partial_config.hot_reload_abort_on_max_retries, "partial hot_reload_abort_on_max_retries should use default true")

-- Test zero retries
local zero_retries_config = runtime_config.new({
    hot_reload_max_retries = 0,
})
assert_equals(0, zero_retries_config.hot_reload_max_retries, "zero hot_reload_max_retries should be 0")

-- Test large retry interval
local large_interval_config = runtime_config.new({
    hot_reload_retry_interval_ms = 999999,
})
assert_equals(999999, large_interval_config.hot_reload_retry_interval_ms, "large hot_reload_retry_interval_ms should be 999999")

-- ============================================================================
-- Results
-- ============================================================================

print("\n========================================")
print(string.format("Tests passed: %d", tests_passed))
print(string.format("Tests failed: %d", tests_failed))
print("========================================")

os.exit(tests_failed > 0 and 1 or 0)