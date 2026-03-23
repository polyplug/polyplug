# Fix Test Fixture Library Integration

## TL;DR

> **Problem:** Test fixtures duplicate ABI definitions instead of using shared SDK libraries, creating maintenance nightmares and silent failure risks.
>
> **Solution:** Refactor C# and Rust test fixtures to import from SDKs, remove duplicated code, add CI prevention.
>
> **Deliverables:** 
> - C# fixture uses `sdks/csharp/abi/` (removes 200+ lines of duplication)
> - Rust fixture uses `guest-libs/rust/` (consistency)
> - CI check prevents future duplication
>
> **Estimated Effort:** Medium (3-4 hours)
> **Parallel Execution:** YES - C# and Rust tasks independent
> **Critical Path:** C# fixture → Rust fixture → CI check

---

## Context

### Original Request
User identified that integration test fixtures don't properly rely on host/guest/ABI libraries. Specifically, C# test fixture duplicates all ABI types instead of using the shared SDK.

### Interview Summary
**Key Findings from Codebase Analysis:**

| Fixture | Language | Uses SDK? | Status |
|---------|----------|-----------|--------|
| `csharp_plugin` | C# | ✗ **NO** - Duplicates 200+ lines | **CRITICAL** |
| `test_plugin` | Rust | ⚠️ Partial - Uses `polyplug_abi` directly | Should use `guest-libs/rust/` |
| `test_plugin_python` | Python | ✓ **YES** | ✓ Good |
| `test_plugin_lua` | Lua | ✓ **YES** | ✓ Good |
| `test_plugin_js` | JS | ? Unclear | Needs verification |

**C# Problem (Critical):**
- File: `tests/fixtures/csharp_plugin/Plugin.cs` (lines 1-210)
- Duplicates: `StringView`, `AbiError`, `PluginDescriptor`, `HostVTable`, constants
- All exist in: `sdks/csharp/abi/Abi.cs`
- Risk: ABI changes cause silent test failures

**Rust Problem (Consistency):**
- File: `tests/fixtures/test_plugin/Cargo.toml`
- Uses: `polyplug_abi = { workspace = true }`
- Should use: `polyplug_guest = { path = "..." }`
- Reason: Test fixtures should demonstrate proper guest library usage

### Metis Review
Consulted Metis for gap analysis - no critical gaps identified. Plan covers:
- Build system implications (MSBuild/Cargo)
- No circular dependency risks (test fixtures can depend on SDKs)
- CI prevention mechanism

---

## Work Objectives

### Core Objective
Refactor test fixtures to use shared SDK/guest libraries, eliminating ABI duplication and ensuring fixtures serve as proper usage examples.

### Concrete Deliverables
1. C# fixture imports from `sdks/csharp/abi/` (remove all duplicated types)
2. Rust fixture uses `guest-libs/rust/` dependency
3. CI check prevents future ABI duplication in fixtures
4. All integration tests pass

### Definition of Done
- [ ] C# fixture `.csproj` references `Polyplug.Abi.csproj`
- [ ] C# fixture `Plugin.cs` has no duplicated ABI types
- [ ] Rust fixture `Cargo.toml` uses `polyplug_guest` dependency
- [ ] Rust fixture `lib.rs` imports from `polyplug_guest`
- [ ] All integration tests pass: `cargo test --test integration`
- [ ] CI check script exists and runs on PR

### Must Have
- C# fixture uses SDK ABI types
- Rust fixture uses guest library
- Tests pass after changes

### Must NOT Have (Guardrails)
- No changes to actual SDK libraries (only fixture changes)
- No ABI breakage (maintain compatibility)
- No changes to test logic (only imports/dependencies)
- No new fixture creation (only fix existing)

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES - `cargo test` works
- **Automated tests**: YES - Tests-after (fixture changes, then run tests)
- **Framework**: Rust built-in test runner + integration tests

### QA Policy
Every task includes agent-executed QA scenarios:
- **Build**: Verify compilation succeeds
- **Test**: Run specific integration tests
- **Evidence**: Screenshot/diff of changes, test output

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: C# fixture project reference [quick]
├── Task 2: Rust fixture dependency update [quick]
└── Task 3: CI check script creation [quick]

Wave 2 (After Wave 1 - implementation, MAX PARALLEL):
├── Task 4: C# fixture remove duplicated types [unspecified-high]
└── Task 5: Rust fixture update imports [quick]

