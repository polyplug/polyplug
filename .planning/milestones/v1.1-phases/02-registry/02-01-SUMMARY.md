---
phase: 02-registry
plan: 01
subsystem: registry
tags: [registry, arc, ffi, plugin-guard, vtable-slot]

requires: []
provides:
  - Direct Arc<GuestContractInterface> storage in RegistrySlot
  - resolve() returns interface pointer directly without guard
  - Simplified FFI with no ResolveHandle or release call
affects: [02-02, 02-03, tests]

tech-stack:
  added: []
  removed: [arc-swap (unused)]
  patterns: [direct interface storage, no RAII guard]

key-files:
  created: []
  modified:
    - crates/polyplug/src/registry/plugin_registry.rs
    - crates/polyplug/src/registry/mod.rs
    - crates/polyplug/src/ffi.rs
    - crates/polyplug/src/runtime.rs

key-decisions:
  - "Remove PluginGuard RAII pattern - hosts destroy instances before hot-reload via callback"
  - "Remove VTableSlot wrapper - RegistrySlot stores interface directly"
  - "Remove ResolveHandle from FFI - return interface pointer directly, no release needed"

patterns-established:
  - "Direct interface access: resolve() returns *const GuestContractInterface without guard"
  - "No quiescence tracking: callback-based hot-reload model handles instance cleanup"

requirements-completed: [REG-01, REG-02, REG-05]

duration: 5min
completed: 2026-04-04
---

# Phase 02 Plan 01: Remove VTableSlot and PluginGuard Summary

**Registry simplified: direct Arc<GuestContractInterface> storage without RAII guard pattern**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-04T12:00:00Z
- **Completed:** 2026-04-04T12:05:00Z
- **Tasks:** 2 (Task 1: types, Task 2: resolve methods)
- **Files modified:** 4

## Accomplishments
- Removed VTableSlot wrapper struct - registry stores interface directly
- Deleted PluginGuard RAII guard and resolve_guard() method
- Simplified resolve() to return interface pointer directly
- Removed ResolveHandle from FFI, simplified polyplug_runtime_resolve_plugin
- Removed polyplug_runtime_release_plugin function (no guard to release)

## Task Commits

1. **Task 1+2 combined: Remove VTableSlot and PluginGuard** - `23f0a52` (refactor)

**Plan metadata:** `b5345df` (docs: complete plan)

## Files Created/Modified
- `crates/polyplug/src/registry/plugin_registry.rs` - Removed PluginGuard, resolve_guard(), ArcSwap import
- `crates/polyplug/src/registry/mod.rs` - Removed PluginGuard and VTableSlot exports
- `crates/polyplug/src/ffi.rs` - Removed ResolveHandle, VTableSlot import, release_plugin function
- `crates/polyplug/src/runtime.rs` - Updated resolve_plugin to return interface pointer

## Decisions Made
- PluginGuard removed because hosts explicitly destroy instances before hot-reload via callback (no Arc quiescence needed)
- VTableSlot wrapper removed - unnecessary indirection when storing Arc<GuestContractInterface> directly
- FFI simplified to return interface pointer directly - no owned handle to manage

## Deviations from Plan

None - plan executed exactly as written. Tests still failing (expected) - updated in Plan 02-02.

## Issues Encountered
- Worktree executor found code divergence - executed directly in main repo instead
- Compilation errors expected from test files referencing deleted types - addressed in Plan 02-02

## Next Phase Readiness
- Registry core simplified, ready for test updates (Plan 02-02)
- Tests will compile after Plan 02-02 updates PluginGuard/VTableSlot references

---
*Phase: 02-registry*
*Plan: 01*
*Completed: 2026-04-04*