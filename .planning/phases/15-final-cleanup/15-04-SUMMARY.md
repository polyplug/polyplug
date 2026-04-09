---
phase: 15-final-cleanup
plan: 04
subsystem: polyplug
tags: [terminology, cleanup, tests]
dependency_graph:
  requires: [15-02]
  provides: [interface-terminology-in-tests]
  affects: [test-helpers, test-variables, test-statics]
tech_stack:
  added: []
  patterns: [variable-rename, function-rename, static-rename]
key_files:
  created: []
  modified:
    - crates/polyplug/tests/hot_reload_safety.rs
    - crates/polyplug/tests/stress_concurrent_registry.rs
    - crates/polyplug/tests/registry_edge_cases.rs
    - crates/polyplug/tests/integration_codegen_cpp.rs
    - crates/polyplug/tests/ffi_edge_cases.rs
    - crates/polyplug/tests/integration_ffi_null.rs
    - crates/polyplug/tests/integration_graph.rs
    - crates/polyplug/tests/integration_cross_plugin.rs
    - crates/polyplug/tests/stress_error.rs
    - crates/polyplug/tests/library_lifetime.rs
    - crates/polyplug/tests/integration_dispatch.rs
    - crates/polyplug/tests/stress_memory.rs
    - crates/polyplug/tests/integration_context.rs
    - crates/polyplug/tests/stress_hot_reload.rs
    - crates/polyplug/tests/integration_ffi_robustness.rs
    - crates/polyplug/tests/integration_panic.rs
    - crates/polyplug/tests/integration_load.rs
decisions:
  - Preserve ABI field names (vtable_version) in FFI types
  - Preserve SDK function names (store_host_vtable, get_host_vtable) as FFI imports
  - Preserve generated file names in string literals (vtables.rs, PANIC_PLUGIN_VTABLE)
  - Rename all test helper functions to interface terminology
  - Rename all test variables from vtable to interface
  - Rename all static test constants from VTABLE_* to INTERFACE_*
metrics:
  duration: 64m
  tasks_completed: 3
  files_modified: 17
  commits: 3
  lines_changed: 446
  completed_date: 2026-04-09
---

# Phase 15 Plan 04: Test Terminology Update Summary

## One-liner

Updated 17 test files to use interface terminology, renaming static constants (VTABLE_* to INTERFACE_*), variables (vtable_ptr to interface_ptr), and function names (init_memory_plugin_vtable to init_memory_plugin_interface) across 446 lines.

## What Changed

### Static Constant Renames

| Old Name | New Name | File |
|----------|----------|------|
| VTABLE_V1 | INTERFACE_V1 | hot_reload_safety.rs |
| VTABLE_V2 | INTERFACE_V2 | hot_reload_safety.rs |
| VTABLES_V1 | INTERFACES_V1 | stress_concurrent_registry.rs |
| VTABLE_SWAP_V1 | INTERFACE_SWAP_V1 | stress_concurrent_registry.rs |
| VTABLE_SWAP_V2 | INTERFACE_SWAP_V2 | stress_concurrent_registry.rs |
| VTABLE_A/B/C | INTERFACE_A/B/C | registry_edge_cases.rs |
| VTABLE | INTERFACE | registry_edge_cases.rs |
| CONCURRENT_VTABLES | CONCURRENT_INTERFACES | registry_edge_cases.rs |
| VTABLE_IMPL_A/B/C | INTERFACE_IMPL_A/B/C | registry_edge_cases.rs |

### Function Renames

| Old Name | New Name | File |
|----------|----------|------|
| test_swap_interface_changes_vtable | test_swap_interface_changes_interface_pointer | hot_reload_safety.rs |
| init_memory_plugin_vtable | init_memory_plugin_interface | stress_memory.rs, integration_ffi_robustness.rs |
| test_init_registers_vtable | test_init_registers_interface | integration_load.rs |
| stress_vtable_handoff_correctness_no_torn_reads | stress_interface_handoff_correctness_no_torn_reads | stress_hot_reload.rs |

### Variable Renames (Pattern Applied Across All Files)

| Old Pattern | New Pattern |
|-------------|-------------|
| vtable_ptr | interface_ptr |
| vtable | interface |
| new_vtable | new_interface |

### Comment/Doc Updates

| Old | New |
|-----|-----|
| Static vtables for testing | Static interfaces for testing |
| vtable pointer | interface pointer |
| vtable entries | interface entries |
| vtable must be resolvable | interface must be resolvable |
| function 0 in the vtable | function 0 in the interface |
| Resolve the vtable | Resolve the interface |
| Register the panic plugin vtable | Register the panic plugin interface |
| VTable handoff correctness | Interface handoff correctness |

### Preserved Terms

The following were intentionally NOT changed:

1. **ABI Field Names**: `vtable_version` - FFI field names in ABI structs
2. **SDK Function Names**: `store_host_vtable`, `get_host_vtable` - FFI function imports
3. **Generated File Names**: `vtables.rs`, `PANIC_PLUGIN_VTABLE` - String literals for generated code

## Verification

- Main polyplug test suite: 99 passed
- Test files compile successfully
- Remaining vtable references (6) are string literals for generated code names

## Commits

1. `caeba5d` - refactor(15-04): rename static vtable constants to interface terminology
2. `8d2d971` - refactor(15-04): update integration_codegen_cpp.rs to interface terminology
3. `43f7606` - refactor(15-04): update remaining test files to interface terminology

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- All 17 modified test files exist: VERIFIED
- All 3 commits exist in git history: VERIFIED
- Remaining vtable references are string literals for generated code: VERIFIED
- Main test suite passes (99 tests): VERIFIED