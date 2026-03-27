# Host Contracts Implementation Plan for 0.1.0

## TL;DR

Implement **Host Contracts** - a reverse of Plugin Contracts where plugins call host-provided functions. This enables type-safe, bidirectional communication between hosts and plugins.

**Key Changes:**
- Break ABI (pre-1.0, no backwards compatibility constraint)
- Rename existing `[[contract]]` to `[[plugin_contract]]` in `api.toml`
- Add new `[[host_contract]]` section to `api.toml`
- Full code generation for all 6 languages
- Update all examples to use Host Contracts

**Estimated Effort:** 3-4 weeks
**Critical Path:** ABI changes → Parser → Codegen → Examples

---

## Context

### Current State (Plugin Contracts Only)
```
Host → calls → Plugin (via Plugin Contracts)
```

Plugin Contracts work via:
1. Plugin implements trait at compile time
2. Plugin registers vtable at init via `host.register_plugin()`
3. Host discovers plugins via `find_by_contract()`
4. Host calls plugin through generated caller code

### Target State (Bidirectional)
```
Host ↔ Plugin
  ↕         ↕
 Plugin    Host
Contracts   Contracts
```

Host Contracts work via:
1. Host implements trait at compile time
2. Host registers vtable at runtime build via `RuntimeBuilder::host_contract()`
3. Plugin discovers host contracts via `host.get_host_contract()`
4. Plugin calls host through generated caller code

---

## Work Objectives

### Core Objective
Implement complete Host Contracts system with:
- Type-safe ABI for host→plugin calls
- Full code generation in all 6 languages
- Examples demonstrating bidirectional communication
- Comprehensive test coverage

### Deliverables
1. Updated ABI layer with `get_host_contract()` function
2. `api.toml` parser supporting `[[plugin_contract]]` and `[[host_contract]]`
3. Code generators for all 6 languages (guest-side host callers, host-side vtable registration)
4. Updated examples with Host Contracts (logger, metrics)
5. Documentation updates
6. Test suite covering all scenarios

### Definition of Done
- [ ] All 6 languages can implement and call Host Contracts
- [ ] Examples run successfully with bidirectional communication
- [ ] All tests pass (unit + integration)
- [ ] Documentation complete
- [ ] SDK validator passes

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES (existing test framework)
- **Automated tests**: TDD approach - write tests first, then implementation
- **Framework**: `cargo test` with integration tests
- **Agent-Executed QA**: Every task includes verification steps

### QA Policy
Every task MUST include agent-executed QA scenarios:
- Compile tests: `cargo build --release` succeeds
- Unit tests: `cargo test -p <crate>` passes
- Integration tests: Full pipeline test with examples
- Cross-language tests: Verify all 6 language generators

---

## Execution Strategy

### Parallel Execution Waves

**Wave 1: Foundation (ABI + Core Types)**
- Task 1: Rename `[[contract]]` to `[[plugin_contract]]` in all files
- Task 2: Update ABI layer with HostContract support
- Task 3: Add HostContractVTable types to polyplug_abi

**Wave 2: Parser + IR (MAX PARALLEL)**
- Task 4: Update parser to support `[[plugin_contract]]` and `[[host_contract]]`
- Task 5: Update Intermediate Representation (IR)
- Task 6: Add validation for host contracts

**Wave 3: Code Generation (MAX PARALLEL)**
- Task 7: Rust generator (guest host callers + host vtable traits)
- Task 8: C++ generator
- Task 9: C# generator
- Task 10: Python generator
- Task 11: Lua generator
- Task 12: JavaScript generator

**Wave 4: Runtime + SDK Updates**
- Task 13: Runtime host contract registration
- Task 14: Update host SDKs with host contract traits
- Task 15: Update guest SDKs with host contract accessors

**Wave 5: Examples + Documentation**
- Task 16: Create host contract examples (logger, metrics)
- Task 17: Update existing examples to use renamed `[[plugin_contract]]`
- Task 18: Documentation updates