Wave 3 (After Wave 2 - verification):
├── Task 6: Run integration tests [unspecified-high]
└── Task 7: Verify CI check works [quick]

Wave FINAL (After ALL tasks - 2 parallel reviews):
├── Task F1: Code quality review (unspecified-high)
└── Task F2: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: Task 1 → Task 4 → Task 6 → F1-F2 → user okay
Parallel Speedup: ~40% faster than sequential
```

### Dependency Matrix
- **1-3**: — — 4-5, 1
- **4**: 1 — 6, 2
- **5**: 2 — 6, 2
- **6**: 4, 5 — 7, 3
- **7**: 3, 6 — F1-F2, 4

### Agent Dispatch Summary
- **1**: **3** tasks → all `quick`
- **2**: **2** tasks → `unspecified-high`, `quick`
- **3**: **2** tasks → `unspecified-high`, `quick`
- **FINAL**: **2** tasks → `unspecified-high`, `deep`

---

## TODOs

- [ ] 1. C# Fixture - Add SDK Project Reference

  **What to do**:
  Modify `tests/fixtures/csharp_plugin/CsharpPlugin.csproj` to reference the SDK ABI project.
  
  Current `.csproj`:
  ```xml
  <Project Sdk="Microsoft.NET.Sdk">
    <PropertyGroup>
      <TargetFramework>net10.0</TargetFramework>
      <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    </PropertyGroup>
  </Project>
  ```
  
  Add reference to:
  ```xml
  <ItemGroup>
    <ProjectReference Include="../../../../sdks/csharp/abi/Polyplug.Abi.csproj" />
  </ItemGroup>
  ```

  **Must NOT do**:
  - Don't add reference to `guest/` (only need ABI types for this fixture)
  - Don't change TargetFramework or other properties
  - Don't add package references (use project reference)

  **Recommended Agent Profile**:
  - **Category**: `quick` - Simple file edit, well-defined scope
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 4
  - **Blocked By**: None

  **References**:
  - `tests/fixtures/csharp_plugin/CsharpPlugin.csproj` - File to modify
  - `sdks/csharp/abi/Polyplug.Abi.csproj` - Project to reference
  - `tests/fixtures/csharp_plugin/Plugin.cs` - To understand what types are used

  **Acceptance Criteria**:
  - [ ] `.csproj` contains ProjectReference to `Polyplug.Abi.csproj`
  - [ ] `dotnet build tests/fixtures/csharp_plugin/` succeeds

  **QA Scenarios**:
  ```
  Scenario: C# project builds with SDK reference
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug/tests/fixtures/csharp_plugin
      2. dotnet build
    Expected Result: Build succeeds with 0 errors
    Failure Indicators: "error MSB", "reference not found"
    Evidence: .sisyphus/evidence/task-1-csharp-build.txt
  ```

  **Evidence to Capture**:
  - [ ] Build output showing success
  - [ ] Screenshot of modified `.csproj` file

  **Commit**: YES
  - Message: `test(csharp): add SDK ABI project reference to fixture`
  - Files: `tests/fixtures/csharp_plugin/CsharpPlugin.csproj`

---

- [ ] 2. Rust Fixture - Update to Use Guest Library

  **What to do**:
  Modify `tests/fixtures/test_plugin/Cargo.toml` to use `polyplug_guest` instead of `polyplug_abi` directly.
  
  Current `Cargo.toml`:
  ```toml
  [dependencies]
  polyplug_abi = { workspace = true }
  ```
  
  Change to:
  ```toml
  [dependencies]
  polyplug_guest = { path = "../../../guest-libs/rust" }
  ```

  **Must NOT do**:
  - Don't use workspace = true for polyplug_guest (it's not in workspace)
  - Don't change crate-type or other metadata
  - Don't update lib.rs yet (that's Task 5)

  **Recommended Agent Profile**:
  - **Category**: `quick` - Simple dependency change
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 5
  - **Blocked By**: None

  **References**:
  - `tests/fixtures/test_plugin/Cargo.toml` - File to modify
  - `guest-libs/rust/Cargo.toml` - Reference for dependency format
  - `guest-libs/rust/src/lib.rs` - To see what's exported

  **Acceptance Criteria**:
  - [ ] `Cargo.toml` uses `polyplug_guest` dependency with correct path
  - [ ] `cargo check -p test_plugin` succeeds

  **QA Scenarios**:
  ```
  Scenario: Rust project compiles with guest library
    Tool: Bash
    Preconditions: None
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug
      2. cargo check -p test_plugin
    Expected Result: Check succeeds with 0 errors
    Failure Indicators: "error", "could not find", "unresolved import"
    Evidence: .sisyphus/evidence/task-2-rust-check.txt
  ```

  **Evidence to Capture**:
  - [ ] Cargo check output
  - [ ] Diff of Cargo.toml changes

  **Commit**: YES (group with Task 5)
  - Message: `test(rust): use polyplug_guest in test fixture`
  - Files: `tests/fixtures/test_plugin/Cargo.toml`, `tests/fixtures/test_plugin/src/lib.rs`

---

- [ ] 3. Create CI Check Script for ABI Duplication

  **What to do**:
  Create a script that checks test fixtures don't duplicate ABI definitions.
  
  Script should:
  1. Look for hardcoded ABI struct definitions in test fixtures
  2. Compare against SDK definitions
  3. Fail if duplication detected
  
  Place at: `.github/scripts/check-fixture-duplication.sh`
  
  Check for patterns like:
  - `struct StringView` in C# fixtures
  - `struct AbiError` in C# fixtures  
  - `public const uint ABI_OK` in C# fixtures
  - Duplicated Rust ABI types

  **Must NOT do**:
  - Don't block on false positives (add exclusions if needed)
  - Don't require SDK files to be identical (just check for duplication)

  **Recommended Agent Profile**:
  - **Category**: `quick` - Script creation
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 7
  - **Blocked By**: None

  **References**:
  - `tests/fixtures/csharp_plugin/Plugin.cs` - Example of what to detect
  - `sdks/csharp/abi/Abi.cs` - Canonical definitions
  - `.github/workflows/` - To see CI integration pattern

  **Acceptance Criteria**:
  - [ ] Script exists at `.github/scripts/check-fixture-duplication.sh`
  - [ ] Script is executable (`chmod +x`)
  - [ ] Script detects duplicated `StringView` in C# fixture (before fix)
  - [ ] Script passes after fixtures are fixed

  **QA Scenarios**:
  ```
  Scenario: Script detects duplication before fix
    Tool: Bash
    Preconditions: C# fixture still has duplicated types
    Steps:
      1. Run script
      2. Verify it detects StringView duplication
    Expected Result: Exit code 1, output mentions "StringView duplication detected"
    Evidence: .sisyphus/evidence/task-3-script-detects.txt
  
  Scenario: Script passes after fix
    Tool: Bash
    Preconditions: C# fixture fixed
    Steps:
      1. Run script
    Expected Result: Exit code 0, "No duplication found"
    Evidence: .sisyphus/evidence/task-3-script-passes.txt
  ```

  **Evidence to Capture**:
  - [ ] Script output before fix (should fail)
  - [ ] Script output after fix (should pass)
  - [ ] Script source code

  **Commit**: YES
  - Message: `ci: add check for fixture ABI duplication`
  - Files: `.github/scripts/check-fixture-duplication.sh`

---

- [ ] 4. C# Fixture - Remove Duplicated ABI Types

  **What to do**:
  Remove all duplicated ABI type definitions from `tests/fixtures/csharp_plugin/Plugin.cs`.
  
  Remove these types (they'll come from SDK):
  - `StringView` struct (lines ~16-24)
  - `AbiError` struct (lines ~29-37)
  - `PluginHandle` struct (lines ~42-50)
  - `HostContext` struct (lines ~55-63)
  - `DispatchType` enum (lines ~68-72)
  - `NativeDispatch` struct (lines ~77-82)
  - `VmDispatch` struct (lines ~87-94)
  - `PluginDispatch` union (lines ~99-109)
  - `PluginInterface` struct (lines ~114-136)
  - `PluginDescriptor` struct (lines ~141-160)
  - `PluginContext` struct (lines ~165-178)
  - `HostVTable` struct (lines ~183-209)
  - `Constants` class (lines ~212-216) - use ABI_OK from SDK
  
  Add using statement:
  ```csharp
  using Polyplug.Abi; // or whatever namespace SDK uses
  ```
  
  Keep:
  - `AddArgs` struct (this is test-specific)
  - `Plugin` class with actual implementations
  - `PolyplugInit` entry point

  **Must NOT do**:
  - Don't remove test-specific types like `AddArgs`
  - Don't change function implementations
  - Don't change entry point signature

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high` - Careful editing, many lines
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 1)
  - **Blocked By**: Task 1 (needs SDK reference first)
  - **Blocks**: Task 6

  **References**:
  - `tests/fixtures/csharp_plugin/Plugin.cs` - File to modify
  - `sdks/csharp/abi/Abi.cs` - Available SDK types
  - Current `Plugin.cs` lines 12-210 - Types to remove

  **Acceptance Criteria**:
  - [ ] No ABI type definitions remain in `Plugin.cs`
  - [ ] All types imported from SDK via `using`
  - [ ] `dotnet build` succeeds
  - [ ] File size reduced significantly (~200 lines removed)

  **QA Scenarios**:
  ```
  Scenario: C# fixture builds after removing types
    Tool: Bash
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug/tests/fixtures/csharp_plugin
      2. dotnet build
      3. Check no StringView definition in Plugin.cs
    Expected Result: Build succeeds, grep "struct StringView" returns nothing
    Failure Indicators: Build errors about missing types
    Evidence: .sisyphus/evidence/task-4-csharp-fixed.txt
  
  Scenario: Integration tests still work
    Tool: Bash
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug
      2. cargo test integration_dotnet -- --nocapture
    Expected Result: Tests pass
    Evidence: .sisyphus/evidence/task-4-integration-test.txt
  ```

  **Evidence to Capture**:
  - [ ] Before/after line count
  - [ ] Diff showing removed types
  - [ ] Build output
  - [ ] Integration test results

  **Commit**: YES (group with Task 1)
  - Message: `test(csharp): remove duplicated ABI types, use SDK`
  - Files: `tests/fixtures/csharp_plugin/Plugin.cs`

