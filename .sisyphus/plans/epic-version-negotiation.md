# Epic 14 — Plugin Versioning and Compatibility

## TL;DR

> **Quick Summary**: Implement version negotiation between plugin bundles using manifest-level major.minor comparison, a three-tier compatibility mode (Strict/Relaxed/Yolo), and a warning callback mechanism — all without touching the frozen ABI structs.
>
> **Deliverables**:
> - `crates/polyplug/src/version/mod.rs` — new `Version` struct + `Compatibility` enum + parse/compare logic
> - `crates/polyplug/src/error/mod.rs` — two new `LoaderError` variants
> - `crates/polyplug/src/runtime/mod.rs` — updated `LoadOptions`, `RuntimeBuilder` fields, global warning callback, negotiation call
> - `crates/polyplug/src/graph/mod.rs` — replace 3-field `Version` with 2-field `version::Version`
> - `crates/polyplug/src/lib.rs` — add `pub mod version;`
> - `crates/polyplugc/src/generators/rust/mod.rs` — emit required version + function count constants in `host/types.rs`
> - `tests/integration_version/mod.rs` — 14 integration tests
> - `crates/polyplug/Cargo.toml` — add `[[test]]` entry
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES — 3 waves
> **Critical Path**: Task 1 → Task 4 → Task 6 → F1/F2/F3

---

## Context

### Original Request
Implement Epic 14 from `polyplug_prd.md §17`: Plugin Versioning and Compatibility. Bundles declare a `version` string (major.minor). Consumers declare `min_version` per dependency. The runtime performs negotiation at load time with configurable compatibility modes.

### Interview Summary
**Key Discussions**:
- **Version source**: Manifest-only negotiation — consumer's `[[dependency]].min_version` vs provider's `version` field. No external required-version source at load time.
- **LoadOptions**: `pub struct LoadOptions { pub compatibility: Compatibility, pub ignore_function_count_mismatch: bool }` — replaces the current empty stub.
- **`graph::Version` collision**: The existing 3-field `Version` in `graph/mod.rs` (with always-zero `patch`) must be replaced with the new 2-field `version::Version`.
- **generator scope**: Generator fixes (Python/Lua/JS missing `function_count`) are Epic 15. Tests use handcrafted `TempDir` manifests.
- **Warning callback OnceLock semantics**: First `on_warning()` call wins; subsequent calls are silently ignored. Test binaries needing different callbacks must be separate test binaries.

**Research Findings**:
- `LoadOptions` at `runtime/mod.rs:103` is currently an empty struct — safe to replace completely.
- `ManifestData.version: String` already exists in the manifest struct — parse it on demand.
- `ManifestData.function_count: HashMap<String, u32>` already exists — key is `"contractname@major"`.
- `graph/mod.rs` constructs `Version { major: 1, minor: 0, patch: 0 }` at 4 sites (3 production + 1 test helper) — all must switch to `Version { major: 1, minor: 0 }` after the struct change.

### Metis Review
**Identified Gaps** (addressed):
- Missing `Display` impl for `Version` (needed by `#[error]` format strings in new `LoaderError` variants) — added to Task 1.
- `OnceLock` warning callback semantics need a doc comment to explain "first registration wins" — added to Task 3.
- `load_bundle` (the non-`_with` variant) must forward `Compatibility::default()` — confirmed Task 3 handles this.
- Test binary separation for warning callback tests — added explicit instruction in Task 6.
- `graph/mod.rs` internal test helper `make_capability` uses `Version { major, minor, patch: 0 }` — must be updated in Task 2.

---

## Work Objectives

### Core Objective
Implement manifest-level plugin version negotiation with Strict/Relaxed/Yolo compatibility modes, a warning callback mechanism, and host-side constants from codegen — without touching any frozen ABI structs.

### Concrete Deliverables
- `crates/polyplug/src/version/mod.rs` — `Version`, `Compatibility`, parse/compare logic
- `crates/polyplug/src/error/mod.rs` — `VersionMismatch` and `FunctionCountMismatch` variants in `LoaderError`
- `crates/polyplug/src/runtime/mod.rs` — `LoadOptions` with fields, `RuntimeBuilder` with `compatibility` + `warning_cb`, `GLOBAL_WARNING_CB` OnceLock, `emit_warning()`, `validate_bundle_compatibility()`, and calls in `build()` and `load_bundle_with()`
- `crates/polyplug/src/graph/mod.rs` — replaced `Version` type (1 struct definition deleted + 3 production construction sites + 1 test helper updated = 5 total edits)
- `crates/polyplug/src/lib.rs` — `pub mod version;`
- `crates/polyplugc/src/generators/rust/mod.rs` — two constants per contract in `host/types.rs`
- `tests/integration_version/mod.rs` — 14 `#[test]` functions
- `crates/polyplug/Cargo.toml` — one new `[[test]]` stanza

### Definition of Done
- [x] `cargo clippy -- -D warnings` → zero warnings
- [x] `cargo fmt --check` → clean
- [x] `cargo test` → all tests pass (including 14 new integration_version tests)
- [x] Existing tests unchanged: `integration_load`, `integration_dispatch`, `integration_graph`, `integration_discovery`, `integration_extension` all still green

### Must Have
- `Version { pub major: u32, pub minor: u32 }` — exactly two public fields
- `is_compatible_with(&self, required: &Version) -> bool` — same major AND self.minor >= required.minor
- `Version::parse(s: &str) -> Result<Version, LoaderError>` — accepts "N.M" only; "1", "1.2.3", "not_a_version" are all errors
- `Compatibility::default()` returns `Compatibility::Strict`
- `RuntimeBuilder::compatibility(self, c: Compatibility) -> RuntimeBuilder` builder method
- `RuntimeBuilder::on_warning(self, cb: impl Fn(&str) + Send + Sync + 'static) -> RuntimeBuilder`
- `GLOBAL_WARNING_CB: OnceLock<Box<dyn Fn(&str) + Send + Sync>>` — first registration wins
- `emit_warning(msg: &str)` — `pub(crate)` function in `runtime/mod.rs`
- `validate_bundle_compatibility(manifests, compatibility) -> Result<(), RuntimeError>` — called in `RuntimeBuilder::build()` after graph construction
- `load_bundle_with` uses `opts.compatibility` (not global) for per-bundle override
- No `.unwrap()` in production code
- All `use` statements at file top only
- All types explicitly annotated
- All module roots at `dirname/mod.rs`

