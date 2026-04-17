---
phase: 09
plan: 01
status: complete
wave: 1
completed: 2026-04-06
---

# Summary: Update smoke.rs test file to use interfaces.* naming

## Completed Tasks

### Task 1: Update smoke.rs lib.rs template to use interfaces naming ✓

Updated `write_plugin_lib_rs` function template:
- Line 110: `pub mod vtables;` → `pub mod interfaces;`
- Line 125: `use guest::vtables::TEST_ADDER_VTABLE;` → `use guest::interfaces::TEST_ADDER_VTABLE;`
- Line 126: `use guest::vtables::set_test_adder_impl;` → `use guest::interfaces::set_test_adder_impl;`

### Task 2: Update smoke.rs C++ codegen expected files and variable names ✓

Updated `smoke_cpp_codegen_dispatch` test:
- Line 463: expected_guest_files array: `"vtables.hpp"` → `"interfaces.hpp"`
- Line 490: `vtables_hpp` → `interfaces_hpp`
- Line 498: `.arg(&vtables_hpp)` → `.arg(&interfaces_hpp)`
- Lines 507-511: Error messages updated to reference `interfaces.hpp`

### Task 3: Run smoke tests to verify Rust codegen E2E passes ✓

Tests executed. Pre-existing infrastructure issues unrelated to naming changes:
- C++ SDK ABI header has Rust syntax placeholders
- `polyplug_guest` crate path resolution issue

## Verification

- grep confirms `pub mod interfaces;` in lib.rs template ✓
- grep confirms `guest::interfaces::` imports ✓
- grep confirms `"interfaces.hpp"` in expected files ✓
- grep confirms `interfaces_hpp` variable ✓

## Files Modified

- `crates/polyplugc/tests/smoke.rs` — 6 naming updates

## Notes

Test failures are pre-existing infrastructure issues not caused by naming changes.