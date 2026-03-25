# Cross-Language SDK Validator - Work Plan

## TL;DR

> **Objective**: Build `sdk-validator` CLI tool using ast-grep CLI to ensure all polyplug SDKs (Rust, C#, Python, Lua, JS/TS, C++) have consistent helper methods across ABI structs.
>
> **Deliverables**:
> - Rust CLI tool (`sdk-validator`) using ast-grep CLI subprocess
> - YAML config (`sdk-validator.yaml`) defines **golden method set**
> - CI integration (GitHub Actions workflow)
> - Tree-sitter for Lua (ast-grep doesn't support it)
>
> **Current Gap**: Many methods missing (e.g., `ends_with` in all SDKs, `starts_with` in Rust/C#/Lua)
>
> **Approach**: 
> 1. Define golden method set in YAML config (authoritative, not extracted from code)
> 2. Run ast-grep CLI per language with generated rules
> 3. Tree-sitter for Lua (ast-grep doesn't support it)
> 4. Report gaps and exit non-zero if required methods missing
>
> **Estimated Effort**: Medium (~2-3 days)
> **Parallel Execution**: YES - 4 waves
> **Metis Review**: ✅ Completed
> **Momus Review**: ✅ APPROVED after fixes

---

## Context

### Current SDK State (6 Languages)

**⚠️ CORRECTED:** No language is complete - YAML defines the target

| Method | Rust | Python | C# | JS/TS | C++ | Lua |
|--------|------|--------|-----|-------|-----|-----|
| `to_str` / `toString` | ✅ `to_str` | ✅ `to_str` | ✅ `ToString` | ✅ `toStr` | ✅ `to_string` | ❌ |
| `starts_with` | ❌ | ✅ | ❌ | ✅ | ✅ | ❌ |
| `ends_with` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| `strip_prefix` | ❌ | ✅ | ❌ | ✅ | ✅ | ❌ |
| `split` | ❌ | ✅ | ❌ | ✅ | ✅ | ❌ |

**Critical Gaps**:
- `ends_with` missing from **ALL** SDKs
- `starts_with`, `strip_prefix`, `split` missing from Rust, C#, Lua

### Golden Method Set (Defined in YAML)

Since no language is complete, the **YAML config defines the authoritative golden set**:

```yaml
methods:
  StringView:
    - to_str        # Required everywhere
    - starts_with   # Required everywhere  
    - ends_with     # Required everywhere (none have it!)
    - strip_prefix  # Required everywhere
    - split         # Required everywhere
```

This means all SDKs (including Rust) need to implement missing methods to pass validation.

### Naming Conventions by Language

| Language | Convention | Example |
|----------|-----------|---------|
| Rust | snake_case | `to_str`, `starts_with` |
| C# | PascalCase | `ToString`, `StartsWith` |
| Python | snake_case | `to_str`, `starts_with` |
| Lua | snake_case | `to_str`, `starts_with` |
| JS/TS | camelCase | `toStr`, `startsWith` |
| C++ | snake_case | `to_string`, `starts_with` |

### Technical Constraints

- ✅ **ast-grep CLI**: Use via subprocess (simpler than library)
- ✅ **Languages supported**: Rust, Python, C#, C++, TypeScript
- ❌ **ast-grep does NOT support**: Lua (use tree-sitter)
- ✅ **CI blocking**: Exit code 1 if required methods missing
- ✅ **Config-driven**: YAML with simple structure

---

## Work Objectives

### Core Objective
Build CLI tool that validates all SDKs have required helper methods per struct, blocking CI if gaps found.

### Concrete Deliverables
1. **Rust CLI** (`crates/sdk-validator/`)
   - Config parser (YAML)
   - ast-grep CLI orchestrator
   - Language validators (6 languages)
   - Lua parser (tree-sitter)
   - Report generator (human + JSON)

2. **YAML Config** (`sdk-validator.yaml`)
   ```yaml
   version: 1
   
   # Golden method set (authoritative, NOT extracted from code)
   methods:
     StringView:
       - to_str
       - starts_with
       - ends_with
       - strip_prefix
       - split
   
   # Naming conventions per language
   naming:
     rust: snake_case
     python: snake_case
     csharp: PascalCase
     js: camelCase
     cpp: snake_case
     lua: snake_case
   
   # Target SDK paths
   targets:
     rust:
       - crates/polyplug_guest/src/lib.rs
     python:
       - sdks/python/polyplug_abi/polyplug_abi/helpers.py
     csharp:
       - sdks/csharp/abi/StringViewHelper.cs
     js:
       - sdks/js/abi/polyplug_abi.ts
     cpp:
       - sdks/cpp/abi/polyplug/helpers.hpp
     lua:
       - sdks/lua/guest/polyplug_guest.lua
   ```

3. **CI Integration** (`.github/workflows/sdk-validation.yml`)
   - Installs ast-grep CLI
   - Runs validation on PR/push
   - Blocks merge if validation fails

4. **Generated ast-grep rules** (runtime generated)
   - Auto-generated from YAML config
   - One rule per language per method

### Definition of Done
- [ ] `sdk-validator --config sdk-validator.yaml` runs without error
- [ ] Reports current gaps accurately
- [ ] CI workflow fails when methods missing
- [ ] Exit code 0 when all methods present
- [ ] Can add new methods by editing YAML

### Must Have
- [ ] **Use ast-grep CLI** (subprocess, NOT library - simpler, well-tested)
- [ ] **Define golden set in YAML** (authoritative, NOT extracted from code)
- [ ] **Validate ALL 6 languages** including Rust (all must implement methods)
- [ ] Support for Python, C#, C++, JS/TS, Rust via ast-grep CLI
- [ ] Support for Lua via tree-sitter
- [ ] YAML config with methods + naming + targets
- [ ] CI blocking (exit non-zero on gaps)
- [ ] Human-readable report (table format)
- [ ] JSON output for programmatic use
- [ ] Generate ast-grep rules from YAML config at runtime

### Must NOT Have (Guardrails)
- [ ] Do NOT modify existing SDK files (this is validation only)
- [ ] Do NOT require manual ast-grep rule writing (auto-generate from YAML)
- [ ] Do NOT use TOML (user prefers YAML)
- [ ] Do NOT make methods optional by default (all are required)
- [ ] Do NOT use ast_grep_core library (too complex, use CLI)
- [ ] Do NOT skip Rust validation (it must implement methods too)

---

## Verification Strategy

### Test Strategy Decision
- **Infrastructure exists**: YES (Rust project with Cargo)
- **Automated tests**: TDD style - unit tests for config parsing, integration tests for validation
- **Framework**: `cargo test`
- **Agent-Executed QA**: Every task includes CLI command validation

### QA Policy
Every task MUST include agent-executable QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation - Config & Core):
├── Task 1: Create sdk-validator crate structure
├── Task 2: Implement YAML config parser
└── Task 3: Design ast-grep CLI orchestrator

Wave 2 (Language Validators - MAX PARALLEL):
├── Task 4: Rust validator (ast-grep CLI)
├── Task 5: Python validator (ast-grep CLI)
├── Task 6: C# validator (ast-grep CLI)
├── Task 7: C++ validator (ast-grep CLI)
├── Task 8: JS/TS validator (ast-grep CLI)
└── Task 9: Lua validator (tree-sitter)

Wave 3 (Aggregation & Reporting):
├── Task 10: Result aggregation logic
├── Task 11: Report generator (table + JSON)
└── Task 12: CLI interface (args, exit codes)

Wave 4 (CI Integration):
├── Task 13: GitHub Actions workflow
├── Task 14: Create sdk-validator.yaml config
└── Task 15: Documentation

Wave FINAL (Verification):
├── Task F1: End-to-end test
├── Task F2: Code quality review
└── Task F3: Final verification

Critical Path: T1 → T2 → T3 → T4-T9 → T10 → T11 → T12 → T13 → T14 → T15 → F1-F3 → user okay
Parallel Speedup: ~70% faster than sequential (6 validators in parallel)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | - | 2, 3 |
| 2 | 1 | 4-9 |
| 3 | 1 | 4-9 |
| 4 | 2, 3 | 10 |
| 5 | 2, 3 | 10 |
| 6 | 2, 3 | 10 |
| 7 | 2, 3 | 10 |
| 8 | 2, 3 | 10 |
| 9 | 2, 3 | 10 |
| 10 | 4-9 | 11, 12 |
| 11 | 10 | 12 |
| 12 | 10, 11 | 13-15 |
| 13 | 12 | F1 |
| 14 | 12 | F1 |
| 15 | 12 | F1 |
| F1 | 13-15 | F2 |
| F2 | F1 | F3 |
| F3 | F2 | user |

### Agent Dispatch Summary

| Wave | Tasks | Profile | Skills |
|------|-------|---------|--------|
| 1 | 1-3 | `quick` | - |
| 2 | 4-9 | `unspecified-high` | [`ast-grep`] |
| 3 | 10-12 | `unspecified-high` | - |
| 4 | 13-15 | `quick` | - |
| FINAL | F1-F3 | `deep`, `unspecified-high`, `oracle` | - |

---

## TODOs

- [ ] 1. Create sdk-validator crate structure

  **What to do**:
  - Create `crates/sdk-validator/` directory
  - Initialize Cargo.toml with dependencies (serde, serde_yaml, anyhow, clap)
  - Set up basic module structure: main.rs, config.rs, validator.rs, reporter.rs, languages/
  - Add to workspace Cargo.toml

  **Must NOT do**:
  - Do NOT implement any logic yet
  - Do NOT add unused dependencies

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (foundation)
  - **Blocks**: Tasks 2-3

  **References**:
  - Pattern: `crates/polyplug/Cargo.toml` - See existing crate structure
  - Pattern: `Cargo.toml` workspace definition

  **Acceptance Criteria**:
  - [ ] `crates/sdk-validator/Cargo.toml` exists
  - [ ] `cargo check` passes with no errors
  - [ ] Basic module files created

  **QA Scenarios**:
  ```
  Scenario: Verify crate structure
    Tool: Bash
    Steps:
      1. Run: `ls crates/sdk-validator/`
      2. Assert: `Cargo.toml`, `src/main.rs`, `src/config.rs`, `src/validator.rs`, `src/reporter.rs` exist
      3. Run: `cargo check --package sdk-validator`
      4. Assert: Exit code 0
    Evidence: .sisyphus/evidence/task-1-crate-structure.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): create sdk-validator crate structure`
  - Files: `crates/sdk-validator/**`

- [ ] 2. Implement YAML config parser

  **What to do**:
  - Define Rust structs for config: version, methods (HashMap<String, Vec<String>>), naming (HashMap<String, String>), targets (HashMap<String, Vec<String>>)
  - Implement YAML deserialization using serde
  - Support simple struct→methods mapping format
  - Add validation (check for duplicate structs, etc.)

  **Must NOT do**:
  - Do NOT support TOML
  - Do NOT add complex optional fields yet

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 1)
  - **Blocks**: Tasks 4-9
  - **Blocked By**: Task 1

  **References**:
  - Example: `serde_yaml` documentation patterns
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See YAML config format

  **Acceptance Criteria**:
  - [ ] Can parse `sdk-validator.yaml` from draft
  - [ ] Unit test: `test_parse_config()` passes
  - [ ] Proper error messages for invalid YAML

  **QA Scenarios**:
  ```
  Scenario: Parse valid config
    Tool: Bash
    Steps:
      1. Create test.yaml with StringView methods
      2. Run: `cargo test --package sdk-validator test_parse_config`
      3. Assert: Test passes
    Evidence: .sisyphus/evidence/task-2-config-parser.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement YAML config parser`
  - Files: `crates/sdk-validator/src/config.rs`, `crates/sdk-validator/src/main.rs`

- [ ] 3. Design ast-grep CLI orchestrator

  **What to do**:
  - Define `AstGrepRunner` struct that runs ast-grep CLI via subprocess
  - Implement method to generate ast-grep rules from config + method name + language
  - Handle naming convention transformation (snake_case → PascalCase, etc.)
  - Parse JSON output from ast-grep
  - Handle errors (ast-grep not installed, parse errors, etc.)

  **Must NOT do**:
  - Do NOT use ast_grep_core library
  - Do NOT write temporary files for rules (use inline rules)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 2)
  - **Blocks**: Tasks 4-9
  - **Blocked By**: Task 1

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See ast-grep rule examples

  **Acceptance Criteria**:
  - [ ] Can generate ast-grep rules for each language
  - [ ] Can run ast-grep CLI and parse JSON output
  - [ ] Unit tests for naming convention transformations

  **QA Scenarios**:
  ```
  Scenario: Generate rule for Python
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_generate_python_rule`
      2. Assert: Generates correct pattern for snake_case
    Evidence: .sisyphus/evidence/task-3-ast-grep-orchestrator.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): design ast-grep CLI orchestrator`
  - Files: `crates/sdk-validator/src/ast_grep.rs`, `crates/sdk-validator/src/naming.rs`

- [ ] 4. Rust validator (ast-grep CLI)

  **What to do**:
  - Implement `RustValidator` struct
  - Generate ast-grep rules for Rust patterns
  - Run ast-grep CLI on Rust source files
  - Parse JSON output to find methods
  - Support detection of: functions, methods

  **Must NOT do**:
  - Do NOT assume Rust is complete (it's missing methods too!)
  - Do NOT skip Rust validation

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: [`ast-grep`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 5-9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See ast-grep examples
  - SDK: `crates/polyplug_guest/src/lib.rs` - See actual Rust helpers

  **Acceptance Criteria**:
  - [ ] Detects `to_str`, `alloc_string` in Rust SDK
  - [ ] Reports missing methods (starts_with, ends_with, etc.)
  - [ ] Returns `ValidationResult` with correct results

  **QA Scenarios**:
  ```
  Scenario: Detect Rust helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_rust_validator`
      2. Assert: Detects to_str
      3. Assert: Reports missing starts_with, ends_with, etc.
    Evidence: .sisyphus/evidence/task-4-rust-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement Rust validator via ast-grep CLI`
  - Files: `crates/sdk-validator/src/languages/rust.rs`