### Must NOT Have (Guardrails)
- **No changes to `PluginVTable`, `HostVTable`, or any `#[repr(C)]` ABI struct**
- **No `.unwrap()` or `.expect()` outside `#[cfg(test)]` blocks**
- **No `use` statements inside function bodies or `impl` blocks**
- **No bare `filename.rs` module roots** (use `dirname/mod.rs`)
- **No string errors** — use typed `LoaderError` variants
- **No inferred types** without annotation (except struct construction and numeric casts)
- **No generator fixes** — Python/Lua/JS manifest generators are Epic 15 scope; do NOT modify them
- **No version-patch semantics** — this epic uses `major.minor` only; no `patch` field

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (`cargo test`)
- **Automated tests**: Tests-after (not TDD — the implementation is fully spec'd)
- **Framework**: `cargo test` / Rust built-in
- **Test file**: `tests/integration_version/mod.rs`

### QA Policy
Every task includes agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.txt`.

- **Library/Module**: Bash (`cargo test --test <name>`) — run tests, assert exit code 0, capture output

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundation, all independent):
├── Task 1: Create version/mod.rs + add `pub mod version;` to lib.rs  [quick]
├── Task 2: Replace graph::Version with version::Version               [quick]
└── Task 3: Update error/mod.rs (VersionMismatch, FunctionCountMismatch variants) [quick]

Wave 2 (After Wave 1 — integration into runtime and codegen):
├── Task 4: Update runtime/mod.rs (LoadOptions, RuntimeBuilder, GLOBAL_WARNING_CB,   [unspecified-high]
│           emit_warning, validate_bundle_compatibility, build() + load_bundle_with())
└── Task 5: Update rust codegen host/types.rs (REQUIRED_VERSION + REQUIRED_FUNCTION_COUNT constants) [quick]

Wave 3 (After Tasks 4 + 5 — tests and registration):
├── Task 6: Write integration_version/mod.rs (14 tests)                               [unspecified-high]
└── Task 7: Register test in Cargo.toml + lib.rs module export                        [quick]

Wave FINAL (After ALL tasks — parallel review):
├── Task F1: Plan Compliance Audit (oracle)
├── Task F2: Code Quality Review (unspecified-high)
└── Task F3: Scope Fidelity Check (deep)
```

**Critical Path**: Task 1 → Task 4 → Task 6 → F1/F2/F3
**Parallel Speedup**: ~50% faster than sequential
**Max Concurrent**: 3 (Wave 1)

### Dependency Matrix

| Task | Blocked By | Blocks |
|------|-----------|--------|
| 1 | — | 2, 3, 4, 6 |
| 2 | 1 | 4 |
| 3 | 1 | 4 |
| 4 | 1, 2, 3 | 6 |
| 5 | 1 | 6 |
| 6 | 4, 5 | F1, F2, F3 |
| 7 | 6 | F1, F2, F3 |

### Agent Dispatch Summary

- **Wave 1**: 3 tasks → T1 `quick`, T2 `quick`, T3 `quick`
- **Wave 2**: 2 tasks → T4 `unspecified-high`, T5 `quick`
- **Wave 3**: 2 tasks → T6 `unspecified-high`, T7 `quick`
- **Final**: 3 tasks → F1 `oracle`, F2 `unspecified-high`, F3 `deep`

---

## TODOs

---

- [x] 1. Create `crates/polyplug/src/version/mod.rs` — Version struct, Compatibility enum, parse, compare, Display

  **What to do**:
  - Create new file at `crates/polyplug/src/version/mod.rs` (AGENTS.md Rule 1: must be `dirname/mod.rs`)
  - Add the module doc comment at top
  - Add `use` statements at file top (AGENTS.md Rule 2): `use crate::error::LoaderError;` and `use std::fmt;`
  - Define the `Version` struct exactly as:
    ```rust
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Version {
        pub major: u32,
        pub minor: u32,
    }
    ```
  - Implement `std::fmt::Display` for `Version`:
    ```rust
    impl fmt::Display for Version {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}.{}", self.major, self.minor)
        }
    }
    ```
    This is required because the `LoaderError::VersionMismatch` `#[error]` format string uses `{required}` and `{found}` which require `Display`.
  - Implement `Version::parse`:
    ```rust
    impl Version {
        pub fn parse(s: &str) -> Result<Version, LoaderError> {
            // split on '.' exactly once
            // both parts must parse as u32
            // exactly two parts required — "1", "1.2.3", "" are all errors
        }
    }
    ```
    Error on anything that is not exactly `"N.M"` format. Use `LoaderError::ManifestParse` with:
    - `path`: the caller passes this as context (the manifest path or contract name)
    - `reason`: e.g. `format!("invalid version string {:?}: expected \"major.minor\" format", s)`
    - Since `parse` doesn't have the path, make the signature `Version::parse(s: &str, context: &str) -> Result<Version, LoaderError>` where `context` is used as the `path` in `ManifestParse`.
  - Implement `is_compatible_with`:
    ```rust
    impl Version {
        pub fn is_compatible_with(&self, required: &Version) -> bool {
            self.major == required.major && self.minor >= required.minor
        }
    }
    ```
  - Define the `Compatibility` enum exactly as:
    ```rust
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Compatibility {
        Strict,
        Relaxed,
        Yolo,
    }
    ```
  - Implement `Default` for `Compatibility`:
    ```rust
    impl Default for Compatibility {
        fn default() -> Compatibility {
            Compatibility::Strict
        }
    }
    ```
  - Add a `#[cfg(test)]` module with these unit tests:
    - `version_parse_valid` — `"1.0"` → `Version { major: 1, minor: 0 }`, `"2.3"` → `Version { major: 2, minor: 3 }`
    - `version_parse_invalid` — `"1"`, `"1.2.3"`, `""`, `"not_a_version"` all return `Err`
    - `version_compatible` — `v1_2.is_compatible_with(&v1_0)` → `true`; `v1_0.is_compatible_with(&v1_2)` → `false`; `v2_0.is_compatible_with(&v1_0)` → `false`
    - `version_display` — `Version { major: 1, minor: 2 }.to_string() == "1.2"`
    - `compatibility_default_is_strict` — `Compatibility::default() == Compatibility::Strict`

  **Also in Task 1 — add `pub mod version;` to `crates/polyplug/src/lib.rs`**:
  - This is required so that Tasks 2, 3, and 4 can compile. Do it in the same commit.
  - Open `lib.rs` and add `pub mod version;` in the `pub mod` block (alphabetically, after `pub mod runtime;`)
  - This is a 1-line change — no other modifications to `lib.rs`

  **Must NOT do**:
  - Do NOT add a `patch` field — this is a 2-field struct only
  - Do NOT import anything inside function bodies
  - Do NOT use `.unwrap()` — use `?` or explicit match

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single new file, clear spec, no external dependencies, no architectural decisions
  - **Skills**: none needed
  - **Skills Evaluated but Omitted**:
    - `git-master`: Not needed — no complex git operations

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2 and 3)
  - **Blocks**: Tasks 2, 3, 4, 6
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `crates/polyplug/src/error/mod.rs:43-127` — how `LoaderError` variants are defined with `thiserror` (follow this pattern for the `ManifestParse` variant signature: `{ path: String, reason: String }`)
  - `crates/polyplug/src/error/mod.rs:1-4` — module doc comment style and top-level `use` placement
  - `crates/polyplug/src/extensions/trace/mod.rs` — example of a new module file with proper doc comment, `use` at top, explicit visibility on all items

  **Type References**:
  - `crates/polyplug/src/error/mod.rs:64-66` — `ManifestParse { path: String, reason: String }` — this is the variant to use in `Version::parse` errors

  **Acceptance Criteria**:

  - [ ] File `crates/polyplug/src/version/mod.rs` exists and compiles (`cargo check -p polyplug` after adding `pub mod version;` to lib.rs — but lib.rs addition is Task 7, so use inline test to verify compile)
  - [ ] `cargo test -p polyplug --lib version` → all 5 unit tests pass

  **QA Scenarios**:

  ```
  Scenario: Unit tests in version/mod.rs all pass
    Tool: Bash (cargo test)
    Preconditions: Task 7 (lib.rs) complete OR test run as --lib targeting the module directly
    Steps:
      1. Run: cargo test -p polyplug --lib 2>&1
      2. Assert: output contains "test version::tests::version_parse_valid ... ok"
      3. Assert: output contains "test version::tests::version_parse_invalid ... ok"
      4. Assert: output contains "test version::tests::version_compatible ... ok"
      5. Assert: output contains "test version::tests::version_display ... ok"
      6. Assert: output contains "test version::tests::compatibility_default_is_strict ... ok"
    Expected Result: All 5 tests pass, exit code 0
    Failure Indicators: Any "FAILED" or "error[E" in output
    Evidence: .sisyphus/evidence/task-1-unit-tests.txt

  Scenario: parse rejects malformed version strings
    Tool: Bash (cargo test)
    Preconditions: Same as above
    Steps:
      1. Run: cargo test -p polyplug --lib version::tests::version_parse_invalid 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "ok"
    Expected Result: Test passes confirming "1", "1.2.3", "", "not_a_version" all return Err
    Evidence: .sisyphus/evidence/task-1-parse-invalid.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-1-unit-tests.txt` — full cargo test output

  **Commit**: YES (groups with Task 3)
  - Message: `feat(version): add Version struct, Compatibility enum, parse and compare logic`
  - Files: `crates/polyplug/src/version/mod.rs`, `crates/polyplug/src/lib.rs`
  - Pre-commit: `cargo test -p polyplug --lib`

---

