---
phase: 05-sdk-updates
verified: 2026-04-04T16:30:00Z
status: gaps_found
score: 5/7 must-haves verified
gaps:
  - gap_id: gap-01
    truth: "C++ SDK PluginGuard removed (replaced by instance wrappers)"
    status: failed
    reason: "C++ SDK was not covered by any plan in phase 05. The sdks/cpp/host/polyplug/runtime.hpp file still contains PluginGuard class (lines 59-114) and resolve_plugin returns PluginGuard (line 188)."
    artifacts:
      - path: "sdks/cpp/host/polyplug/runtime.hpp"
        issue: "PluginGuard class still present, RuntimeConfigC struct missing compatibility field"
    suggested_plan: "05-07"
    plan_objective: "Update C++ SDK: remove PluginGuard class, add RuntimeConfig FFI struct (24 bytes), update resolve_plugin to return raw handle"
  - gap_id: gap-02
    truth: "No *C suffix types in SDK FFI (types named RuntimeConfig, not RuntimeConfigC)"
    status: failed
    reason: "All SDKs (Python, C#, Lua, C++) use RuntimeConfigC naming with C suffix instead of RuntimeConfig. This violates the naming convention - types should match polyplug_abi exactly."
    artifacts:
      - path: "sdks/python/host/polyplug/runtime.py"
        issue: "RuntimeConfigC should be named RuntimeConfig"
      - path: "sdks/csharp/host/NativeMethods.cs"
        issue: "RuntimeConfigC should be named RuntimeConfig"
      - path: "sdks/lua/host/polyplug/runtime.lua"
        issue: "RuntimeConfigC in ffi.cdef should be named RuntimeConfig"
      - path: "sdks/cpp/host/polyplug/runtime.hpp"
        issue: "RuntimeConfigC should be named RuntimeConfig"
    suggested_plan: "05-08"
    plan_objective: "Rename RuntimeConfigC → RuntimeConfig in Python, C#, Lua, and C++ SDKs for naming consistency"
---

# Phase 05: SDK Updates Verification Report

**Phase Goal:** All five SDKs use types from polyplug_abi without duplicates
**Verified:** 2026-04-04T16:30:00Z
**Status:** gaps_found
**Re-verification:** Yes — updated after user feedback

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Rust SDK imports RuntimeConfig, ReloadPhase from polyplug_abi (no duplicates) | VERIFIED | manifest.rs re-exports from polyplug::loader; no duplicate type definitions |
| 2 | Python SDK removes RuntimeConfig duplicate, uses abi module types | FAILED | runtime_config.py DELETED ✓; but still uses RuntimeConfigC naming |
| 3 | C# SDK removes RuntimeConfig duplicate, uses Abi namespace types | FAILED | HostRuntimeConfig.cs DELETED ✓; but still uses RuntimeConfigC naming |
| 4 | Lua SDK uses FFI cdef types from polyplug_abi | FAILED | runtime_config.lua DELETED ✓; but still uses RuntimeConfigC naming |
| 5 | JS SDK uses TypeScript interfaces from polyplug_abi | VERIFIED | runtime_config.js DELETED; 24-byte buffer packing (no struct naming issue) |
| 6 | PluginGuard removed from all SDKs (replaced by instance wrappers) | FAILED | C++ SDK still has PluginGuard class |
| 7 | All SDKs generate instance-based wrappers via codegen | VERIFIED | All 6 generators have create_instance/destroy_instance patterns |

**Score:** 5/7 truths verified

---

## Gap 1: C++ SDK Not Updated

**Suggested Plan:** 05-07

**Requirement:** SDK-06 — Remove PluginGuard from all SDKs

**Issue:** The C++ SDK (`sdks/cpp/host/polyplug/runtime.hpp`) was not included in any phase 05 plan.

**Current State:**
```cpp
// Lines 59-114: PluginGuard class (SHOULD BE REMOVED)
class PluginGuard {
    // ...
};

// Lines 48-54: RuntimeConfigC (INCOMPLETE - missing compatibility)
struct RuntimeConfigC {
    uint8_t hot_reload_enabled;
    uint32_t hot_reload_max_retries;
    uint64_t hot_reload_retry_interval_ms;
    uint8_t hot_reload_abort_on_max_retries;
    // MISSING: compatibility field
};
```

**Required Changes:**
1. Remove `PluginGuard` class entirely
2. Add `RuntimeConfig` FFI struct (24 bytes with compatibility field)
3. Update `resolve_plugin` to return `ResolveHandle*` instead of `PluginGuard`

---

## Gap 2: RuntimeConfigC → RuntimeConfig Rename

**Suggested Plan:** 05-08

**Requirement:** CLN-02 — No *C suffix types in FFI (all types from polyplug_abi are canonical)

**Issue:** All native SDKs use `RuntimeConfigC` naming instead of `RuntimeConfig`. The "C" suffix is a legacy naming anti-pattern that should match the canonical `polyplug_abi::RuntimeConfig`.

**Files to Update:**

| SDK | File | Line(s) | Change |
|-----|------|---------|--------|
| Python | sdks/python/host/polyplug/runtime.py | ~31, ~35 | `class RuntimeConfigC` → `class RuntimeConfig` |
| C# | sdks/csharp/host/NativeMethods.cs | 68 | `struct RuntimeConfigC` → `struct RuntimeConfig` |
| Lua | sdks/lua/host/polyplug/runtime.lua | 43 | `} RuntimeConfigC;` → `} RuntimeConfig;` |
| C++ | sdks/cpp/host/polyplug/runtime.hpp | 49 | `struct RuntimeConfigC` → `struct RuntimeConfig` |

**Note:** JS SDK uses `Uint8Array(24)` buffer packing, not a named struct, so no rename needed.

---

## Summary

| Gap | Plan | Objective | Files |
|-----|------|-----------|-------|
| C++ SDK missing | 05-07 | Remove PluginGuard, add RuntimeConfig (24 bytes) | sdks/cpp/host/polyplug/runtime.hpp |
| RuntimeConfigC naming | 05-08 | Rename RuntimeConfigC → RuntimeConfig | Python, C#, Lua, C++ SDKs |

---

Verified: 2026-04-04T16:30:00Z