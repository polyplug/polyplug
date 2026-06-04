// tests/integration/cpp/hot_reload_test.cpp
// Unit tests for ReloadPhase and RuntimeConfig types.
//
// Compile with: g++ -std=c++17 -I../../../sdks/cpp/abi -I../../../sdks/cpp/host hot_reload_test.cpp -o hot_reload_test
// Run: ./hot_reload_test

#include <cassert>
#include <cstdint>
#include <cstring>
#include <iostream>

#include "polyplug/abi.hpp"
#include "polyplug/runtime_config.hpp"

static int tests_passed = 0;
static int tests_failed = 0;

#define ASSERT_EQUALS(expected, actual, message) \
    do { \
        if ((expected) == (actual)) { \
            tests_passed++; \
        } else { \
            tests_failed++; \
            std::cerr << "FAIL: " << (message) << std::endl; \
            std::cerr << "  Expected: " << (expected) << std::endl; \
            std::cerr << "  Actual: " << (actual) << std::endl; \
        } \
    } while (0)

#define ASSERT_TRUE(value, message) \
    do { \
        if ((value) == true) { \
            tests_passed++; \
        } else { \
            tests_failed++; \
            std::cerr << "FAIL: " << (message) << std::endl; \
            std::cerr << "  Expected: true" << std::endl; \
            std::cerr << "  Actual: " << (value) << std::endl; \
        } \
    } while (0)

#define ASSERT_FALSE(value, message) \
    do { \
        if ((value) == false) { \
            tests_passed++; \
        } else { \
            tests_failed++; \
            std::cerr << "FAIL: " << (message) << std::endl; \
            std::cerr << "  Expected: false" << std::endl; \
            std::cerr << "  Actual: " << (value) << std::endl; \
        } \
    } while (0)

// ============================================================================
// ReloadPhase Type Constants Tests
// ============================================================================

void test_reload_phase_type_constants() {
    std::cout << "=== ReloadPhase Type Constants ===" << std::endl;

    ASSERT_EQUALS(0U, static_cast<uint32_t>(ReloadPhaseType::Preparing), "TYPE_PREPARING should be 0");
    ASSERT_EQUALS(1U, static_cast<uint32_t>(ReloadPhaseType::Reloaded), "TYPE_RELOADED should be 1");
    ASSERT_EQUALS(2U, static_cast<uint32_t>(ReloadPhaseType::Failed), "TYPE_FAILED should be 2");
}

// ============================================================================
// ReloadPhase Struct Tests
// ============================================================================

void test_reload_phase_struct() {
    std::cout << "\n=== ReloadPhase Struct ===" << std::endl;

    // Test struct with all fields set
    ReloadPhase phase;
    phase.phase_type = ReloadPhaseType::Preparing;
    phase.bundle_id = 12345ULL;
    phase.bundle_name = StringView{reinterpret_cast<const uint8_t*>("TestBundle"), 10};
    phase.reason = StringView{reinterpret_cast<const uint8_t*>("Test reason"), 11};

    ASSERT_EQUALS(static_cast<uint32_t>(ReloadPhaseType::Preparing), static_cast<uint32_t>(phase.phase_type), "phase.phase_type should be Preparing");
    ASSERT_EQUALS(12345ULL, phase.bundle_id, "phase.bundle_id should be 12345");
    ASSERT_EQUALS(10U, phase.bundle_name.len, "phase.bundle_name.len should be 10");
    ASSERT_EQUALS(11U, phase.reason.len, "phase.reason.len should be 11");

    // Test struct with default-like values
    ReloadPhase phase_defaults;
    phase_defaults.phase_type = ReloadPhaseType::Reloaded;
    phase_defaults.bundle_id = 999ULL;
    phase_defaults.bundle_name = StringView{reinterpret_cast<const uint8_t*>("MyBundle"), 8};
    phase_defaults.reason = StringView{nullptr, 0};

    ASSERT_EQUALS(static_cast<uint32_t>(ReloadPhaseType::Reloaded), static_cast<uint32_t>(phase_defaults.phase_type), "phase_defaults.phase_type should be Reloaded");
    ASSERT_EQUALS(999ULL, phase_defaults.bundle_id, "phase_defaults.bundle_id should be 999");
    ASSERT_EQUALS(0U, phase_defaults.reason.len, "phase_defaults.reason.len should be 0");

    // Test Failed phase with reason
    ReloadPhase failed_phase;
    failed_phase.phase_type = ReloadPhaseType::Failed;
    failed_phase.bundle_id = 1ULL;
    failed_phase.bundle_name = StringView{reinterpret_cast<const uint8_t*>("FailedBundle"), 12};
    failed_phase.reason = StringView{reinterpret_cast<const uint8_t*>("Error occurred"), 14};

    ASSERT_EQUALS(static_cast<uint32_t>(ReloadPhaseType::Failed), static_cast<uint32_t>(failed_phase.phase_type), "failed_phase.phase_type should be Failed");
    ASSERT_EQUALS(14U, failed_phase.reason.len, "failed_phase.reason.len should be 14");
}