- [x] 2. Replace `graph::Version` with `version::Version` in `crates/polyplug/src/graph/mod.rs`

  **What to do**:
  - Remove the existing `Version` struct definition at `graph/mod.rs:20-25` (the 3-field struct with `pub patch: u32`)
  - Add `use crate::version::Version;` at the top of the file (with the other `use` statements, after Task 1 creates `version/mod.rs` and after Task 7 adds `pub mod version;` to lib.rs — but since this task runs in Wave 1 alongside Task 7 not yet done, use `crate::version::Version` path; the module will exist once Task 1 is done and lib.rs is updated)
  
  **IMPORTANT**: Task 7 (adding `pub mod version;` to `lib.rs`) must be done before this compiles. Coordinate: do Task 1 first, then Task 7 should be done before or alongside Task 2 — but Task 7 is Wave 3. **SOLUTION**: This task can be written and staged; it will only fail to compile until Task 7 is done. The executor should be aware: Tasks 1+2+3 can be written in Wave 1, but compilation of the whole crate will succeed only after Task 7 adds the module declaration. Run `cargo check` after Task 7 to verify.
  
  - Update the `ContractCapability` struct — field `version: Version` continues to work (same name, new type with 2 fields instead of 3)
  - Update `graph/mod.rs:38-45` — `ContractCapability::new()` receives a `Version` — no change needed (it just passes through)
  - Update the 4 construction sites where `Version { major: 1, minor: 0, patch: 0 }` is used:
    - Line 211: `Version { major: 1, minor: 0, patch: 0 }` → `Version { major: 1, minor: 0 }`
    - Line 233: same change
    - Line 272: same change
    - Line 309: same change
    (Verify line numbers by reading the file — exact line numbers are approximate)
  - Update the `#[cfg(test)]` helper at approximately line 306: `make_capability` constructs `Version { major, minor, patch: 0 }` → `Version { major, minor }`
  - All other `Version` usages (field access `.major`, `.minor`) continue to work unchanged

  **Must NOT do**:
  - Do NOT add a `patch` field to the new `Version` — 2 fields only
  - Do NOT change the `ContractCapability` struct shape in any other way
  - Do NOT change `CapabilityGraph` logic — only the `Version` type changes

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Mechanical replacement of one type with another, 5 total edits (1 deletion + 3 production sites + 1 test helper), no logic changes
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 1 and 3 — can be written concurrently with Task 1, though it depends on Task 1 for compilation)
  - **Parallel Group**: Wave 1 (written concurrently; compilation dependency on Task 1 is acceptable — commit after Task 7)
  - **Blocks**: Task 4 (runtime reads graph which uses Version)
  - **Blocked By**: Task 1 (version module must exist for the import)

  **References**:

  **Pattern References**:
  - `crates/polyplug/src/graph/mod.rs:6-17` — existing `use` block at file top (add `use crate::version::Version;` here)
  - `crates/polyplug/src/graph/mod.rs:19-25` — the `Version` struct to DELETE
  - `crates/polyplug/src/graph/mod.rs:209-217` — first construction site `Version { major: 1, minor: 0, patch: 0 }`
  - `crates/polyplug/src/graph/mod.rs:233-239` — second construction site
  - `crates/polyplug/src/graph/mod.rs:272-278` — third construction site
  - `crates/polyplug/src/graph/mod.rs:309-315` — fourth construction site
  - `crates/polyplug/src/graph/mod.rs:306-315` — test helper `make_capability` using `Version { major, minor, patch: 0 }`

  **Acceptance Criteria**:

  - [ ] `graph/mod.rs` has NO definition of a `Version` struct (deleted)
  - [ ] `graph/mod.rs` imports `use crate::version::Version;`
  - [ ] No `patch` field in any `Version` construction in graph/mod.rs
  - [ ] `cargo test -p polyplug --test integration_graph` → passes (after Task 7 adds lib.rs module)

  **QA Scenarios**:

  ```
  Scenario: graph tests still pass after Version type replacement
    Tool: Bash (cargo test)
    Preconditions: Task 1 done, Task 7 done (lib.rs has pub mod version)
    Steps:
      1. Run: cargo test -p polyplug --test integration_graph 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test result: ok"
    Expected Result: All graph integration tests pass
    Failure Indicators: "error[E0425]" (unresolved import) or "FAILED" in output
    Evidence: .sisyphus/evidence/task-2-graph-tests.txt

  Scenario: No patch field in graph module
    Tool: Bash (grep)
    Preconditions: File edited
    Steps:
      1. Run: grep -n "patch" crates/polyplug/src/graph/mod.rs 2>&1
      2. Assert: output is empty (no "patch" references in the file)
    Expected Result: Zero lines containing "patch"
    Evidence: .sisyphus/evidence/task-2-no-patch.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-2-graph-tests.txt`
  - [ ] `.sisyphus/evidence/task-2-no-patch.txt`

  **Commit**: YES (groups with Task 1)
  - Message: `refactor(graph): replace 3-field graph::Version with 2-field version::Version`
  - Files: `crates/polyplug/src/graph/mod.rs`
  - Pre-commit: `cargo check -p polyplug` (not test — lib.rs may not have module yet)

---

- [x] 3. Add `VersionMismatch` and `FunctionCountMismatch` variants to `LoaderError` in `crates/polyplug/src/error/mod.rs`

  **What to do**:
  - Add `use crate::version::Version;` at the top of `error/mod.rs` (with existing `use` statements)
  - Add two new variants to the `LoaderError` enum (after the existing variants, before the closing `}`):
    ```rust
    #[error("version mismatch for contract `{contract}`: required={required}, found={found}")]
    VersionMismatch {
        contract: String,
        required: Version,
        found: Version,
    },

    #[error("function count mismatch for contract `{contract}`: expected={expected}, found={found}")]
    FunctionCountMismatch {
        contract: String,
        expected: u32,
        found: u32,
    },
    ```
  - The `{required}` and `{found}` in `VersionMismatch` will use `Version`'s `Display` impl (implemented in Task 1 — they must coordinate; Task 3 depends on Task 1 being done first for the type to exist)
  - No other changes to `error/mod.rs`

  **Must NOT do**:
  - Do NOT add variants to `RuntimeError` or `GraphError` — only `LoaderError`
  - Do NOT change existing variant names, fields, or `#[error]` strings

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Additive-only change to an existing enum, 2 new variants, exact spec provided
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 1 and 2 — but depends on Task 1 for the `Version` type)
  - **Parallel Group**: Wave 1 (write concurrently; compile after Task 1 is complete)
  - **Blocks**: Task 4 (runtime uses these error variants)
  - **Blocked By**: Task 1 (for `Version` type in `VersionMismatch`)

  **References**:

  **Pattern References**:
  - `crates/polyplug/src/error/mod.rs:51-56` — `AbiVersionMismatch` variant style (follow this pattern for `VersionMismatch`)
  - `crates/polyplug/src/error/mod.rs:43-127` — full `LoaderError` enum — add new variants at the end before `}`
  - `crates/polyplug/src/error/mod.rs:1-4` — top-level `use` placement

  **API/Type References**:
  - `crates/polyplug/src/version/mod.rs` (Task 1 output) — `Version` struct with `Display` impl

  **Acceptance Criteria**:

  - [ ] `LoaderError::VersionMismatch { contract: "foo".to_owned(), required: Version { major: 1, minor: 0 }, found: Version { major: 0, minor: 9 } }.to_string()` compiles and produces the expected string
  - [ ] `LoaderError::FunctionCountMismatch { contract: "foo".to_owned(), expected: 3, found: 2 }.to_string()` compiles
  - [ ] `cargo check -p polyplug` → zero errors (after Task 1 and Task 7 are done)

  **QA Scenarios**:

  ```
  Scenario: New error variants format correctly
    Tool: Bash (cargo test)
    Preconditions: Task 1 done, Task 7 done
    Steps:
      1. Run: cargo test -p polyplug --lib error 2>&1
      2. Assert: exit code 0 (no compile errors)
      3. Run: cargo check -p polyplug 2>&1
      4. Assert: exit code 0, no "error[E" lines
    Expected Result: Crate compiles cleanly
    Failure Indicators: "error[E0412]" (type not found) or "error[E0277]" (Display not implemented)
    Evidence: .sisyphus/evidence/task-3-error-compile.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-3-error-compile.txt`

  **Commit**: YES (groups with Task 1)
  - Message: `feat(error): add VersionMismatch and FunctionCountMismatch to LoaderError`
  - Files: `crates/polyplug/src/error/mod.rs`
  - Pre-commit: `cargo check -p polyplug`

