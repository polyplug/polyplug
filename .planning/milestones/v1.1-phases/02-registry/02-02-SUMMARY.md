---
phase: 02-registry
plan: 02
subsystem: registry
tags: [registry, arc-swap, tests, blocker]

requires: []
provides: []
affects: []

tech-stack:
  added: []
  removed: []
  patterns: []

key-files:
  created: []
  modified:
    - crates/polyplug/src/registry/plugin_registry.rs
    - crates/polyplug/Cargo.toml
    - crates/polyplug/tests/registry_edge_cases.rs
    - crates/polyplug/tests/hot_reload_safety.rs
    - crates/polyplug/tests/stress_concurrent_registry.rs
    - crates/polyplug/tests/stress_hot_reload.rs
    - crates/polyplug/benches/registry_resolve.rs
    - crates/polyplug/tests/integration_cross_plugin.rs

key-decisions: []

requirements-completed: []

duration: 45min
completed: 2026-04-04
---

# Phase 02 Plan 02: Remove arc_swap and Update Tests Summary

**BLOCKED: Phase 01 ABI changes not fully integrated into polyplug crate**

## Blocker Details

During execution, discovered that Phase 01 ABI type changes were not propagated to the polyplug crate. The following issues prevent completion:

### Type Mismatches (17 compilation errors)

1. **GuestContractId/BundleId type changes**: The registry and FFI code use raw `u64` but the new ABI uses typed IDs (`GuestContractId`, `BundleId`)

2. **RuntimeConfigC vs RuntimeConfig**: FFI layer has type mismatches

3. **HostContractInterface structure**: Missing `header` field - structure was changed in Phase 01

4. **GuestContractInterface doesn't implement Clone**: Required for Arc storage, but not derived

5. **GuestContractId doesn't implement Default**: Required for serde deserialization in manifest parsing

6. **Missing get_vtable_arc method**: Removed in Plan 02-01 but still referenced in reload.rs

## Completed Work

### Task 1: Remove arc_swap import and dependency (PARTIAL)

- Removed `get_interface_arc()` method from plugin_registry.rs
- Removed `arc-swap` from Cargo.toml
- No arc_swap references remain in plugin_registry.rs

### Task 2: Update tests (PARTIAL)

Files updated (but compilation blocked by type mismatches):
- `tests/registry_edge_cases.rs` - Updated to use `resolve()` instead of `resolve_guard()`
- `tests/hot_reload_safety.rs` - Removed PluginGuard patterns
- `tests/stress_concurrent_registry.rs` - Updated to use new patterns
- `tests/stress_hot_reload.rs` - Updated to use new patterns
- `benches/registry_resolve.rs` - Renamed benchmarks
- `tests/integration_cross_plugin.rs` - Fixed resolve calls

Files deleted:
- `tests/integration_quiescence.rs` - Quiescence timeout tests
- `tests/stress_quiescence_race.rs` - Quiescence race tests

## Recommended Resolution

Create a new plan to integrate Phase 01 ABI changes into the polyplug crate:

1. Add `Clone` derive to `GuestContractInterface`
2. Add `Default` impl to `GuestContractId` (or use Option in manifests)
3. Update FFI layer for `RuntimeConfig` vs `RuntimeConfigC`
4. Update `HostContractInterface` usage to match new structure
5. Update all `find_by_contract` callers to use `GuestContractId`
6. Update all `register` callers to use `BundleId`
7. Remove `get_vtable_arc` references from reload.rs

## Files Modified (not compiled)

- crates/polyplug/src/registry/plugin_registry.rs
- crates/polyplug/Cargo.toml
- crates/polyplug/tests/*.rs
- crates/polyplug/benches/*.rs
- tests/fixtures/test_plugin/src/lib.rs

## Self-Check

| Check | Status |
|-------|--------|
| arc-swap removed from Cargo.toml | DONE |
| arc_swap imports removed | DONE |
| PluginGuard references removed from tests | DONE |
| Tests compile | FAILED (17 errors) |
| Tests pass | NOT RUN |

---
*Phase: 02-registry*
*Plan: 02*
*Status: BLOCKED*