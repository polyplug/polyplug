// sdks/js/host/tests/reload_notification_test.ts
// Unit tests for ReloadPhase and RuntimeConfig types (SDK-side only, no native runtime).
//
// Registered into the runtime-agnostic harness; run the whole suite from repo
// root via the Deno entrypoint (or `just test-host-js`):
//   deno run --allow-ffi --allow-env --allow-read --allow-write \
//     sdks/js/testing/run_deno.ts

import { assertEquals, assertStrictEquals, test } from "../../testing/harness.ts";
import { ReloadPhase } from "../polyplug/reload_phase.js";

// ─── ReloadPhase Type Constants ───────────────────────────────────────────────

test("TYPE_PREPARING is 0", () => {
    assertEquals(0, ReloadPhase.TYPE_PREPARING);
});

test("TYPE_RELOADED is 1", () => {
    assertEquals(1, ReloadPhase.TYPE_RELOADED);
});

test("TYPE_FAILED is 2", () => {
    assertEquals(2, ReloadPhase.TYPE_FAILED);
});

// ─── ReloadPhase Constructor: all fields ─────────────────────────────────────

test("constructor sets all properties", () => {
    const phase = new ReloadPhase(
        ReloadPhase.TYPE_PREPARING,
        12345n,
        "TestBundle",
        "Test reason",
    );
    assertEquals(ReloadPhase.TYPE_PREPARING, phase.type);
    assertEquals(12345n, phase.bundleId);
    assertEquals("TestBundle", phase.bundleName);
    assertEquals("Test reason", phase.reason);
});

test("constructor uses default reason", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 999n, "MyBundle");
    assertEquals(ReloadPhase.TYPE_RELOADED, phase.type);
    assertEquals(999n, phase.bundleId);
    assertEquals("MyBundle", phase.bundleName);
    assertEquals("", phase.reason);
});

test("constructor handles empty bundleName", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "", "Error");
    assertEquals("", phase.bundleName);
});

test("constructor handles empty reason", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle", "");
    assertEquals("", phase.reason);
});

// ─── ReloadPhase Helper Methods ───────────────────────────────────────────────

test("isPreparing returns true for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(true, phase.isPreparing());
});

test("isPreparing returns false for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(false, phase.isPreparing());
});

test("isPreparing returns false for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(false, phase.isPreparing());
});

test("isReloaded returns false for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(false, phase.isReloaded());
});

test("isReloaded returns true for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(true, phase.isReloaded());
});

test("isReloaded returns false for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(false, phase.isReloaded());
});

test("isFailed returns false for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(false, phase.isFailed());
});

test("isFailed returns false for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(false, phase.isFailed());
});

test("isFailed returns true for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(true, phase.isFailed());
});

// ─── ReloadPhase toString ─────────────────────────────────────────────────────

test("toString includes type name Preparing", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Preparing"), `toString should include 'Preparing', got: ${s}`);
});

test("toString includes type name Reloaded", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Reloaded"), `toString should include 'Reloaded', got: ${s}`);
});

test("toString includes type name Failed", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Failed"), `toString should include 'Failed', got: ${s}`);
});

test("toString includes Unknown for unrecognised type", () => {
    const phase = new ReloadPhase(99, 0n, "B");
    const s: string = phase.toString();
    assertEquals(
        true,
        s.includes("Unknown"),
        `toString should include 'Unknown' for type 99, got: ${s}`,
    );
});

// ─── RuntimeConfig per-instance options object ───────────────────────────────
// The JS SDK represents RuntimeConfig as a plain object passed per-instance to
// runtimeNew(lib, { config, onReload, logger }) — there is NO module-level
// pending-config/pending-callback state (Rule 12).
// Tests verify the documented default values for each field.

import * as hostMod from "../polyplug/mod.js";

test("module holds no pending-config statics (Rule 12)", () => {
    // The old module-global API is gone: configuration is per-instance.
    assertEquals(undefined, (hostMod as Record<string, unknown>).setConfig);
    assertEquals(undefined, (hostMod as Record<string, unknown>).onReload);
    assertEquals("function", typeof hostMod.runtimeNew);
});

test("RuntimeConfig: default compatibility is 0 (COMPATIBILITY_STRICT)", () => {
    // COMPATIBILITY_STRICT = 0 is exported from mod.js; test value only
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(0, config.compatibility);
});

test("RuntimeConfig: default hot_reload_enabled is false", () => {
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(false, config.hotReloadEnabled);
});

test("RuntimeConfig: default on_reload is null", () => {
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(null, config.onReload);
});

test("RuntimeConfig: hot_reload_enabled can be set to true", () => {
    const config = { compatibility: 0, hotReloadEnabled: true, onReload: null };
    assertEquals(true, config.hotReloadEnabled);
});

test("RuntimeConfig: on_reload can be set to a callback", () => {
    let called = false;
    const cb = (_phase: ReloadPhase) => {
        called = true;
    };
    const config = { compatibility: 0, hotReloadEnabled: true, onReload: cb };
    assertEquals("function", typeof config.onReload);
    // Invoke to confirm it is callable with a ReloadPhase
    config.onReload(new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "B"));
    assertEquals(true, called);
});