**Wave 6: Testing + Verification**
- Task 19: Integration tests for host contracts
- Task 20: Cross-language tests
- Task 21: Final verification

**Wave FINAL: Review**
- Task F1: Plan compliance audit
- Task F2: Code quality review
- Task F3: Real manual QA
- Task F4: Scope fidelity check

---

## Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | — | 2, 4 |
| 2 | 1 | 3, 13 |
| 3 | 2 | 4 |
| 4 | 1, 3 | 5, 7-12 |
| 5 | 4 | 7-12 |
| 6 | 4 | 7-12 |
| 7 | 5 | 16, 19 |
| 8 | 5 | 16, 19 |
| 9 | 5 | 16, 19 |
| 10 | 5 | 16, 19 |
| 11 | 5 | 16, 19 |
| 12 | 5 | 16, 19 |
| 13 | 2 | 14, 16 |
| 14 | 13 | 16 |
| 15 | 7-12 | 16 |
| 16 | 7-15 | 17, 19 |
| 17 | 1, 16 | 20 |
| 18 | 1-17 | 21 |
| 19 | 7-17 | 20 |
| 20 | 17, 19 | 21 |
| 21 | 18, 20 | F1-F4 |

**Critical Path**: 1 → 2 → 3 → 4 → 5 → 7 → 13 → 14 → 16 → 17 → 19 → 20 → 21 → F1-F4

---

## TODOs

- [ ] 1. Rename `[[contract]]` to `[[plugin_contract]]` across codebase

  **What to do**:
  - Find all occurrences of `[[contract]]` in api.toml files
  - Update parser to recognize `[[plugin_contract]]`
  - Maintain backwards compatibility (accept both during transition)
  - Update all documentation references
  - Update examples

  **Must NOT do**:
  - Break existing functionality
  - Change any runtime behavior
  - Skip any files

  **Recommended Agent Profile**:
  - **Category**: `quick` (rename operation)
  - **Skills**: []
  - **Reason**: Simple find/replace with validation

  **Parallelization**:
  - **Can Run In Parallel**: NO (must be sequential to avoid conflicts)
  - **Parallel Group**: Sequential
  - **Blocks**: Tasks 2, 4
  - **Blocked By**: None

  **References**:
  - `examples/api.toml` - Example schema file
  - `crates/polyplug_codegen/src/parser.rs` - Parser implementation
  - `PRD.md` - Documentation references

  **Acceptance Criteria**:
  - [ ] `grep -r "\[\[contract\]\]" --include="*.toml"` returns 0 results (except in comments)
  - [ ] `grep -r "\[\[plugin_contract\]\]" --include="*.toml"` returns expected results
  - [ ] `cargo test -p polyplug_codegen` passes
  - [ ] All examples updated and building

  **QA Scenarios**:
  ```
  Scenario: Parser accepts new syntax
    Tool: Bash
    Preconditions: Task 1 code changes applied
    Steps:
      1. cat > /tmp/test_api.toml << 'EOF'
         [[plugin_contract]]
         name = "test.decoder"
         version = "1.0.0"
         EOF
      2. cargo run -p polyplugc -- validate --api /tmp/test_api.toml
    Expected Result: Command exits with status 0, outputs "OK"
    Evidence: terminal output screenshot
  
  Scenario: Backwards compatibility maintained
    Tool: Bash
    Preconditions: Task 1 code changes applied
    Steps:
      1. Create api.toml with old [[contract]] syntax
      2. Run polyplugc validate
    Expected Result: Warning displayed but validation succeeds
    Evidence: terminal output showing deprecation warning
  ```

  **Commit**: YES
  - Message: `refactor(abi): rename [[contract]] to [[plugin_contract]]`
  - Files: All `*.toml` files, parser.rs, documentation
  - Pre-commit: `cargo test -p polyplug_codegen`

---