---

- [x] 4. Update `crates/polyplug/src/runtime/mod.rs` — `LoadOptions`, `RuntimeBuilder`, warning callback, negotiation

  **What to do**:

  **Step A — Update `use` block at file top** (add these two imports with the existing `use` statements):
  ```rust
  use crate::version::Compatibility;
  use crate::version::Version;
  ```

  **Step B — Replace `LoadOptions` (currently at line 103 as empty struct)**:
  ```rust
  /// Options for `Runtime::load_bundle_with`.
  ///
  /// The `compatibility` field overrides the global `RuntimeBuilder::compatibility` setting
  /// for this specific bundle load only.
  pub struct LoadOptions {
      pub compatibility: Compatibility,
      pub ignore_function_count_mismatch: bool,
  }
  ```

  **Step C — Add global warning callback after `GLOBAL_EXTENSION_MAP` (around line 49)**:
  ```rust
  /// Global warning callback. Set once via `RuntimeBuilder::on_warning()`.
  ///
  /// Only the first registered warning callback takes effect.
  /// Subsequent registrations are silently ignored (OnceLock semantics).
  /// Test binaries needing different callbacks must be separate test binaries.
  static GLOBAL_WARNING_CB: OnceLock<Box<dyn Fn(&str) + Send + Sync>> = OnceLock::new();

  /// Emit a warning through the registered callback, or fall back to stderr.
  pub(crate) fn emit_warning(msg: &str) {
      match GLOBAL_WARNING_CB.get() {
          Some(cb) => cb(msg),
          None => eprintln!("[polyplug] warning: {msg}"),
      }
  }
  ```

  **Step D — Add fields to `RuntimeBuilder`** (currently at lines 106-110):
  ```rust
  pub struct RuntimeBuilder {
      plugin_dirs: Vec<PathBuf>,
      loaders: Vec<Box<dyn BundleLoader>>,
      extensions: Vec<Box<dyn Extension>>,
      compatibility: Compatibility,
      warning_cb: Option<Box<dyn Fn(&str) + Send + Sync + 'static>>,
  }
  ```

  **Step E — Update `RuntimeBuilder::new()`** to initialize new fields:
  ```rust
  pub fn new() -> RuntimeBuilder {
      RuntimeBuilder {
          plugin_dirs: Vec::new(),
          loaders: Vec::new(),
          extensions: Vec::new(),
          compatibility: Compatibility::default(),
          warning_cb: None,
      }
  }
  ```

  **Step F — Add builder methods** after `extension()`:
  ```rust
  /// Set the global compatibility mode for version negotiation.
  /// Defaults to `Compatibility::Strict`.
  pub fn compatibility(mut self, c: Compatibility) -> RuntimeBuilder {
      self.compatibility = c;
      self
  }

  /// Register a warning callback.
  ///
  /// Only the first registered callback takes effect (OnceLock semantics).
  /// The callback receives human-readable warning strings.
  pub fn on_warning(mut self, cb: impl Fn(&str) + Send + Sync + 'static) -> RuntimeBuilder {
      self.warning_cb = Some(Box::new(cb));
      self
  }
  ```

  **Step G — In `RuntimeBuilder::build()`**, after the `set_global_registry` call and before the discovery phase, install the warning callback:
  ```rust
  // Install warning callback if provided (OnceLock::set returns Err when already set — expected).
  if let Some(cb) = self.warning_cb {
      let _: Result<(), Box<dyn Fn(&str) + Send + Sync>> = GLOBAL_WARNING_CB.set(cb);
  }
  ```

  **Step H — Add `validate_bundle_compatibility` function** (standalone `pub(crate)` fn after the `impl RuntimeBuilder` block):
  ```rust
  /// Validate version compatibility for all discovered bundles.
  ///
  /// Iterates each bundle's dependencies. For each dependency with a `min_version`,
  /// finds the provider bundle and compares versions.
  /// Also checks that each provided contract has a `function_count` entry.
  ///
  /// Behaviour depends on `compatibility`:
  /// - `Strict`: returns `Err` on any mismatch
  /// - `Relaxed`: emits warning, continues
  /// - `Yolo`: silently ignores all mismatches
  pub(crate) fn validate_bundle_compatibility(
      manifests: &[(PathBuf, ManifestData)],
      compatibility: Compatibility,
  ) -> Result<(), RuntimeError> {
      // Build provider_version_map: bundle_name -> parsed Version (from manifest.version)
      // Build provider_function_count_map: (bundle_name, contract_name) -> count
      // For each manifest (the consumer):
      //   For each resolved dependency:
      //     Look up provider bundle by contract name (find which bundle provides it)
      //     Parse provider version (use Version::parse(provider_manifest.version, &provider_name))
      //     Parse consumer min_version (use Version::parse(dep.min_version, &consumer_name))
      //     Call provided_version.is_compatible_with(&required_version)
      //     If not compatible:
      //       Strict => return Err(RuntimeError::Loader(LoaderError::VersionMismatch { ... }))
      //       Relaxed => emit_warning(&format!("..."))
      //       Yolo => continue
      //   For each contract in manifest.provides:
      //     Check function_count contains an entry for this contract
      //     If missing:
      //       Strict => return Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch { ... }))
      //       Relaxed => emit_warning(...)
      //       Yolo => continue
      Ok(())  // placeholder — implement the above logic
  }
  ```

  **Implementation detail for finding the provider**: build a `HashMap<String, &ManifestData>` mapping contract names to their provider manifests. Iterate all manifests; for each entry in `manifest.provides`, insert `provides_entry -> manifest`.

  **Implementation detail for `function_count` check**: The `function_count` map in `ManifestData` uses keys like `"contractname@major"`. Check for the existence of key `format!("{}@{}", contract_name, major)` where `major` is parsed from the provider's `version` field. If no `function_count` entry exists for any contract listed in `provides`, that is a `FunctionCountMismatch` (expected: unknown, found: 0 — use `expected: 0, found: 0` to signal "missing entry").

  **Implementation detail for empty/missing `manifest.version`**: `ManifestData.version` is `String` and may be empty (`""`) for bundles that don't set it. When parsing the provider version:
  - If `manifest.version.is_empty()`: treat as `Version { major: 0, minor: 0 }` (do NOT call `Version::parse` — it would error). Use a local helper:
    ```rust
    fn parse_manifest_version(v: &str, bundle_name: &str) -> Result<Version, RuntimeError> {
        if v.is_empty() {
            Ok(Version { major: 0, minor: 0 })
        } else {
            Version::parse(v, bundle_name).map_err(|e| RuntimeError::Loader(e))
        }
    }
    ```
  - A `version = ""` bundle with dependents will thus appear as v0.0, which is likely incompatible with any consumer requesting `min_version = "1.0"` — Strict mode will return `VersionMismatch`, Relaxed will warn, Yolo will ignore. This is the correct behavior.
  **Step I — Call `validate_bundle_compatibility` in `build()`**:
  In the `if !discovered.is_empty()` block, after `CapabilityGraph::from_manifests(&discovered)` (Phase 2), add Phase 2.5:
  ```rust
  // Phase 2.5: Validate version compatibility
  validate_bundle_compatibility(&discovered, self.compatibility)?;
  ```

  **Step J — Update `load_bundle`**:
  The `load_bundle` method currently calls `self.load_bundle_with(path, LoadOptions {})`. Update to:
  ```rust
  pub fn load_bundle(&self, path: &Path) -> Result<(), PolyplugError> {
      self.load_bundle_with(path, LoadOptions {
          compatibility: Compatibility::default(),
          ignore_function_count_mismatch: false,
      })
  }
  ```

  **Step K — Update `load_bundle_with` signature and body**:
  - Change `_opts: LoadOptions` to `opts: LoadOptions` (remove the underscore — we now use it)
  - After parsing the manifest (around line 351-353), add version validation for this single bundle:
  ```rust
  // Validate version compatibility for this explicit load
  // (single-bundle load: no cross-bundle negotiation possible,
  //  but validate function_count entries exist)
  if !opts.ignore_function_count_mismatch {
      for contract in &manifest.provides {
          let major_str: &str = manifest.version.split('.').next().unwrap_or("0");
          let key: String = format!("{}@{}", contract, major_str);
          if !manifest.function_count.contains_key(&key) && opts.compatibility != Compatibility::Yolo {
              let msg: String = format!(
                  "bundle {:?} provides {:?} but has no function_count entry for key {:?}",
                  manifest.bundle_name, contract, key
              );
              if opts.compatibility == Compatibility::Strict {
                  return Err(PolyplugError::Loader(LoaderError::FunctionCountMismatch {
                      contract: contract.clone(),
                      expected: 0,
                      found: 0,
                  }));
              } else {
                  emit_warning(&msg);
              }
          }
      }
  }
  ```
  Note: `split('.').next()` is acceptable here because it's extracting the major component for a map key lookup, not performing version comparison. Use `unwrap_or("0")` as the fallback — this is an infallible operation on a `str`.

  **Must NOT do**:
  - Do NOT use `.unwrap()` except in the one `unwrap_or` for major extraction (which is safe as shown above — prefer doing it with a match or `split_once` instead if possible)
  - Do NOT add any new `unsafe` blocks
  - Do NOT change the `Registry`, `LoadedBundle`, `HostVTable`, or `PluginVTable` types
  - Do NOT add `Compatibility` to `Runtime` struct (it belongs only on `RuntimeBuilder` and `LoadOptions`)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multi-step changes to a complex file; requires careful integration of 8+ distinct code changes; needs understanding of existing OnceLock patterns
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 5)
  - **Parallel Group**: Wave 2 (after Wave 1 complete)
  - **Blocks**: Task 6 (tests depend on the runtime API)
  - **Blocked By**: Tasks 1, 2, 3 (all must be complete for types and error variants to exist)

  **References**:

  **Pattern References**:
  - `crates/polyplug/src/runtime/mod.rs:44-66` — `GLOBAL_REGISTRY` and `GLOBAL_EXTENSION_MAP` OnceLock pattern (follow for `GLOBAL_WARNING_CB`)
  - `crates/polyplug/src/runtime/mod.rs:45` — `static GLOBAL_REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();` — exact OnceLock pattern
  - `crates/polyplug/src/runtime/mod.rs:106-110` — `RuntimeBuilder` struct definition to extend
  - `crates/polyplug/src/runtime/mod.rs:112-146` — existing builder methods (`plugin_dir`, `loader`, `extension`) — follow the pattern for `compatibility` and `on_warning`
  - `crates/polyplug/src/runtime/mod.rs:153-280` — `RuntimeBuilder::build()` — insert Phase 2.5 call after line ~220 (after `from_manifests`)
  - `crates/polyplug/src/runtime/mod.rs:336-375` — `load_bundle` and `load_bundle_with` — update both
  - `crates/polyplug/src/extensions/trace/mod.rs` — example of storing a callback in a struct field and calling it

  **API/Type References**:
  - `crates/polyplug/src/version/mod.rs` (Task 1) — `Version`, `Compatibility`, `Version::parse()`, `is_compatible_with()`
  - `crates/polyplug/src/error/mod.rs` (Task 3) — `LoaderError::VersionMismatch`, `LoaderError::FunctionCountMismatch`
  - `crates/polyplug/src/loader/manifest/mod.rs:79-109` — `ManifestData` struct fields: `version: String`, `provides: Vec<String>`, `function_count: HashMap<String, u32>`, `dependencies: Vec<RawManifestDependency>`, `bundle_name: String`
  - `crates/polyplug/src/loader/manifest/mod.rs:59-73` — `ManifestDependency` enum variants with `min_version: String` field

  **Acceptance Criteria**:

  - [ ] `LoadOptions` has two public fields: `compatibility: Compatibility` and `ignore_function_count_mismatch: bool`
  - [ ] `RuntimeBuilder::compatibility(self, c: Compatibility)` exists and returns `RuntimeBuilder`
  - [ ] `RuntimeBuilder::on_warning(self, cb: impl Fn(&str) + Send + Sync + 'static)` exists and returns `RuntimeBuilder`
  - [ ] `emit_warning(msg: &str)` function exists in `runtime/mod.rs` as `pub(crate)`
  - [ ] `validate_bundle_compatibility` function exists
  - [ ] `cargo check -p polyplug` → zero errors
  - [ ] `cargo test -p polyplug --lib` → all existing runtime unit tests pass (no regressions)

  **QA Scenarios**:

  ```
  Scenario: Existing runtime unit tests still pass
    Tool: Bash (cargo test)
    Preconditions: Tasks 1, 2, 3 complete; Task 7 complete
    Steps:
      1. Run: cargo test -p polyplug --lib runtime 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test result: ok"
      4. Assert: output does not contain "FAILED"
    Expected Result: All existing runtime unit tests pass
    Failure Indicators: "FAILED" or "error[E" in output
    Evidence: .sisyphus/evidence/task-4-runtime-unit-tests.txt

  Scenario: Compatible version loads successfully (Strict mode)
    Tool: Bash (cargo test)
    Preconditions: Task 6 (integration tests) complete
    Steps:
      1. Run: cargo test -p polyplug --test integration_version compatible_exact_version_strict_loads_ok 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "ok"
    Expected Result: Test passes — provider v1.0 satisfies consumer min_version v1.0 in Strict mode
    Evidence: .sisyphus/evidence/task-4-strict-compat.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-4-runtime-unit-tests.txt`
  - [ ] `.sisyphus/evidence/task-4-strict-compat.txt`

  **Commit**: YES (solo commit)
  - Message: `feat(runtime): add version negotiation, Compatibility mode, and warning callback`
  - Files: `crates/polyplug/src/runtime/mod.rs`
  - Pre-commit: `cargo test -p polyplug --lib`

