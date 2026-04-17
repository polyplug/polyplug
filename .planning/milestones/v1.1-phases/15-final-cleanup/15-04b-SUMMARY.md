---
phase: 15-final-cleanup
plan: 04b
subsystem: polyplugc
tags: [terminology, cleanup, tests]
dependency_graph:
  requires: [15-02]
  provides: [interface-terminology-in-polyplugc-tests]
  affects: [test-assertions, test-helpers, test-variables]
tech_stack:
  added: []
  patterns: [function-rename, variable-rename, assertion-fix]
key_files:
  created: []
  modified:
    - crates/polyplugc/tests/smoke.rs
    - crates/polyplugc/tests/interface_factories_tests.rs
    - crates/polyplugc/tests/generator_correctness.rs
    - crates/polyplugc/tests/integration_codegen_rust.rs
decisions:
  - Preserve ABI field names (vtable_version) in FFI types
  - Preserve SDK function names (store_host_vtable, get_host_vtable) as FFI imports
  - Update all test function names to interface terminology
  - Update all test variable names from vtable to interface
  - Fix assertion patterns to match actual generator output
metrics:
  duration: 28m
  tasks_completed: 3
  files_modified: 4
  commits: 3
  lines_changed: 264
  completed_date: 2026-04-09
---

# Phase 15 Plan 04b: Polyplugc Test Terminology Update Summary

## One-liner

Updated 4 polyplugc test files to use interface terminology, renaming functions, variables, and fixing assertions to match generator output, eliminating vtable naming across 260 lines.

## What Changed

### smoke.rs Updates

| Change Type | Old | New |
|-------------|-----|-----|
| Constant | TEST_ADDER_VTABLE | TEST_ADDER_INTERFACE |
| Import | crates/polyplug_guest | sdks/rust/guest |
| Type | ABI_ERROR_GENERIC (u32) | AbiErrorCode::Generic |
| Dispatch signature | fn(*const (), *mut ()) | fn(GuestContractInstance, *const (), *mut ()) |
| Comment | "compile of vtables.hpp" | "compile of interfaces.hpp" |
| Temp file | smoke_cpp_vtables.o | smoke_cpp_interfaces.o |

### interface_factories_tests.rs Updates

| Change Type | Old | New |
|-------------|-----|-----|
| Helper function | generate_host_vtable_factories | generate_host_interface_factories |
| Test functions | test_vtable_factory_* | test_interface_factory_* |
| Variables | vtables | interfaces |
| File reference | vtable_factories.rs | interface_factories.rs |

### generator_correctness.rs Updates

| Change Type | Old | New |
|-------------|-----|-----|
| Helper function | generate_guest_vtables | generate_guest_interfaces |
| Test functions | vtable_slots_are_sequential | interface_slots_are_sequential |
| Variables | vtables | interfaces |

### integration_codegen_rust.rs Updates

| Change Type | Old | New |
|-------------|-----|-----|
| Module doc | "through the vtable" | "through the interface" |
| Constant | TEST_ADDER_VTABLE | TEST_ADDER_INTERFACE |
| SAFETY comment | TEST_ADDER_VTABLE | TEST_ADDER_INTERFACE |

### Assertion Fixes (Deviation)

Fixed assertions to match actual generator output:

1. **contract_id**: Uses `HostContractId::from(0x...)` format (not `contract_id: 0x...`)
2. **contract_version**: Uses `Version { major: 1, minor: 0, patch: 0 }` struct (not separate `contract_major/minor` fields)
3. **function_count**: Only appears in NATIVE factory (VM uses VmDispatch without function_count)
4. **Panic error**: Uses `AbiErrorCode::Panic` (not `ABI_ERROR_PANIC` constant)

## Verification

- smoke_rust_codegen_dispatch: PASSED
- interface_factories_tests (10 tests): PASSED
- generator_correctness (11 tests): PASSED
- integration_codegen_rust (2 tests): PASSED
- smoke_cpp_codegen_dispatch: FAILED (pre-existing C++ SDK issue - abi.hpp syntax error)

## Commits

1. `df2ca87` - refactor(15-04b): update smoke.rs to interface terminology and fix ABI
2. `c7bc1a3` - refactor(15-04b): update interface_factories_tests.rs to interface terminology
3. `d10ecd7` - refactor(15-04b): update generator_correctness and integration_codegen_rust to interface terminology

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Wrong guest SDK path in smoke.rs**
- **Found during:** Task 1 execution
- **Issue:** Test referenced `crates/polyplug_guest` but actual path is `sdks/rust/guest`
- **Fix:** Updated path in smoke.rs
- **Files modified:** crates/polyplugc/tests/smoke.rs
- **Commit:** df2ca87

**2. [Rule 3 - Blocking] Outdated ABI type usage in smoke.rs**
- **Found during:** Task 1 execution
- **Issue:** Used `ABI_ERROR_GENERIC` (u32) instead of `AbiErrorCode::Generic`
- **Fix:** Updated to use AbiErrorCode enum
- **Files modified:** crates/polyplugc/tests/smoke.rs
- **Commit:** df2ca87

**3. [Rule 3 - Blocking] Outdated dispatch signature in smoke.rs**
- **Found during:** Task 1 execution
- **Issue:** Old signature `fn(*const (), *mut ())` missing GuestContractInstance parameter
- **Fix:** Updated to include GuestContractInstance as first parameter
- **Files modified:** crates/polyplugc/tests/smoke.rs
- **Commit:** df2ca87

**4. [Rule 1 - Bug] Incorrect assertion patterns in interface_factories_tests.rs**
- **Found during:** Task 2 verification
- **Issue:** Assertions expected old generator output format
- **Fix:** Updated assertions to match actual generator output
- **Files modified:** crates/polyplugc/tests/interface_factories_tests.rs
- **Commit:** c7bc1a3

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- All 4 modified test files exist: VERIFIED
- All 3 commits exist in git history: VERIFIED (df2ca87, c7bc1a3, d10ecd7)
- smoke_rust_codegen_dispatch test passes: VERIFIED
- interface_factories_tests passes (10 tests): VERIFIED
- generator_correctness passes (11 tests): VERIFIED
- integration_codegen_rust passes (2 tests): VERIFIED
- No vtable terminology remains in modified test files (except preserved ABI terms): VERIFIED