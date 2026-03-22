// host-libs/js-deno/tests/reload_notification_test.ts
// Unit tests for ReloadPhase and RuntimeConfig types.
//
// Run with: deno test --allow-read host-libs/js-deno/tests/reload_notification_test.ts

import { assertEquals } from "https://deno.land/std@0.208.0/assert/mod.ts";
import { ReloadPhase } from "../polyplug/reload_phase.js";
import { RuntimeConfig } from "../polyplug/runtime_config.js";

// ============================================================================
// ReloadPhase Type Constant Tests
// ============================================================================

Deno.test("ReloadPhase.TYPE_PREPARING is 0", () => {
    assertEquals(ReloadPhase.TYPE_PREPARING, 0);
});

Deno.test("ReloadPhase.TYPE_RELOADED is 1", () => {
    assertEquals(ReloadPhase.TYPE_RELOADED, 1);
});

Deno.test("ReloadPhase.TYPE_FAILED is 2", () => {
    assertEquals(ReloadPhase.TYPE_FAILED, 2);
});

// ============================================================================
// ReloadPhase Constructor Tests
// ============================================================================

Deno.test("ReloadPhase constructor sets all properties", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_PREPARING,
        12345n,
        "TestBundle",
        2,
        "Test reason"
    );
    assertEquals(phase.type, ReloadPhase.TYPE_PREPARING);
    assertEquals(phase.bundleId, 12345n);
    assertEquals(phase.bundleName, "TestBundle");
    assertEquals(phase.retryCount, 2);
    assertEquals(phase.reason, "Test reason");
});

Deno.test("ReloadPhase constructor uses default values", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_RELOADED,
        999n,
        "MyBundle"
    );
    assertEquals(phase.type, ReloadPhase.TYPE_RELOADED);
    assertEquals(phase.bundleId, 999n);
    assertEquals(phase.bundleName, "MyBundle");
    assertEquals(phase.retryCount, 0);
    assertEquals(phase.reason, "");
});

Deno.test("ReloadPhase constructor handles empty string bundleName", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_FAILED,
        1n,
        "",
        0,
        "Error"
    );
    assertEquals(phase.bundleName, "");
});

Deno.test("ReloadPhase constructor handles empty string reason", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_FAILED,
        1n,
        "Bundle",
        0,
        ""
    );
    assertEquals(phase.reason, "");
});

// ============================================================================
// ReloadPhase Helper Method Tests
// ============================================================================

Deno.test("ReloadPhase.isPreparing returns true when type is Preparing", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertEquals(phase.isPreparing(), true);
});

Deno.test("ReloadPhase.isPreparing returns false when type is not Preparing", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertEquals(phase.isPreparing(), false);
});

Deno.test("ReloadPhase.isReloaded returns true when type is Reloaded", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertEquals(phase.isReloaded(), true);
});

Deno.test("ReloadPhase.isReloaded returns false when type is not Reloaded", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertEquals(phase.isReloaded(), false);
});

Deno.test("ReloadPhase.isFailed returns true when type is Failed", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertEquals(phase.isFailed(), true);
});

Deno.test("ReloadPhase.isFailed returns false when type is not Failed", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertEquals(phase.isFailed(), false);
});

// ============================================================================
// ReloadPhase toString Tests
// ============================================================================

Deno.test("ReloadPhase toString for Preparing includes all relevant fields", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_PREPARING,
        42n,
        "TestBundle",
        3,
        "Ignored"
    );
    const result = phase.toString();
    assertEquals(result.includes("Preparing"), true);
    assertEquals(result.includes("42"), true);
    assertEquals(result.includes("TestBundle"), true);
    assertEquals(result.includes("3"), true);
});

Deno.test("ReloadPhase toString for Reloaded includes bundle info", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_RELOADED,
        99n,
        "MyPlugin"
    );
    const result = phase.toString();
    assertEquals(result.includes("Reloaded"), true);
    assertEquals(result.includes("99"), true);
    assertEquals(result.includes("MyPlugin"), true);
});

Deno.test("ReloadPhase toString for Failed includes reason", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_FAILED,
        1n,
        "FailedBundle",
        0,
        "Connection timeout"
    );
    const result = phase.toString();
    assertEquals(result.includes("Failed"), true);
    assertEquals(result.includes("1"), true);
    assertEquals(result.includes("FailedBundle"), true);
    assertEquals(result.includes("Connection timeout"), true);
});

