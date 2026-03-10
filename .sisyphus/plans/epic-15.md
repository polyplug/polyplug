# Epic 15 — Complete polyplugc: Generator Hardening, Incremental Generation, Pack Command, 36 Cross-Language Tests

## TL;DR

> **Quick Summary**: Audits and hardens all seven code generators for correctness and consistency, adds incremental generation with IR-hash-based skipping, adds the `polyplugc pack` command for all seven languages, and runs the full 36-combination cross-language test matrix.
>
> **Deliverables**:
> - All 7 generators emit complete, consistent manifest.toml (all required fields)
> - All 7 generators emit `ffi.metatype` (Lua) and `needs_reinit_on_dep_reload` field
> - `polyplugc generate` prints "regenerated N files, skipped M unchanged"
> - `polyplugc pack --api api.toml --lang <lang> --out <dir>` command working for 7 languages
> - `tests/cross_language/mod.rs`: 36 passing test functions
> - `tests/cross_language_deno/mod.rs`: js-deno combination tests passing
> - Integration codegen tests for C#, Python, Lua, js-quickjs, js-deno (5 new test files)
> - Dead code cleaned: `crates/polyplug-dotnet/src/config/mod.rs` deleted
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 6 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Tasks 5-11 → Task 13 → Task 23 → Tasks 28-32 → Tasks 33-37

---

## Context

### Original Request
Epic 15 — Complete polyplugc for All Generators. Final codegen hardening before showcase.
Covers: generator audit+fixes, consistent manifest conventions, incremental generation,
polyplugc pack command, 36 cross-language combination tests.

### Interview Summary
**Key Discussions**:
- js-deno combination tests: separate file (`tests/cross_language_deno/mod.rs`), not integrated into the 36-matrix
- No additional generator gaps beyond the known list

**Research Findings (code audit)**:
- `needs_reinit_on_dep_reload` missing from parser, IR, ManifestData, AND all 7 generators
- `function_count` missing from C#, Python, Lua, js-quickjs, js-deno manifests
- Lua generator has zero manifest.toml generation
- Python manifest is only `runtime = "python"` — missing all other fields
- js-quickjs/js-deno emit `bundle_name` key; Rust/C++ emit `name` key — both needed
- Lua generator uses `ffi.cdef` + `ffi.cast` but never `ffi.metatype`
- `polyplugc pack` command entirely absent
- `tests/cross_language/` does not exist
- `crates/polyplug-dotnet/src/config/mod.rs` is a stray dead-code duplicate (crate root is `src/lib/mod.rs`)
- `tests/fixtures/test_plugin_js/bundle.js` exists but has no `manifest.toml`
- `tests/fixtures/test_plugin_js_deno/` does not exist

**Items confirmed correct (no work needed)**:
- EXT_TRACE_ID emitted correctly in all 7 generators
- js-quickjs: `polyplug.getExtension(...)` ✓
- js-deno: `Deno.core.ops.op_get_extension(...)` ✓
- `[SuppressGCTransition]` on all hot-path delegates in `host-libs/csharp/src/Abi.cs` ✓
- Python CFUNCTYPE cached at module level ✓
- js-quickjs `{ lo, hi }` for u64 ✓ — js-deno `bigint` for u64 ✓
- Auto-generated headers in all generators ✓

### Metis Review
**Identified Gaps (addressed)**:
- `name` vs `bundle_name` field: both fields must be emitted by all generators. ManifestData already has both; no runtime change needed beyond adding `needs_reinit_on_dep_reload`.
- `requires` field is dead spec superseded by `[[dependency]]` tables — excluded from plan.
- `file` field in Rust/C++ manifests is Linux-specific (`lib{name}.so`). Convention: always `.so` — CI is Linux, showcase targets Linux. Platform-conditional generation is out of scope.
- Integration codegen tests for C#/Python/Lua/js-quickjs/js-deno must be added in this epic (not deferred) since the 36-combination matrix depends on generators being verified independently first.

---

## Work Objectives

### Core Objective
Bring all seven code generators to complete, consistent output across all scenarios (API generation, bundle generation, manifest.toml); add incremental generation and a pack command; verify correctness with 36 cross-language combination tests.

### Concrete Deliverables
- `crates/polyplugc/src/parser/mod.rs`: `needs_reinit_on_dep_reload` field in `RawBundleMeta`
- `crates/polyplugc/src/ir/mod.rs`: `needs_reinit_on_dep_reload: bool` in `ResolvedBundle`
- `crates/polyplug/src/loader/manifest/mod.rs`: `needs_reinit_on_dep_reload: bool` with `#[serde(default)]` in `ManifestData`
- All 7 generators: complete manifest.toml (all 8 required fields + `bundle_name`)
- Lua generator: `ffi.metatype` for all user-defined struct types
- `crates/polyplugc/src/main.rs`: incremental generation with hash cache + stats output
- `crates/polyplugc/src/main.rs`: `pack` subcommand wired
- `crates/polyplugc/src/pack/mod.rs`: pack implementation for all 7 languages
- `tests/cross_language/mod.rs`: 36 test functions
- `tests/cross_language_deno/mod.rs`: js-deno combination tests
- `tests/fixtures/test_plugin_js/manifest.toml`: added
- `tests/fixtures/test_plugin_js_deno/index.ts` + `manifest.toml`: created
- `tests/fixtures/build_all.sh`: created
- `tests/integration_codegen_csharp/mod.rs`, `_python`, `_lua`, `_js_quickjs`, `_js_deno`: 5 new test files
- `crates/polyplug-dotnet/src/config/mod.rs`: deleted (dead code)

### Definition of Done
- [ ] `cargo test --workspace` passes with zero failures
- [ ] `cargo clippy -- -D warnings` passes with zero warnings
- [ ] `cargo test --workspace -- cross_language` shows 36 passes
- [ ] `cargo test --workspace -- cross_language_deno` shows ≥1 pass
- [ ] `polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/t && cat /tmp/t/manifest.toml` shows all 8 required fields
- [ ] Second `polyplugc generate` run prints "skipped N unchanged" where N > 0
- [ ] `polyplugc pack --api tests/fixtures/test_api.toml --lang rust --out /tmp/pack_test` produces `Cargo.toml` + `src/`

### Must Have
- All 8 manifest fields present in every generator: `name`, `bundle_name`, `version`, `runtime`, `file`, `provides`, `function_count`, `needs_reinit_on_dep_reload`
- Lua generator emits `ffi.metatype` for every user-defined struct type
- Incremental generation: manifest.toml always regenerated; other files skipped when IR hash unchanged
- `polyplugc pack` works for all 7 languages
- 36 cross-language test functions (all must pass)
- Separate js-deno combination test file
- `build_all.sh` documents fixture rebuild process

### Must NOT Have (Guardrails)
- Do NOT add `requires` field — it is dead spec, superseded by `[[dependency]]` tables
- Do NOT change the `CodeGenerator` trait signature
- Do NOT use platform-conditional `file` field values — always emit `.so` convention
- Do NOT implement production publishing in pack command — scaffold + metadata only
- Do NOT create abstraction layers/traits for pack formats across languages
- Do NOT add test helper frameworks or test DSLs — each test function is standalone
- Do NOT add more than 2 contract function calls per cross-language test

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (cargo test --workspace)
- **Automated tests**: Tests-after (implementation then tests)
- **Framework**: `cargo test`

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/`.
- Generator output: Bash (run polyplugc, inspect output files)
- Tests: Bash (cargo test --test <name>)
- Clippy: Bash (cargo clippy -- -D warnings)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — all independent, start immediately):
├── Task 1:  Add needs_reinit_on_dep_reload to parser RawBundleMeta [quick]
├── Task 2:  Add needs_reinit_on_dep_reload to IR ResolvedBundle [quick]
├── Task 3:  Add needs_reinit_on_dep_reload to ManifestData [quick]
└── Task 4:  Delete stray crates/polyplug-dotnet/src/config/mod.rs [quick]

Wave 2 (Generator manifest fixes — after Wave 1):
├── Task 5:  Fix Rust generator manifest (add bundle_name, needs_reinit) [quick]
├── Task 6:  Fix C++ generator manifest (add bundle_name, needs_reinit) [quick]
├── Task 7:  Fix C# generator manifest (add file, function_count, bundle_name, needs_reinit) [quick]
├── Task 8:  Fix Python generator manifest (add all missing fields) [quick]
├── Task 9:  Add Lua generator manifest.toml generation (new function, all fields) [quick]
├── Task 10: Fix js-quickjs generator manifest (add name, file, provides, function_count, needs_reinit) [quick]
└── Task 11: Fix js-deno generator manifest (same as js-quickjs) [quick]

Wave 3 (Generator feature fixes — after Wave 2):
├── Task 12: Add ffi.metatype to Lua generator for all user-defined struct types [quick]
└── Task 13: Add incremental generation (IR hash cache, stats output, always-regen manifest) [unspecified-high]

Wave 4 (Pack command — after Wave 3):
├── Task 14: polyplugc pack: CLI subcommand + pack module scaffold [quick]
├── Task 15: pack: Rust language output structure [quick]
├── Task 16: pack: C++ language output structure [quick]
├── Task 17: pack: C# NuGet language output structure [quick]
├── Task 18: pack: Python pip language output structure [quick]
├── Task 19: pack: Lua module language output structure [quick]
├── Task 20: pack: js-quickjs npm + rolldown invocation [unspecified-high]
└── Task 21: pack: js-deno directory + optional rolldown [quick]

Wave 5 (Test fixtures + codegen integration tests — after Wave 2):
├── Task 22: Add manifest.toml to tests/fixtures/test_plugin_js/ [quick]
├── Task 23: Create tests/fixtures/test_plugin_js_deno/ (index.ts + manifest.toml) [quick]
├── Task 24: Create tests/fixtures/build_all.sh [quick]
├── Task 25: integration_codegen_csharp — generate, compile, load, call, assert [unspecified-high]
├── Task 26: integration_codegen_python — generate, run, assert [unspecified-high]
├── Task 27: integration_codegen_lua — generate, run, assert [unspecified-high]
├── Task 28: integration_codegen_js_quickjs — generate, run bundle.js, assert [unspecified-high]
└── Task 29: integration_codegen_js_deno — generate, run index.ts, assert [unspecified-high]

Wave 6 (Cross-language matrix — after Waves 3 + 5):
├── Task 30: tests/cross_language/mod.rs — 36-combination matrix skeleton + Rust×Rust [unspecified-high]
├── Task 31: cross_language — Rust-host rows (Rust×C++, Rust×C#, Rust×Python, Rust×Lua, Rust×js-quickjs) [unspecified-high]
├── Task 32: cross_language — C++ host rows (C++×all 6) [unspecified-high]
├── Task 33: cross_language — C# host rows (C#×all 6) [unspecified-high]
├── Task 34: cross_language — Python host rows (Python×all 6) [unspecified-high]
├── Task 35: cross_language — Lua host rows (Lua×all 6) [unspecified-high]
├── Task 36: cross_language — js-quickjs host rows (js-quickjs×all 6) [unspecified-high]
└── Task 37: tests/cross_language_deno/mod.rs — js-deno combination tests [unspecified-high]

Wave FINAL (After ALL tasks — 4 parallel reviewers):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Code quality review (clippy, no .unwrap, no AI slop) [unspecified-high]
├── Task F3: Real end-to-end QA (all 36 cross-language tests, pack command, incremental) [unspecified-high]
└── Task F4: Scope fidelity check [deep]
```