---

- [ ] 5. Rust Fixture - Update Imports to Use Guest Library

  **What to do**:
  Update `tests/fixtures/test_plugin/src/lib.rs` to import from `polyplug_guest` instead of `polyplug_abi`.
  
  Current import:
  ```rust
  use polyplug_abi::*;
  ```
  
  Change to:
  ```rust
  use polyplug_guest::*;
  ```
  
  May need to update:
  - Import statements
  - Type references (if any use full paths)
  - Check that all used types are available in `polyplug_guest`

  **Must NOT do**:
  - Don't change function implementations
  - Don't change logic
  - Don't change ABI constants usage

  **Recommended Agent Profile**:
  - **Category**: `quick` - Simple import change
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 2)
  - **Blocked By**: Task 2 (needs Cargo.toml updated)
  - **Blocks**: Task 6

  **References**:
  - `tests/fixtures/test_plugin/src/lib.rs` - File to modify
  - `guest-libs/rust/src/lib.rs` - What's exported from guest library
  - `guest-libs/rust/README.md` - Usage examples

  **Acceptance Criteria**:
  - [ ] Import changed from `polyplug_abi` to `polyplug_guest`
  - [ ] `cargo check -p test_plugin` succeeds
  - [ ] No other code changes needed (or documented if needed)

  **QA Scenarios**:
  ```
  Scenario: Rust fixture compiles with guest library
    Tool: Bash
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug
      2. cargo check -p test_plugin
    Expected Result: Check succeeds
    Evidence: .sisyphus/evidence/task-5-rust-import.txt
  ```

  **Evidence to Capture**:
  - [ ] Diff of import changes
  - [ ] Cargo check output

  **Commit**: YES (group with Task 2)
  - Message: `test(rust): use polyplug_guest imports in test fixture`
  - Files: `tests/fixtures/test_plugin/src/lib.rs`

