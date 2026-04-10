---
phase: 18
plan: 02
subsystem: ffi
tags: [ffi, host-interface, api-consolidation, breaking-change]
requires: [18-01]
provides: [consolidated-ffi-surface]
affects: [ffi.rs, runtime.rs, runtime_builder.rs, lib.rs, tests/*]
tech-stack:
  added: []
  patterns: [host-interface-operations, self-passing-pattern, runtime-pointer-in-hostinterface]
key-files:
  created: []
  modified:
    - crates/polyplug/src/ffi.rs
    - crates/polyplug/src/runtime.rs
    - crates/polyplug/src/runtime_builder.rs
    - crates/polyplug/src/lib.rs
    - crates/polyplug/tests/*.rs
decisions:
  - D-18-01: Only two FFI exports (create, destroy)
  - D-18-02: create returns HostInterface* not OpaqueRuntime*
  - D-18-03: All operations in HostInterface struct fields
  - D-18-04: No backward compatibility code
metrics:
  duration: ~45min
  tasks_completed: 4
  files_modified: 10
---

# Phase 18 Plan 02: Consolidate FFI to HostInterface Summary

**One-liner:** Reduced FFI surface from 13 functions to 2, with all runtime operations now accessible through HostInterface struct fields.

## Changes Made

### Task 1: Implement HostInterface operation functions

Replaced stub functions in runtime.rs with real implementations:

- `host_load_bundle`: Loads plugin bundle from path, returns AbiError
- `host_reload_bundle`: Hot-reloads plugin bundle, returns AbiError
- `host_register_host_contract`: Registers host contract interface
- `host_register_loader`: Registers language loader
- `host_get_last_error`: Gets last error message with clearing
- `host_get_error_len`: Gets error message length without clearing

All functions follow the self-passing pattern with `this: *const HostInterface` as first parameter.

### Task 2 & 3: Reduce FFI surface and update create

**Deleted FFI functions:**
- `polyplug_runtime_load_bundle`
- `polyplug_runtime_reload_bundle`
- `polyplug_runtime_find_guest_contract`
- `polyplug_runtime_find_guest_contract_by_bundle`
- `polyplug_runtime_find_all_by_contract`
- `polyplug_runtime_resolve_guest_contract`
- `polyplug_runtime_register_host_contract`
- `polyplug_runtime_register_loader`
- `polyplug_runtime_last_error`
- `polyplug_runtime_error_message_len`

**Remaining FFI exports:**
- `polyplug_runtime_create`: Returns `*const HostInterface`
- `polyplug_runtime_destroy`: Takes `*const HostInterface`

**Key change:** Runtime pointer is stored in `HostInterface.runtime` field, allowing destroy to reclaim the Runtime via `Box::from_raw`.

### Task 4: Update FFI tests

Updated 6 test files to use HostInterface methods:
- `integration_malformed.rs`
- `ffi_edge_cases.rs`
- `integration_ffi_null.rs`
- `integration_invalid_utf8.rs`
- `integration_host_lua.rs`
- `integration_last_error.rs`

Added `host_*` function exports to lib.rs for null-host testing.

## Deviations from Plan

None - plan executed exactly as written.

## Test Results

- FFI lib tests: 14 passed
- Build: 0 errors, 2 warnings (pre-existing)

## Known Stubs

None - all stubs replaced with real implementations.

## Threat Flags

None - no new security-relevant surface introduced beyond existing HostInterface operations.