### Dependency Matrix

- **1, 2, 3, 4**: no deps → wave 1
- **5–11**: need 1, 2, 3 → wave 2
- **12**: needs 9 (Lua generator has manifest function) → wave 3
- **13**: needs 5–11 (manifest force_regenerate flag needs complete manifests) → wave 3
- **14–21**: need 13 → wave 4
- **22–29**: need 5–11 → wave 5 (runs in parallel with waves 3–4)
- **30–37**: need 12, 13, 22, 23 → wave 6

### Agent Dispatch Summary

- **Wave 1**: T1–T4 → `quick`
- **Wave 2**: T5–T11 → `quick`
- **Wave 3**: T12 → `quick`, T13 → `unspecified-high`
- **Wave 4**: T14–T16, T17–T19, T21 → `quick`, T20 → `unspecified-high`
- **Wave 5**: T22–T24 → `quick`, T25–T29 → `unspecified-high`
- **Wave 6**: T30–T37 → `unspecified-high`
- **Final**: F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs


- [ ] 1. Add `needs_reinit_on_dep_reload` to `RawBundleMeta` (parser)

  **What to do**:
  - Open `crates/polyplugc/src/parser/mod.rs`
  - In `struct RawBundleMeta` (line 85), add the field after `api`:
    ```rust
    #[serde(default)]
    pub needs_reinit_on_dep_reload: bool,
    ```
  - Field must have explicit type annotation: `pub needs_reinit_on_dep_reload: bool`
  - Do NOT add any Default impl — `#[serde(default)]` handles it (bool defaults to false)

  **Must NOT do**:
  - Do NOT add validation logic for this field
  - Do NOT define a Default impl — serde handles it

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
    - Reason: Single-field struct addition, no domain complexity

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 5–11 (generators read needs_reinit_on_dep_reload from IR)
  - **Blocked By**: None

  **References**:
  - `crates/polyplugc/src/parser/mod.rs:85` — `RawBundleMeta` struct definition, add field here
  - Pattern: see existing `#[serde(default)]` on `api: Option<String>` in the same struct

  **Acceptance Criteria**:
  ```
  Scenario: parser accepts needs_reinit_on_dep_reload = true
    Tool: Bash
    Steps:
      1. printf '[bundle]\nname="foo"\nversion="1.0.0"\nneeds_reinit_on_dep_reload=true' > /tmp/test_reinit.toml
      2. polyplugc validate --bundle /tmp/test_reinit.toml 2>&1
    Expected Result: exit code 0, output contains "OK:"
    Evidence: .sisyphus/evidence/task-1-parser-reinit.txt

  Scenario: parser defaults false when field absent
    Tool: Bash
    Steps:
      1. polyplugc validate --bundle tests/fixtures/test_bundle.toml 2>&1
    Expected Result: exit code 0 (no error for missing field)
    Evidence: .sisyphus/evidence/task-1-parser-default.txt
  ```

  **Commit**: YES (grouped with Tasks 2, 3 in commit 1)


- [ ] 2. Add `needs_reinit_on_dep_reload` to `ResolvedBundle` (IR)

  **What to do**:
  - Open `crates/polyplugc/src/ir/mod.rs`
  - In `struct ResolvedBundle` (line 208), add field after `dependencies`:
    ```rust
    #[allow(dead_code)]
    pub needs_reinit_on_dep_reload: bool,
    ```
  - Find the site where `ResolvedBundle { ... }` is constructed in the IR lowering code
    (in `parser/mod.rs` or a `lower` function in `ir/mod.rs` — search for `ResolvedBundle {`)
  - Add `needs_reinit_on_dep_reload: raw_bundle.bundle.needs_reinit_on_dep_reload` in the constructor
  - Explicit type annotation on the struct field: `pub needs_reinit_on_dep_reload: bool`

  **Must NOT do**:
  - Do NOT rename existing fields
  - Do NOT change field order — only append

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Tasks 5–11
  - **Blocked By**: None (but must be sequenced after Task 1 if run serially)

  **References**:
  - `crates/polyplugc/src/ir/mod.rs:208` — `ResolvedBundle` struct
  - Search `crates/polyplugc/src/` for `ResolvedBundle {` to find construction site
  - Pattern: all other `ResolvedBundle` fields annotated with `#[allow(dead_code)]`

  **Acceptance Criteria**:
  ```
  Scenario: ResolvedBundle carries field through IR lowering
    Tool: Bash
    Steps:
      1. cargo build -p polyplugc 2>&1
    Expected Result: exit code 0, no compile errors
    Evidence: .sisyphus/evidence/task-2-ir-build.txt
  ```

  **Commit**: YES (grouped with Tasks 1, 3)


- [ ] 3. Add `needs_reinit_on_dep_reload` to `ManifestData` (runtime)

  **What to do**:
  - Open `crates/polyplug/src/loader/manifest/mod.rs`
  - In `struct ManifestData`, add after the `function_count` field (line ~108):
    ```rust
    /// Whether this bundle needs re-initialization when a dependency is hot-reloaded.
    /// Defaults to false. Most bundles do not need it.
    #[serde(default)]
    pub needs_reinit_on_dep_reload: bool,
    ```
  - No other runtime changes needed — serde default handles absence in existing manifests

  **Must NOT do**:
  - Do NOT change ManifestData field order — only append
  - Do NOT touch scanner, loader dispatch, or any other runtime code

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Tasks 5–11 (manifest roundtrip test needs ManifestData to accept the field)
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/loader/manifest/mod.rs:84` — `ManifestData` struct
  - `crates/polyplug/src/loader/manifest/mod.rs:108` — `function_count` field (add after)

  **Acceptance Criteria**:
  ```
  Scenario: ManifestData parses the new field
    Tool: Bash
    Steps:
      1. cargo test -p polyplug -- manifest 2>&1
    Expected Result: exit code 0, all manifest tests pass
    Evidence: .sisyphus/evidence/task-3-manifest-tests.txt
  ```

  **Commit**: YES (grouped with Tasks 1, 2)


- [ ] 4. Delete stray `crates/polyplug-dotnet/src/config/mod.rs`

  **What to do**:
  - Verify: `grep -rn 'mod config' crates/polyplug-dotnet/src/` — should only appear in
    `src/lib/mod.rs`, NOT anywhere else (the stray top-level file is never declared)
  - Delete the file: `crates/polyplug-dotnet/src/config/mod.rs`
  - Run `cargo build -p polyplug-dotnet` to confirm nothing breaks

  **Must NOT do**:
  - Do NOT touch `crates/polyplug-dotnet/src/lib/config/mod.rs` — that is the real config

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Nothing
  - **Blocked By**: None

  **References**:
  - `crates/polyplug-dotnet/Cargo.toml`: `path = "src/lib/mod.rs"` — crate root confirmation
  - `crates/polyplug-dotnet/src/config/mod.rs` — file to delete
    (contains only `DotnetConfig` without `HostfxrLocation` — clearly stale)

  **Acceptance Criteria**:
  ```
  Scenario: crate builds after deletion
    Tool: Bash
    Steps:
      1. cargo build -p polyplug-dotnet 2>&1
    Expected Result: exit code 0, no missing module errors
    Evidence: .sisyphus/evidence/task-4-dotnet-build.txt
  ```

  **Commit**: YES (grouped with commit 8 — chore)


- [ ] 5. Fix Rust generator manifest: add `bundle_name` and `needs_reinit_on_dep_reload`

  **What to do**:
  - Open `crates/polyplugc/src/generators/rust/mod.rs`
  - Find function `generate_bundle_manifest` (around line 226)
  - In the final `format!()` string (around line 299), add two fields:
    1. After `name = "{name}"`, add `bundle_name = "{name}"` (same value, two keys)
    2. After `function_count = {function_count_toml}`, add `needs_reinit_on_dep_reload = {reinit}`
  - Add local binding before format!: `let reinit: bool = bundle.needs_reinit_on_dep_reload;`
  - Explicit type annotation: `let reinit: bool = bundle.needs_reinit_on_dep_reload;`
  - The `file` field already emits `lib{name}.so` — leave it unchanged
  - Verify: generated manifest now has BOTH `name` AND `bundle_name` keys

  **Must NOT do**:
  - Do NOT change any other generator functions — only `generate_bundle_manifest`
  - Do NOT change the `file` field to platform-conditional — always `.so`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 6, 7, 8, 9, 10, 11)
  - **Blocks**: Task 13 (incremental generation needs all manifests complete)
  - **Blocked By**: Tasks 1, 2, 3 (needs `needs_reinit_on_dep_reload` in IR)

  **References**:
  - `crates/polyplugc/src/generators/rust/mod.rs:226` — `generate_bundle_manifest` function
  - `crates/polyplugc/src/generators/rust/mod.rs:299` — final `format!()` string to edit
  - `crates/polyplugc/src/ir/mod.rs:208` — `ResolvedBundle.needs_reinit_on_dep_reload`

  **Acceptance Criteria**:
  ```
  Scenario: Rust manifest has all 8 required fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/rust_manifest_test
      2. cat /tmp/rust_manifest_test/manifest.toml 2>&1
    Expected Result: output contains all of:
      name =
      bundle_name =
      version =
      runtime =
      file =
      provides =
      function_count =
      needs_reinit_on_dep_reload =
    Evidence: .sisyphus/evidence/task-5-rust-manifest.txt

  Scenario: manifest.toml roundtrips through ManifestData parser
    Tool: Bash
    Steps:
      1. cargo test -p polyplug -- integration_codegen_rust 2>&1
    Expected Result: exit code 0
    Evidence: .sisyphus/evidence/task-5-rust-codegen-test.txt
  ```

  **Commit**: YES (grouped with Tasks 6–11 in commit 2)


- [ ] 6. Fix C++ generator manifest: add `bundle_name` and `needs_reinit_on_dep_reload`

  **What to do**:
  - Open `crates/polyplugc/src/generators/cpp/mod.rs`
  - Find the bundle manifest generation function (the `format!()` that emits `function_count`
    — around line 614)
  - In the manifest format string, add:
    1. `bundle_name = "{name}"` after `name = "{name}"`
    2. `needs_reinit_on_dep_reload = {reinit}` after `function_count = ...`
  - Add local binding: `let reinit: bool = bundle.needs_reinit_on_dep_reload;`
  - Explicit type annotation: `let reinit: bool = ...`

  **Must NOT do**:
  - Do NOT touch the C++ host manifest (only the bundle manifest)
  - Do NOT change the `file` field

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 7, 8, 9, 10, 11)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/cpp/mod.rs:574` — bundle manifest function
  - `crates/polyplugc/src/generators/cpp/mod.rs:614` — final format! string

  **Acceptance Criteria**:
  ```
  Scenario: C++ manifest has all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang cpp --out /tmp/cpp_manifest_test
      2. cat /tmp/cpp_manifest_test/manifest.toml
    Expected Result: output contains name, bundle_name, version, runtime, file, provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-6-cpp-manifest.txt
  ```

  **Commit**: YES (grouped with Tasks 5, 7–11 in commit 2)