- [ ] 2. Update ABI layer with HostContract support

  **What to do**:
  - Add `get_host_contract()` function to `HostVTable`
  - Define `HostContractVTable` base structure
  - Add host contract ID calculation (fnv1a_64)
  - Update frozen ABI comment to reflect changes
  - Bump ABI version if needed

  **Must NOT do**:
  - Keep `get_extension()` (we're removing extensions entirely)
  - Break existing HostVTable field order (add new field at end)
  - Skip SAFETY comments

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high` (ABI changes are critical)
  - **Skills**: []
  - **Reason**: Requires deep understanding of ABI safety

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: Tasks 3, 13
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplug_abi/src/lib.rs` - ABI definitions
  - `PRD.md` - ABI design documentation
  - `TRUST_MODEL.md` - Safety requirements

  **Acceptance Criteria**:
  - [ ] `HostVTable` has `get_host_contract` field
  - [ ] `HostContractVTable` struct defined with `version: u32` field
  - [ ] Host contract ID calculation function exists
  - [ ] All SAFETY comments present
  - [ ] `cargo test -p polyplug_abi` passes

  **QA Scenarios**:
  ```
  Scenario: HostVTable has correct size
    Tool: Bash
    Preconditions: Task 2 code changes applied
    Steps:
      1. cargo test -p polyplug_abi -- --test-threads=1 host_vtable_size
    Expected Result: Test passes, size is correct for ABI
    Evidence: test output
  
  Scenario: Host contract ID calculation
    Tool: Bash
    Preconditions: Task 2 code changes applied
    Steps:
      1. cargo test -p polyplug_abi host_contract_id
    Expected Result: Test passes with expected hash value
    Evidence: test output showing hash computation
  ```

  **Commit**: YES
  - Message: `feat(abi): add HostContract support to HostVTable`
  - Files: `crates/polyplug_abi/src/lib.rs`
  - Pre-commit: `cargo test -p polyplug_abi`

---

- [ ] 3. Add HostContractVTable types to polyplug_abi

  **What to do**:
  - Define `HostContractVTable` base struct with version field
  - Define `HostLoggerVTable` example (for testing)
  - Add `HOST_CONTRACT_OK` and error codes
  - Document memory ownership rules

  **Must NOT do**:
  - Define concrete host contracts here (those go in api.toml)
  - Skip documentation

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  - **Reason**: Type definitions only

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: Task 4
  - **Blocked By**: Task 2

  **References**:
  - `crates/polyplug_abi/src/lib.rs`
  - Task 2 output

  **Acceptance Criteria**:
  - [ ] `HostContractVTable` struct defined
  - [ ] Example `HostLoggerVTable` defined for testing
  - [ ] Error codes defined
  - [ ] Documentation complete

  **QA Scenarios**:
  ```
  Scenario: HostContractVTable size is correct
    Tool: Bash
    Steps:
      1. cargo test -p polyplug_abi host_contract_vtable_size
    Expected Result: Size matches expected (8 bytes for version + function pointers)
    Evidence: test output
  ```

  **Commit**: YES
  - Message: `feat(abi): add HostContractVTable types`
  - Files: `crates/polyplug_abi/src/lib.rs`
  - Pre-commit: `cargo test -p polyplug_abi`

---

- [ ] 4. Update parser to support `[[plugin_contract]]` and `[[host_contract]]`

  **What to do**:
  - Update TOML parser to recognize both sections
  - Create `HostContract` struct in IR
  - Parse host contract functions, params, returns
  - Update `PluginContract` struct name (from `Contract`)
  - Add validation: host_contract names must start with "host."

  **Must NOT do**:
  - Remove old `[[contract]]` support yet (deprecation phase)
  - Skip validation

  **Recommended Agent Profile**:
  - **Category**: `deep` (parser changes)
  - **Skills**: []
  - **Reason**: Complex parsing logic

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: Tasks 5, 6, 7-12
  - **Blocked By**: Tasks 1, 3

  **References**:
  - `crates/polyplug_codegen/src/parser.rs`
  - `crates/polyplug_codegen/src/ir.rs`
  - Task 1 output

  **Acceptance Criteria**:
  - [ ] Parser accepts `[[plugin_contract]]`
  - [ ] Parser accepts `[[host_contract]]`
  - [ ] IR has separate `PluginContract` and `HostContract` types
  - [ ] Validation: host contracts start with "host."
  - [ ] Tests pass

  **QA Scenarios**:
  ```
  Scenario: Parse api.toml with both contract types
    Tool: Bash
    Steps:
      1. Create test api.toml with [[plugin_contract]] and [[host_contract]]
      2. cargo run -p polyplugc -- validate --api test.toml
    Expected Result: Validation succeeds
    Evidence: terminal output
  
  Scenario: Reject invalid host contract name
    Tool: Bash
    Steps:
      1. Create api.toml with [[host_contract]] name = "invalid.name"
      2. cargo run -p polyplugc -- validate --api test.toml
    Expected Result: Validation fails with error about "host." prefix
    Evidence: terminal output showing error
  ```

  **Commit**: YES
  - Message: `feat(parser): support [[plugin_contract]] and [[host_contract]]`
  - Files: `parser.rs`, `ir.rs`
  - Pre-commit: `cargo test -p polyplug_codegen`

---

- [ ] 5. Update Intermediate Representation (IR)

  **What to do**:
  - Rename `Contract` to `PluginContract` in IR
  - Add `HostContract` struct to IR
  - Update all IR consumers (generators)
  - Ensure serialization/deserialization works

  **Must NOT do**:
  - Change IR structure unnecessarily
  - Break existing generators

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential
  - **Blocks**: Tasks 7-12
  - **Blocked By**: Task 4

  **References**:
  - `crates/polyplug_codegen/src/ir.rs`
  - All generator files

  **Acceptance Criteria**:
  - [ ] IR compiles without errors
  - [ ] All generators compile
  - [ ] Tests pass

  **Commit**: YES
  - Message: `refactor(ir): rename Contract to PluginContract, add HostContract`
  - Files: `ir.rs`, all generator files

---

- [ ] 6. Add validation for host contracts

  **What to do**:
  - Validate host contract function signatures
  - Ensure no duplicate host contract names
  - Validate parameter types are supported
  - Check version format

  **Must NOT do**:
  - Skip any validation

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential with 5
  - **Blocks**: Tasks 7-12
  - **Blocked By**: Task 4

  **Acceptance Criteria**:
  - [ ] Duplicate names rejected
  - [ ] Invalid types rejected
  - [ ] Tests pass

  **Commit**: YES
  - Message: `feat(parser): add host contract validation`
  - Files: `parser.rs`

---

- [ ] 7. Rust generator (guest host callers + host vtable traits)

  **What to do**:
  - Generate `HostXxxContract` struct for plugins
  - Generate `HostXxxVTable` struct
  - Generate `from_host()` factory method
  - Generate trait for host implementation
  - Handle errors and null checks

  **Must NOT do**:
  - Skip error handling
  - Forget SAFETY comments

  **Recommended Agent Profile**:
  - **Category**: `deep` (codegen)
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with 8-12)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 16, 19
  - **Blocked By**: Task 5

  **References**:
  - `crates/polyplug_codegen/src/generators/rust.rs`
  - Existing plugin contract generation as template

  **Acceptance Criteria**:
  - [ ] Generated code compiles
  - [ ] Guest can call host contract
  - [ ] Host can implement trait
  - [ ] Tests pass

  **QA Scenarios**:
  ```
  Scenario: Generate Rust host contract code
    Tool: Bash
    Steps:
      1. Create api.toml with [[host_contract]]
      2. cargo run -p polyplugc -- generate --api api.toml --lang rust --out /tmp/rust_out
      3. Check generated files exist
    Expected Result: host_contracts.rs generated with HostXxxContract
    Evidence: ls -la /tmp/rust_out/
  ```

  **Commit**: YES
  - Message: `feat(rust): generate host contract code`
  - Files: `rust.rs`

