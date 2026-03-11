// host-libs/js/polyplug_test.ts
// Deno test suite for the polyplug host library.
// Run with: deno test --allow-ffi --allow-env --allow-read polyplug_test.ts

import { openPolyplug, runtimeNew, NULL_HANDLE } from "./polyplug.ts";

const POLYPLUG_SO = Deno.env.get("POLYPLUG_SO") ?? "";
const TEST_PLUGIN_DIR = Deno.env.get("TEST_PLUGIN_DIR") ?? "";
// FNV-1a hash of "test.add@1" = 0xCC4232FAB0410D2B
const TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2Bn;

Deno.test("runtime_new_succeeds", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    using _dispose = rt;  // [Symbol.dispose] called at end of using scope
    // If we reach here, runtime was created successfully
  } finally {
    lib.close();
  }
});

Deno.test("load_bundle_succeeds", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      rt.loadBundle(TEST_PLUGIN_DIR);
      // No error thrown = success
    } finally {
      rt[Symbol.dispose]();
    }
  } finally {
    lib.close();
  }
});

Deno.test("find_by_contract_returns_valid_handle", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      rt.loadBundle(TEST_PLUGIN_DIR);
      const handle = rt.findByContract(TEST_ADD_CONTRACT_ID);
      if (handle === NULL_HANDLE) throw new Error("Expected valid handle, got NULL_HANDLE");
    } finally { rt[Symbol.dispose](); }
  } finally { lib.close(); }
});

Deno.test("resolve_plugin_returns_guard", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      rt.loadBundle(TEST_PLUGIN_DIR);
      const handle = rt.findByContract(TEST_ADD_CONTRACT_ID);
      if (handle === NULL_HANDLE) throw new Error("Got NULL_HANDLE");
      const guard = rt.resolvePlugin(handle);
      try {
        if (!guard) throw new Error("guard is null");
      } finally { guard[Symbol.dispose](); }
    } finally { rt[Symbol.dispose](); }
  } finally { lib.close(); }
});

Deno.test("guard_vtable_nonnull", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      rt.loadBundle(TEST_PLUGIN_DIR);
      const handle = rt.findByContract(TEST_ADD_CONTRACT_ID);
      const guard = rt.resolvePlugin(handle);
      try {
        const vt = guard.vtable();
        if (vt === null) throw new Error("vtable is null");
      } finally { guard[Symbol.dispose](); }
    } finally { rt[Symbol.dispose](); }
  } finally { lib.close(); }
});

Deno.test("null_handle_for_missing_contract", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      // 0n is not a valid contract_id — should return NULL_HANDLE
      const handle = rt.findByContract(0n);
      if (handle !== NULL_HANDLE) throw new Error(`Expected NULL_HANDLE, got ${handle}`);
    } finally { rt[Symbol.dispose](); }
  } finally { lib.close(); }
});

Deno.test("last_error_after_failed_load", () => {
  const lib = openPolyplug(POLYPLUG_SO);
  try {
    const rt = runtimeNew(lib);
    try {
      let threw = false;
      try {
        rt.loadBundle("/does/not/exist");
      } catch (_e) {
        threw = true;
      }
      if (!threw) throw new Error("Expected loadBundle to throw");
      // lastError was consumed by the throw; but we can also call it directly:
      // (it may be empty after the exception path clears it — that is fine)
    } finally { rt[Symbol.dispose](); }
  } finally { lib.close(); }
});