---

- [x] 5. Update Rust codegen to emit `REQUIRED_VERSION` and `REQUIRED_FUNCTION_COUNT` constants in `host/types.rs`

  **What to do**:
  - Open `crates/polyplugc/src/generators/rust/mod.rs`
  - Find the section that generates the `host/types.rs` content (currently around lines 40-78) — specifically after the block that emits `DEP_*` and `DEP_*_MIN_VERSION` constants
  - After generating per-contract type definitions (the `for contract in &ir.contracts` loop at around line 50-57), add a NEW block that emits two constants per contract in `ir.contracts`:
    ```rust
    // After the existing type generation loops, before push to files:
    for contract in &ir.contracts {
        let contract_upper: String = contract.name
            .to_uppercase()
            .replace(['.', '-'], "_");
        // The required version comes from the contract definition in the IR
        // (contract.version.major and contract.version.minor)
        types_out.push_str(&format!(
            "pub const {contract_upper}_REQUIRED_VERSION: polyplug::version::Version = \
             polyplug::version::Version {{ major: {major}, minor: {minor} }};\n",
            major = contract.version.major,
            minor = contract.version.minor,
        ));
        types_out.push_str(&format!(
            "pub const {contract_upper}_REQUIRED_FUNCTION_COUNT: u32 = {};\n",
            contract.functions.len()
        ));
    }
    types_out.push('\n');
    ```
  - **Placement**: This loop goes OUTSIDE the `if let Some(ref bundle) = ir.bundle` block (which wraps the `DEP_*` constants). The `_REQUIRED_VERSION` and `_REQUIRED_FUNCTION_COUNT` constants are contract-level, not bundle-level — they should always be emitted when there are contracts, even for plugin-side code that doesn't have a bundle declaration. Add the loop AFTER the closing `}` of `if let Some(ref bundle)` (after line ~78 in the current file), before `files.files.push(GeneratedFile { path: "host/types.rs", ... })` at line ~80.
  - **Confirm**: `ResolvedContract.version` has `pub major: u32, pub minor: u32, pub patch: u32` in `ir/mod.rs` — use only `major` and `minor` for the 2-field `polyplug::version::Version` constants.
  - These constants are for app-developer ergonomics only — they are NOT read by the runtime negotiation logic. Do NOT wire them into the runtime.

  **Must NOT do**:
  - Do NOT modify any other generator (C++, Python, Lua, C#, JS) — only the Rust generator
  - Do NOT modify the runtime to read these constants — they are purely informational for app developers
  - Do NOT change any existing constant generation (the `MY_BUNDLE_ID`, `DEP_*`, `DEP_*_MIN_VERSION` blocks must remain unchanged)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Additive-only change to codegen; add a small string-generation block; no logic or architecture changes
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 4)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 6 (tests may verify constants appear in generated output)
  - **Blocked By**: Task 1 (for the `Version` type path; and to understand the type shape)

  **References**:

  **Pattern References**:
  - `crates/polyplugc/src/generators/rust/mod.rs:59-78` — existing constant generation for `MY_BUNDLE_ID`, `DEP_*`, `DEP_*_MIN_VERSION` — follow this exact pattern for the new constants
  - `crates/polyplugc/src/generators/rust/mod.rs:60-63` — `format!("pub const {} ...", ...)` pattern

  **API/Type References**:
  - `crates/polyplugc/src/ir/mod.rs` — `ResolvedContract` struct — check for `version`, `functions`, `name` fields
  - `crates/polyplug/src/version/mod.rs` (Task 1) — `Version { major: u32, minor: u32 }` — this is the type the constants reference with the fully-qualified path `polyplug::version::Version`

  **Test References**:
  - `tests/integration_codegen_rust/mod.rs` — existing Rust codegen integration test — add a check that the new constants appear in generated output for the test contract

  **Acceptance Criteria**:

  - [ ] Generated `host/types.rs` for a contract named `"image.decode"` with version `1.0` and 3 functions contains:
    - `pub const IMAGE_DECODE_REQUIRED_VERSION: polyplug::version::Version = polyplug::version::Version { major: 1, minor: 0 };`
    - `pub const IMAGE_DECODE_REQUIRED_FUNCTION_COUNT: u32 = 3;`
  - [ ] Existing codegen integration test `integration_codegen_rust` still passes
  - [ ] `cargo test -p polyplugc --test integration_codegen_rust` → green

  **QA Scenarios**:

  ```
  Scenario: Generated host/types.rs contains REQUIRED_VERSION constant
    Tool: Bash (cargo test)
    Preconditions: polyplugc compiles cleanly
    Steps:
      1. Run: cargo test -p polyplug --test integration_codegen_rust 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "ok"
    Expected Result: Codegen test passes, verifying new constants are emitted
    Evidence: .sisyphus/evidence/task-5-codegen-test.txt

  Scenario: Old constants still generated (no regression)
    Tool: Bash (cargo test)
    Preconditions: Same
    Steps:
      1. Run: cargo test -p polyplug --test integration_codegen_rust 2>&1
      2. Assert: no "FAILED" in output
      3. Assert: no regression on DEP_* or MY_BUNDLE_ID constants
    Expected Result: All codegen tests pass
    Evidence: .sisyphus/evidence/task-5-codegen-regression.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-5-codegen-test.txt`

  **Commit**: YES (solo commit)
  - Message: `feat(codegen/rust): emit REQUIRED_VERSION and REQUIRED_FUNCTION_COUNT constants in host/types.rs`
  - Files: `crates/polyplugc/src/generators/rust/mod.rs`
  - Pre-commit: `cargo test -p polyplug --test integration_codegen_rust`