// ============================================================================
// RuntimeConfig Default Values Tests
// ============================================================================

Deno.test("RuntimeConfig default constructor sets default values", () => {
    const config = new RuntimeConfig();
    assertEquals(config.hotReloadMaxRetries, 3);
    assertEquals(config.hotReloadRetryIntervalMs, 1000);
    assertEquals(config.hotReloadAbortOnMaxRetries, true);
});

Deno.test("RuntimeConfig parameterless constructor creates valid instance", () => {
    const config = new RuntimeConfig();
    assertEquals(config instanceof RuntimeConfig, true);
});

// ============================================================================
// RuntimeConfig Custom Values Tests
// ============================================================================

Deno.test("RuntimeConfig parameterized constructor sets custom values", () => {
    const config = new RuntimeConfig({
        hotReloadMaxRetries: 10,
        hotReloadRetryIntervalMs: 5000,
        hotReloadAbortOnMaxRetries: false,
    });
    assertEquals(config.hotReloadMaxRetries, 10);
    assertEquals(config.hotReloadRetryIntervalMs, 5000);
    assertEquals(config.hotReloadAbortOnMaxRetries, false);
});

Deno.test("RuntimeConfig properties can be modified", () => {
    const config = new RuntimeConfig();
    config.hotReloadMaxRetries = 5;
    config.hotReloadRetryIntervalMs = 2000;
    config.hotReloadAbortOnMaxRetries = false;
    assertEquals(config.hotReloadMaxRetries, 5);
    assertEquals(config.hotReloadRetryIntervalMs, 2000);
    assertEquals(config.hotReloadAbortOnMaxRetries, false);
});

Deno.test("RuntimeConfig allows zero retries", () => {
    const config = new RuntimeConfig({
        hotReloadMaxRetries: 0,
    });
    assertEquals(config.hotReloadMaxRetries, 0);
});

Deno.test("RuntimeConfig allows large retry interval", () => {
    const config = new RuntimeConfig({
        hotReloadRetryIntervalMs: Number.MAX_SAFE_INTEGER,
    });
    assertEquals(config.hotReloadRetryIntervalMs, Number.MAX_SAFE_INTEGER);
});

// ============================================================================
// RuntimeConfig Static Factory Method Tests
// ============================================================================

Deno.test("RuntimeConfig.default creates config with default values", () => {
    const config = RuntimeConfig.default();
    assertEquals(config.hotReloadMaxRetries, 3);
    assertEquals(config.hotReloadRetryIntervalMs, 1000);
    assertEquals(config.hotReloadAbortOnMaxRetries, true);
});

Deno.test("RuntimeConfig.infiniteRetries creates config with zero retries and no abort", () => {
    const config = RuntimeConfig.infiniteRetries();
    assertEquals(config.hotReloadMaxRetries, 0);
    assertEquals(config.hotReloadRetryIntervalMs, 1000);
    assertEquals(config.hotReloadAbortOnMaxRetries, false);
});

Deno.test("RuntimeConfig.infiniteRetries accepts custom retry interval", () => {
    const config = RuntimeConfig.infiniteRetries(5000);
    assertEquals(config.hotReloadMaxRetries, 0);
    assertEquals(config.hotReloadRetryIntervalMs, 5000);
    assertEquals(config.hotReloadAbortOnMaxRetries, false);
});

Deno.test("RuntimeConfig.withRetries creates config with custom retry count", () => {
    const config = RuntimeConfig.withRetries(10);
    assertEquals(config.hotReloadMaxRetries, 10);
    assertEquals(config.hotReloadRetryIntervalMs, 1000);
    assertEquals(config.hotReloadAbortOnMaxRetries, true);
});

Deno.test("RuntimeConfig.withRetries accepts custom retry interval", () => {
    const config = RuntimeConfig.withRetries(5, 2000);
    assertEquals(config.hotReloadMaxRetries, 5);
    assertEquals(config.hotReloadRetryIntervalMs, 2000);
    assertEquals(config.hotReloadAbortOnMaxRetries, true);
});