// ============================================================================
// RuntimeConfig Default Values Tests
// ============================================================================

void test_runtime_config_defaults() {
    std::cout << "\n=== RuntimeConfig Default Values ===" << std::endl;

    polyplug::RuntimeConfig config;

    ASSERT_EQUALS(3U, config.hot_reload_max_retries, "default hot_reload_max_retries should be 3");
    ASSERT_EQUALS(1000LL, config.hot_reload_retry_interval.count(), "default hot_reload_retry_interval should be 1000ms");
    ASSERT_TRUE(config.hot_reload_abort_on_max_retries, "default hot_reload_abort_on_max_retries should be true");
}

// ============================================================================
// RuntimeConfig Custom Values Tests
// ============================================================================

void test_runtime_config_custom() {
    std::cout << "\n=== RuntimeConfig Custom Values ===" << std::endl;

    // Test custom values
    polyplug::RuntimeConfig custom;
    custom.hot_reload_max_retries = 10;
    custom.hot_reload_retry_interval = std::chrono::milliseconds(5000);
    custom.hot_reload_abort_on_max_retries = false;

    ASSERT_EQUALS(10U, custom.hot_reload_max_retries, "custom hot_reload_max_retries should be 10");
    ASSERT_EQUALS(5000LL, custom.hot_reload_retry_interval.count(), "custom hot_reload_retry_interval should be 5000ms");
    ASSERT_FALSE(custom.hot_reload_abort_on_max_retries, "custom hot_reload_abort_on_max_retries should be false");

    // Test partial override (other values should use defaults)
    polyplug::RuntimeConfig partial;
    partial.hot_reload_max_retries = 5;

    ASSERT_EQUALS(5U, partial.hot_reload_max_retries, "partial hot_reload_max_retries should be 5");
    ASSERT_EQUALS(1000LL, partial.hot_reload_retry_interval.count(), "partial hot_reload_retry_interval should use default 1000ms");
    ASSERT_TRUE(partial.hot_reload_abort_on_max_retries, "partial hot_reload_abort_on_max_retries should use default true");

    // Test zero retries
    polyplug::RuntimeConfig zero_retries;
    zero_retries.hot_reload_max_retries = 0;

    ASSERT_EQUALS(0U, zero_retries.hot_reload_max_retries, "zero hot_reload_max_retries should be 0");

    // Test large retry interval
    polyplug::RuntimeConfig large_interval;
    large_interval.hot_reload_retry_interval = std::chrono::milliseconds(999999);

    ASSERT_EQUALS(999999LL, large_interval.hot_reload_retry_interval.count(), "large hot_reload_retry_interval should be 999999ms");
}

// ============================================================================
// Results
// ============================================================================

int main() {
    test_reload_phase_type_constants();
    test_reload_phase_struct();
    test_runtime_config_defaults();
    test_runtime_config_custom();

    std::cout << "\n========================================" << std::endl;
    std::cout << "Tests passed: " << tests_passed << std::endl;
    std::cout << "Tests failed: " << tests_failed << std::endl;
    std::cout << "========================================" << std::endl;

    return tests_failed > 0 ? 1 : 0;
}
