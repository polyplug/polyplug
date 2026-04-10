---
phase: 18
plan: 05
subsystem: codegen
tags: [codegen, host-interface, generators, api-consolidation]
requires: [18-02, 18-03, 18-04]
provides: [host-interface-codegen]
affects: [polyplugc/generators/*, polyplug/build.rs]
tech-stack:
  added: []
  patterns: [host-interface-method-calls, self-passing-pattern]
key-files:
  created: []
  modified:
    - crates/polyplugc/src/generators/rust.rs
    - crates/polyplugc/src/generators/python.rs
    - crates/polyplugc/src/generators/cpp.rs
    - crates/polyplug/build.rs
decisions:
  - D-18-33: polyplugc generates host_callers.rs using HostInterface methods
  - D-18-34: All generators updated for unified HostInterface API
metrics:
  duration: ~30min
  tasks_completed: 3
  files_modified: 4
---

# Phase 18 Plan 05: Update Code Generators for HostInterface API

**One-liner:** Updated polyplugc generators to produce HostInterface-based code instead of direct FFI function calls.

## Changes Made

### Task 1: Rust Generator

Updated `crates/polyplugc/src/generators/rust.rs`:

- Removed import of `polyplug_runtime_resolve_guest_contract` FFI function
- Changed host caller code to use `HostInterface.resolve_guest_contract` method
- Pattern: `(iface.resolve_guest_contract)(host, handle)` with self-passing

### Task 2: Python/C++ Generators

Updated `crates/polyplugc/src/generators/python.rs`:

- Cast host pointer to HostInterface struct
- Call `resolve_guest_contract` through struct field: `host_iface.contents.resolve_guest_contract(host, handle)`

Updated `crates/polyplugc/src/generators/cpp.rs`:

- Use C++ member access: `host->resolve_guest_contract(host, handle)`
- Removed polyplug_runtime_resolve_contract FFI call

### Task 3: Lua/JS Generators

Verified `lua.rs` and `js_quickjs.rs` already use SDK-level abstractions:
- Lua generator uses `runtime:resolve_contract(handle)` (SDK method)
- JS generator uses struct definitions for HostInterface access
- No direct FFI function calls found

### Deviation: Bug Fix in build.rs

**Rule 1 - Auto-fix bug:** Fixed workspace root calculation in `crates/polyplug/build.rs`.

- **Found during:** Attempting to run tests that load plugins
- **Issue:** Workspace root was calculated as one level up from `crates/polyplug`, giving `crates/` instead of workspace root
- **Fix:** Changed to two levels up: `crates/polyplug -> crates -> polyplug`
- **Files modified:** `crates/polyplug/build.rs`
- **Commit:** 52f6265

## Deviations from Plan

### Pre-existing Test Failures (Out of Scope)

The plan listed test files (`ffi_edge_cases.rs`, `integration_ffi_null.rs`, `integration_ffi_robustness.rs`) to be updated. Investigation revealed:

- These tests were ALREADY updated in 18-02 (commit 20e5003)
- Some tests that load plugins fail due to missing loader registration
- This failure existed BEFORE phase 18 started (pre-existing issue)
- Per deviation scope boundary, pre-existing failures are out of scope

Tests that don't load plugins pass:
- `test_resolve_plugin_null_host` - PASS
- `test_resolve_plugin_null_handle` - PASS  
- `test_find_all_guest_contracts_empty_registry` - PASS

Pre-existing failing tests (deferred):
- `test_resolve_plugin_stale_handle` - needs loader registration
- `test_find_all_guest_contracts_single_plugin` - needs loader registration
- `test_find_all_guest_contracts_multiple_plugins` - needs loader registration

### Pre-existing Generator Test Failure (Out of Scope)

- `generators::cpp::tests::plugin_class_name_conversion` - FAIL (pre-existing)

## Deferred Items

### ffi_edge_cases.rs Plugin Loading Tests

Tests that call `load_bundle` fail because no loader is registered when using `polyplug_runtime_create()`. Resolution requires:

1. Either: Auto-register NativeLoader in `polyplug_runtime_create`
2. Or: Add helper function in tests to register loader
3. Or: Use `Runtime::builder().loader(NativeLoader::new(...)).build()` pattern

This is a design decision that requires a separate plan.

## Test Results

- `cargo build --package polyplugc` - SUCCESS (0 errors)
- `cargo test --package polyplug --test integration_ffi_null` - 7 passed
- `cargo test --package polyplug --test ffi_edge_cases` (null tests) - 2 passed

## Verification

1. No `polyplug_runtime_*` function references in generators (verified)
2. All generators produce HostInterface-based code (verified)
3. HostInterface method calls use self-passing pattern (verified)

## Known Stubs

None - generators produce complete HostInterface-based code.

## Threat Flags

None - no new security-relevant surface introduced.