- [ ] 7. Fix C# generator manifest: add `file`, `function_count`, `bundle_name`, `needs_reinit_on_dep_reload`

  **What to do**:
  - Open `crates/polyplugc/src/generators/csharp/mod.rs`
  - Find function `generate_bundle_manifest_csharp` (line 475)
  - Current output has: name, version, runtime, provides, [[dependency]] tables
  - Add the missing fields to the manifest format string:
    1. `bundle_name = "{name}"` — add after `name = "{name}"`
    2. `file = "{name}.dll"` — C# bundles are `.dll` files
    3. `function_count = {function_count_toml}` — compute same as Rust/C++ generators:
       iterate `ir.contracts`, compute `fn_count = contract.functions.len() as u32`,
       format as TOML inline table `{ "name@major" = count }`
    4. `needs_reinit_on_dep_reload = {reinit}` — read from `bundle.needs_reinit_on_dep_reload`
  - All new local bindings must have explicit type annotations

  **Must NOT do**:
  - Do NOT touch `generate_host_manifest` (only the bundle manifest)
  - Do NOT change the [[dependency]] table format

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 8, 9, 10, 11)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/csharp/mod.rs:475` — `generate_bundle_manifest_csharp`
  - `crates/polyplugc/src/generators/rust/mod.rs:262` — function_count computation pattern to copy

  **Acceptance Criteria**:
  ```
  Scenario: C# manifest has all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang csharp --out /tmp/csharp_manifest_test
      2. cat /tmp/csharp_manifest_test/manifest.toml
    Expected Result: output contains name, bundle_name, version, runtime = "dotnet", file, provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-7-csharp-manifest.txt
  ```

  **Commit**: YES (grouped with Tasks 5, 6, 8–11 in commit 2)


- [ ] 8. Fix Python generator manifest: add all missing fields

  **What to do**:
  - Open `crates/polyplugc/src/generators/python/mod.rs`
  - Find the manifest generation code (lines 93–101):
    ```rust
    let manifest_content: String =
        "# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.\nruntime = \"python\"\n".to_owned();
    ```
  - Replace with a proper manifest generation function `generate_bundle_manifest_python(ir)` that emits:
    1. `name = "{bundle.name}"`
    2. `bundle_name = "{bundle.name}"`
    3. `version = "{bundle.version.major}.{bundle.version.minor}.{bundle.version.patch}"`
    4. `runtime = "python"`
    5. `file = "{bundle.name}.py"`
    6. `provides = [...]` — collect all `implements` from all plugins, dedup, same logic as Rust generator
    7. `function_count = {...}` — inline TOML table, same format as Rust generator
    8. `needs_reinit_on_dep_reload = {reinit}`
    9. `[[dependency]]` tables if `bundle.dependencies` is non-empty (same format as Rust)
  - All local bindings must have explicit type annotations
  - Pattern to follow: `crates/polyplugc/src/generators/rust/mod.rs:226` (`generate_bundle_manifest`)

  **Must NOT do**:
  - Do NOT change anything else in the Python generator

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7, 9, 10, 11)
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/python/mod.rs:93` — manifest code to replace
  - `crates/polyplugc/src/generators/rust/mod.rs:226` — full manifest template to follow

  **Acceptance Criteria**:
  ```
  Scenario: Python manifest has all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang python --out /tmp/py_manifest_test
      2. cat /tmp/py_manifest_test/manifest.toml
    Expected Result: output contains name, bundle_name, version, runtime = "python", file = "test_bundle.py", provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-8-python-manifest.txt
  ```

  **Commit**: YES (grouped with Tasks 5–11 in commit 2)


- [ ] 9. Add Lua generator manifest.toml generation (new function, all fields)

  **What to do**:
  - Open `crates/polyplugc/src/generators/lua/mod.rs`
  - Add a new function `generate_bundle_manifest_lua(ir: &ValidatedIr) -> String`
  - The function emits ALL 8 required fields + `[[dependency]]` tables:
    1. `name = "{bundle.name}"`
    2. `bundle_name = "{bundle.name}"`
    3. `version = "{bundle.version.major}.{bundle.version.minor}.{bundle.version.patch}"`
    4. `runtime = "lua"`
    5. `file = "{bundle.name}.lua"`
    6. `provides = [...]` — collect all `implements` from all plugins, same pattern as Rust
    7. `function_count = {...}` — inline TOML table `{ "name@major" = count }`, same format as Rust
    8. `needs_reinit_on_dep_reload = {reinit}`
    9. `[[dependency]]` tables if non-empty
  - In `generate_guest()`, add the manifest push GUARDED by `ir.bundle.is_some()` (same pattern as Python/C#/Rust):
    ```rust
    if ir.bundle.is_some() {
        files.files.push(GeneratedFile {
            path: PathBuf::from("manifest.toml"),
            content: generate_bundle_manifest_lua(ir),
            force_regenerate: true,
        });
    }
    ```
  - All local bindings in the new function must have explicit type annotations
  - Add the `# THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.` header as first line

  **Must NOT do**:
  - Do NOT change `generate_host()` — only `generate_guest()` and the new function

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7, 8, 10, 11)
  - **Blocks**: Task 12 (ffi.metatype task logically follows generator completeness)
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/lua/mod.rs:45` — `generate_guest()` function, add manifest push here
  - `crates/polyplugc/src/generators/rust/mod.rs:226` — full manifest template to follow exactly
  - `tests/fixtures/test_plugin.manifest.toml` — existing Lua manifest (only has runtime+file) for reference

  **Acceptance Criteria**:
  ```
  Scenario: Lua manifest is generated with all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang lua --out /tmp/lua_manifest_test
      2. cat /tmp/lua_manifest_test/manifest.toml
    Expected Result: file exists, contains name, bundle_name, version, runtime = "lua", file = "test_bundle.lua", provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-9-lua-manifest.txt

  Scenario: manifest roundtrips through ManifestData parser
    Tool: Bash
    Steps:
      1. cargo test -p polyplug -- manifest 2>&1
    Expected Result: exit code 0
    Evidence: .sisyphus/evidence/task-9-lua-manifest-roundtrip.txt
  ```

  **Commit**: YES (grouped with Tasks 5–11 in commit 2)


- [ ] 10. Fix js-quickjs generator manifest: add `name`, `file`, `provides`, `function_count`, `needs_reinit_on_dep_reload`

  **What to do**:
  - Open `crates/polyplugc/src/generators/js_quickjs/mod.rs`
  - Find function `generate_manifest_toml` (the function that currently emits only runtime/bundle_name/version)
  - **CRITICAL**: The current js-quickjs generator emits manifest.toml UNCONDITIONALLY in `generate_guest()`.
    This is a bug — it must be guarded by `ir.bundle.is_some()` like all other generators.
    Wrap the manifest push inside `if ir.bundle.is_some()` in `generate_guest()`.
  - Inside the guard, replace `generate_manifest_toml` body to emit all 8 required fields:
    1. `name = "{bundle_name}"` — use `ir.bundle.as_ref().map(|b| b.name.as_str()).unwrap_or_default()`
    2. `bundle_name = "{bundle_name}"` — same value
    3. `version = "{version}"` — use `ir.bundle.as_ref().map(|b| format!("{}.{}.{}", b.version.major, b.version.minor, b.version.patch)).unwrap_or_default()`
    4. `runtime = "js-quickjs"`
    5. `file = "bundle.js"` — js-quickjs bundles are always bundle.js
    6. `provides = [...]` — collect all implements, same pattern as Rust generator
    7. `function_count = {...}` — inline TOML table, same format as Rust
    8. `needs_reinit_on_dep_reload = {reinit}` — from `ir.bundle.as_ref().map(|b| b.needs_reinit_on_dep_reload).unwrap_or(false)`
  - All local bindings with explicit type annotations

  **Must NOT do**:
  - Do NOT change the TypeScript output files
  - Do NOT change the README.md generation

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5‑9)
  - **Blocks**: Tasks 13, 22, 28
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/js_quickjs/mod.rs` — `generate_manifest_toml` function
  - `crates/polyplugc/src/generators/rust/mod.rs:226` — full manifest template

  **Acceptance Criteria**:
  ```
  Scenario: js-quickjs manifest has all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang js-quickjs --out /tmp/jsq_manifest_test
      2. cat /tmp/jsq_manifest_test/manifest.toml
    Expected Result: output contains name, bundle_name, version, runtime = "js-quickjs", file = "bundle.js", provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-10-jsquickjs-manifest.txt
  ```

  **Commit**: YES (grouped with Tasks 5–11 in commit 2)