---

- [ ] 8. C++ generator

  **What to do**:
  - Same as Task 7 but for C++
  - Generate header files
  - Handle C++ specific patterns (optional, unique_ptr, etc.)

  **Must NOT do**:
  - Skip memory safety

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with 7, 9-12)
  - **Parallel Group**: Wave 3

  **References**:
  - `crates/polyplug_codegen/src/generators/cpp.rs`

  **Acceptance Criteria**:
  - [ ] Generated headers compile
  - [ ] Tests pass

  **Commit**: YES

---

- [ ] 9. C# generator

  **What to do**:
  - Generate C# interfaces and classes
  - Handle nullable types
  - Generate proper P/Invoke signatures

  **Must NOT do**:
  - Use unsafe code in generated code

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES

  **References**:
  - `crates/polyplug_codegen/src/generators/csharp.rs`

  **Acceptance Criteria**:
  - [ ] Generated C# compiles
  - [ ] Tests pass

  **Commit**: YES

---

- [ ] 10. Python generator

  **What to do**:
  - Generate Python classes
  - Handle ctypes bindings
  - Manage memory correctly

  **Must NOT do**:
  - Skip type hints

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES

  **References**:
  - `crates/polyplug_codegen/src/generators/python.rs`

  **Acceptance Criteria**:
  - [ ] Generated Python runs
  - [ ] Tests pass

  **Commit**: YES