---

- [x] 6. Write `tests/integration_version/mod.rs` — 14 integration tests for version negotiation

  **What to do**:
  - Create new file `tests/integration_version/mod.rs`
  - Add `#![allow(clippy::expect_used)]` at the top (test files may use `.expect()`)
  - Add `use` statements at the top (ALL imports at file top — AGENTS.md Rule 2):
    ```rust
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;
    use polyplug::runtime::Runtime;
    use polyplug::runtime::LoadOptions;
    use polyplug::version::Compatibility;
    use polyplug::version::Version;
    use polyplug::error::LoaderError;
    use polyplug::error::RuntimeError;
    use tempfile::TempDir;
    ```
  - Use a helper function `write_manifest(dir: &TempDir, bundle_name: &str, version: &str, provides: &[&str], deps: &[DepSpec]) -> std::path::PathBuf` that writes a `.manifest.toml` file. Define `DepSpec` as a struct in the test file with fields needed to construct `[[dependency]]` entries.
  - Each test creates its own `TempDir` and writes minimal manifest TOMLs. The `provides` list and `version` field are the key variables. No actual `.so` files are needed for tests that test discovery-level errors (the error happens before `dlopen`).

  **IMPORTANT — Warning Callback Thread-Safety**:
  The `GLOBAL_WARNING_CB` OnceLock is set once per process. Tests that need to capture warnings MUST be in tests that run in isolation (separate test binary) OR use the same callback. Since `integration_version` is its own test binary (separate `[[test]]` stanza in Cargo.toml), all 14 tests share ONE process and thus ONE OnceLock. **Solution**: register the warning callback ONCE in a `std::sync::OnceLock<()>` setup function at the top of the test file, shared across tests that need it:
  ```rust
  static WARNING_SINK: OnceLock<Arc<Mutex<Vec<String>>>> = OnceLock::new();

  fn shared_warning_sink() -> Arc<Mutex<Vec<String>>> {
      Arc::clone(WARNING_SINK.get_or_init(|| {
          Arc::new(Mutex::new(Vec::new()))
      }))
  }
  ```
  Tests that don't need to inspect warnings can use `Compatibility::Strict` (no callback needed). Tests that need `Relaxed`/`Yolo` should not register their own callback — they use the shared sink. The `Runtime::builder().on_warning(...)` call installs the callback into the global `GLOBAL_WARNING_CB`; after the first test that calls `.on_warning()`, subsequent calls are silently ignored.

  **CRITICAL**: Only ONE `Runtime::builder()` chain across ALL 14 tests should call `.on_warning(cb)`. The pattern is:
  ```rust
  // Called once per process (OnceLock ensures this)
  fn ensure_warning_registered() {
      static REGISTER: std::sync::OnceLock<()> = std::sync::OnceLock::new();
      REGISTER.get_or_init(|| {
          let sink: Arc<Mutex<Vec<String>>> = shared_warning_sink();
          // This one builder only exists to register the callback.
          // It builds with no plugin_dirs, so it succeeds immediately.
          let _: Result<_, _> = Runtime::builder()
              .on_warning(move |msg: &str| {
                  sink.lock().expect("lock").push(msg.to_owned());
              })
              .build();
      });
  }
  ```
  Tests 6, 9, 12 (Relaxed warns) call `ensure_warning_registered()` at the start, then build their own `Runtime` instances with `TempDir` manifests for the actual test. After the test load call, they read from `shared_warning_sink()` to inspect warnings. Tests 5, 8, 11 (Strict errors) do NOT call `ensure_warning_registered()` and do NOT need the callback.

  **Manifest TOML format** (write using `std::fs::write`):
  ```toml
  runtime = "native"
  bundle_name = "{name}"
  version = "{version_string}"
  provides = ["{contract_name}"]
  [function_count]
  "{contract_name}@{major}" = {count}

  [[dependency]]
  kind = "contract"
  contract = "{contract_name}"
  min_version = "{min_version}"
  # NOTE: DO NOT include a `bundle` field for ByContract dependencies.
  # `bundle: Option<String>` defaults to None when absent (via #[serde(default)]).
  # Writing `bundle = ""` would produce Some("") and be treated as ByBundle, causing failures.
  contract_id = {contract_id_u64}
  ```

  **The 14 tests** (one `#[test]` per):

  1. `compatible_exact_version_strict_loads_ok`
     - Provider: `version = "1.0"`, provides `"test.contract"`, `function_count = { "test.contract@1" = 2 }`
     - Consumer: depends on `"test.contract"` with `min_version = "1.0"`
     - Build: `Runtime::builder().plugin_dir(dir).compatibility(Compatibility::Strict).build()`
     - Assert: `Ok(runtime)` (no error)

  2. `compatible_superset_version_strict_loads_ok`
     - Provider: `version = "1.2"`, consumer min_version `"1.0"`, Strict
     - Assert: `Ok(runtime)`

  3. `compatible_superset_version_relaxed_loads_ok`
     - Provider: `version = "1.2"`, consumer min_version `"1.0"`, Relaxed
     - Assert: `Ok(runtime)` (no mismatch → no warning)

  4. `compatible_superset_version_yolo_loads_ok`
     - Provider: `version = "1.2"`, consumer min_version `"1.0"`, Yolo
     - Assert: `Ok(runtime)`

  5. `too_old_strict_returns_version_mismatch`
     - Provider: `version = "1.0"`, consumer min_version `"1.2"`, Strict
     - Assert: `Err(RuntimeError::Loader(LoaderError::VersionMismatch { .. }))` using `matches!`

  6. `too_old_relaxed_warns_and_loads`
     - Provider: `version = "1.0"`, consumer min_version `"1.2"`, Relaxed
     - Assert: `Ok(runtime)` AND warnings sink has at least one entry containing `"version mismatch"` (case-insensitive)

  7. `too_old_yolo_loads_silently`
     - Provider: `version = "1.0"`, consumer min_version `"1.2"`, Yolo
     - Assert: `Ok(runtime)` AND no warning emitted for this specific case
     (Since shared sink is shared, check count before and after — or simply assert `Ok`)

  8. `major_mismatch_strict_returns_version_mismatch`
     - Provider: `version = "1.0"`, consumer min_version `"2.0"`, Strict
     - Assert: `Err(RuntimeError::Loader(LoaderError::VersionMismatch { .. }))`

  9. `major_mismatch_relaxed_warns_and_loads`
     - Provider: `version = "1.0"`, consumer min_version `"2.0"`, Relaxed
     - Assert: `Ok(runtime)` AND warning emitted

  10. `major_mismatch_yolo_loads_silently`
      - Provider: `version = "1.0"`, consumer min_version `"2.0"`, Yolo
      - Assert: `Ok(runtime)`

  11. `function_count_mismatch_strict_returns_error`
      - Provider: `version = "1.0"`, provides `"test.contract"`, NO `function_count` entry
      - Consumer: depends on `"test.contract"` with `min_version = "1.0"`, Strict
      - Assert: `Err(RuntimeError::Loader(LoaderError::FunctionCountMismatch { .. }))`

  12. `function_count_mismatch_relaxed_warns_and_loads`
      - Same as 11 but Relaxed
      - Assert: `Ok(runtime)` AND warning emitted

  13. `function_count_mismatch_yolo_ignored`
      - Same as 11 but Yolo
      - Assert: `Ok(runtime)`

  14. `malformed_version_returns_manifest_parse_error`
      - Provider: `version = "not_a_version"`, consumer min_version `"1.0"`, Strict
      - Assert: `Err(RuntimeError::Loader(LoaderError::ManifestParse { .. }))` using `matches!`

  **Per-contract IDs**: Use `polyplug::abi::contract_id("test.contract", 1)` to compute the `contract_id` value to embed in manifest TOML.

  **Must NOT do**:
  - Do NOT use `.unwrap()` except inside the `#![allow(clippy::expect_used)]`-gated test file where `.expect()` is acceptable in test assertions
  - Do NOT require actual compiled `.so` files — tests exercise the validation phase before `dlopen`
  - Do NOT share `TempDir` across tests — each test owns its own temp directory

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 14 tests with careful setup; warning callback OnceLock coordination; manifest TOML construction; nuanced per-test assertions
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 7)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1, F2, F3 (final review)
  - **Blocked By**: Tasks 4 and 5 (runtime API and codegen must be ready)

  **References**:

  **Pattern References**:
  - `tests/integration_load/mod.rs:1-60` — test file structure, imports at top, `#![allow(clippy::expect_used)]`, TempDir pattern
  - `tests/integration_graph/mod.rs` — how `ManifestData` is constructed inline with `RawManifestDependency` — follow this pattern for the helper function
  - `crates/polyplug/src/graph/mod.rs:405-493` — `from_manifests_chain_order` test — shows how to build `ManifestData` structs with dependencies (follow for the manifest-writing helper)
  - `tests/integration_discovery/mod.rs` — how TempDir and manifest files are written in tests
  - `crates/polyplug/src/runtime/mod.rs:552-590` — `ensure_test_plugin_registered` OnceLock pattern for shared test setup — follow for `WARNING_SINK` setup

  **API/Type References**:
  - `crates/polyplug/src/runtime/mod.rs` (Task 4 output) — `Runtime::builder()`, `RuntimeBuilder::compatibility()`, `RuntimeBuilder::on_warning()`, `LoadOptions { compatibility, ignore_function_count_mismatch }`
  - `crates/polyplug/src/version/mod.rs` (Task 1) — `Compatibility`, `Version`
  - `crates/polyplug/src/error/mod.rs` — `LoaderError::VersionMismatch`, `LoaderError::FunctionCountMismatch`, `LoaderError::ManifestParse`, `RuntimeError::Loader`
  - `crates/polyplug/src/loader/manifest/mod.rs:79-109` — `ManifestData` struct (used in graph construction tests for reference)

  **Acceptance Criteria**:

  - [ ] File `tests/integration_version/mod.rs` exists with exactly 14 `#[test]` functions
  - [ ] `cargo test -p polyplug --test integration_version` → 14/14 pass, exit code 0
  - [ ] No existing tests broken: `cargo test -p polyplug` → all tests pass

  **QA Scenarios**:

  ```
  Scenario: All 14 version integration tests pass
    Tool: Bash (cargo test)
    Preconditions: Tasks 1-5, 7 complete
    Steps:
      1. Run: cargo test -p polyplug --test integration_version 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test result: ok. 14 passed"
      4. Assert: output does not contain "FAILED"
    Expected Result: All 14 tests pass
    Failure Indicators: Any "FAILED", "error[E", or panic in output
    Evidence: .sisyphus/evidence/task-6-all-tests.txt

  Scenario: Strict mode rejects version-too-old
    Tool: Bash (cargo test)
    Preconditions: Same
    Steps:
      1. Run: cargo test -p polyplug --test integration_version too_old_strict_returns_version_mismatch -- --nocapture 2>&1
      2. Assert: exit code 0 (test passes — it asserts Err is returned)
      3. Assert: output contains "ok"
    Expected Result: Test confirms VersionMismatch is returned in Strict mode
    Evidence: .sisyphus/evidence/task-6-strict-mismatch.txt

  Scenario: Relaxed mode warns but loads
    Tool: Bash (cargo test)
    Preconditions: Same
    Steps:
      1. Run: cargo test -p polyplug --test integration_version too_old_relaxed_warns_and_loads -- --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "ok"
    Expected Result: Test passes confirming load succeeds and warning was captured
    Evidence: .sisyphus/evidence/task-6-relaxed-warns.txt

  Scenario: No regression in existing integration tests
    Tool: Bash (cargo test)
    Preconditions: Same
    Steps:
      1. Run: cargo test -p polyplug 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test result: ok"
      4. Assert: no "FAILED" lines
    Expected Result: All tests in the crate pass
    Evidence: .sisyphus/evidence/task-6-no-regression.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-6-all-tests.txt`
  - [ ] `.sisyphus/evidence/task-6-no-regression.txt`

  **Commit**: YES (solo commit)
  - Message: `test(integration_version): add 14 integration tests for version negotiation`
  - Files: `tests/integration_version/mod.rs`
  - Pre-commit: `cargo test -p polyplug --test integration_version`