- [ ] 11. Fix js-deno generator manifest: same as Task 10 but runtime = "js-deno" and file = "index.ts"

  **What to do**:
  - Open `crates/polyplugc/src/generators/js_deno/mod.rs`
  - Find function `generate_manifest_toml` (same minimal implementation as js-quickjs)
  - **CRITICAL**: The current js-deno generator emits manifest.toml UNCONDITIONALLY in `generate_guest()`.
    Same bug as js-quickjs — guard with `if ir.bundle.is_some()` in `generate_guest()`.
  - Inside the guard, replace body with complete manifest emitting all 8 fields:
    1. `name = "{bundle_name}"` — from `ir.bundle.as_ref().map(|b| b.name.as_str()).unwrap_or_default()`
    2. `bundle_name = "{bundle_name}"` — same value
    3. `version = "{version}"` — from `ir.bundle`
    4. `runtime = "js-deno"`
    5. `file = "index.ts"` — js-deno bundles default to index.ts
    6. `provides = [...]`
    7. `function_count = {...}`
    8. `needs_reinit_on_dep_reload = {reinit}`
  - Same pattern as Task 10. All local bindings with explicit type annotations.

  **Must NOT do**:
  - Do NOT change TypeScript output files or README generation

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5‑10)
  - **Blocks**: Tasks 13, 23, 29
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `crates/polyplugc/src/generators/js_deno/mod.rs` — `generate_manifest_toml` function
  - Task 10 above — identical pattern, only runtime and file fields differ

  **Acceptance Criteria**:
  ```
  Scenario: js-deno manifest has all 8 fields
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang js-deno --out /tmp/jsd_manifest_test
      2. cat /tmp/jsd_manifest_test/manifest.toml
    Expected Result: output contains name, bundle_name, version, runtime = "js-deno", file = "index.ts", provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-11-jsdeno-manifest.txt
  ```

  **Commit**: YES (grouped with Tasks 5‑10 in commit 2)


- [ ] 12. Add `ffi.metatype` to Lua generator for all user-defined struct types

  **What to do**:
  - Open `crates/polyplugc/src/generators/lua/mod.rs`
  - Find where user-defined struct types are emitted for guest bundles
    (look for where `ir.types` is iterated and `ffi.cdef` is emitted)
  - After each user-defined struct type's `ffi.cdef` declaration, emit `ffi.metatype`:
    ```lua
    ffi.metatype("{TypeName}", {{}})
    ```
    The `{{}}` is an empty method table — purpose is to enable LuaJIT allocation sinking,
    not to add methods. The double-brace is Rust format string escaping for a literal `{}`.
  - This must be emitted for every type in `ir.types` (user-defined `[[types]]` structs)
  - Do NOT emit ffi.metatype for built-in ABI types (StringView, Buffer, etc.)
    — only for `ResolvedType` entries from `ir.types`
  - If the init.lua file emits the ffi.cdef calls, the ffi.metatype calls go immediately after each one
  - All format! strings must use explicit type annotations on any `let` bindings

  **Must NOT do**:
  - Do NOT add metatype to StringView, Buffer, PluginVTable, or other ABI-level structs
  - Do NOT add any methods to the metatype table

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (runs in Wave 3 alongside Task 13)
  - **Parallel Group**: Wave 3 (after Wave 2 completes)
  - **Blocks**: Task 27 (Lua codegen integration test expects metatype)
  - **Blocked By**: Task 9 (Lua manifest must exist before validating Lua generator completeness)

  **References**:
  - `crates/polyplugc/src/generators/lua/mod.rs` — find ffi.cdef emission pattern in generate_guest()
  - PRD section 10 (polyplug-lua): "ffi.metatype used for domain types — enables JIT allocation sinking"
  - LuaJIT docs: `ffi.metatype(ct, mt)` — ct is the ctype, mt is the metatable

  **Acceptance Criteria**:
  ```
  Scenario: ffi.metatype emitted for each user-defined type
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang lua --out /tmp/lua_metatype_test
      2. grep -c 'ffi.metatype' /tmp/lua_metatype_test/init.lua
    Expected Result: count >= number of types in test_api.toml [[types]] section (at least 1: AddArgs)
    Evidence: .sisyphus/evidence/task-12-lua-metatype.txt

  Scenario: no metatype on built-in types
    Tool: Bash
    Steps:
      1. grep 'ffi.metatype.*StringView\|ffi.metatype.*Buffer' /tmp/lua_metatype_test/init.lua
    Expected Result: no output (grep returns non-zero, no matches)
    Evidence: .sisyphus/evidence/task-12-lua-no-builtin-metatype.txt
  ```

  **Commit**: YES (grouped with commit 3)


- [ ] 13. Add incremental generation with IR hash cache and stats output

  **What to do**:
  - Open `crates/polyplugc/src/main.rs`
  - Add a `GeneratedFile` field `force_regenerate: bool` (default false) — add it to the
    `GeneratedFile` struct in `crates/polyplugc/src/generators/mod.rs`:
    ```rust
    pub struct GeneratedFile {
        pub path: PathBuf,
        pub content: String,
        pub force_regenerate: bool,
    }
    ```
  - In every generator's manifest push, set `force_regenerate: true`:
    e.g. in Rust generator: `GeneratedFile { path: PathBuf::from("manifest.toml"), content: ..., force_regenerate: true }`
  - In `write_files_if_changed` (main.rs line ~141), replace the content-comparison logic with a
    hash-cache approach:
    1. Compute `FNV-1a` hash of `file.content` (use `crates/polyplug/src/abi/mod.rs`'s existing `fnv1a_64` if accessible, otherwise implement a standalone hash function in `polyplugc`)
    2. Cache location: `<out_dir>/.polyplugc-cache/hashes.toml` as a TOML file `{ path = hash_u64 }` map
    3. If `file.force_regenerate == true`: always write (skip cache check)
    4. If hash matches cached hash: skip writing, increment `skipped` counter
    5. If hash differs or no cache: write file, update cache, increment `regenerated` counter
    6. After all files processed, print: `regenerated {regenerated} files, skipped {skipped} unchanged`
  - All local bindings with explicit type annotations: `let skipped: u32 = 0_u32;` etc.
  - Use `?` operator throughout — no `.unwrap()`, no `.expect()`
  - The `.polyplugc-cache/` directory is created alongside out_dir

  **Must NOT do**:
  - Do NOT change the `CodeGenerator` trait interface
  - Do NOT add Hash derive to IR types
  - Do NOT write the cache file inside `generated/` — always in `out_dir/.polyplugc-cache/`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, alongside Task 12)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 14–21 (pack command uses same CLI structure)
  - **Blocked By**: Tasks 5–11 (all generators need force_regenerate on manifests)

  **References**:
  - `crates/polyplugc/src/main.rs:141` — `write_files_if_changed` function to extend
  - `crates/polyplugc/src/generators/mod.rs` — `GeneratedFile` struct to extend
  - `crates/polyplug/src/abi/mod.rs` — `fnv1a_64` function (check if pub; if not, implement standalone)
  - Pattern: toml = { workspace = true } already in polyplugc Cargo.toml for TOML serialization

  **Acceptance Criteria**:
  ```
  Scenario: First run regenerates all files
    Tool: Bash
    Steps:
      1. rm -rf /tmp/incr_test
      2. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/incr_test 2>&1
    Expected Result: output contains "regenerated N files, skipped 0 unchanged" where N > 0
    Evidence: .sisyphus/evidence/task-13-incremental-first.txt

  Scenario: Second run skips all unchanged files
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/incr_test 2>&1
    Expected Result: output contains "regenerated 1 files, skipped M unchanged" where M > 0
      (manifest.toml always regenerates = 1; others skipped)
    Evidence: .sisyphus/evidence/task-13-incremental-second.txt

  Scenario: manifest.toml is always regenerated
    Tool: Bash
    Steps:
      1. polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/incr_test 2>&1
    Expected Result: "regenerated" count includes at least 1 (the manifest.toml)
    Evidence: .sisyphus/evidence/task-13-incremental-manifest.txt
  ```

  **Commit**: YES (grouped with commit 4)


