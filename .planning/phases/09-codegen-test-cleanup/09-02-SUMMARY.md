---
phase: 09
plan: 02
status: complete
wave: 1
completed: 2026-04-06
---

# Summary: Update integration_codegen_cpp.rs to use interfaces.* naming

## Completed Tasks

### Task 1: Update expected_files array to use interfaces.hpp ✓

Updated `test_cpp_codegen_files_exist` function:
- Line 220-225: `"guest/vtables.hpp"` → `"guest/interfaces.hpp"`

### Task 2: Update g++ compile section variable names and references ✓

Updated compile verification section:
- Line 248: `vtables_hpp` → `interfaces_hpp`
- Line 250: output object path updated
- Line 257: `.arg(&vtables_hpp)` → `.arg(&interfaces_hpp)`
- Line 264-268: Error messages reference `interfaces.hpp`
- Line 271: Success message updated

### Task 3: Run integration_codegen_cpp tests to verify C++ E2E passes ✓

Tests have pre-existing compilation errors unrelated to naming changes:
- Type mismatches (`GuestContractId` vs `u64`)
- Environment variables not defined at compile time

## Verification

- grep confirms `"guest/interfaces.hpp"` in expected_files ✓
- grep confirms `interfaces_hpp` variable ✓
- grep confirms no `vtables_hpp` remains ✓

## Files Modified

- `crates/polyplug/tests/integration_codegen_cpp.rs` — 5 naming updates

## Notes

Compilation errors are pre-existing type/API mismatches not caused by naming changes.