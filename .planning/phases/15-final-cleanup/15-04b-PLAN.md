---
phase: 15-final-cleanup
plan: 04b
type: execute
wave: 3
depends_on: [15-02]
files_modified:
  - crates/polyplugc/tests/smoke.rs
  - crates/polyplugc/tests/interface_factories_tests.rs
  - crates/polyplugc/tests/generator_correctness.rs
  - crates/polyplugc/tests/integration_codegen_rust.rs
  - crates/polyplugc/tests/integration_codegen_cpp.rs
autonomous: true
requirements: [CLN-01, CLN-04]
user_setup: []
must_haves:
  truths:
    - "polyplugc test files use interface terminology"
    - "Test function names renamed appropriately"
    - "Static test constants renamed"
    - "All polyplugc tests pass"
  artifacts:
    - path: "crates/polyplugc/tests/*.rs"
      provides: "Generator and codegen tests"
      contains: "INTERFACE terminology"
  key_links:
    - from: "polyplugc tests"
      to: "generator output assertions"
      via: "test assertions"
      pattern: "TEST_ADDER_INTERFACE"
---

<objective>
Update all polyplugc test files to use interface terminology. These tests verify generator output and contain references to vtable in variable names, test function names, and assertion messages.

Purpose: Ensure polyplugc test suite uses consistent interface terminology to match generator changes.
Output: 5 polyplugc test files updated, all tests passing.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/phases/15-final-cleanup/15-RESEARCH.md

<interfaces>
<!-- From grep audit - polyplugc test file patterns -->

Key patterns to replace:
- `TEST_ADDER_VTABLE` constant → `TEST_ADDER_INTERFACE`
- `vtable_factories.rs` file references → `interface_factories.rs`
- `create_host_logger_vtable` → `create_host_logger_interface`
- Test function names: `test_vtable_factory_*` → `test_interface_factory_*`
- Comments: "vtable factory" → "interface factory"
- Variable names: `vtables` → `interfaces`
- Error messages in assertions

Files with occurrences:
- smoke.rs: 5 occurrences (TEST_ADDER_VTABLE constant, SAFETY comment)
- interface_factories_tests.rs: 104 occurrences (test names, assertions)
- generator_correctness.rs: 44 occurrences (comments, assertions)
- integration_codegen_rust.rs: 5 occurrences (comments)
- integration_codegen_cpp.rs: 10 occurrences (comments, assertions)
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Update smoke.rs - TEST_ADDER_VTABLE constant and references</name>
  <files>crates/polyplugc/tests/smoke.rs</files>
  <read_first>
    - crates/polyplugc/tests/smoke.rs (current state)
    - crates/polyplugc/src/generators/rust.rs (check what constant name is generated)
  </read_first>
  <behavior>
    - Constant `TEST_ADDER_VTABLE` renamed to `TEST_ADDER_INTERFACE`
    - SAFETY comment at line 172: "TEST_ADDER_VTABLE" → "TEST_ADDER_INTERFACE"
    - Reference at line 177: "&TEST_ADDER_VTABLE" → "&TEST_ADDER_INTERFACE"
    - Import at line 126: updated to use new constant name
    - Test passes after rename
  </behavior>
  <action>
    1. Read smoke.rs to see current state (lines 120-180 for the vtable references).
    2. Read rust.rs generator to verify the generated constant name pattern.
    3. Update line 126 import:
       - `use guest::interfaces::TEST_ADDER_VTABLE;` → `use guest::interfaces::TEST_ADDER_INTERFACE;`
       - Also update `set_test_adder_impl` if it has vtable references
    4. Update SAFETY comment at line 172:
       - "desc and TEST_ADDER_VTABLE are 'static" → "desc and TEST_ADDER_INTERFACE are 'static"
    5. Update reference at line 177:
       - `&TEST_ADDER_VTABLE as *const _` → `&TEST_ADDER_INTERFACE as *const _`
    6. Run smoke test to verify:
       ```bash
       cargo test -p polyplugc smoke -q
       ```
    Note: This requires the generator (plan 15-01) to produce `TEST_ADDER_INTERFACE` instead of `TEST_ADDER_VTABLE`. The generated constant name is defined in the generator's string templates.
  </action>
  <verify>
    <automated>grep -n "TEST_ADDER_VTABLE" crates/polyplugc/tests/smoke.rs | wc -l</automated>
    Expected: 0
  </verify>
  <done>smoke.rs uses TEST_ADDER_INTERFACE constant. Test passes with generator changes.</done>
</task>