- [ ] 14. Add `polyplugc pack` CLI subcommand and `pack/mod.rs` module scaffold

  **What to do**:
  - Open `crates/polyplugc/src/main.rs`
  - Add `Pack` variant to the `Command` enum with the same args as `Generate`:
    ```rust
    Pack {
        #[arg(short, long)] api: Option<PathBuf>,
        #[arg(short, long)] bundle: Option<PathBuf>,
        #[arg(short, long)] lang: String,
        #[arg(short, long)] out: PathBuf,
    }
    ```
  - In the `main` match arm, add a `Command::Pack { ... }` arm that calls `pack::run(config, out)`
  - Create `crates/polyplugc/src/pack/mod.rs` with a single `pub(crate) fn run(ir: &ValidatedIr, out: &Path, lang: &str) -> Result<(), CodegenError>` function stub
  - The help text for the Pack command MUST include: "Generates scaffold metadata for packaging (no build execution)"
  - AGENTS.md Rule 1: new module as `src/pack/mod.rs`
  - Declare it in main.rs: `mod pack;`

  **Must NOT do**:
  - Do NOT implement any language-specific logic in this task — that is Tasks 15-21
  - Do NOT run any build tools or network calls

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (must follow Wave 3 completion)
  - **Parallel Group**: Wave 4 (alone first, then T15-21 depend on it)
  - **Blocks**: Tasks 15, 16, 17, 18, 19, 20, 21
  - **Blocked By**: Task 13

  **References**:
  - `crates/polyplugc/src/main.rs:30` — Command enum, add Pack variant here
  - `crates/polyplugc/src/main.rs` — match arm for Command::Generate, follow same pattern
  - `crates/polyplugc/src/error/mod.rs` — CodegenError type to return

  **Acceptance Criteria**:
  ```
  Scenario: pack --help shows usage and scaffold disclaimer
    Tool: Bash
    Steps:
      1. polyplugc pack --help 2>&1
    Expected Result: exit 0, output contains "scaffold" or "no build"
    Evidence: .sisyphus/evidence/task-14-pack-help.txt
  ```

  **Commit**: YES (grouped with Tasks 14-21 in commit 5)


