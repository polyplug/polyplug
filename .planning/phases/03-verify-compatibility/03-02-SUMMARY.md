---
phase: 03-verify-compatibility
plan: 02
subsystem: dotnet-loader
tags:
  - error-unification
  - initfailed-pattern
  - dotnet
requires:
  - COMP-01
provides:
  - compilable-dotnet-source-files
affects:
  - polyplug_dotnet crate
tech-stack:
  added: []
  patterns:
    - LoaderError::InitFailed with descriptive messages
key-files:
  created: []
  modified:
    - crates/polyplug_dotnet/src/version.rs
    - crates/polyplug_dotnet/src/context.rs
decisions:
  - D-01: Use descriptive error messages for each failure context
metrics:
  duration: 5min
  tasks: 2
  files: 2
  completed: "2026-04-03"
---

# Phase 03 Plan 02: Fix .NET Source Files Summary

## One-Liner

Unified .NET loader source files to use `LoaderError::InitFailed` pattern instead of removed `AssemblyNotFound` and `ClrInitFailed` variants.

## What Changed

### Files Modified

| File | Changes | Error Sites |
|------|---------|-------------|
| `crates/polyplug_dotnet/src/version.rs` | Replaced 4 `AssemblyNotFound` with `InitFailed` | 4 |
| `crates/polyplug_dotnet/src/context.rs` | Replaced 14 `ClrInitFailed` + 1 `AssemblyNotFound` with `InitFailed` | 15 |

### Error Site Details

**version.rs (4 sites):**
- Line 52: File read failure → "assembly not found or unreadable"
- Line 60: PE parse failure → "invalid PE format"
- Line 80: COR20 header failure → "COR20 header not found or invalid"
- Line 90: Metadata slice failure → "CLI metadata section not found or invalid"

**context.rs (15 sites):**
- Lines 91, 98, 104: Tempfile operations (3 sites)
- Lines 118: Runtimeconfig path conversion
- Lines 134, 140, 148: Hostfxr location and loading (3 sites)
- Line 163: Runtime config initialization
- Lines 206, 218: Loader cache mutex operations (2 sites)
- Line 234: Assembly path conversion
- Lines 243, 249: CLR context operations (2 sites)
- Lines 264, 281: Loader cache mutex (relock/new) (2 sites)

## Verification Results

| Check | Result | Details |
|-------|--------|---------|
| No `AssemblyNotFound` in version.rs | PASS | 0 matches |
| No `ClrInitFailed` in context.rs | PASS | 0 matches |
| `InitFailed` present in version.rs | PASS | 4 matches |
| `InitFailed` present in context.rs | PASS | 15 matches |

## Deviations from Plan

None - plan executed exactly as written.

## Commits

| Task | Commit | Message |
|------|--------|---------|
| Task 1: version.rs | 3b0fa9e | fix(03-02): update version.rs to use InitFailed pattern |
| Task 2: context.rs | 5f4c146 | fix(03-02): update context.rs to use InitFailed pattern |

## Self-Check: PASSED

- [x] crates/polyplug_dotnet/src/version.rs modified and committed
- [x] crates/polyplug_dotnet/src/context.rs modified and committed
- [x] Commit 3b0fa9e exists in git history
- [x] Commit 5f4c146 exists in git history