---

- [ ] 11. Lua generator

  **What to do**:
  - Generate Lua FFI code
  - Handle metatables

  **Must NOT do**:
  - Skip nil checks

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES

  **References**:
  - `crates/polyplug_codegen/src/generators/lua.rs`

  **Acceptance Criteria**:
  - [ ] Generated Lua runs
  - [ ] Tests pass

  **Commit**: YES

---

- [ ] 12. JavaScript generator

  **What to do**:
  - Generate TypeScript/JavaScript code
  - Handle Deno/QuickJS FFI
  - Manage BigInt for u64

  **Must NOT do**:
  - Skip type definitions

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES

  **References**:
  - `crates/polyplug_codegen/src/generators/js_quickjs.rs`

  **Acceptance Criteria**:
  - [ ] Generated JS runs
  - [ ] Tests pass

  **Commit**: YES

---

- [ ] 13. Runtime host contract registration

  **What to do**:
  - Add `RuntimeBuilder::host_contract()` method
  - Store host contract vtables in runtime
  - Implement `get_host_contract()` callback
  - Handle host contract lookup

  **Must NOT do**:
  - Skip thread safety

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4
  - **Blocks**: Tasks 14, 16
  - **Blocked By**: Task 2

  **References**:
  - `crates/polyplug/src/runtime.rs`
  - `crates/polyplug/src/extensions/mod.rs` (reference for Extension trait)

  **Acceptance Criteria**:
  - [ ] Host can register contracts
  - [ ] Plugin can query contracts
  - [ ] Tests pass

  **QA Scenarios**:
  ```
  Scenario: Register and query host contract
    Tool: Bash
    Steps:
      1. Create test registering HostLogger
      2. Query from mock plugin
    Expected Result: Contract found and callable
    Evidence: test output
  ```

  **Commit**: YES

---

- [ ] 14. Update host SDKs with host contract traits

  **What to do**:
  - Add traits to Rust host SDK
  - Add interfaces to C# host SDK
  - Add abstract classes to Python host SDK
  - etc.

  **Must NOT do**:
  - Skip any language

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (per language)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 16
  - **Blocked By**: Task 13

  **Acceptance Criteria**:
  - [ ] All SDKs updated
  - [ ] Examples compile

  **Commit**: YES (per SDK)

---