- [ ] 15. Pack: Rust language scaffold output

  **What to do**:
  - Open `crates/polyplugc/src/pack/mod.rs`
  - Add match arm for `lang == "rust"` in the `run()` function
  - Create the following files under `out/`:
    - `Cargo.toml` with: `[package] name = "{bundle_name}" version = "0.1.0" edition = "2021"` + `[lib] crate-type = ["cdylib"]` + `[dependencies] polyplug-guest = "*"` (user fills in path/version)
    - `src/lib.rs` with the boilerplate stub: `// TODO: implement {contract_name} for this plugin`
  - All string formatting must use explicit `let s: String = format!(...)` with explicit type

  **Must NOT do**:
  - Do NOT run `cargo init` or any subprocess
  - Do NOT add workspace or dependencies beyond polyplug-guest placeholder

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 16-21, after Task 14)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `crates/polyplugc/src/pack/mod.rs` — add to run() function
  - `crates/polyplugc/src/generators/rust/mod.rs:226` — bundle name pattern

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang rust produces Cargo.toml + src/lib.rs
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/pack_rust 2>&1
      2. ls /tmp/pack_rust/
      3. cat /tmp/pack_rust/Cargo.toml
    Expected Result: Cargo.toml present with [package] section; src/lib.rs present
    Evidence: .sisyphus/evidence/task-15-pack-rust.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 16. Pack: C++ language scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "cpp"` arm
  - Create under `out/`:
    - `include/{bundle_name}.hpp` — single-include header with: `#pragma once`, `#include "polyplug/polyplug.hpp"`, stub comment
    - `CMakeLists.txt` stub: `cmake_minimum_required(VERSION 3.16)`, `project({bundle_name})`, `add_library({bundle_name} SHARED src/{bundle_name}.cpp)`
    - `src/{bundle_name}.cpp` — stub implementation file with auto-gen header comment

  **Must NOT do**:
  - Do NOT run cmake or any subprocess

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `crates/polyplugc/src/pack/mod.rs` — add lang arm
  - `host-libs/cpp/polyplug.hpp` — this is the include path users need

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang cpp produces header + CMakeLists.txt
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang cpp --out /tmp/pack_cpp
      2. ls /tmp/pack_cpp/include/ /tmp/pack_cpp/src/
    Expected Result: .hpp file in include/, .cpp in src/, CMakeLists.txt at root
    Evidence: .sisyphus/evidence/task-16-pack-cpp.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 17. Pack: C# NuGet scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "csharp"` arm
  - Create under `out/`:
    - `{BundleName}.csproj` — with `<Project Sdk="Microsoft.NET.Sdk">`, `<PropertyGroup>`, `<TargetFramework>net10.0</TargetFramework>`, `<AllowUnsafeBlocks>true</AllowUnsafeBlocks>`, `<AssemblyName>{BundleName}</AssemblyName>`
    - `{BundleName}.nuspec` — minimal NuSpec with `<id>`, `<version>`, `<description>`
    - `Plugin.cs` — stub class with auto-gen header comment
  - Bundle name → PascalCase conversion for file names: split on `-`, capitalize each part

  **Must NOT do**:
  - Do NOT run dotnet or nuget

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `tests/fixtures/csharp_plugin/CsharpPlugin.csproj` — follow this .csproj structure
  - `tests/fixtures/csharp_plugin/Plugin.cs` — follow Plugin.cs stub pattern

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang csharp produces .csproj + .nuspec + Plugin.cs
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang csharp --out /tmp/pack_csharp
      2. ls /tmp/pack_csharp/
    Expected Result: .csproj, .nuspec, Plugin.cs present
    Evidence: .sisyphus/evidence/task-17-pack-csharp.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 18. Pack: Python pip scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "python"` arm
  - Create under `out/`:
    - `pyproject.toml` — with `[build-system]` (requires hatchling), `[project]` with name/version/description
    - `{bundle_name_underscored}/plugin.py` — stub plugin file with auto-gen header comment
    - `{bundle_name_underscored}/__init__.py` — empty init file
  - Bundle name → Python package name: replace `-` with `_`

  **Must NOT do**:
  - Do NOT run pip or any subprocess

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `tests/fixtures/test_plugin.py` — plugin structure to follow for stub

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang python produces pyproject.toml + package dir
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang python --out /tmp/pack_python
      2. ls /tmp/pack_python/
    Expected Result: pyproject.toml + package directory with plugin.py present
    Evidence: .sisyphus/evidence/task-18-pack-python.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 19. Pack: Lua module scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "lua"` arm
  - Create under `out/`:
    - `init.lua` — stub Lua module file with auto-gen header comment + `local M = {}; return M`
    - `{bundle_name}-{version}.rockspec` — minimal LuaRocks rockspec with package/version/source stub

  **Must NOT do**:
  - Do NOT run luarocks or any subprocess

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `tests/fixtures/test_plugin.lua` — Lua plugin structure to follow for stub

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang lua produces init.lua + rockspec
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang lua --out /tmp/pack_lua
      2. ls /tmp/pack_lua/
    Expected Result: init.lua + .rockspec present
    Evidence: .sisyphus/evidence/task-19-pack-lua.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 20. Pack: js-quickjs npm scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "js-quickjs"` arm
  - Create under `out/`:
    - `package.json` — with name, version, description, `"main": "bundle.js"`, `"scripts": { "build": "rolldown index.ts --format esm --file bundle.js" }`
    - `index.ts` — stub TypeScript file with auto-gen header comment + comment indicating where to implement contract methods
    - `.gitignore` — `node_modules/`, `bundle.js`
  - The package.json `name` should be the bundle_name (lowercase, hyphens OK)

  **Must NOT do**:
  - Do NOT run npm, rolldown, or any subprocess
  - Do NOT create node_modules/

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `tests/fixtures/test_plugin_js/bundle.js` — pattern for quickjs guest code

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang js-quickjs produces package.json + index.ts
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang js-quickjs --out /tmp/pack_jsq
      2. cat /tmp/pack_jsq/package.json
    Expected Result: package.json has name, version, main = bundle.js; index.ts present
    Evidence: .sisyphus/evidence/task-20-pack-jsquickjs.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 21. Pack: js-deno scaffold output

  **What to do**:
  - In `crates/polyplugc/src/pack/mod.rs`, add `lang == "js-deno"` arm
  - Create under `out/`:
    - `index.ts` — stub TypeScript file with Deno.core.ops BigInt comment, auto-gen header, stub impl for the contract
    - `deno.json` — Deno project config with `"name"`, `"version"`, optional compilerOptions
    - `README.md` — instructions: TypeScript loaded natively by deno_core, optional rolldown for bundling

  **Must NOT do**:
  - Do NOT run deno or any subprocess

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, after T14)
  - **Blocks**: Task F3
  - **Blocked By**: Task 14

  **References**:
  - `tests/fixtures/test_plugin_js_deno/index.ts` (created in Task 23) — follow this pattern for index.ts stub
  - `crates/polyplugc/src/generators/js_deno/mod.rs` — README.md generation for reference content

  **Acceptance Criteria**:
  ```
  Scenario: pack --lang js-deno produces index.ts + deno.json
    Tool: Bash
    Steps:
      1. polyplugc pack --bundle tests/fixtures/test_bundle.toml --lang js-deno --out /tmp/pack_jsd
      2. ls /tmp/pack_jsd/
    Expected Result: index.ts, deno.json, README.md present
    Evidence: .sisyphus/evidence/task-21-pack-jsdeno.txt
  ```

  **Commit**: YES (grouped with commit 5)


- [ ] 22. Add `manifest.toml` to `tests/fixtures/test_plugin_js/`

  **What to do**:
  - Create `tests/fixtures/test_plugin_js/manifest.toml` with the following content:
    ```toml
    # THIS FILE IS PART OF THE POLYPLUG TEST FIXTURES
    # DO NOT EDIT BY HAND
    name = "test_bundle"
    bundle_name = "test_bundle"
    version = "1.0.0"
    runtime = "js-quickjs"
    file = "bundle.js"
    provides = ["test.add@1"]
    needs_reinit_on_dep_reload = false

    [function_count]
    "test.add" = 4
    ```
  - The `provides` entry `"test.add@1"` matches the contract in `test_api.toml`
  - The `function_count` value `4` matches `fnCount = 4` in `bundle.js`
  - Update `crates/polyplug/build.rs` to emit `cargo:rerun-if-changed` for this file:
    add `println!("cargo:rerun-if-changed={}", fixtures_dir.join("test_plugin_js").join("manifest.toml").display());`

  **Must NOT do**:
  - Do NOT modify `bundle.js`
  - Do NOT add the file to the Cargo workspace

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5, alongside T23-T24 and T25-T29)
  - **Parallel Group**: Wave 5 (can run as soon as Wave 2 is done, since it's a fixture file)
  - **Blocks**: Task 28 (js-quickjs codegen integration test needs correct manifest)
  - **Blocked By**: Tasks 10 (defines field convention for js-quickjs manifests)

  **References**:
  - `tests/fixtures/test_bundle.toml` — bundle name is `test_bundle`
  - `tests/fixtures/test_plugin_js/bundle.js` lines 1-6 — contractLo/Hi and fnCount
  - `crates/polyplug/build.rs:502-512` — existing js fixture rerun-if-changed pattern

  **Acceptance Criteria**:
  ```
  Scenario: manifest.toml is readable by ManifestData deserializer
    Tool: Bash
    Steps:
      1. cat tests/fixtures/test_plugin_js/manifest.toml
    Expected Result: file exists and contains name, bundle_name, version, runtime, file, provides, function_count, needs_reinit_on_dep_reload
    Evidence: .sisyphus/evidence/task-22-jsquickjs-manifest-fixture.txt
  ```

  **Commit**: YES (grouped with commit 8)


- [ ] 23. Create `tests/fixtures/test_plugin_js_deno/` fixture (index.ts + manifest.toml)

  **What to do**:
  - Create directory `tests/fixtures/test_plugin_js_deno/`
  - Create `tests/fixtures/test_plugin_js_deno/index.ts` — pure TypeScript for deno_core:
    ```typescript
    // THIS FILE IS PART OF THE POLYPLUG TEST FIXTURES
    // DO NOT EDIT BY HAND
    // Runtime: js-deno (loaded natively by deno_core — no compilation needed)

    // Contract: test.add@1
    // FNV-1a hash of "test.add@1": 0xCC4232FAB0410D2B
    const CONTRACT_ID: bigint = 0xCC4232FAn << 32n | 0xB0410D2Bn;
    const VTABLE_ID: bigint = 1n;
    const FN_COUNT: number = 4;

    // Register vtable with host
    Deno.core.ops.op_register_vtable(CONTRACT_ID, VTABLE_ID, FN_COUNT);
    ```
  - Create `tests/fixtures/test_plugin_js_deno/manifest.toml`:
    ```toml
    # THIS FILE IS PART OF THE POLYPLUG TEST FIXTURES
    # DO NOT EDIT BY HAND
    name = "test_bundle"
    bundle_name = "test_bundle"
    version = "1.0.0"
    runtime = "js-deno"
    file = "index.ts"
    provides = ["test.add@1"]
    needs_reinit_on_dep_reload = false

    [function_count]
    "test.add" = 4
    ```
  - Update `crates/polyplug/build.rs` to add `TEST_JS_DENO_PLUGIN` env var:
    ```rust
    println!("cargo:rerun-if-changed={}", fixtures_dir.join("test_plugin_js_deno").join("index.ts").display());
    println!("cargo:rustc-env=TEST_JS_DENO_PLUGIN={}", fixtures_dir.join("test_plugin_js_deno").display());
    ```
  - Note: `TEST_JS_DENO_PLUGIN` points to the DIRECTORY (same pattern as `TEST_JS_PLUGIN`)

  **Must NOT do**:
  - Do NOT use quickjs-style `polyplug.getExtension()` or `typeof polyplug` checks — pure deno_core only
  - Do NOT use lo/hi u32 split — use BigInt throughout
  - Do NOT create any compiled .so or .dll

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5)
  - **Blocks**: Tasks 29, 37
  - **Blocked By**: Tasks 11 (defines field convention for js-deno manifests)

  **References**:
  - `tests/fixtures/test_plugin_js/bundle.js` — contractLo/Hi values to use (same contract ID)
  - `tests/integration_js/mod.rs:207` — `js_deno_load_bundle_and_call` test — loads from JS_PLUGIN path; after this task, tests will use TEST_JS_DENO_PLUGIN
  - `crates/polyplug/build.rs:502-512` — TEST_JS_PLUGIN pattern to follow exactly

  **Acceptance Criteria**:
  ```
  Scenario: index.ts exists and contains Deno.core.ops.op_register_vtable
    Tool: Bash
    Steps:
      1. cat tests/fixtures/test_plugin_js_deno/index.ts
    Expected Result: file contains 'Deno.core.ops.op_register_vtable' and BigInt literals
    Evidence: .sisyphus/evidence/task-23-deno-fixture.txt

  Scenario: TEST_JS_DENO_PLUGIN env var is set by build.rs
    Tool: Bash
    Steps:
      1. cargo build -p polyplug 2>&1 | grep TEST_JS_DENO
    Expected Result: no error; env var will be available to tests at compile time
    Evidence: .sisyphus/evidence/task-23-deno-buildrs.txt
  ```

  **Commit**: YES (grouped with commit 8)


- [ ] 24. Create `tests/fixtures/build_all.sh`

  **What to do**:
  - Create `tests/fixtures/build_all.sh` as an executable shell script
  - Content should document how to rebuild ALL pre-compiled fixtures:
    ```bash
    #!/usr/bin/env bash
    # build_all.sh — rebuilds all pre-compiled test fixtures
    # Run this after making changes to fixture source code
    set -e

    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    WORKSPACE_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

    # Rust fixtures
    cargo build -p test_plugin --release ...

    # C++ fixtures
    g++ -shared -fPIC ...

    # C# fixture
    cd ${SCRIPT_DIR}/csharp_plugin && dotnet build ...

    # Python and Lua: source-only, no build needed
    echo 'Python (.py) and Lua (.lua) fixtures are source-only, no build required.'

    # js-quickjs fixture: bundle.js is hand-written, no build needed
    echo 'js-quickjs bundle.js is hand-written.'

    # js-deno fixture: index.ts loaded natively by deno_core, no build needed
    echo 'js-deno index.ts is loaded natively by deno_core.'
    ```
  - Make the file executable: note in the task that the executor must mark it executable (`chmod +x`) if possible, or add a comment about this
  - Refer to actual build commands from build.rs for correct g++ flags and cargo target paths

  **Must NOT do**:
  - Do NOT execute the build commands in this task
  - Do NOT hardcode absolute paths — use `$SCRIPT_DIR` relative paths

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5, fully independent)
  - **Blocks**: none
  - **Blocked By**: none (purely documentation)

  **References**:
  - `crates/polyplug/build.rs` — read the g++ compile commands and Rust build patterns to document
  - `tests/fixtures/csharp_plugin/CsharpPlugin.csproj` — dotnet build target

  **Acceptance Criteria**:
  ```
  Scenario: build_all.sh file exists and is a valid shell script
    Tool: Bash
    Steps:
      1. bash -n tests/fixtures/build_all.sh
    Expected Result: exit 0 (no syntax errors)
    Evidence: .sisyphus/evidence/task-24-build-all-sh.txt
  ```

  **Commit**: YES (grouped with commit 8)


- [ ] 25. Integration codegen test for C# generator

  **What to do**:
  - Create `tests/integration_codegen_csharp/mod.rs` following the pattern of `tests/integration_codegen_cpp/mod.rs`
  - Test structure:
    - **Part A (always runs)**: Run `polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang csharp --out <tmpdir>`
      - Assert all expected output files exist: `guest/Types.cs`, `guest/Contracts.cs`, `guest/Vtables.cs`, `guest/Init.cs`, `guest/BundleConstants.cs`, `manifest.toml`
      - Note: Use `test_bundle.toml` (NOT test_api.toml) to trigger `ir.bundle.is_some()` and get manifest.toml + BundleConstants.cs
    - **Part B (skip if dotnet unavailable)**: Check if `dotnet --version` succeeds; if yes, run `dotnet build` on generated code
  - Use `env!("CARGO_BIN_EXE_polyplugc")` to run the binary (same as existing codegen tests)
  - Use `env!("CARGO_TARGET_TMPDIR")` for temp directory
  - Add `#![allow(clippy::expect_used)]` at file top (same as existing codegen tests)
  - Wire it up in `crates/polyplug/Cargo.toml` as a `[[test]]` entry (check existing codegen test entries for pattern)

  **Must NOT do**:
  - Do NOT assert the content of generated files — only existence
  - Do NOT run `dotnet test` — only `dotnet build`
  - Do NOT add a dependency on the C# host lib in this test

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 26-29, after Wave 2)
  - **Parallel Group**: Wave 5
  - **Blocks**: Task 33 (C#-host cross-language tests)
  - **Blocked By**: Tasks 7 (C# manifest must be complete before testing it)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — complete pattern to follow
  - `crates/polyplug/Cargo.toml` — find `[[test]]` entry for `integration_codegen_cpp` and duplicate pattern
  - `tests/fixtures/test_bundle.toml` — use this for generation (triggers manifest emission)

  **Acceptance Criteria**:
  ```
  Scenario: C# codegen test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test integration_codegen_csharp 2>&1
    Expected Result: exit 0, at least 1 test passed, output confirms all files present
    Evidence: .sisyphus/evidence/task-25-codegen-csharp.txt
  ```

  **Commit**: YES (grouped with commit 6)


- [ ] 26. Integration codegen test for Python generator

  **What to do**:
  - Create `tests/integration_codegen_python/mod.rs` following the same pattern
  - Use `test_bundle.toml` (triggers manifest.toml emission)
  - Part A (always runs): assert files exist: `guest/types.py`, `guest/types.pyi`, `guest/contracts.py`, `guest/contracts.pyi`, `guest/init.py`, `manifest.toml`
  - Part B (skip if python unavailable): check `python3 --version`, if OK, import the generated module
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]`

  **Must NOT do**:
  - Do NOT assert file contents, only existence

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5)
  - **Blocks**: Task 34 (Python-host cross-language tests)
  - **Blocked By**: Task 8 (Python manifest must be complete)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — pattern to follow
  - `crates/polyplug/Cargo.toml` — [[test]] entry pattern
  - `tests/fixtures/test_bundle.toml` — use for generation

  **Acceptance Criteria**:
  ```
  Scenario: Python codegen test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test integration_codegen_python 2>&1
    Expected Result: exit 0, at least 1 test passed
    Evidence: .sisyphus/evidence/task-26-codegen-python.txt
  ```

  **Commit**: YES (grouped with commit 6)


- [ ] 27. Integration codegen test for Lua generator

  **What to do**:
  - Create `tests/integration_codegen_lua/mod.rs`
  - Use `test_bundle.toml` (same decision as C# and Python — Lua manifest will be guarded by
    `ir.bundle.is_some()` after Task 9, so `test_api.toml` would NOT produce manifest.toml)
  - Part A (always runs): assert files exist: `guest/types.lua`, `guest/contracts.lua`, `guest/init.lua`, `manifest.toml`
    - Note: `manifest.toml` will only appear after Task 9 adds Lua manifest generation. Verify Task 9 is done.
  - Part B (skip if luajit unavailable): check `luajit --version`, if OK, run `luajit` on generated types.lua
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]`

  **Must NOT do**:
  - Do NOT assert ffi.metatype content in this test (that is Task 12's QA)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5)
  - **Blocks**: Task 35 (Lua-host cross-language tests)
  - **Blocked By**: Tasks 9 (Lua manifest), 12 (ffi.metatype)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — pattern
  - `crates/polyplug/Cargo.toml` — [[test]] entry pattern

  **Acceptance Criteria**:
  ```
  Scenario: Lua codegen test passes (manifest.toml must be present)
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test integration_codegen_lua 2>&1
    Expected Result: exit 0, manifest.toml existence confirmed
    Evidence: .sisyphus/evidence/task-27-codegen-lua.txt
  ```

  **Commit**: YES (grouped with commit 6)


- [ ] 28. Integration codegen test for js-quickjs generator

  **What to do**:
  - Create `tests/integration_codegen_js_quickjs/mod.rs`
  - Use `test_api.toml` (js-quickjs generates all files unconditionally AFTER Task 10 guards manifest)
    Wait — after Task 10 adds the `ir.bundle.is_some()` guard, `test_api.toml` will NOT produce manifest.toml.
    Use `test_bundle.toml` to get manifest.toml.
  - Part A (always runs): assert files exist: `guest/types.ts`, `guest/contracts.ts`, `guest/vtable.ts`, `guest/init.ts`, `manifest.toml`, `README.md`
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]`

  **Must NOT do**:
  - Do NOT run node or any JS runtime in this test

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5)
  - **Blocks**: Task 36 (js-quickjs-host cross-language tests)
  - **Blocked By**: Tasks 10 (js-quickjs manifest), 22 (js-quickjs fixture manifest.toml)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — pattern
  - `crates/polyplugc/src/generators/js_quickjs/mod.rs:39-75` — exact output file paths
  - `tests/fixtures/test_bundle.toml` — use for generation to get manifest

  **Acceptance Criteria**:
  ```
  Scenario: js-quickjs codegen test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test integration_codegen_js_quickjs 2>&1
    Expected Result: exit 0, all 6 files confirmed present
    Evidence: .sisyphus/evidence/task-28-codegen-jsquickjs.txt
  ```

  **Commit**: YES (grouped with commit 6)


- [ ] 29. Integration codegen test for js-deno generator

  **What to do**:
  - Create `tests/integration_codegen_js_deno/mod.rs`
  - Use `test_bundle.toml` (to trigger manifest.toml after Task 11's ir.bundle.is_some() guard)
  - Part A (always runs): assert files exist: `guest/types.ts`, `guest/contracts.ts`, `guest/init.ts`, `manifest.toml`, `README.md`
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]`

  **Must NOT do**:
  - Do NOT run deno in this test

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 5)
  - **Blocks**: Task 37 (js-deno cross-language tests)
  - **Blocked By**: Tasks 11 (js-deno manifest), 23 (js-deno fixture)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — pattern
  - `crates/polyplugc/src/generators/js_deno/mod.rs:49-75` — exact output file paths

  **Acceptance Criteria**:
  ```
  Scenario: js-deno codegen test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test integration_codegen_js_deno 2>&1
    Expected Result: exit 0, all 5 files confirmed present
    Evidence: .sisyphus/evidence/task-29-codegen-jsdeno.txt
  ```

  **Commit**: YES (grouped with commit 6)


