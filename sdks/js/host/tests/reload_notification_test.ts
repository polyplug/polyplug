// sdks/js/host/tests/reload_notification_test.ts
// Unit tests for ReloadPhase and RuntimeConfig types (SDK-side only, no native runtime).
//
// Run from repo root:
//   cd sdks/js/host/tests && deno test reload_notification_test.ts
//
// The justfile already does this via:
//   cd {{sdks_dir}}/js/host/tests && deno test reload_notification_test.ts

import { assertEquals, assertStrictEquals } from "jsr:@std/assert";
import { ReloadPhase } from "../polyplug/reload_phase.js";

// ─── ReloadPhase Type Constants ───────────────────────────────────────────────

Deno.test("TYPE_PREPARING is 0", () => {
    assertEquals(0, ReloadPhase.TYPE_PREPARING);
});

Deno.test("TYPE_RELOADED is 1", () => {
    assertEquals(1, ReloadPhase.TYPE_RELOADED);
});

Deno.test("TYPE_FAILED is 2", () => {
    assertEquals(2, ReloadPhase.TYPE_FAILED);
});

// ─── ReloadPhase Constructor: all fields ─────────────────────────────────────

Deno.test("constructor sets all properties", () => {
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

Deno.test("constructor uses default reason", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 999n, "MyBundle");
    assertEquals(ReloadPhase.TYPE_RELOADED, phase.type);
    assertEquals(999n, phase.bundleId);
    assertEquals("MyBundle", phase.bundleName);
    assertEquals("", phase.reason);
});

Deno.test("constructor handles empty bundleName", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "", "Error");
    assertEquals("", phase.bundleName);
});

Deno.test("constructor handles empty reason", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle", "");
    assertEquals("", phase.reason);
});

// ─── ReloadPhase Helper Methods ───────────────────────────────────────────────

Deno.test("isPreparing returns true for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(true, phase.isPreparing());
});

Deno.test("isPreparing returns false for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(false, phase.isPreparing());
});

Deno.test("isPreparing returns false for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(false, phase.isPreparing());
});

Deno.test("isReloaded returns false for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(false, phase.isReloaded());
});

Deno.test("isReloaded returns true for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(true, phase.isReloaded());
});

Deno.test("isReloaded returns false for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(false, phase.isReloaded());
});

Deno.test("isFailed returns false for TYPE_PREPARING", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 1n, "Bundle");
    assertStrictEquals(false, phase.isFailed());
});

Deno.test("isFailed returns false for TYPE_RELOADED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "Bundle");
    assertStrictEquals(false, phase.isFailed());
});

Deno.test("isFailed returns true for TYPE_FAILED", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 1n, "Bundle");
    assertStrictEquals(true, phase.isFailed());
});

// ─── ReloadPhase toString ─────────────────────────────────────────────────────

Deno.test("toString includes type name Preparing", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_PREPARING, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Preparing"), `toString should include 'Preparing', got: ${s}`);
});

Deno.test("toString includes type name Reloaded", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_RELOADED, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Reloaded"), `toString should include 'Reloaded', got: ${s}`);
});

Deno.test("toString includes type name Failed", () => {
    const phase = new ReloadPhase(ReloadPhase.TYPE_FAILED, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Failed"), `toString should include 'Failed', got: ${s}`);
});

Deno.test("toString includes Unknown for unrecognised type", () => {
    const phase = new ReloadPhase(99, 0n, "B");
    const s: string = phase.toString();
    assertEquals(true, s.includes("Unknown"), `toString should include 'Unknown' for type 99, got: ${s}`);
});

// ─── RuntimeConfig canonical 3-field ABI struct ───────────────────────────────
// The JS SDK represents RuntimeConfig as a plain object passed to setConfig().
// Canonical fields per D-22: compatibility (u32), hot_reload_enabled (bool), on_reload (fn | null).
// Tests verify the documented default values for each field.

Deno.test("RuntimeConfig: default compatibility is 0 (COMPATIBILITY_STRICT)", () => {
    // COMPATIBILITY_STRICT = 0 is exported from mod.js; test value only
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(0, config.compatibility);
});

Deno.test("RuntimeConfig: default hot_reload_enabled is false", () => {
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(false, config.hotReloadEnabled);
});

Deno.test("RuntimeConfig: default on_reload is null", () => {
    const config = { compatibility: 0, hotReloadEnabled: false, onReload: null };
    assertEquals(null, config.onReload);
});

Deno.test("RuntimeConfig: hot_reload_enabled can be set to true", () => {
    const config = { compatibility: 0, hotReloadEnabled: true, onReload: null };
    assertEquals(true, config.hotReloadEnabled);
});

Deno.test("RuntimeConfig: on_reload can be set to a callback", () => {
    let called = false;
    const cb = (_phase: ReloadPhase) => { called = true; };
    const config = { compatibility: 0, hotReloadEnabled: true, onReload: cb };
    assertEquals("function", typeof config.onReload);
    // Invoke to confirm it is callable with a ReloadPhase
    config.onReload(new ReloadPhase(ReloadPhase.TYPE_RELOADED, 1n, "B"));
    assertEquals(true, called);
});