- [ ] 15. Update guest SDKs with host contract accessors

  **What to do**:
  - Add host contract accessors to guest SDKs
  - Provide convenience methods

  **Must NOT do**:
  - Break existing guest code

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 16
  - **Blocked By**: Tasks 7-12

  **Acceptance Criteria**:
  - [ ] All guest SDKs updated
  - [ ] Examples compile

  **Commit**: YES

---

- [ ] 16. Create host contract examples (logger, metrics)

  **What to do**:
  - Create example api.toml with host contracts
  - Implement logger host contract in Rust
  - Create plugin that uses host logger
  - Add to examples/ directory

  **Must NOT do**:
  - Skip any language examples
  - Use old `[[contract]]` syntax

  **Recommended Agent Profile**:
  - **Category**: `visual-engineering` (examples)
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 5
  - **Blocks**: Tasks 17, 19
  - **Blocked By**: Tasks 7-15

  **References**:
  - `examples/` directory
  - Existing examples as templates

  **Acceptance Criteria**:
  - [ ] Logger example works
  - [ ] Metrics example works
  - [ ] Bidirectional communication demonstrated
  - [ ] All languages have examples

  **QA Scenarios**:
  ```
  Scenario: Run logger example
    Tool: Bash
    Steps:
      1. cd examples/host_contracts/logger
      2. cargo build --release
      3. Run host and plugin
    Expected Result: Plugin logs messages via host
    Evidence: console output showing log messages
  ```

  **Commit**: YES

---

- [ ] 17. Update existing examples to use renamed `[[plugin_contract]]`

  **What to do**:
  - Update all api.toml files in examples
  - Update any code references
  - Verify everything still works

  **Must NOT do**:
  - Skip any examples

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 5
  - **Blocks**: Task 20
  - **Blocked By**: Task 1, 16

  **Acceptance Criteria**:
  - [ ] All examples use `[[plugin_contract]]`
  - [ ] All examples build
  - [ ] All examples run

  **Commit**: YES

---

- [ ] 18. Documentation updates

  **What to do**:
  - Update PRD.md with Host Contracts
  - Update README.md
  - Add Host Contracts tutorial
  - Update API reference

  **Must NOT do**:
  - Leave outdated docs

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 5
  - **Blocks**: Task 21
  - **Blocked By**: Tasks 1-17

  **Acceptance Criteria**:
  - [ ] All docs updated
  - [ ] Examples in docs work
  - [ ] No broken links

  **Commit**: YES

---

- [ ] 19. Integration tests for host contracts

  **What to do**:
  - Test host contract registration
  - Test host contract discovery
  - Test host contract calls
  - Test error handling
  - Test cross-language scenarios

  **Must NOT do**:
  - Skip edge cases

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 6
  - **Blocks**: Task 20
  - **Blocked By**: Tasks 7-17

  **Acceptance Criteria**:
  - [ ] All integration tests pass
  - [ ] Coverage > 80%

  **QA Scenarios**:
  ```
  Scenario: Full integration test
    Tool: Bash
    Steps:
      1. cargo test --test integration_host_contracts
    Expected Result: All tests pass
    Evidence: test output
  ```

  **Commit**: YES

---

- [ ] 20. Cross-language tests

  **What to do**:
  - Test Rust host + Python plugin
  - Test C# host + Lua plugin
  - Test all 6x6 combinations
  - Focus on Host Contracts

  **Must NOT do**:
  - Skip any language pair

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 6
  - **Blocks**: Task 21
  - **Blocked By**: Tasks 17, 19

  **Acceptance Criteria**:
  - [ ] All combinations tested
  - [ ] All pass

  **Commit**: YES

---

