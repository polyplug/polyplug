---
phase: 15-final-cleanup
plan: 03
subsystem: polyplug
tags: [terminology, cleanup, tests]
dependency_graph:
  requires: [15-02]
  provides: [interface-terminology-in-runtime-tests]
  affects: [test-helpers, test-variables]
tech_stack:
  added: []
  patterns: [function-rename, variable-rename]
key_files:
  created: []
  modified:
    - crates/polyplug/src/runtime.rs
decisions:
  - Preserve ABI field names (vtable_version) if present elsewhere
  - Rename test helper functions to interface terminology
  - Rename all test variables from vtable to interface
metrics:
  duration: 6m
  tasks_completed: 2
  files_modified: 1
  commits: 2
  lines_changed: 26
  completed_date: 2026-04-09
---

# Phase 15 Plan 03: Runtime Source Terminology Update Summary

## One-liner

Updated runtime.rs test section to use interface terminology for helper functions and test variables, eliminating vtable naming in 26 lines across 12 locations.

## What Changed

### Function Renames

| Old Name | New Name | Location |
|----------|----------|----------|
| `create_host_contract_vtable` | `create_host_contract_interface` | Line 1617 |
| `create_counting_host_contract_vtable` | `create_counting_host_contract_interface` | Line 1885 |

### Variable Renames

| Old Name | New Name | Tests Affected |
|----------|----------|----------------|
| `vtable` | `interface` | runtime_host_contracts_register_and_lookup, runtime_host_contracts_unregister, runtime_host_contracts_version_check, host_get_host_contract_callback_returns_registered_contract |
| `found_vtable` | `found_interface` | runtime_host_contracts_register_and_lookup |
| `vtable1` | `interface1` | runtime_host_contracts_duplicate_registration_fails |
| `vtable2` | `interface2` | runtime_host_contracts_duplicate_registration_fails |

### Call Sites Updated

All 10 call sites for the renamed functions updated:
- `create_host_contract_interface`: 7 calls (1 definition + 6 test uses)
- `create_counting_host_contract_interface`: 5 calls (1 definition + 4 test uses)

## Verification

- `grep -E "let vtable|found_vtable|vtable1|vtable2" crates/polyplug/src/runtime.rs` returns 0 matches
- `grep -n "vtable" crates/polyplug/src/runtime.rs | grep -v "HostContractVTable\|vtable_version"` returns 0 matches
- All runtime_host_contracts tests pass (4 tests)
- Singleton/multi_instance tests pass (1 test)
- Full polyplug test suite passes for runtime tests (99 passed)

## Commits

1. `e14b74b` - refactor(15-03): rename test helper functions to interface terminology
2. `16b6b8f` - refactor(15-03): rename test variables to interface terminology

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- crates/polyplug/src/runtime.rs modified: VERIFIED
- No vtable terminology remains (excluding ABI preserved terms): VERIFIED
- Commit e14b74b exists in git history: VERIFIED
- Commit 16b6b8f exists in git history: VERIFIED
- All runtime_host_contracts tests pass: VERIFIED