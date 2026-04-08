---
phase: 13-cpp-codegen-modernization
plan: 02
type: execute
wave: 2
depends_on: [13-01]
tags: [codegen, cpp, integration-test, naming, verification]
requirements: [CG-02, CG-03, CG-04, CG-05, INST-01, INST-02, INST-03, INST-04, INST-05, INST-06, D-08, D-09]
---

# Phase 13 Plan 02: Integration test for C++ codegen naming modernization

**One-liner:** Created integration test file verifying C++ codegen produces correct HostContractInterface/_INTERFACE naming and guest contract instance wrapper pattern.

## Summary

This plan created the `integration_codegen_cpp.rs` test file that validates the naming modernization from Plan 01. The tests verify:

1. Generated C++ files exist for both guest and host sides
2. `_INTERFACE` suffix is used (not legacy `_VTABLE`)
3. `GuestContractInterface` and `HostContractInterface` naming
4. Guest contract instance wrapper pattern (create_instance/destroy_instance lifecycle)
5. Factory functions use inline HostContractInterface fields (no HostContractVTableHeader)
6. No legacy `PluginVTable` naming in generated code

Additionally, a blocking issue was fixed: missing `resolve_host_contract_interface` field in `integration_codegen_rust.rs` and `smoke.rs` test files (added in phase 13-01 but not updated in tests).

## Tasks Completed

| Task | Name | Commit | Files Modified |
|------|------|--------|----------------|
| 1 | Create integration_codegen_cpp.rs test file | 57db138 | integration_codegen_cpp.rs (397 lines) |
| 2 | Run sdk_validator and verify C++ SDK consistency | a0518c9 | justfile (package name fix) |

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Missing resolve_host_contract_interface field in test files**
- **Found during:** Task 1 - running tests
- **Issue:** `integration_codegen_rust.rs` and `smoke.rs` had HostInterface structs missing the new `resolve_host_contract_interface` field added in phase 13-01
- **Fix:** Added stub function and field to both test files
- **Files modified:** integration_codegen_rust.rs, smoke.rs
- **Commit:** 67605d3

**2. [Rule 3 - Blocking] Wrong sdk_validator package name in justfile**
- **Found during:** Task 2 - running just validate-sdks
- **Issue:** justfile referenced `sdk_validator` but actual package name is `sdk-validator` (with hyphen)
- **Fix:** Changed package name in justfile to `sdk-validator`
- **Files modified:** justfile
- **Commit:** a0518c9

### Deferred Issues

**C++ SDK abi.hpp contains invalid placeholder syntax** (see deferred-items.md)
- Pre-existing stub functions use Rust-like syntax (`&[u8]`, `&str`) instead of valid C++
- Causes smoke_cpp_codegen_dispatch test to fail when compiling generated code
- Out of scope - not caused by this plan's changes

## Key Changes

### Integration Test File (Task 1)

Created 7 test functions in `integration_codegen_cpp.rs`:

| Test | Purpose |
|------|---------|
| `test_generate_cpp_guest_files_exist` | Verifies guest-side files: types.hpp, contracts.hpp, interfaces.hpp, init.hpp |
| `test_generate_cpp_host_files_exist` | Verifies host-side files: types.hpp, host_callers.hpp, manifest.toml |
| `test_cpp_codegen_uses_interface_naming` | Checks `_INTERFACE` suffix, no `_VTABLE`, `GuestContractInterface` |
| `test_cpp_codegen_host_contract_uses_interface` | Checks `HostContractInterface`, `interface_` member (not `vtable_`) |
| `test_cpp_codegen_guest_instance_wrapper_exists` | Checks create_instance/destroy_instance stubs, instance lifecycle |
| `test_cpp_codegen_factory_uses_inline_fields` | Checks inline HostContractInterface fields, no HostContractVTableHeader |
| `test_cpp_codegen_no_legacy_vtable_naming` | Checks no PluginVTable or static HostVTable in generated code |

### SDK Validation (Task 2)

C++ SDK validation passed (all 5 StringView helper methods present):
```
StringView Methods:
  Method       | rust | python | csharp | cpp | js | lua |
  -------------|------|--------|--------|-----|----|-----|
  ends_with    |   X  |   Y    |   Y    |  Y  | Y  |  Y  |
  split        |   X  |   Y    |   Y    |  Y  | Y  |  Y  |
  starts_with  |   X  |   Y    |   Y    |  Y  | Y  |  Y  |
  strip_prefix |   X  |   Y    |   Y    |  Y  | Y  |  Y  |
  to_str       |   X  |   Y    |   Y    |  Y  | Y  |  Y  |

Summary: 25/30 method implementations found (83.3%)
```

## Verification Results

| Check | Result | Evidence |
|-------|--------|----------|
| cargo test -p polyplugc --test integration_codegen_cpp | PASSED | 7 tests passed |
| just validate-sdks (C++ column) | PASSED | All 5 methods checked |
| integration_codegen_cpp.rs exists | VERIFIED | 397 lines, 7 test functions |
| Test contains assertions for instance wrapper | VERIFIED | create_instance, destroy_instance, instance_, interface_ |

## Files Modified

| File | Changes |
|------|---------|
| crates/polyplugc/tests/integration_codegen_cpp.rs | NEW - 397 lines, 7 integration tests |
| crates/polyplugc/tests/integration_codegen_rust.rs | Added missing resolve_host_contract_interface stub |
| crates/polyplugc/tests/smoke.rs | Added missing resolve_host_contract_interface stub |
| justfile | Fixed sdk-validator package name |
| .planning/phases/13-cpp-codegen-modernization/deferred-items.md | NEW - documented pre-existing C++ SDK issue |

## Metrics

- **Duration:** ~15 minutes
- **Commits:** 3
- **Files Modified:** 4
- **Tests Created:** 7
- **Tests Passed:** 7

## Requirements Addressed

| Requirement | Status | Evidence |
|-------------|--------|----------|
| CG-02 | VERIFIED | test_cpp_codegen_guest_instance_wrapper_exists checks create_instance call |
| CG-03 | VERIFIED | test_cpp_codegen_guest_instance_wrapper_exists checks destroy_instance call |
| CG-04 | VERIFIED | test_cpp_codegen_guest_instance_wrapper_exists checks instance_ member and dispatch passing instance |
| CG-05 | VERIFIED | test_cpp_codegen_factory_uses_inline_fields checks HostContractInterface and no HostContractVTableHeader |
| INST-01 through INST-06 | VERIFIED | All instance model tests pass |
| D-08 | SATISFIED | integration_codegen_cpp.rs created with all specified tests |
| D-09 | SATISFIED | All verification tests in D-09 implemented and passing |

## Self-Check: PASSED

- [x] integration_codegen_cpp.rs exists with 397 lines
- [x] 7 test functions present
- [x] cargo test -p polyplugc --test integration_codegen_cpp passes (7 tests)
- [x] sdk_validator passes for C++ SDK (all 5 methods)
- [x] Test contains `_INTERFACE` assertion
- [x] Test contains `HostContractInterface` assertion
- [x] Test contains `create_instance` assertion
- [x] Test contains `destroy_instance` assertion
- [x] Test contains `instance_` member assertion

---
*Summary created: 2026-04-08*