- [ ] 5. Python validator (ast-grep CLI)

  **What to do**:
  - Implement `PythonValidator` struct
  - Generate ast-grep rules for Python patterns
  - Run ast-grep CLI on Python source files
  - Support detection of: standalone functions

  **Must NOT do**:
  - Do NOT hard-code method names (use config)
  - Do NOT miss type-annotated functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: [`ast-grep`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4, 6-9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See Python ast-grep examples
  - SDK: `sdks/python/polyplug_abi/polyplug_abi/helpers.py` - See actual helpers

  **Acceptance Criteria**:
  - [ ] Detects `to_str`, `starts_with`, `strip_prefix`, `split` in Python SDK
  - [ ] Generates valid ast-grep rules
  - [ ] Returns `ValidationResult` with correct results

  **QA Scenarios**:
  ```
  Scenario: Detect Python helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_python_validator`
      2. Assert: Detects existing methods
      3. Assert: Reports missing ends_with
    Evidence: .sisyphus/evidence/task-5-python-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement Python validator via ast-grep CLI`
  - Files: `crates/sdk-validator/src/languages/python.rs`

- [ ] 6. C# validator (ast-grep CLI)

  **What to do**:
  - Implement `CSharpValidator` struct
  - Generate ast-grep rules for C# patterns
  - Handle extension methods (`this StringView`)
  - Handle static class methods (`StringViewHelper.ToString`)

  **Must NOT do**:
  - Do NOT assume all methods are extension methods
  - Do NOT miss static helper class patterns

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: [`ast-grep`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4-5, 7-9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See C# ast-grep examples
  - SDK: `sdks/csharp/abi/StringViewHelper.cs` - See actual helpers

  **Acceptance Criteria**:
  - [ ] Detects `ToString` extension method
  - [ ] Handles PascalCase naming correctly
  - [ ] Reports missing methods (starts_with, ends_with, etc.)

  **QA Scenarios**:
  ```
  Scenario: Detect C# helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_csharp_validator`
      2. Assert: Detects ToString
      3. Assert: Reports missing starts_with, ends_with, etc.
    Evidence: .sisyphus/evidence/task-6-csharp-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement C# validator via ast-grep CLI`
  - Files: `crates/sdk-validator/src/languages/csharp.rs`

- [ ] 7. C++ validator (ast-grep CLI)

  **What to do**:
  - Implement `CppValidator` struct
  - Generate ast-grep rules for C++ patterns
  - Handle namespace functions (`polyplug::abi::to_string`)
  - Handle inline functions

  **Must NOT do**:
  - Do NOT require namespace qualification in detection
  - Do NOT miss header-only implementations

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: [`ast-grep`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4-6, 8-9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See C++ ast-grep examples
  - SDK: `sdks/cpp/abi/polyplug/helpers.hpp` - See actual helpers

  **Acceptance Criteria**:
  - [ ] Detects `to_string`, `to_string_view`, `starts_with`, etc.
  - [ ] Handles namespace patterns
  - [ ] Reports missing methods (ends_with)

  **QA Scenarios**:
  ```
  Scenario: Detect C++ helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_cpp_validator`
      2. Assert: Detects to_string, starts_with, etc.
      3. Assert: Reports missing ends_with
    Evidence: .sisyphus/evidence/task-7-cpp-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement C++ validator via ast-grep CLI`
  - Files: `crates/sdk-validator/src/languages/cpp.rs`

- [ ] 8. JS/TS validator (ast-grep CLI)

  **What to do**:
  - Implement `JsValidator` struct
  - Generate ast-grep rules for TypeScript patterns
  - Handle function declarations and arrow functions
  - Handle union types (`StringView | string`)

  **Must NOT do**:
  - Do NOT validate JavaScript files (TS only for now)
  - Do NOT require exact type annotations

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: [`ast-grep`]

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4-7, 9)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - Draft: `.sisyphus/drafts/sdk-validator-design.md` - See TS ast-grep examples
  - SDK: `sdks/js/abi/polyplug_abi.ts` - See actual helpers

  **Acceptance Criteria**:
  - [ ] Detects `toStr`, `startsWith`, `stripPrefix`, `split`
  - [ ] Handles camelCase naming
  - [ ] Reports missing methods (endsWith)

  **QA Scenarios**:
  ```
  Scenario: Detect JS/TS helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_js_validator`
      2. Assert: Detects toStr, startsWith, etc.
      3. Assert: Reports missing endsWith
    Evidence: .sisyphus/evidence/task-8-js-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement JS/TS validator via ast-grep CLI`
  - Files: `crates/sdk-validator/src/languages/js.rs`

- [ ] 9. Lua validator (tree-sitter)

  **What to do**:
  - Implement `LuaValidator` struct
  - Use tree-sitter-lua crate (not ast-grep)
  - Parse Lua files to find function definitions
  - Support detection of: global functions, table methods

  **Must NOT do**:
  - Do NOT use ast-grep (doesn't support Lua)
  - Do NOT use regex-only (too fragile)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4-8)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 2-3

  **References**:
  - SDK: `sdks/lua/abi/polyplug_abi.lua` - See actual Lua code
  - SDK: `sdks/lua/guest/polyplug_guest.lua` - See guest helpers

  **Acceptance Criteria**:
  - [ ] Detects existing Lua helpers (if any)
  - [ ] Reports ALL methods missing (current state)
  - [ ] Uses tree-sitter for parsing

  **QA Scenarios**:
  ```
  Scenario: Detect Lua helpers
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_lua_validator`
      2. Assert: Parses Lua files without error
      3. Assert: Reports all methods missing (current state)
    Evidence: .sisyphus/evidence/task-9-lua-detection.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement Lua validator via tree-sitter`
  - Files: `crates/sdk-validator/src/languages/lua.rs`

- [ ] 10. Result aggregation logic

  **What to do**:
  - Aggregate results from all 6 language validators
  - Build `ValidationReport` struct
  - Calculate completeness per struct
  - Identify missing methods per struct/language

  **Must NOT do**:
  - Do NOT generate reports here (that's Task 11)
  - Do NOT exit with error here

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (needs all validators)
  - **Blocks**: Task 11
  - **Blocked By**: Tasks 4-9

  **Acceptance Criteria**:
  - [ ] Aggregates results from all 6 languages
  - [ ] Correctly identifies which methods missing where
  - [ ] Unit tests for aggregation logic

  **QA Scenarios**:
  ```
  Scenario: Aggregate results
    Tool: Bash
    Steps:
      1. Run: `cargo test --package sdk-validator test_aggregation`
      2. Assert: Report shows correct coverage
    Evidence: .sisyphus/evidence/task-10-aggregation.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement result aggregation`
  - Files: `crates/sdk-validator/src/aggregator.rs`

- [ ] 11. Report generator (table + JSON)

  **What to do**:
  - Implement `Reporter` struct
  - Generate human-readable table output
  - Generate JSON output for CI/programmatic use
  - Show ✓/✗ per struct/method/language

  **Must NOT do**:
  - Do NOT write to file (output to stdout)
  - Do NOT add colors (keep it simple)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 10)
  - **Blocks**: Task 12
  - **Blocked By**: Task 10

  **Acceptance Criteria**:
  - [ ] Table output matches design in draft
  - [ ] JSON output has correct schema
  - [ ] Unit tests for both formats

  **QA Scenarios**:
  ```
  Scenario: Generate table report
    Tool: Bash
    Steps:
      1. Run: `cargo run --package sdk-validator -- --config sdk-validator.yaml`
      2. Assert: Table shows ✓/✗ for each method
    Evidence: .sisyphus/evidence/task-11-table-report.txt

  Scenario: Generate JSON report
    Tool: Bash
    Steps:
      1. Run: `cargo run --package sdk-validator -- --config sdk-validator.yaml --json`
      2. Assert: Valid JSON output
    Evidence: .sisyphus/evidence/task-11-json-report.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement report generator`
  - Files: `crates/sdk-validator/src/reporter.rs`

- [ ] 12. CLI interface (args, exit codes)

  **What to do**:
  - Implement CLI using clap
  - Arguments: `--config`, `--json`, `--struct`, `--fail-on-missing`
  - Exit code 0 if valid, 1 if gaps found (with `--fail-on-missing`)
  - Help text and error messages

  **Must NOT do**:
  - Do NOT add unnecessary flags
  - Do NOT exit 1 without showing report

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 11)
  - **Blocks**: Tasks 13-15
  - **Blocked By**: Task 11

  **Acceptance Criteria**:
  - [ ] CLI parses arguments correctly
  - [ ] Exit code 0 when all methods present
  - [ ] Exit code 1 when gaps found (with `--fail-on-missing`)
  - [ ] Help text is helpful

  **QA Scenarios**:
  ```
  Scenario: CLI exit codes
    Tool: Bash
    Steps:
      1. Run: `cargo run --package sdk-validator -- --config sdk-validator.yaml --fail-on-missing; echo $?`
      2. Assert: Exit code is 1 (current gaps exist)
    Evidence: .sisyphus/evidence/task-12-exit-codes.txt
  ```

  **Commit**: YES
  - Message: `feat(validator): implement CLI interface`
  - Files: `crates/sdk-validator/src/main.rs`, `crates/sdk-validator/src/cli.rs`

- [ ] 13. GitHub Actions workflow

  **What to do**:
  - Create `.github/workflows/sdk-validation.yml`
  - Install ast-grep in CI
  - Build sdk-validator
  - Run validation on PR/push
  - Block PR if validation fails

  **Must NOT do**:
  - Do NOT run on every file change (only SDK changes)
  - Do NOT require manual approval

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 14)
  - **Blocks**: Task F1
  - **Blocked By**: Task 12

  **References**:
  - Pattern: `.github/workflows/*.yml` - See existing workflows

  **Acceptance Criteria**:
  - [ ] Workflow file created
  - [ ] Installs ast-grep
  - [ ] Runs validation
  - [ ] Blocks PR on failure

  **QA Scenarios**:
  ```
  Scenario: CI workflow syntax check
    Tool: Bash
    Steps:
      1. Run: `actionlint .github/workflows/sdk-validation.yml`
      2. Assert: No syntax errors
    Evidence: .sisyphus/evidence/task-13-ci-syntax.txt
  ```

  **Commit**: YES
  - Message: `ci: add SDK validation workflow`
  - Files: `.github/workflows/sdk-validation.yml`

- [ ] 14. Create sdk-validator.yaml config

  **What to do**:
  - Create root-level `sdk-validator.yaml`
  - Define StringView, Buffer, PluginHandle structs
  - List required methods for each
  - Configure naming conventions
  - Define target paths

  **Must NOT do**:
  - Do NOT add methods that don't exist yet (this is the target state)
  - Do NOT use TOML

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 13)
  - **Blocks**: Task F1
  - **Blocked By**: Task 12

  **Acceptance Criteria**:
  - [ ] Config file exists at root
  - [ ] Validator can parse it
  - [ ] Reflects current design decisions

  **QA Scenarios**:
  ```
  Scenario: Config is valid
    Tool: Bash
    Steps:
      1. Run: `cargo run --package sdk-validator -- --config sdk-validator.yaml --dry-run`
      2. Assert: Config loads successfully
    Evidence: .sisyphus/evidence/task-14-config-valid.txt
  ```

  **Commit**: YES
  - Message: `config: add sdk-validator.yaml`
  - Files: `sdk-validator.yaml`

- [ ] 15. Documentation

  **What to do**:
  - Add README.md for sdk-validator
  - Document config format
  - Document CLI usage
  - Document how to add new methods

  **Must NOT do**:
  - Do NOT document internal implementation details
  - Do NOT duplicate ast-grep documentation

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 13-14)
  - **Blocks**: Task F1
  - **Blocked By**: Task 12

  **Acceptance Criteria**:
  - [ ] README explains usage
  - [ ] Config format documented
  - [ ] Contributing guide for adding methods

  **QA Scenarios**:
  ```
  Scenario: Documentation is helpful
    Tool: Bash
    Steps:
      1. Read: `crates/sdk-validator/README.md`
      2. Assert: Contains usage examples
    Evidence: .sisyphus/evidence/task-15-docs.txt
  ```

  **Commit**: YES
  - Message: `docs: add sdk-validator documentation`
  - Files: `crates/sdk-validator/README.md`, `docs/sdk-validation.md`

---

## Final Verification Wave

- [ ] F1. **End-to-end test** — `deep`
  Run complete validation: `cargo run --package sdk-validator -- --config sdk-validator.yaml --fail-on-missing`
  Verify: detects all current gaps, produces correct report, exits with code 1
  Output: `Exit code: 1 | StringView: 1/6 methods complete | ends_with missing in all languages`

- [ ] F2. **Code quality review** — `unspecified-high`
  Run `cargo clippy --package sdk-validator`, `cargo fmt --check`, `cargo test --package sdk-validator`
  Review: all validators follow patterns, no unwrap() in production code, proper error types
  Output: `Clippy: 0 warnings | Tests: N/N pass | VERDICT`

- [ ] F3. **Final verification** — `oracle`
  Verify: ALL 6 languages validated, YAML config correct, CI workflow present, exit codes correct
  Check: matches user's "simple struct→methods list" requirement
  Output: `Compliant: YES/NO | Issues: [list if any]`

---

## Commit Strategy

- **1-3**: Single commit `feat(validator): create SDK validator foundation`
- **4-9**: One commit per language `feat(validator): add {language} validator`
- **10-12**: Single commit `feat(validator): add aggregation and reporting`
- **13-15**: Individual commits per task
- **F1-F3**: No commits (verification only)

---

## Success Criteria

### Verification Commands

```bash
# Full validation
cargo run --package sdk-validator -- --config sdk-validator.yaml --fail-on-missing
# Expected: exit code 1 (gaps exist), human-readable report

# JSON output
cargo run --package sdk-validator -- --config sdk-validator.yaml --json
# Expected: valid JSON with current gaps

# Specific struct
cargo run --package sdk-validator -- --config sdk-validator.yaml --struct StringView
# Expected: only StringView methods

# Tests
cargo test --package sdk-validator
# Expected: all tests pass

# Code quality
cargo clippy --package sdk-validator -- -D warnings
cargo fmt --package sdk-validator -- --check
# Expected: no warnings, clean formatting
```

### Final Checklist
- [ ] Validator detects methods in all 6 languages
- [ ] CI workflow blocks PRs with gaps
- [ ] Config is simple YAML with struct→methods list
- [ ] Adding new method requires only editing YAML
- [ ] Documentation explains usage