---

- [x] 7. Register `integration_version` test binary in `Cargo.toml`

  **What to do**:

  **Note**: `pub mod version;` was already added to `lib.rs` in Task 1 (same commit). This task only handles the `Cargo.toml` test registration.

  **`crates/polyplug/Cargo.toml`**:
  - Add a new `[[test]]` stanza at the end of the test stanzas (before `[[bench]]` at line 109):
    ```toml
    [[test]]
    name = "integration_version"
    path = "../../tests/integration_version/mod.rs"
    ```

  **Must NOT do**:
  - Do NOT change any existing `pub mod` declarations in `lib.rs` (already done in Task 1)
  - Do NOT change any existing `[[test]]` stanzas in `Cargo.toml`
  - Do NOT add `version` to any `pub use` re-exports

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single trivial additive change in one file, no logic involved
  - **Skills**: none needed

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 6)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1, F2, F3
  - **Blocked By**: Task 6 (file to register must exist before `Cargo.toml` entry is useful)

  **References**:

  **Pattern References**:
  - `crates/polyplug/Cargo.toml:23-108` — existing `[[test]]` stanzas — follow exact format
  - `crates/polyplug/Cargo.toml:59-62` — example `[[test]]` stanza for `integration_graph`

  **Acceptance Criteria**:

  - [ ] `Cargo.toml` contains `[[test]]` stanza with `name = "integration_version"` and `path = "../../tests/integration_version/mod.rs"`
  - [ ] `cargo test -p polyplug --test integration_version` → test binary compiles and runs

  **QA Scenarios**:

  ```
  Scenario: Test binary registered and runs
    Tool: Bash (cargo test)
    Preconditions: Task 6 complete (test file exists)
    Steps:
      1. Run: cargo test -p polyplug --test integration_version 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test result: ok"
    Expected Result: Test binary compiles and all 14 tests pass
    Evidence: .sisyphus/evidence/task-7-test-binary.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-7-test-binary.txt`

  **Commit**: YES (groups with Task 6)
  - Message: `chore(polyplug): register integration_version test binary in Cargo.toml`

  - Files: `crates/polyplug/Cargo.toml`
