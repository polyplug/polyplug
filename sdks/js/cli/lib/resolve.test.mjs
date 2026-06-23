/**
 * Unit tests for lib/resolve.mjs.
 * Run with: node --test lib/resolve.test.mjs
 *
 * No network I/O, no filesystem access, no real binary required.
 */

import { test } from "node:test";
import assert from "node:assert/strict";
import { resolvePlatform } from "./resolve.mjs";

test("linux/x64 resolves to linux package with bare binary name", () => {
  const info = resolvePlatform("linux", "x64");
  assert.equal(info.packageName, "@polyplug/cli-linux-x64");
  assert.equal(info.binaryName, "polyplugc");
});

test("darwin/arm64 resolves to darwin package with bare binary name", () => {
  const info = resolvePlatform("darwin", "arm64");
  assert.equal(info.packageName, "@polyplug/cli-darwin-arm64");
  assert.equal(info.binaryName, "polyplugc");
});

test("win32/x64 resolves to win32 package with .exe binary name", () => {
  const info = resolvePlatform("win32", "x64");
  assert.equal(info.packageName, "@polyplug/cli-win32-x64");
  assert.equal(info.binaryName, "polyplugc.exe");
});

test("unsupported platform throws with helpful message", () => {
  assert.throws(
    () => resolvePlatform("freebsd", "x64"),
    (err) => {
      assert.ok(err instanceof Error);
      assert.ok(err.message.includes("freebsd-x64"), `expected platform in message, got: ${err.message}`);
      assert.ok(err.message.includes("cargo install polyplugc"), `expected install hint in message, got: ${err.message}`);
      assert.ok(err.message.includes("https://github.com/polyplug/polyplug/releases"), `expected URL in message, got: ${err.message}`);
      return true;
    }
  );
});

test("unsupported arch on known platform throws", () => {
  assert.throws(
    () => resolvePlatform("linux", "arm64"),
    (err) => {
      assert.ok(err instanceof Error);
      assert.ok(err.message.includes("linux-arm64"));
      return true;
    }
  );
});