---

- [ ] 6. Run Full Integration Test Suite

  **What to do**:
  Run all integration tests to verify fixtures work correctly after changes.
  
  Tests to run:
  ```bash
  cargo test --test integration
  # Or specifically:
  cargo test integration_dotnet
  cargo test integration_ffi_native
  cargo test cross_language
  ```

  **Must NOT do**:
  - Don't skip failing tests (fix them if they fail)
  - Don't ignore warnings (address them)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high` - Running test suite
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 4, 5)
  - **Blocked By**: Task 4, Task 5
  - **Blocks**: Task 7

  **References**:
  - `tests/integration/tests/` - Integration test files
  - `tests/integration/tests/integration_dotnet.rs` - C# specific tests

  **Acceptance Criteria**:
  - [ ] All integration tests pass
  - [ ] No regressions in test count
  - [ ] No new warnings

  **QA Scenarios**:
  ```
  Scenario: All integration tests pass
    Tool: Bash
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug
      2. cargo test --test integration 2>&1 | tee test_output.txt
      3. grep "test result:" test_output.txt
    Expected Result: "test result: ok" for all test groups
    Failure Indicators: "test result: FAILED", "error:"
    Evidence: .sisyphus/evidence/task-6-test-results.txt
  ```

  **Evidence to Capture**:
  - [ ] Full test output
  - [ ] Summary of passed/failed tests
  - [ ] Any warnings that appeared

  **Commit**: NO (no code changes in this task)