- [ ] 30. Cross-language test matrix skeleton: `tests/cross_language/mod.rs` + Rust×Rust

  **What to do**:
  - Create `tests/cross_language/mod.rs`
  - Add file-level doc comment explaining the 6×6 matrix
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]` named `cross_language`
  - Add all needed `use` imports at the top of the file (AGENTS.md Rule 2 — no use inside fns)
  - Implement the first test: `fn test_rust_host_rust_guest()`
    - Load `libtest_plugin.so` (from `env!("TEST_PLUGIN_SO")`) with a RustLoader or directly with libloading
    - Dispatch `add(3, 5)` and assert == 8
    - Follow pattern from `tests/integration_codegen_rust/mod.rs` for libloading + vtable dispatch
  - Each test must be standalone (no shared state between tests)
  - Test function naming convention: `fn test_{host_lang}_host_{guest_lang}_guest()`

  **Must NOT do**:
  - Do NOT use any shared global state across tests
  - Do NOT add helper abstractions or test frameworks
  - Do NOT add more than 2 contract function calls per test

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6, after Waves 3-5)
  - **Parallel Group**: Wave 6 (with Tasks 31-36)
  - **Blocks**: Task F3
  - **Blocked By**: Tasks 12, 13, 22, 23 (all generators and fixtures complete)

  **References**:
  - `tests/integration_codegen_rust/mod.rs:260-325` — libloading + vtable + dispatch pattern
  - `tests/integration_cross_plugin/mod.rs` — if exists, check cross-plugin loading patterns
  - `crates/polyplug/Cargo.toml` — add [[test]] for cross_language, see existing test entries

  **Acceptance Criteria**:
  ```
  Scenario: Rust-host × Rust-guest test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language test_rust_host_rust_guest 2>&1
    Expected Result: exit 0, 1 test passed
    Evidence: .sisyphus/evidence/task-30-cross-rust-rust.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 31. Cross-language: remaining Rust-host rows (Rust×C++, Rust×C#, Rust×Python, Rust×Lua, Rust×js-quickjs)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` the 5 remaining Rust-host tests:
    - `fn test_rust_host_cpp_guest()` — load `libtest_plugin_cpp.so` (from `env!("TEST_PLUGIN_CPP_SO")`)
    - `fn test_rust_host_csharp_guest()` — load C# via DotnetLoader, path from `env!("TEST_CSHARP_PLUGIN_DLL")`; skip if `DOTNET_NOT_AVAILABLE`
    - `fn test_rust_host_python_guest()` — load via PythonLoader, path from `env!("TEST_PYTHON_PLUGIN")`; skip if `PYTHON_NOT_AVAILABLE`
    - `fn test_rust_host_lua_guest()` — load via LuaLoader, path from `env!("TEST_LUA_PLUGIN")`
    - `fn test_rust_host_jsquickjs_guest()` — load via JsLoader, dir from `env!("TEST_JS_PLUGIN")`
  - For each: dispatch `add(3, 5)` via vtable and assert result == 8
  - Tests that require unavailable toolchains must print `"skipping: <reason>"` and return early

  **Must NOT do**:
  - Do NOT hardcode fixture paths — always use `env!()` for fixture locations
  - Do NOT share state between tests

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6, with Tasks 30, 32-36)
  - **Blocks**: Task F3
  - **Blocked By**: Tasks 25-29 (codegen tests verify generators work before cross-lang)

  **References**:
  - `tests/integration_codegen_cpp/mod.rs` — C++ plugin load pattern
  - `tests/integration_dotnet/mod.rs` — DotnetLoader load pattern
  - `tests/integration_python/mod.rs` — PythonLoader load pattern
  - `tests/integration_lua/mod.rs` — LuaLoader load pattern
  - `tests/integration_js/mod.rs` — JsLoader load pattern

  **Acceptance Criteria**:
  ```
  Scenario: All Rust-host tests compile and run
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language rust_host 2>&1
    Expected Result: 6 tests (1 from Task 30 + 5 from this task) either pass or skip with message
    Evidence: .sisyphus/evidence/task-31-cross-rust-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 32. Cross-language: C++ host rows (C++×all 6 guests)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` the 6 C++ host tests:
    - `fn test_cpp_host_rust_guest()`
    - `fn test_cpp_host_cpp_guest()`
    - `fn test_cpp_host_csharp_guest()`
    - `fn test_cpp_host_python_guest()`
    - `fn test_cpp_host_lua_guest()`
    - `fn test_cpp_host_jsquickjs_guest()`
  - C++ host means: use the C++ plugin loading mechanism (libloading + C++ vtable dispatch pattern from integration_codegen_cpp)
  - Actually in this test framework, "C++ host" means: the Rust test harness simulates what a C++ host would do — just load the guest plugin .so and dispatch the vtable function directly via raw fn pointers
  - Each test: load guest fixture, call add(3,5), assert == 8; skip if toolchain unavailable

  **Must NOT do**:
  - Do NOT create actual C++ host code — the Rust test harness handles all host simulation

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocks**: Task F3
  - **Blocked By**: Tasks 25-29

  **References**:
  - `tests/cross_language/mod.rs` (created in T30) — add to this file
  - `tests/integration_codegen_cpp/mod.rs:62-110` — C++ vtable dispatch pattern

  **Acceptance Criteria**:
  ```
  Scenario: All C++ host tests compile
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language cpp_host 2>&1
    Expected Result: 6 tests compile and either pass or skip with message
    Evidence: .sisyphus/evidence/task-32-cross-cpp-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 33. Cross-language: C# host rows (C#×all 6 guests)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` 6 tests for C# host:
    - `fn test_csharp_host_rust_guest()`, `_cpp_guest()`, `_csharp_guest()`, `_python_guest()`, `_lua_guest()`, `_jsquickjs_guest()`
  - C# host means: use DotnetLoader to load the guest plugin (DotnetLoader loads the DLL and exposes vtable)
  - For non-C# guests: the Rust test harness uses raw libloading to load the guest .so and dispatches vtable
  - For C# guest: use DotnetLoader + csharp_plugin DLL
  - All tests skip gracefully if dotnet unavailable

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocked By**: Tasks 25, 30

  **References**:
  - `tests/integration_dotnet/mod.rs` — DotnetLoader load pattern + CSHARP_DLL env var
  - `tests/cross_language/mod.rs` (T30) — add to this file

  **Acceptance Criteria**:
  ```
  Scenario: C# host rows compile
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language csharp_host 2>&1
    Expected Result: 6 tests compile; pass or skip with message
    Evidence: .sisyphus/evidence/task-33-cross-csharp-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 34. Cross-language: Python host rows (Python×all 6 guests)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` 6 tests for Python host:
    - `fn test_python_host_rust_guest()`, `_cpp_guest()`, `_csharp_guest()`, `_python_guest()`, `_lua_guest()`, `_jsquickjs_guest()`
  - Python host means: the Rust test harness uses PythonLoader to load the guest .py file
  - For non-Python guests: use libloading + vtable dispatch
  - Skip if Python unavailable

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocked By**: Tasks 26, 30

  **References**:
  - `tests/integration_python/mod.rs` — PythonLoader + TEST_PYTHON_PLUGIN pattern

  **Acceptance Criteria**:
  ```
  Scenario: Python host rows compile
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language python_host 2>&1
    Expected Result: 6 tests compile; pass or skip
    Evidence: .sisyphus/evidence/task-34-cross-python-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 35. Cross-language: Lua host rows (Lua×all 6 guests)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` 6 tests for Lua host:
    - `fn test_lua_host_rust_guest()`, `_cpp_guest()`, `_csharp_guest()`, `_python_guest()`, `_lua_guest()`, `_jsquickjs_guest()`
  - Lua host: use LuaLoader to load the .lua guest; for non-Lua guests: libloading + vtable dispatch

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocked By**: Tasks 27, 30

  **References**:
  - `tests/integration_lua/mod.rs` — LuaLoader + TEST_LUA_PLUGIN pattern

  **Acceptance Criteria**:
  ```
  Scenario: Lua host rows compile
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language lua_host 2>&1
    Expected Result: 6 tests compile; pass or skip
    Evidence: .sisyphus/evidence/task-35-cross-lua-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 36. Cross-language: js-quickjs host rows (js-quickjs×all 6 guests)

  **What to do**:
  - Add to `tests/cross_language/mod.rs` 6 tests for js-quickjs host:
    - `fn test_jsquickjs_host_rust_guest()`, `_cpp_guest()`, `_csharp_guest()`, `_python_guest()`, `_lua_guest()`, `_jsquickjs_guest()`
  - js-quickjs host: use JsLoader (polyplug_js::JsLoader) to load the .js guest bundle
  - For non-JS guests: the Rust test harness uses libloading + vtable dispatch directly

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocked By**: Tasks 28, 30

  **References**:
  - `tests/integration_js/mod.rs` — JsLoader load + dispatch pattern
  - `tests/cross_language/mod.rs` (T30) — add to this file

  **Acceptance Criteria**:
  ```
  Scenario: js-quickjs host rows compile and pass
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language jsquickjs_host 2>&1
    Expected Result: 6 tests compile; pass or skip
    Evidence: .sisyphus/evidence/task-36-cross-jsquickjs-all.txt
  ```

  **Commit**: YES (grouped with commit 7)


- [ ] 37. Cross-language deno: `tests/cross_language_deno/mod.rs`

  **What to do**:
  - Create `tests/cross_language_deno/mod.rs`
  - Wire up in `crates/polyplug/Cargo.toml` as `[[test]]` named `cross_language_deno`
  - Implement tests for js-deno as HOST (using JsDenoLoader) loading each of the 6 guest plugins:
    - `fn test_jsdeno_host_rust_guest()`
    - `fn test_jsdeno_host_cpp_guest()`
    - `fn test_jsdeno_host_csharp_guest()` (skip if dotnet unavailable)
    - `fn test_jsdeno_host_python_guest()` (skip if python unavailable)
    - `fn test_jsdeno_host_lua_guest()`
    - `fn test_jsdeno_host_jsquickjs_guest()`
  - Also implement deno as GUEST (other hosts loading the deno fixture):
    - `fn test_rust_host_jsdeno_guest()` — load index.ts via JsDenoLoader
    - `fn test_jsquickjs_host_jsdeno_guest()` — load index.ts via JsLoader (if supported) OR JsDenoLoader
  - Each test: dispatch `add(3, 5)` and assert == 8; skip gracefully if runtime unavailable
  - Use `env!("TEST_JS_DENO_PLUGIN")` for the deno fixture directory path

  **Must NOT do**:
  - Do NOT put these tests in cross_language/mod.rs — they must be separate (user decision)
  - Do NOT use a different contract other than test.add

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 6)
  - **Blocks**: Task F3
  - **Blocked By**: Tasks 23 (deno fixture), 29 (deno codegen test), 30 (cross_language skeleton pattern to follow)

  **References**:
  - `tests/cross_language/mod.rs` (T30) — follow the same test pattern
  - `tests/integration_js/mod.rs:207` — js_deno_load_bundle_and_call pattern
  - `crates/polyplug/Cargo.toml` — [[test]] entry for cross_language, duplicate for cross_language_deno

  **Acceptance Criteria**:
  ```
  Scenario: cross_language_deno tests compile and run
    Tool: Bash
    Steps:
      1. cargo test -p polyplug --test cross_language_deno 2>&1
    Expected Result: exit 0, at least 2 tests pass or skip with message
    Evidence: .sisyphus/evidence/task-37-cross-jsdeno.txt
  ```

  **Commit**: YES (grouped with commit 7)

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read this plan end-to-end. For each Must Have: verify implementation exists (read file, run cargo test). For each Must NOT Have: search codebase for forbidden patterns. Verify all 36 cross-language tests pass. Verify all evidence files exist.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tests [36/36] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings`. Run `cargo fmt --check`. Search for `.unwrap()` outside tests. Check for commented-out code, unused imports, AI slop patterns (over-abstraction, generic names).
  Output: `Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | Unwrap hits [N] | VERDICT`