---
  - Pre-commit: `cargo check -p polyplug`

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 3 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [x] F1. **Plan Compliance Audit** — `oracle`

  Read the plan end-to-end. For each "Must Have": verify implementation exists (read the relevant file, run `cargo check -p polyplug`). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found.
  
  Specifically check:
  - `crates/polyplug/src/version/mod.rs` exists and exports `Version { major, minor }`, `Compatibility`, `Default for Compatibility`, `Version::parse`, `Version::is_compatible_with`, `Display for Version`
  - `crates/polyplug/src/error/mod.rs` — `VersionMismatch` and `FunctionCountMismatch` in `LoaderError`
  - `crates/polyplug/src/runtime/mod.rs` — `LoadOptions` has two fields, `RuntimeBuilder` has `compatibility` and `warning_cb`, `GLOBAL_WARNING_CB` OnceLock exists, `emit_warning` is `pub(crate)`, `validate_bundle_compatibility` exists
  - `crates/polyplug/src/graph/mod.rs` — no `pub struct Version { ... patch` (deleted), has `use crate::version::Version`
  - `crates/polyplug/src/lib.rs` — has `pub mod version;`
  - `crates/polyplugc/src/generators/rust/mod.rs` — emits `_REQUIRED_VERSION` and `_REQUIRED_FUNCTION_COUNT` constants
  - `tests/integration_version/mod.rs` — exists with 14 tests
  - `crates/polyplug/Cargo.toml` — has `[[test]] name = "integration_version"`
  - Search for `.unwrap()` in non-test files: `grep -n '\.unwrap()' crates/polyplug/src/version/mod.rs crates/polyplug/src/error/mod.rs` (outside `#[cfg(test)]`) → must be zero
  - Search for `use` inside function bodies: `grep -n 'fn.*{' crates/polyplug/src/version/mod.rs` then verify no `use` statements inside
  - Run: `cargo check -p polyplug` → zero errors
  
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Files [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`

  Run:
  - `cargo clippy -p polyplug -- -D warnings` → zero warnings
  - `cargo fmt --check -p polyplug` → clean
  - `cargo fmt --check -p polyplugc` → clean
  - `cargo test -p polyplug` → all tests pass (including 14 new)
  - `cargo test -p polyplugc` → all codegen tests pass

  Review changed files for:
  - Any `as any` casts, `@ts-ignore` equivalent (`#[allow(unused)]` unexplained)
  - Empty match arms without `// intentional` comment
  - Any `todo!()` or `unimplemented!()` macros
  - Missing doc comments on public items (all `pub` items in `version/mod.rs` must have doc comments)
  - AI slop: over-generic names (`data`, `result`, `item`), excessive inline comments restating code

  Output: `Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [x] F3. **Scope Fidelity Check** — `deep`

  For each task, read "What to do" and inspect actual diff (`git diff HEAD~7..HEAD` or `git log --oneline` + `git show`). Verify:
  - Task 1: `version/mod.rs` created — only contains `Version`, `Compatibility`, `Display`, `parse`, `is_compatible_with`, and unit tests. Nothing extra.
  - Task 2: `graph/mod.rs` — only change is `Version` struct deleted and `use crate::version::Version` added + 3 production construction sites + 1 test helper updated. No logic changes.
  - Task 3: `error/mod.rs` — only 2 new variants added. No existing variants changed.
  - Task 4: `runtime/mod.rs` — only: `LoadOptions` fields, `RuntimeBuilder` fields, builder methods, warning callback global + `emit_warning`, `validate_bundle_compatibility`, call in `build()`, updated `load_bundle` + `load_bundle_with`. No other changes.
  - Task 5: `generators/rust/mod.rs` — only: added constant emission block. No existing generation changed.
  - Task 6: `tests/integration_version/mod.rs` — new file, exactly 14 tests.
  - Task 7: `lib.rs` — one line added. `Cargo.toml` — one stanza added.
  - Check for cross-task contamination: Task N touching Task M's files.
  - Check "Must NOT Have": no generator changes beyond Rust generator, no ABI struct modifications.
  
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

| Commit | Message | Files |
|--------|---------|-------|
| 1 | `feat(version): add Version struct, Compatibility enum, parse and compare logic` | `crates/polyplug/src/version/mod.rs`, `crates/polyplug/src/lib.rs` |
| 2 | `refactor(graph): replace 3-field graph::Version with 2-field version::Version` | `crates/polyplug/src/graph/mod.rs` |
| 3 | `feat(error): add VersionMismatch and FunctionCountMismatch to LoaderError` | `crates/polyplug/src/error/mod.rs` |
| 4 | `feat(runtime): add version negotiation, Compatibility mode, and warning callback` | `crates/polyplug/src/runtime/mod.rs` |
| 5 | `feat(codegen/rust): emit REQUIRED_VERSION and REQUIRED_FUNCTION_COUNT constants in host/types.rs` | `crates/polyplugc/src/generators/rust/mod.rs` |
| 6 | `test(integration_version): add 14 integration tests for version negotiation` | `tests/integration_version/mod.rs` |
| 7 | `chore(polyplug): register integration_version test binary in Cargo.toml` | `crates/polyplug/Cargo.toml` |

Pre-commit verification for ALL commits: `cargo clippy -p polyplug -- -D warnings && cargo fmt --check -p polyplug`

---

## Success Criteria

### Verification Commands
```bash
cargo clippy -p polyplug -- -D warnings    # Expected: no warnings
cargo fmt --check -p polyplug              # Expected: no formatting issues
cargo fmt --check -p polyplugc             # Expected: no formatting issues
cargo test -p polyplug                     # Expected: all tests pass including 14 new
cargo test -p polyplugc                    # Expected: codegen tests still pass
```

### Final Checklist
- [x] `crates/polyplug/src/version/mod.rs` exists: `Version { major, minor }`, `Compatibility { Strict, Relaxed, Yolo }`, `Default → Strict`, `Version::parse`, `is_compatible_with`, `Display`
- [x] `LoaderError::VersionMismatch` and `FunctionCountMismatch` defined
- [x] `RuntimeBuilder::compatibility()` and `::on_warning()` builder methods exist
- [x] `validate_bundle_compatibility()` called in `RuntimeBuilder::build()`
- [x] `GLOBAL_WARNING_CB` OnceLock installed in `runtime/mod.rs`
- [x] `graph::Version` (3-field) fully replaced — no `patch` field anywhere in `graph/mod.rs`
- [x] Rust codegen emits `_REQUIRED_VERSION` and `_REQUIRED_FUNCTION_COUNT` constants
- [x] 14 integration tests all green
- [x] Zero `.unwrap()` in production code
- [x] All `use` statements at file top (no inline `use`)
- [x] All new module roots at `dirname/mod.rs`
- [x] `cargo clippy -- -D warnings` → zero warnings
- [x] No changes to `PluginVTable`, `HostVTable`, or any `#[repr(C)]` ABI structs
- [x] No changes to Python, Lua, C#, C++, or JS generators