---

- [ ] 7. Verify CI Check Script Works

  **What to do**:
  Run the CI check script to verify it passes with the fixed fixtures.
  
  Also, integrate it into CI if not already done.
  
  Check `.github/workflows/` for where to add the check.

  **Must NOT do**:
  - Don't modify the script logic in this task (that was Task 3)
  - Don't break existing CI

  **Recommended Agent Profile**:
  - **Category**: `quick` - Script verification
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 3, 6)
  - **Blocked By**: Task 3 (script creation), Task 6 (fixtures fixed)
  - **Blocks**: Wave FINAL

  **References**:
  - `.github/scripts/check-fixture-duplication.sh` - Script from Task 3
  - `.github/workflows/` - CI workflows to integrate with

  **Acceptance Criteria**:
  - [ ] Script runs without errors
  - [ ] Script passes (exit code 0) with fixed fixtures
  - [ ] Script integrated into CI workflow (optional but recommended)

  **QA Scenarios**:
  ```
  Scenario: CI check script passes
    Tool: Bash
    Steps:
      1. cd /mnt/data/Projects/Utils/polyplug
      2. .github/scripts/check-fixture-duplication.sh
      3. echo $?  # Check exit code
    Expected Result: Exit code 0, "No duplication detected" message
    Evidence: .sisyphus/evidence/task-7-ci-check.txt
  ```

  **Evidence to Capture**:
  - [ ] Script execution output
  - [ ] Exit code verification

  **Commit**: YES (if CI integration added)
  - Message: `ci: integrate fixture duplication check into workflow`
  - Files: `.github/workflows/*.yml`

---

## Final Verification Wave (MANDATORY - after ALL implementation tasks)

> 2 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Code Quality Review** - `unspecified-high`
  Run `cargo clippy`, `cargo fmt --check`, `dotnet format` (if available). Check all changed files for:
  - Proper formatting
  - No unused imports
  - No warnings
  - Proper error handling preserved
  
  Output: `Clippy [PASS/FAIL] | Format [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [ ] F2. **Scope Fidelity Check** - `deep`
  Verify:
  - C# fixture has NO duplicated ABI types (grep for struct definitions)
  - Rust fixture imports from `polyplug_guest`
  - No changes to SDK libraries themselves
  - Test logic unchanged (only imports/dependencies)
  - All "Must NOT Have" items respected
  
  Output: `C# Fixture [FIXED/STILL_DUPLICATES] | Rust Fixture [FIXED/STILL_DIRECT] | SDK Untouched [YES/NO] | VERDICT`

---

## Commit Strategy

- **1**: Group Task 1 + Task 4: `test(csharp): use SDK ABI types in fixture`
- **2**: Group Task 2 + Task 5: `test(rust): use polyplug_guest in test fixture`
- **3**: Task 3: `ci: add fixture duplication check script`
- **4**: Task 7 (if CI integration): `ci: integrate duplication check into workflow`

---

## Success Criteria

### Verification Commands
```bash
# C# fixture builds
cd tests/fixtures/csharp_plugin && dotnet build

# Rust fixture checks
cargo check -p test_plugin

# All integration tests pass
cargo test --test integration

# CI check passes
.github/scripts/check-fixture-duplication.sh
```

### Final Checklist
- [ ] C# fixture uses SDK ABI (no duplicated types)
- [ ] Rust fixture uses `polyplug_guest`
- [ ] All integration tests pass
- [ ] CI check script exists and passes
- [ ] No changes to SDK libraries
- [ ] Code quality checks pass