- [ ] F3. **Real end-to-end QA** — `unspecified-high`
  Run `cargo test --workspace`. Run `polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang lua --out /tmp/lua_out && cat /tmp/lua_out/manifest.toml` — verify all 8 fields. Run `polyplugc generate` twice — verify "skipped" output. Run `polyplugc pack --api tests/fixtures/test_api.toml --lang rust --out /tmp/pack_rust && ls /tmp/pack_rust/` — verify Cargo.toml + src/ present.
  Output: `Tests [N/N] | Manifest fields [8/8] | Incremental [PASS] | Pack [PASS] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1. Check Must NOT Do compliance. Flag any scope creep (e.g. `requires` field added, CodeGenerator trait changed, production publishing code added).
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **1**: `fix(polyplugc): add needs_reinit_on_dep_reload to parser, IR, ManifestData`
- **2**: `fix(codegen): complete manifest.toml output for all 7 generators`
- **3**: `feat(codegen/lua): emit ffi.metatype for all user-defined struct types`
- **4**: `feat(polyplugc): incremental generation with IR hash cache and stats output`
- **5**: `feat(polyplugc): add pack command for all 7 languages`
- **6**: `test: add integration_codegen tests for C#, Python, Lua, js-quickjs, js-deno`
- **7**: `test: add 36-combination cross-language test matrix + js-deno separate tests`
- **8**: `chore: delete stray polyplug-dotnet/src/config/mod.rs dead code; add build_all.sh`

---

## Success Criteria

### Verification Commands
```bash
cargo test --workspace                           # Expected: all tests pass
cargo test --workspace -- cross_language         # Expected: 36 tests pass
cargo test --workspace -- cross_language_deno    # Expected: >=1 test passes
cargo clippy -- -D warnings                      # Expected: exit 0, no output
polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/t
cat /tmp/t/manifest.toml | grep -c "^"          # Expected: >=8 lines (8+ fields)
polyplugc generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/t 2>&1 | grep skipped
                                                 # Expected: "skipped N unchanged" where N>0
polyplugc pack --api tests/fixtures/test_api.toml --lang rust --out /tmp/pack && ls /tmp/pack
                                                 # Expected: Cargo.toml  src/
```

### Final Checklist
- [ ] All 8 manifest fields in every generator output
- [ ] Lua ffi.metatype present for all struct types
- [ ] Incremental generation skips unchanged files
- [ ] Pack command produces valid scaffold for all 7 languages
- [ ] 36 cross-language combination tests pass
- [ ] js-deno combination tests pass (separate file)
- [ ] No `.unwrap()` in polyplugc production code
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test --workspace` passes