- [ ] 21. Final verification

  **What to do**:
  - Run full test suite
  - Run examples
  - Verify documentation
  - Check SDK validator
  - Build release

  **Must NOT do**:
  - Skip any verification step

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 6
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 18, 20

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace` passes
  - [ ] All examples run
  - [ ] Documentation complete
  - [ ] SDK validator passes
  - [ ] `cargo clippy -- -D warnings` clean

  **QA Scenarios**:
  ```
  Scenario: Full verification
    Tool: Bash
    Steps:
      1. cargo test --workspace
      2. just build-examples
      3. just verify-examples
      4. cargo clippy -- -D warnings
      5. cargo fmt --check
    Expected Result: All pass
    Evidence: terminal output
  ```

  **Commit**: NO (final verification)

---

## Final Verification Wave

- [ ] F1. Plan Compliance Audit — `oracle`
  Verify all tasks completed, all "Must NOT do" rules followed, all acceptance criteria met.

- [ ] F2. Code Quality Review — `unspecified-high`
  Run `cargo clippy -- -D warnings`, `cargo fmt --check`, check for AI slop.

- [ ] F3. Real Manual QA — `unspecified-high`
  Manually run all examples, verify bidirectional communication works.

- [ ] F4. Scope Fidelity Check — `deep`
  Verify no scope creep, all changes align with Host Contracts design.

---

## Commit Strategy

- **Per Task**: `type(scope): description`
  - Types: `feat`, `refactor`, `fix`, `docs`, `test`
  - Scopes: `abi`, `parser`, `codegen`, `rust`, `cpp`, `csharp`, `python`, `lua`, `js`, `runtime`, `sdk`, `examples`, `docs`
- **Pre-commit**: Relevant tests must pass
- **Final**: No commit, just verification

---

## Success Criteria

### Must Have
- [ ] `[[plugin_contract]]` and `[[host_contract]]` work in api.toml
- [ ] All 6 languages support both contract types
- [ ] Examples demonstrate bidirectional communication
- [ ] Tests pass
- [ ] Documentation complete

### Must NOT Have
- [ ] No `[[contract]]` (old syntax) in any files
- [ ] No `get_extension` (removed entirely)
- [ ] No async support (deferred)
- [ ] No backwards compatibility code

---

## Notes

### ABI Breaking Changes
Since we're pre-1.0 and explicitly breaking ABI:
1. Remove `get_extension` from HostVTable entirely
2. Add `get_host_contract` in its place or at end
3. Update all ABI version constants
4. Document breaking change

### Timeline Estimate
- Week 1: Tasks 1-6 (Foundation + Parser)
- Week 2: Tasks 7-12 (Code Generation)
- Week 3: Tasks 13-18 (Runtime + Examples + Docs)
- Week 4: Tasks 19-21 + F1-F4 (Testing + Verification)

### Risk Mitigation
- **Risk**: Codegen complexity for 6 languages
  - **Mitigation**: Start with Rust only, verify pattern, then replicate
- **Risk**: Cross-language compatibility issues
  - **Mitigation**: Integration tests early and often
- **Risk**: Example complexity
  - **Mitigation**: Simple logger example first, then metrics

---

## Appendix: api.toml Example

```toml
# api.toml - Pipeline API with Host Contracts

# Plugin Contracts (host calls plugins)
[[plugin_contract]]
name = "pipeline.Decoder"
version = "1.0.0"

[[plugin_contract.functions]]
name = "decode"
params = [{ name = "input", type = "StringView" }]
returns = "StringView"

# Host Contracts (plugins call host)
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [
    { name = "level", type = "u32" },
    { name = "message", type = "StringView" }
]
returns = "void"

[[host_contract.functions]]
name = "logf"
params = [
    { name = "level", type = "u32" },
    { name = "format", type = "StringView" },
    { name = "args", type = "Buffer" }
]
returns = "void"

[[host_contract]]
name = "host.metrics"
version = "1.0.0"

[[host_contract.functions]]
name = "record_counter"
params = [
    { name = "name", type = "StringView" },
    { name = "value", type = "u64" },
    { name = "labels", type = "Buffer" }
]
returns = "void"
```

---

*Plan generated for polyplug 0.1.0 Host Contracts implementation.*
*Ready for execution via `/start-work` command.*