<task type="auto">
  <name>Task 2: Update interface_factories_tests.rs - test names and assertions</name>
  <files>crates/polyplugc/tests/interface_factories_tests.rs</files>
  <read_first>
    - crates/polyplugc/tests/interface_factories_tests.rs (current state - 104 occurrences)
  </read_first>
  <behavior>
    - Module doc comment at line 1: "vtable factory generation" → "interface factory generation"
    - All test function names: `test_vtable_factory_*` → `test_interface_factory_*`
    - Variable names: `vtables` → `interfaces`, `vtable` → `interface`
    - Assertions: `create_host_logger_vtable` → `create_host_logger_interface`
    - Comments: "vtable" → "interface" (except when referring to generated file names)
    - Generated file reference `vtable_factories.rs` → `interface_factories.rs` (if generator changed)
  </behavior>
  <action>
    1. Read interface_factories_tests.rs to see current state.
    
    2. Update module doc comment (line 1):
       - "Tests for host-side vtable factory generation" → "Tests for host-side interface factory generation"
       - Line 4: "Vtable header" → "Interface header"
    
    3. Rename test functions (lines 91, 136, 164, 201, 250, 266, 282, 304):
       - `test_vtable_factory_generates_native_and_vm_factories` → `test_interface_factory_generates_native_and_vm_factories`
       - `test_vtable_factory_header_has_correct_contract_id` → `test_interface_factory_header_has_correct_contract_id`
       - `test_vtable_factory_header_has_correct_function_count` → `test_interface_factory_header_has_correct_function_count`
       - `test_vtable_factory_thunks_have_panic_safety` → `test_interface_factory_thunks_have_panic_safety`
       - `test_vtable_factory_native_dispatch_type` → `test_interface_factory_native_dispatch_type`
       - `test_vtable_factory_vm_dispatch_type` → `test_interface_factory_vm_dispatch_type`
       - `test_vtable_factory_native_leaks_vtable` → `test_interface_factory_native_leaks_interface`
       - `test_vtable_factory_vm_leaks_vtable` → `test_interface_factory_vm_leaks_interface`
    
    4. Update variable names and comments:
       - Line 54: `generate_host_vtable_factories` → `generate_host_interface_factories`
       - Line 77-78: `vtable_factories.rs` → `interface_factories.rs` (if generator produces this)
       - Lines 94, 139, 167, 204, 253, 269, 285, 307: `vtables:` → `interfaces:`
       - Lines 98-105: Assertions about `create_host_logger_vtable` → `create_host_logger_interface`
       - Lines 148, 176: "NATIVE vtable must" → "NATIVE interface must"
       - Lines 289-290: `Box::leak(Box::new(vtable))` → `Box::leak(Box::new(interface))`
       - Line 296: "NATIVE factory must leak implementation" (no change needed for this one)
    
    5. Run tests:
       ```bash
       cargo test -p polyplugc interface_factories -q
       ```
    
    Note: Assertions about generated code content depend on generator changes from plan 15-01.
  </action>
  <verify>
    <automated>grep -n "vtable_factory\|vtables:" crates/polyplugc/tests/interface_factories_tests.rs | wc -l</automated>
    Expected: 0
  </verify>
  <done>interface_factories_tests.rs uses interface terminology. Tests pass with generator changes.</done>
</task>

<task type="auto">
  <name>Task 3: Update generator_correctness.rs and integration_codegen files</name>
  <files>crates/polyplugc/tests/generator_correctness.rs, crates/polyplugc/tests/integration_codegen_rust.rs, crates/polyplugc/tests/integration_codegen_cpp.rs</files>
  <read_first>
    - crates/polyplugc/tests/generator_correctness.rs (current state - 44 occurrences)
    - crates/polyplugc/tests/integration_codegen_rust.rs (current state - 5 occurrences)
    - crates/polyplugc/tests/integration_codegen_cpp.rs (current state - 10 occurrences)
  </read_first>
  <behavior>
    - generator_correctness.rs: Comments updated, assertions about generated output
    - integration_codegen_rust.rs: Comments updated
    - integration_codegen_cpp.rs: Comments and assertions updated
    - All tests pass
  </behavior>
  <action>
    1. Read generator_correctness.rs:
       - Update comments referencing "vtable" → "interface"
       - Update assertions about generated code content (depends on generator output)
    
    2. Read integration_codegen_rust.rs:
       - Line 5: Comment "vtable when TEST_PLUGIN_SO" → "interface when..."
       - Update any other comments
    
    3. Read integration_codegen_cpp.rs:
       - Line 5: Comment "C++ test plugin vtable" → "C++ test plugin interface"
       - Update any assertions about `_VTABLE` suffix → `_INTERFACE` suffix
       - Note: This file was partially addressed in plan 15-04 but is in polyplugc crate
    
    4. Run polyplugc tests to verify:
       ```bash
       cargo test -p polyplugc -q
       ```
  </action>
  <verify>
    <automated>grep -rn "vtable" crates/polyplugc/tests/generator_correctness.rs crates/polyplugc/tests/integration_codegen_rust.rs crates/polyplugc/tests/integration_codegen_cpp.rs | grep -v "vtable_version" | wc -l</automated>
    Expected: 0
  </verify>
  <done>generator_correctness.rs and integration_codegen files updated. polyplugc tests pass.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Test assertions → Generator output | Assertions must match generator output after plan 15-01 changes |
| polyplugc tests → Generated code | Test expectations depend on generator string templates |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-15-04b-01 | Tampering | Test assertions | mitigate | Assertions depend on generator output; run tests after plan 15-01 |
| T-15-04b-02 | Denial | Test failures | mitigate | Coordinate with generator changes from plan 15-01 |
</threat_model>

<verification>
Run `cargo test -p polyplugc -q` to verify all polyplugc tests pass after changes.
Note: Tests depend on generator output from plan 15-01. Run in wave order.
</verification>

<success_criteria>
- All 5 polyplugc test files use interface terminology
- TEST_ADDER_VTABLE constant renamed to TEST_ADDER_INTERFACE
- Test function names renamed: test_vtable_factory_* → test_interface_factory_*
- Variable names renamed: vtables → interfaces
- Comments and assertions updated
- All polyplugc tests pass (after generator changes)
</success_criteria>

<output>
After completion, create `.planning/phases/15-final-cleanup/15-04b-SUMMARY.md`
</output>