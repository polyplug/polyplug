# Epic — Bundle-as-Directory Enforcement

## TL;DR

> **Quick Summary**: Enforce that every polyplug bundle is a **directory** containing `manifest.toml` + the plugin file(s). Removes the legacy flat `.so + .manifest.toml` side-by-side model from scanner, loader, runtime, reload, build.rs, and all integration tests. Also refactors `load_bundle()` to accept a pre-parsed `ManifestData` (eliminating a double-manifest-read), makes the file watcher recursive, and updates the two native generators to emit canonical manifests with a `file = "libfoo.so"` flat string (PerPlatform table deferred to follow-up).
>
> **Deliverables**:
> - `scanner/mod.rs`: flat-file branch removed; only `dir/manifest.toml` accepted
> - `loader/mod.rs`: `load_bundle()` accepts `ManifestData` (no manifest re-read)
> - `loader/manifest/mod.rs`: `parse_manifest()` reads `dir/manifest.toml`; validates `file` field
> - `runtime/mod.rs`: `load_bundle_with()` accepts dir path; watcher uses `RecursiveMode::Recursive`
> - `reload/mod.rs`: `reload_bundle_impl` derives bundle dir via `path.parent()`
> - `build.rs`: creates bundle dirs for reload/depender/test_plugin; emits `*_DIR` env vars
> - Integration tests: `integration_discovery`, `integration_version`, `integration_reload`, `library_lifetime` updated
> - Generators (rust, cpp): manifest template emits canonical `file = "libfoo.so"` (no template comment noise)
> - Flat fixture `.manifest.toml` files deleted
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 4 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Task 7 → Task 8 → Tasks 9-11 → Tasks 12-14 → F1-F4

---

## Context

### Original Request
Enforce bundle-as-directory model across the entire polyplug codebase. The codebase is partially migrated — the scanner and runtime already handle directory bundles (alongside flat), and JS fixtures are already directory-based. This epic completes the migration and removes the flat-bundle path entirely.

### Interview Summary
**Key Discussions**:
- JS fixture dirs (`test_plugin_js/`, `test_plugin_js_deno/`) are already correct — DO NOT touch
- `BundleFile::PerPlatform` (per-platform HashMap serde) is **deferred** — ship with `file = "libfoo.so"` string only
- `reload_bundle()` public API accepts `.so` path (watcher fires on `.so` events); internally derives bundle dir via `path.parent()`
- All flat `.manifest.toml` fixture files become dead code and must be deleted

**Research Findings**:
- Double-manifest-read: `load_bundle()` calls `parse_manifest()` internally, but Runtime has already parsed it. After migration, `parse_manifest(effective_path)` would look for `libfoo.manifest.toml` next to the `.so` — which won't exist. **Must fix.**
- `watch_plugin_dir` uses `RecursiveMode::NonRecursive` — won't see `.so` files inside bundle subdirs. **Must fix.**
- `manifest.path` stored in `bundle_manifests` must be the bundle DIRECTORY, not the `.so` path, for cascade reload to work.
- `library_lifetime/mod.rs` calls `load_bundle()` directly with raw `.so` — must be updated for new API.

### Metis Review
**Identified Gaps** (addressed):
- **Double-manifest-read**: `load_bundle()` must accept `ManifestData` parameter — caller provides it, no re-read.
- **Watcher RecursiveMode**: Change `NonRecursive` → `Recursive` at `runtime/mod.rs:660`.
- **manifest.path must store bundle dir**: Updated in Task 5 (build) and Task 6 (reload/scan store).
- **library_lifetime test**: Now covered in Task 9 (updated for new `load_bundle()` signature).
- **`explicit_load_bundle_missing_manifest_errors` behavioral change**: Now expects `BundleNotADirectory` error.
- **BundleFile::PerPlatform deferred**: Not in scope.

---

## Work Objectives

### Core Objective
Remove all flat-bundle (`*.so + *.manifest.toml`) code paths. Every bundle is now a directory. The Runtime, scanner, loader, reload system, generators, and tests all operate exclusively on directory bundles.

### Concrete Deliverables
- `crates/polyplug/src/loader/manifest/mod.rs` — `parse_manifest()` reads `dir/manifest.toml`; validates non-empty `file` field; new `LoaderError` variants
- `crates/polyplug/src/loader/mod.rs` — `load_bundle()` accepts `(path, manifest, registry, host_vtable)` — no internal manifest re-read
- `crates/polyplug/src/loader/scanner/mod.rs` — flat-file branch removed; unit tests updated
- `crates/polyplug/src/runtime/mod.rs` — `load_bundle_with()` accepts dir; watcher `Recursive`
- `crates/polyplug/src/reload/mod.rs` — derives bundle dir via `path.parent()`
- `crates/polyplug-js/src/lib/loader/mod.rs` — `is_dir()` check removed (dead code)
- `crates/polyplug-js-deno/src/lib/loader/mod.rs` — uses resolved file path directly
- `crates/polyplugc/src/generators/rust/mod.rs` — manifest template: canonical `file = "libfoo.so"`
- `crates/polyplugc/src/generators/cpp/mod.rs` — manifest template: canonical `file = "libfoo.so"`
- `crates/polyplug/build.rs` — bundle dirs created; `*_DIR` env vars emitted; flat `.so + .manifest.toml` cleanup
- `tests/integration_discovery/mod.rs`, `tests/integration_version/mod.rs`, `tests/integration_reload/mod.rs`, `tests/library_lifetime/mod.rs` — updated
- Flat fixture files deleted: `libreload_plugin_v1.manifest.toml`, `libreload_plugin_v2.manifest.toml`, `libdepender_plugin.manifest.toml`, `test_plugin.manifest.toml`

### Definition of Done
- [ ] `cargo clippy -- -D warnings` → zero warnings
- [ ] `cargo fmt --check` → clean
- [ ] `cargo test --test integration_discovery` → all 5 tests pass
- [ ] `cargo test --test integration_version` → all 14+ tests pass
- [ ] `cargo test --test integration_reload` → all 9 tests (a–i) pass
- [ ] `cargo test --test library_lifetime` → passes
- [ ] `cargo test --workspace` → all tests pass
- [ ] `grep -r 'with_extension.*manifest.toml' crates/` → zero results

### Must Have
- Every bundle load (scanner-based and explicit) reads from `dir/manifest.toml`
- `load_bundle()` receives a pre-parsed `ManifestData` — no internal manifest re-read
- `manifest.path` in `bundle_manifests` stores the bundle DIRECTORY path
- `watch_plugin_dir` uses `RecursiveMode::Recursive`
- Flat `.manifest.toml` fixture files deleted

### Must NOT Have (Guardrails)
- **NO** `BundleFile::PerPlatform` (HashMap serde, per-platform `[bundle.file]` table) — deferred
- **NO** backwards-compatibility shim for flat bundles — clean break
- **NO** deprecation warnings — flat bundles are simply removed
- **NO** migration utility / bundle converter tool
- **NO** new public API methods (`Runtime::from_directory()` etc.)
- **NO** touching `integration_load`, `integration_dispatch`, `integration_graph`, `cross_language`, `cross_language_deno`, `stress_error`, `stress_memory`, `integration_python`, `integration_lua`, `integration_js`, `integration_dotnet`, `integration_codegen_*`, `smoke` — they bypass the manifest system
- **NO** touching JS fixture dirs `test_plugin_js/`, `test_plugin_js_deno/` — already correct
- **NO** touching `allocator`, `graph`, ABI structs, `abi/mod.rs`, `registry/mod.rs`
- **NO** touching `pack/mod.rs` scaffold generators (out of scope)
- **NO** extra validation beyond the 2 new `LoaderError` variants needed (`BundleNotADirectory`, `ManifestMissingFile`)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: Tests-after (existing test suite)
- **Framework**: `cargo test`

### QA Policy
Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **API/Backend**: Use Bash (cargo test / grep / cargo clippy)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — must be sequential within wave):
└── Task 1: Add new LoaderError variants [quick]
└── Task 2: Refactor parse_manifest() + ManifestData [unspecified-high]

Wave 2 (Core API refactor — Task 3 depends on 1+2):
└── Task 3: Refactor load_bundle() to accept ManifestData [unspecified-high]
└── Task 4: Update scanner — remove flat-file branch [quick]

Wave 3 (Runtime + Reload — depend on 1+2+3+4):
├── Task 5: Update build.rs — create bundle dirs + *_DIR env vars [unspecified-high]
├── Task 6: Update runtime/mod.rs — load_bundle_with(), watcher, manifest.path storage [unspecified-high]
└── Task 7: Update reload/mod.rs — path.parent() bundle dir derivation [quick]

Wave 4 (Language loaders + Generators — depend on 3):
├── Task 8: Update polyplug-js + polyplug-js-deno loaders [quick]
└── Task 9: Update generators (rust, cpp manifest templates) [quick]

Wave 5 (Tests — depend on 3+4+5+6+7):
├── Task 10: Update integration_discovery tests [unspecified-high]
├── Task 11: Update integration_version tests [unspecified-high]
├── Task 12: Update integration_reload tests [unspecified-high]
└── Task 13: Update library_lifetime test + delete flat fixture files [quick]

Wave 6 (Cleanup + Docs):
└── Task 14: Update polyplug_prd.md sections 11 + 13 [writing]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Code quality review [unspecified-high]
├── Task F3: Real QA — full cargo test [unspecified-high]
└── Task F4: Scope fidelity check [deep]
```

**Critical Path**: T1 → T2 → T3 → T4 → T5+T6+T7 → T8+T9 → T10+T11+T12+T13 → T14 → F1-F4
**Parallel Speedup**: ~50% faster than sequential
**Max Concurrent**: 3 (Wave 3 + Wave 4)

### Dependency Matrix
- T1: — → T2, T3, T6, T10
- T2: T1 → T3, T4, T6, T10, T11
- T3: T1, T2 → T6, T7, T8, T10, T11, T12, T13
- T4: T2 → T6, T10
- T5: T2 → T12, T13
- T6: T1, T2, T3, T4 → T10, T12
- T7: T3 → T12
- T8: T3 → (none, leaf)
- T9: — → (none, leaf)
- T10: T2, T3, T4, T6 → F1-F4
- T11: T2, T3 → F1-F4
- T12: T3, T5, T6, T7 → F1-F4
- T13: T3, T5 → F1-F4
- T14: all → F1-F4

---

## TODOs

---

- [x] 1. Add new `LoaderError` variants (`BundleNotADirectory`, `ManifestMissingFile`)

  **What to do**:
  - Open `crates/polyplug/src/error/mod.rs`
  - Inside the `LoaderError` enum (currently ends at line 154), add exactly these two variants:
    ```rust
    #[error("bundle path is not a directory: `{path}`")]
    BundleNotADirectory { path: std::path::PathBuf },

    #[error("bundle \"{bundle}\" manifest.toml has an empty or missing `file` field")]
    ManifestMissingFile { bundle: String },
    ```
  - No other changes to this file.

  **Must NOT do**:
  - Do NOT add `BundleFile::PerPlatform` / `PlatformNotSupported` / `ManifestWrongFileFormat` / `ManifestInvalidFilePath` variants (deferred)
  - Do NOT touch `RuntimeError`, `RegistryError`, `GraphError`, `AllocatorError`

  **Recommended Agent Profile**:
  > Single file, ~8 lines added, no logic.
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (foundation for T2)
  - **Parallel Group**: Wave 1, sequential with T2
  - **Blocks**: Tasks 2, 3, 6, 10
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/error/mod.rs:53-154` — `LoaderError` enum; insert after line 154 (after `FunctionCountMismatch`)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` → PASS (no compilation errors)
  - [ ] `grep -n 'BundleNotADirectory\|ManifestMissingFile' crates/polyplug/src/error/mod.rs` → 2 matches (the variant definitions)

  **QA Scenarios**:
  ```
  Scenario: New error variants compile and are reachable
    Tool: Bash (cargo)
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Run: grep -c 'BundleNotADirectory\|ManifestMissingFile' crates/polyplug/src/error/mod.rs
    Expected Result: Build succeeds (exit 0); grep finds exactly 2 lines
    Evidence: .sisyphus/evidence/task-1-compile.txt
  ```

  **Commit**: YES (group with T2)
  - Message: `feat(loader): add BundleNotADirectory/ManifestMissingFile error variants`
  - Files: `crates/polyplug/src/error/mod.rs`
  - Pre-commit: `cargo build -p polyplug`


- [x] 2. Refactor `parse_manifest()` and `ManifestData` in `loader/manifest/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/loader/manifest/mod.rs` (204 lines currently)
  - Change the `parse_manifest()` function signature from `fn parse_manifest(bundle_path: &Path) -> Result<ManifestData, LoaderError>` (currently in `loader/mod.rs` line 162) to accept a DIRECTORY path:
    - NOTE: `parse_manifest()` lives in `crates/polyplug/src/loader/mod.rs` (line 162), not in manifest/mod.rs. We are refactoring it in Task 2 because it depends on `ManifestData`. The actual change is in `loader/mod.rs`.
  - In `crates/polyplug/src/loader/manifest/mod.rs`: add a validation method to `ManifestData`:
    ```rust
    impl ManifestData {
        /// Validate that the `file` field is non-empty after parsing.
        /// Returns Err(ManifestMissingFile) if the field is empty.
        pub fn validate_file(&self) -> Result<(), crate::error::LoaderError> {
            if self.file.trim().is_empty() {
                return Err(crate::error::LoaderError::ManifestMissingFile {
                    bundle: self.bundle_name.clone(),
                });
            }
            Ok(())
        }
    }
    ```
  - In `crates/polyplug/src/loader/mod.rs`, rewrite `parse_manifest()` (lines 162-221) to:
    1. Accept `bundle_dir: &Path` instead of `bundle_path: &Path`
    2. Check `!bundle_dir.is_dir()` → return `Err(LoaderError::BundleNotADirectory { path: bundle_dir.to_path_buf() })`
    3. Read `bundle_dir.join("manifest.toml")`
    4. If the file doesn't exist → return `Err(LoaderError::ManifestParse { path: ..., reason: "manifest.toml not found in bundle directory" })`  (NOT silent default — the old fallback is removed)
    5. Parse TOML → validate runtime non-empty (same as now)
    6. Call `data.validate_file()?` to ensure `file` is non-empty
    7. Set `manifest.path = bundle_dir.to_path_buf()` (store the DIRECTORY, not the .so path)
    8. Return `Ok(manifest)`
  - Update the doc comment on `parse_manifest()` to describe the new contract
  - Change visibility from `pub(crate)` to `pub` (it's needed by the `library_lifetime` integration test in Task 13)
  - Remove the `#[allow(dead_code)]` attribute from `parse_manifest()` if present (line 161)

  **Must NOT do**:
  - Do NOT add `BundleFile` enum (deferred)
  - Do NOT change `ManifestData.file: String` type
  - Do NOT implement path security validation (`..` or `/` prefix checks) — deferred with PerPlatform
  - Do NOT touch `RawManifestDependency`, `ManifestDependency`, or `resolved_dependencies()`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (must follow T1)
  - **Parallel Group**: Wave 1 (sequential after T1)
  - **Blocks**: Tasks 3, 4, 6, 10, 11
  - **Blocked By**: Task 1 (needs `BundleNotADirectory` variant)

  **References**:
  - `crates/polyplug/src/loader/mod.rs:153-221` — current `parse_manifest()` implementation to rewrite
  - `crates/polyplug/src/loader/manifest/mod.rs:79-128` — `ManifestData` struct + `impl` block; add `validate_file()` after `resolved_dependencies()`
  - `crates/polyplug/src/error/mod.rs:54-58` — `LoaderError::BundleNotADirectory` + `ManifestMissingFile` (just added in T1)
  - `crates/polyplug/src/loader/scanner/mod.rs:93-124` — EXISTING directory-bundle branch in scanner; `parse_manifest` equivalent is done inline there (read + toml::from_str). After T2, scanner could call `parse_manifest(entry_path)` instead of inline TOML parsing (optional simplification, but NOT required).

  **Acceptance Criteria**:
  - [ ] `cargo test -p polyplug --lib` → PASS
  - [ ] `cargo test -p polyplug loader::tests` → PASS

  **QA Scenarios**:
  ```
  Scenario: parse_manifest rejects non-directory path
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test -p polyplug loader::tests 2>&1
    Expected Result: Tests pass (no compilation failures)
    Evidence: .sisyphus/evidence/task-2-loader-unit.txt

  Scenario: parse_manifest rejects missing file field (unit test)
    Tool: Bash (cargo test)
    Preconditions: write a temp dir with manifest.toml containing `runtime = "native"` and no `file` field
    Steps:
      1. Run: cargo test -p polyplug 2>&1 | grep -E 'FAILED|ok'
    Expected Result: All tests pass; ManifestMissingFile is returned for empty-file manifests
    Evidence: .sisyphus/evidence/task-2-missing-file.txt
  ```

  **Commit**: YES (group with T1)
  - Message: `feat(loader): parse_manifest reads dir/manifest.toml; validate file field`
  - Files: `crates/polyplug/src/loader/mod.rs`, `crates/polyplug/src/loader/manifest/mod.rs`
  - Pre-commit: `cargo test -p polyplug`


- [x] 3. Refactor `load_bundle()` to accept `ManifestData` — eliminate double-manifest-read

  **What to do**:
  - In `crates/polyplug/src/loader/mod.rs`, change `load_bundle()` signature from:
    ```rust
    pub fn load_bundle(path: &Path, registry: &Registry, host_vtable: &'static HostVTable) -> Result<(), LoaderError>
    ```
    to:
    ```rust
    pub fn load_bundle(path: &Path, manifest: &ManifestData, registry: &Registry, host_vtable: &'static HostVTable) -> Result<(), LoaderError>
    ```
  - Remove the internal `parse_manifest()` call at `load_bundle()` line 248 (`let mut manifest: ManifestData = parse_manifest(path)?;`)
  - Remove the `bundle_id` computation from `load_bundle()` (it was: `manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name)` at line 251). The caller now provides a fully-populated `ManifestData` (with `bundle_id` already set).
  - The rest of `load_bundle()` (dep declaration, dlopen, ABI version check, init, registrar callback) stays unchanged.
  - Update `NativeBundleLoader::load()` (lines 124-131) which calls `load_bundle(path, ...)`. The `NativeBundleLoader` does NOT have access to the pre-parsed manifest. The loader trait `BundleLoader::load()` signature is: `fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError>`. The manifest is NOT passed through the trait. So `NativeBundleLoader` must call `parse_manifest(path.parent().unwrap_or(path))` to get the manifest before calling `load_bundle(path, &manifest, ...)`. This is only used by the `library_lifetime` test (direct `load_bundle()` call). When called via `RuntimeBuilder::build()`, the native loader is dispatched via `loader.load(effective_path, registrar)` → `NativeBundleLoader::load(effective_path)` → it will call `parse_manifest(effective_path.parent())` to re-read. This is acceptable — the double-read is avoided in the SCANNER path via build() which already has manifest; for NativeBundleLoader's direct load path, one re-read is necessary.
    - WAIT — re-read Metis recommendation. Actually the correct fix is: `NativeBundleLoader` derives the bundle dir as `path.parent()` and calls `parse_manifest(bundle_dir)?` then `load_bundle(path, &manifest, ...)`. The runtime's `build()` currently calls `loader.load(&effective_path, &mut registrar)`. `effective_path` is the `.so` file. `NativeBundleLoader::load(effective_path)` then derives `effective_path.parent()` = bundle dir → reads manifest (one re-read only). This is clean.
  - Update `library_lifetime/mod.rs` test to provide a manifest. See Task 13.

  **Must NOT do**:
  - Do NOT change the `BundleLoader` trait signature (`fn load(&self, path: &Path, registrar: &mut PluginRegistrar)`)
  - Do NOT change `RegistryError`, `RuntimeError`
  - Do NOT pass manifest through the BundleLoader trait

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (must follow T2)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 6, 7, 8, 10, 11, 12, 13
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug/src/loader/mod.rs:240-394` — full `load_bundle()` function to modify (signature + remove internal manifest parse)
  - `crates/polyplug/src/loader/mod.rs:100-131` — `NativeBundleLoader` and its `load()` impl
  - `crates/polyplug/src/loader/mod.rs:162-221` — `parse_manifest()` (now rewritten in T2); NativeBundleLoader calls it with `path.parent().unwrap_or(path)`
  - `crates/polyplug/src/loader/manifest/mod.rs:80-116` — `ManifestData` struct (the type now passed as `manifest: &ManifestData`)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` → PASS (no compilation errors)
  - [ ] `cargo test -p polyplug --lib` → PASS

  **QA Scenarios**:
  ```
  Scenario: load_bundle compiles with new signature
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
    Expected Result: Exit 0, no errors
    Evidence: .sisyphus/evidence/task-3-build.txt
  ```

  **Commit**: YES (group with T4)
  - Message: `refactor(loader): load_bundle accepts ManifestData; NativeBundleLoader derives bundle dir`
  - Files: `crates/polyplug/src/loader/mod.rs`
  - Pre-commit: `cargo build -p polyplug`


- [x] 4. Update scanner — remove flat-file branch; update unit tests

  **What to do**:
  - In `crates/polyplug/src/loader/scanner/mod.rs`:
  - **Remove the entire flat-file branch** (lines 59–92): the `if metadata.is_file()` block that checks for `.so/.dll/.dylib` extension and companion `.manifest.toml`. Delete this block entirely.
  - The `else if metadata.is_dir()` branch (lines 93–124) becomes the ONLY path — it stays unchanged except:
    - Now it's just an `if metadata.is_dir()` block (no `else if`)
    - After parsing, call `data.validate_file()?` or handle the ManifestMissingFile error (log warning + skip, consistent with other skipped entries)
  - **Update unit tests** in the `#[cfg(test)]` block (lines 156-245):
    - `scan_dir_skips_bundle_without_manifest` (line 170): currently creates `plugin.so` with no companion manifest. After migration, this test is no longer meaningful (flat .so with no manifest is silently ignored anyway since it's not a directory). Change test to: create a DIRECTORY with no `manifest.toml` inside → verify it is skipped.
    - `scan_dir_finds_bundle_with_manifest` (line 179): currently creates `myplugin.so` + `myplugin.manifest.toml`. Change to: create `myplugin/` directory with `myplugin/manifest.toml` (containing `bundle_name = "myplugin"` + `runtime = "native"` + `file = "myplugin.so"`).
    - `scan_dirs_deduplicates_by_path` (line 222): currently uses flat `.so + .manifest.toml`. Change to: create `plugin/` directory with `plugin/manifest.toml`.
    - `scan_dir_finds_dir_bundle_with_manifest` (line 201): already tests directory bundles — keep as-is, just verify it still passes.
    - Add a new test: `scan_dir_ignores_flat_so_files` — creates a flat `plugin.so` in a tempdir (no directory), verifies `scan_dir()` returns empty (flat .so files are now ignored).

  **Must NOT do**:
  - Do NOT add any flat-file fallback path
  - Do NOT change `scan_dirs()` logic
  - Do NOT change `scan_dir()` return type

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T3, within Wave 2)
  - **Parallel Group**: Wave 2 (T3 and T4 can run in parallel after T2)
  - **Blocks**: Tasks 6, 10
  - **Blocked By**: Task 2

  **References**:
  - `crates/polyplug/src/loader/scanner/mod.rs:59-92` — flat-file branch to DELETE
  - `crates/polyplug/src/loader/scanner/mod.rs:93-124` — directory branch to KEEP (becomes the only branch)
  - `crates/polyplug/src/loader/scanner/mod.rs:156-245` — unit tests to update

  **Acceptance Criteria**:
  - [ ] `cargo test -p polyplug loader::scanner::tests` → PASS (all 4 original + 1 new test)
  - [ ] After test pass: `grep -n 'manifest.toml' crates/polyplug/src/loader/scanner/mod.rs` shows only `join("manifest.toml")` (no `with_extension`)

  **QA Scenarios**:
  ```
  Scenario: Scanner ignores flat .so files
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test -p polyplug loader::scanner::tests 2>&1
    Expected Result: All tests pass including new scan_dir_ignores_flat_so_files
    Evidence: .sisyphus/evidence/task-4-scanner-tests.txt
  ```

  **Commit**: YES (group with T3)
  - Files: `crates/polyplug/src/loader/scanner/mod.rs`


- [x] 5. Update `build.rs` — create bundle directories for reload/depender plugins; emit `*_DIR` env vars

  **What to do**:
  - Open `crates/polyplug/build.rs`
  - For each of the 5 native fixture plugins that need bundle dirs: `reload_plugin_v1`, `reload_plugin_v2`, `depender_plugin`, `test_plugin`, and the C++ test plugin:
    - After copying the `.so` to `tests/fixtures/`, additionally:
      1. Create a bundle directory: `tests/fixtures/{bundle_name_dir}/` (e.g. `tests/fixtures/reload_plugin_v1/`)
      2. Copy the `.so` into that dir: `tests/fixtures/reload_plugin_v1/libreload_plugin_v1.so`
      3. Write a `manifest.toml` into that dir with the correct content (see exact content below for each)
      4. Emit a new env var: `RELOAD_PLUGIN_V1_DIR`, `RELOAD_PLUGIN_V2_DIR`, `DEPENDER_PLUGIN_DIR`, `TEST_PLUGIN_DIR`
  - Keep existing `*_SO` env vars (pointing to flat `.so` in fixtures root) — these are still used by `reload_bundle_impl` which receives the `.so` path from the watcher and derives the parent dir.
  - **Exact manifest.toml content for each bundle dir:**
    - `tests/fixtures/reload_plugin_v1/manifest.toml`:
      ```toml
      bundle_name                = "reload_plugin_v1"
      version                    = "1.0"
      runtime                    = "native"
      file                       = "libreload_plugin_v1.so"
      needs_reinit_on_dep_reload = false
      provides                   = ["reload.test"]
      [function_count]
      "reload.test@1" = 1
      ```
    - `tests/fixtures/reload_plugin_v2/manifest.toml`:
      ```toml
      bundle_name                = "reload_plugin_v1"
      version                    = "2.0"
      runtime                    = "native"
      file                       = "libreload_plugin_v2.so"
      needs_reinit_on_dep_reload = false
      provides                   = ["reload.test"]
      [function_count]
      "reload.test@1" = 1
      ```
      NOTE: `bundle_name` is `reload_plugin_v1` (same contract hash), version bumped to `2.0`, file is `libreload_plugin_v2.so`.
    - `tests/fixtures/depender_plugin/manifest.toml`:
      ```toml
      bundle_name                = "depender_plugin"
      version                    = "1.0"
      runtime                    = "native"
      file                       = "libdepender_plugin.so"
      needs_reinit_on_dep_reload = true
      provides                   = ["depender.test"]
      [function_count]
      "depender.test@1" = 1
      [[dependency]]
      kind        = "bundle"
      contract    = "reload.test@1"
      min_version = "1.0"
      bundle      = "reload_plugin_v1"
      contract_id = 16526955377754357857
      bundle_id   = 16808897324254478442
      ```
      NOTE: `contract_id` and `bundle_id` values are from the current `tests/fixtures/libdepender_plugin.manifest.toml` — copy them verbatim.
    - `tests/fixtures/test_plugin_dir/manifest.toml`:
      ```toml
      bundle_name = "test_plugin"
      version     = "1.0"
      runtime     = "native"
      file        = "libtest_plugin.so"
      provides    = ["test.add"]
      needs_reinit_on_dep_reload = false
      [function_count]
      "test.add@1" = 4
      ```
      The `.so` copied into `tests/fixtures/test_plugin_dir/libtest_plugin.so`.
      Emit: `TEST_PLUGIN_DIR` env var pointing to `tests/fixtures/test_plugin_dir/`.
  - Also add `cargo:rerun-if-changed` for the fixture manifest content (use the source `.so` files as triggers — already present).

  **Must NOT do**:
  - Do NOT remove existing `RELOAD_PLUGIN_V1_SO`, `RELOAD_PLUGIN_V2_SO`, `DEPENDER_PLUGIN_SO`, `TEST_PLUGIN_SO` env vars
  - Do NOT create bundle dirs for: `memory_plugin`, `error_plugin`, C++ plugins (not needed by tests that use directory bundles)
  - Do NOT change the Lua/Python/JS fixture paths (`TEST_LUA_PLUGIN`, `TEST_PYTHON_PLUGIN`, `TEST_JS_PLUGIN`, `TEST_JS_DENO_PLUGIN`)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T6 and T7, within Wave 3)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 12, 13
  - **Blocked By**: Task 2

  **References**:
  - `crates/polyplug/build.rs:201-334` — reload/depender plugin build sections (copy pattern to follow)
  - `tests/fixtures/libdepender_plugin.manifest.toml` — VERBATIM copy contract_id and bundle_id values from here
  - `tests/fixtures/libreload_plugin_v1.manifest.toml` — source of version/bundle_name values

  **Acceptance Criteria**:
  - [ ] After `cargo build -p polyplug`: directories `tests/fixtures/reload_plugin_v1/`, `tests/fixtures/reload_plugin_v2/`, `tests/fixtures/depender_plugin/`, `tests/fixtures/test_plugin_dir/` all exist with `.so` and `manifest.toml` inside
  - [ ] `RELOAD_PLUGIN_V1_DIR`, `RELOAD_PLUGIN_V2_DIR`, `DEPENDER_PLUGIN_DIR`, `TEST_PLUGIN_DIR` env vars are emitted

  **QA Scenarios**:
  ```
  Scenario: Bundle dirs are created with correct layout
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1 | tail -5
      2. Run: ls tests/fixtures/reload_plugin_v1/
      3. Run: ls tests/fixtures/depender_plugin/
    Expected Result: Each dir contains manifest.toml + the .so file
    Evidence: .sisyphus/evidence/task-5-bundle-dirs.txt
  ```

  **Commit**: YES (group with T6+T7)
  - Files: `crates/polyplug/build.rs`


- [x] 6. Update `runtime/mod.rs` — `load_bundle_with()` accepts dir; `effective_path` uses `BundleFile::resolve()`; watcher goes recursive

  **What to do**:
  - In `crates/polyplug/src/runtime/mod.rs`:

  **A. `load_bundle_with()` (lines 487-549):**
  - Change the opening guard (lines 489-495) from checking `path.with_extension("manifest.toml").exists()` to:
    ```rust
    if !path.is_dir() {
        return Err(PolyplugError::Loader(LoaderError::BundleNotADirectory {
            path: path.to_path_buf(),
        }));
    }
    let manifest_path: PathBuf = path.join("manifest.toml");
    if !manifest_path.exists() {
        return Err(PolyplugError::Loader(LoaderError::ManifestParse {
            path: manifest_path.display().to_string(),
            reason: "manifest.toml not found in bundle directory".to_owned(),
        }));
    }
    ```
  - Change `parse_manifest(path)` call (line 497) to `parse_manifest(path)` — after T2's rewrite, `parse_manifest` accepts a dir path directly, so this call stays as-is (just pass `path` which is now a dir).
  - Change `loader.load(path, &mut registrar)` (line 546) to resolve the actual file path first:
    ```rust
    let effective_path: PathBuf = if !manifest.file.is_empty() {
        path.join(&manifest.file)
    } else {
        path.to_path_buf()
    };
    let result: Result<(), PolyplugError> = loader.load(&effective_path, &mut registrar);
    ```

  **B. `build()` — `effective_path` logic (lines 364-368):**
  - The existing `effective_path` logic already handles directory bundles correctly. After T4 (scanner drops flat branch), `bundle_path` will always be a directory. Simplify the condition:
    ```rust
    // After T4, bundle_path is always a directory with manifest.file populated.
    let effective_path: PathBuf = if !manifest.file.is_empty() {
        bundle_path.join(&manifest.file)
    } else {
        bundle_path.clone()
    };
    ```
  - (The old `bundle_path.is_dir()` check can be kept or removed — simplification is optional.)

  **C. `build()` — `manifest.path` storage (line 309-310):**
  - Currently: `stored_manifest.path = path.clone()` where `path` comes from `(path, manifest)` pair in `discovered`. After T4, the scanner always emits the DIRECTORY as the path. So `stored_manifest.path` will already be the bundle dir. **No change needed** — just verify the scanner's path is the dir.

  **D. `watch_plugin_dir()` — RecursiveMode (line 660):**
  - Change: `watcher.watch(&canonical_dir, notify::RecursiveMode::NonRecursive)` → `watcher.watch(&canonical_dir, notify::RecursiveMode::Recursive)`
  - The watcher event filter (line 618-620) checks for `ext == "so" | "dll" | "dylib"` — this stays correct (watcher now sees `.so` files inside subdirs).
  - The event fires with `path` = the `.so` file inside a bundle subdir. `reload_bundle_impl` receives this `.so` path — after T7, it will derive the bundle dir via `path.parent()`.

  **Must NOT do**:
  - Do NOT change `RuntimeBuilder` struct fields
  - Do NOT change `Runtime::load_bundle()` public signature (it delegates to `load_bundle_with()`)
  - Do NOT change `BundleLoader` trait

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5 and T7, within Wave 3)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 10, 12
  - **Blocked By**: Tasks 1, 2, 3, 4

  **References**:
  - `crates/polyplug/src/runtime/mod.rs:364-368` — effective_path logic in build()
  - `crates/polyplug/src/runtime/mod.rs:487-549` — load_bundle_with() full implementation
  - `crates/polyplug/src/runtime/mod.rs:655-666` — watcher.watch() call with RecursiveMode
  - `crates/polyplug/src/error/mod.rs` — BundleNotADirectory variant (added in T1)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` → PASS
  - [ ] `cargo test -p polyplug --lib` → PASS
  - [ ] `grep 'RecursiveMode' crates/polyplug/src/runtime/mod.rs` → shows `Recursive` (not `NonRecursive`)

  **QA Scenarios**:
  ```
  Scenario: load_bundle_with rejects non-directory path
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test -p polyplug runtime::tests 2>&1
    Expected Result: Tests pass
    Evidence: .sisyphus/evidence/task-6-runtime-tests.txt

  Scenario: Watcher uses recursive mode
    Tool: Bash (grep)
    Steps:
      1. Run: grep 'RecursiveMode' crates/polyplug/src/runtime/mod.rs
    Expected Result: Shows 'Recursive' not 'NonRecursive'
    Evidence: .sisyphus/evidence/task-6-recursive.txt
  ```

  **Commit**: YES (group with T5+T7)
  - Files: `crates/polyplug/src/runtime/mod.rs`


- [x] 7. Update `reload/mod.rs` — `reload_bundle_impl` derives bundle dir via `path.parent()`

  **What to do**:
  - In `crates/polyplug/src/reload/mod.rs`, update `reload_bundle_impl()` (lines 47-242):
  - At line 59, `reload_bundle_impl` calls `crate::loader::parse_manifest(path)`. After T2, `parse_manifest` accepts a directory. The watcher fires with `path` = the `.so` file inside a bundle dir. So derive the bundle dir first:
    ```rust
    let bundle_dir: &Path = path.parent().unwrap_or(path);
    let mut manifest: ManifestData = crate::loader::parse_manifest(bundle_dir)
        .map_err(|e: crate::error::LoaderError| PolyplugError::Loader(e))?;
    manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
    manifest.path = bundle_dir.to_path_buf();  // store the bundle DIR, not the .so path
    ```
  - At line 86-93 (dlopen): `reload_bundle_impl` calls `libloading::Library::new(path)` with the `.so` path. This stays correct — `path` is still the `.so` file (watcher fires on it). The only change is the manifest read now uses the parent dir.
  - At line 61-63 (non-native check): `if manifest.runtime != "native"` — stays unchanged.
  - At line 239 (cascade): `for (_dep_name, dep_path) in dependents { reload_bundle_impl(runtime, &dep_path, ...) }`. `dep_path` comes from `manifest.path` in `find_cascade_targets()`. After this task, `manifest.path` stores the bundle DIR. So `reload_bundle_impl` will be called with a DIR path. At the start of `reload_bundle_impl`, it will call `path.parent()` on a DIR path — this would be the PARENT of the bundle dir, which is wrong.
    - **Fix**: At the start of `reload_bundle_impl`, check if `path` is a directory:
      ```rust
      // If path is a directory (cascade reload passes bundle dir),
      // find the .so file inside it using manifest.file.
      // If path is a file (watcher fires on .so), use path.parent() as the bundle dir.
      let (bundle_dir_path, so_path): (PathBuf, PathBuf) = if path.is_dir() {
          // Path is already the bundle directory (e.g. from cascade).
          // Need to find the .so: parse manifest first to get file name.
          let temp_manifest: ManifestData = crate::loader::parse_manifest(path)
              .map_err(|e| PolyplugError::Loader(e))?;
          let so: PathBuf = path.join(&temp_manifest.file);
          (path.to_path_buf(), so)
      } else {
          // Path is the .so file (watcher path). Bundle dir is parent.
          let dir: PathBuf = path.parent().unwrap_or(path).to_path_buf();
          (dir, path.to_path_buf())
      };
      let mut manifest: ManifestData = crate::loader::parse_manifest(&bundle_dir_path)
          .map_err(|e| PolyplugError::Loader(e))?;
      manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
      manifest.path = bundle_dir_path.clone();
      ```
    - Then use `so_path` (not `path`) for the `libloading::Library::new()` call and for `path_str`.
    - Update `ReloadEvent.bundle_path` to use `bundle_dir_path.display().to_string()`.

  **Must NOT do**:
  - Do NOT change `reload_bundle()` public signature
  - Do NOT change `find_cascade_targets()` (it uses `manifest.path` — correct after the path store fix)
  - Do NOT change quiescence timeout or MAX_CASCADE_DEPTH

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T5 and T6, within Wave 3)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 12
  - **Blocked By**: Task 3

  **References**:
  - `crates/polyplug/src/reload/mod.rs:47-242` — full `reload_bundle_impl()` to update
  - `crates/polyplug/src/reload/mod.rs:244-267` — `find_cascade_targets()` — reads `manifest.path` for cascade; must stay unchanged
  - `crates/polyplug/src/reload/mod.rs:269-281` — `Runtime::reload_bundle()` public wrapper — keep as-is

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` → PASS
  - [ ] `cargo test -p polyplug --lib` → PASS

  **QA Scenarios**:
  ```
  Scenario: reload_bundle_impl derives bundle dir for watcher-fired .so path
    Tool: Bash (cargo build)
    Steps:
      1. Run: cargo build -p polyplug 2>&1
    Expected Result: Exit 0, no errors
    Evidence: .sisyphus/evidence/task-7-build.txt
  ```

  **Commit**: YES (group with T5+T6)
  - Message: `feat(runtime): bundle dirs in build.rs; load_bundle_with dir; recursive watcher; reload path.parent()`
  - Files: `crates/polyplug/src/reload/mod.rs`
  - Pre-commit: `cargo build -p polyplug`


- [x] 8. Update JS loaders — remove dead `is_dir()` check; use resolved file path directly

  **What to do**:
  - In `crates/polyplug-js/src/lib/loader/mod.rs`:
    - Find the `is_dir()` check (approximately lines that do: `let bundle_path = if path.is_dir() { path.join("bundle.js") } else { path.to_path_buf() };`)
    - Replace with: `let bundle_path: PathBuf = path.to_path_buf();`
    - Remove the dead `if path.is_dir()` branch entirely.
    - Remove the comment about directory bundle layout (it no longer applies — the runtime resolves the file before calling the loader).
  - In `crates/polyplug-js-deno/src/lib/loader/mod.rs`:
    - Find the code that unconditionally joins `bundle.js`/`index.ts` onto `path` (the path is assumed to be a directory).
    - Change it to use the given `path` directly as the module file:
      ```rust
      let module_path: PathBuf = path.to_owned();
      let module_source: String = std::fs::read_to_string(&module_path) ...;
      ```
    - The `bundle_path.join("bundle.js") / join("index.ts")` probing logic is removed — the runtime already resolved `manifest.file` and passes the exact file path.

  **Must NOT do**:
  - Do NOT change the `BundleLoader` trait implementation signatures
  - Do NOT touch `polyplug-lua/src/lib/loader/mod.rs` (already correct — reads file directly)
  - Do NOT touch `polyplug-python/src/lib/mod.rs` (already correct)
  - Do NOT touch `polyplug-dotnet/src/lib/mod.rs` (already correct)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T9, within Wave 4)
  - **Parallel Group**: Wave 4
  - **Blocks**: None (leaf)
  - **Blocked By**: Task 3

  **References**:
  - `crates/polyplug-js/src/lib/loader/mod.rs` — full file; find `is_dir()` check and remove
  - `crates/polyplug-js-deno/src/lib/loader/mod.rs` — full file; find `join("bundle.js")` / `join("index.ts")` probing and replace with direct path use

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug-js` → PASS
  - [ ] `cargo build -p polyplug-js-deno` → PASS
  - [ ] `grep -n 'is_dir' crates/polyplug-js/src/lib/loader/mod.rs` → zero results

  **QA Scenarios**:
  ```
  Scenario: JS loaders build cleanly without is_dir check
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug-js -p polyplug-js-deno 2>&1
    Expected Result: Exit 0
    Evidence: .sisyphus/evidence/task-8-js-loaders.txt
  ```

  **Commit**: YES (group with T9)
  - Message: `refactor(js-loaders): remove dead is_dir() check; use resolved file path`
  - Files: `crates/polyplug-js/src/lib/loader/mod.rs`, `crates/polyplug-js-deno/src/lib/loader/mod.rs`
  - Pre-commit: `cargo build -p polyplug-js -p polyplug-js-deno`


- [x] 9. Update native generators (rust, cpp) — canonical `file = "libfoo.so"` manifest template

  **What to do**:
  - In `crates/polyplugc/src/generators/rust/mod.rs`, in `generate_bundle_manifest()` (line 234):
    - Line 245: `let file: String = format!("lib{}.so", bundle.name);` — this is already the correct value for Linux.
    - The generated manifest currently emits `file = "lib{name}.so"`. This is correct for directory bundles (the file lives inside the bundle dir, the manifest records its name).
    - Add a comment above the `file` line in the generated manifest making it clear the path is relative to the bundle directory:
      ```rust
      format!(...
", "# file is relative to this bundle directory\n", "file = \"{file}\"\n", ...)
      ```
    - The template already outputs the right value — minimal change needed.
  - In `crates/polyplugc/src/generators/cpp/mod.rs`, in `generate_bundle_manifest_cpp()` (line 546):
    - Same: `let file: String = format!("lib{}.so", bundle.name);` at line 557 — already correct.
    - Add the same comment above the `file` line in the generated output.
  - Both generators: remove any generated comment noise about per-platform tables (since `BundleFile::PerPlatform` is deferred, no need to emit commented-out `[bundle.file]` examples).

  **Must NOT do**:
  - Do NOT change csharp, python, lua, js_quickjs, js_deno generators (their `file = "..."` is already correct for non-native)
  - Do NOT add `[bundle.file]` table emission (deferred)
  - Do NOT change the function signatures or return types

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T8, within Wave 4)
  - **Parallel Group**: Wave 4
  - **Blocks**: None (leaf)
  - **Blocked By**: None (independent; but logically follows T2 which established `file = "..."` as canonical)

  **References**:
  - `crates/polyplugc/src/generators/rust/mod.rs:234-322` — `generate_bundle_manifest()` function
  - `crates/polyplugc/src/generators/cpp/mod.rs:546-636` — `generate_bundle_manifest_cpp()` function

  **Acceptance Criteria**:
  - [ ] `cargo test -p polyplugc 2>&1` → PASS (generator unit tests pass)
  - [ ] Generated manifest content for a sample native bundle contains `file = "lib<name>.so"` (relative path, not absolute)

  **QA Scenarios**:
  ```
  Scenario: Rust generator produces correct manifest
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test -p polyplugc 2>&1 | tail -20
    Expected Result: All tests pass
    Evidence: .sisyphus/evidence/task-9-generator-tests.txt
  ```

  **Commit**: YES (group with T8)
  - Files: `crates/polyplugc/src/generators/rust/mod.rs`, `crates/polyplugc/src/generators/cpp/mod.rs`
  - Pre-commit: `cargo test -p polyplugc`


- [x] 10. Update `integration_discovery` tests — directory-bundle model

  **What to do**:
  - Open `tests/integration_discovery/mod.rs`
  - **Rewrite `write_manifest()` helper** (lines 20-25). Change from creating `{stem}.so + {stem}.manifest.toml` flat files to creating a bundle directory:
    ```rust
    /// Write a bundle directory: `<dir>/<stem>/manifest.toml` + `<dir>/<stem>/<stem>.so` stub.
    fn write_bundle_dir(dir: &Path, stem: &str, toml_content: &str) {
        let bundle_dir: PathBuf = dir.join(stem);
        fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        let so_name: String = format!("{stem}.so");
        fs::write(bundle_dir.join(&so_name), b"").expect("write stub so");
        let manifest_toml: String = format!("{}\nfile = \"{}\"\n", toml_content, so_name);
        fs::write(bundle_dir.join("manifest.toml"), manifest_toml).expect("write manifest.toml");
    }
    ```
    NOTE: The `toml_content` passed in by tests does NOT include a `file` field currently. `write_bundle_dir` must inject the `file` field. Check existing test call sites — if they already include `file =` in the toml_content, do NOT double-add.
  - **Update all 5 tests** that call `write_manifest()` to call `write_bundle_dir()` instead (rename only — no other change needed since the content is the same).
  - **Test `explicit_load_bundle_missing_manifest_errors`** (lines 294-327): Currently passes a `.so` file path to `runtime.load_bundle()` and expects `ManifestParse` error. After migration, `load_bundle()` requires a directory. Change test to:
    - Create a temp dir and pass `tmp.path()` directly (no files created inside) — OR — create a subdirectory without `manifest.toml` inside.
    - Expect `Err(PolyplugError::Loader(LoaderError::ManifestParse { .. }))` — the message will say "manifest.toml not found in bundle directory".
    - OR: pass a regular file path (not a dir) and expect `BundleNotADirectory` error.
    - **Recommended**: create a plain file (not a dir) and pass it, expect `BundleNotADirectory`.
  - **Test `malformed_manifest_skips_bundle`** (line 228): currently writes a bad manifest with `bundle_b.so + bundle_b.manifest.toml`. Change to create `bundle_b/` directory with a malformed `bundle_b/manifest.toml`. The test verifies bundle_b is skipped — logic stays the same.

  **Must NOT do**:
  - Do NOT change the `write_script_bundle()` helper (lines 27-39) — already correct
  - Do NOT add new tests beyond the 5 existing + the `explicit_load_bundle_missing_manifest_errors` update
  - Do NOT touch `integration_load`, `integration_dispatch` etc.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T11, T12, T13 in Wave 5)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final wave
  - **Blocked By**: Tasks 2, 3, 4, 6

  **References**:
  - `tests/integration_discovery/mod.rs:1-327` — full file to update
  - `crates/polyplug/src/loader/scanner/mod.rs:23-132` — scan_dir() after T4; shows what the scanner expects (directory with manifest.toml)

  **Acceptance Criteria**:
  - [ ] `cargo test --test integration_discovery` → all 5 tests pass

  **QA Scenarios**:
  ```
  Scenario: All 5 integration_discovery tests pass
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test --test integration_discovery 2>&1
    Expected Result: 5 tests pass, 0 failures
    Evidence: .sisyphus/evidence/task-10-discovery.txt
  ```

  **Commit**: YES (group with T11+T12+T13)
  - Files: `tests/integration_discovery/mod.rs`


- [x] 11. Update `integration_version` tests — directory-bundle model

  **What to do**:
  - Open `tests/integration_version/mod.rs` (599 lines)
  - **Rewrite `write_bundle_manifest()` helper** (lines 66-123): Change from creating `{bundle_name}.so + {bundle_name}.manifest.toml` flat to creating a bundle directory:
    ```rust
    fn write_bundle_manifest(
        dir: &TempDir,
        bundle_name: &str,
        version: &str,
        provides: &[&str],
        function_count_entries: &[(&str, u32)],
        deps: &[(&str, u64, &str)],
    ) -> PathBuf {
        // Create bundle directory
        let bundle_dir: PathBuf = dir.path().join(bundle_name);
        fs::create_dir_all(&bundle_dir).expect("create bundle dir");
        // Write stub .so inside bundle dir
        let so_name: String = format!("{bundle_name}.so");
        let so_path: PathBuf = bundle_dir.join(&so_name);
        fs::write(&so_path, b"").expect("write stub so");
        // ... build TOML content (same as before) ...
        // Add file = "..." to the TOML
        let toml_content: String = format!(..., file = so_name, ...);
        fs::write(bundle_dir.join("manifest.toml"), toml_content).expect("write manifest");
        bundle_dir  // return the DIRECTORY path, not the .so path
    }
    ```
  - The return value changes from `PathBuf` (the `.so`) to `PathBuf` (the bundle DIR).
  - **Update ALL call sites** of `write_bundle_manifest()` throughout the file (14+ tests). Each call site currently passes the returned `so_path` to `Runtime::builder().plugin_dir(path.parent().unwrap())`. After migration, the returned `bundle_dir` is inside `dir.path()` — call sites can still do `Runtime::builder().plugin_dir(dir.path().to_path_buf())` (same as before, since the bundle dir is a subdir of the temp dir).
  - Most tests use `plugin_dir(tmp.path().to_path_buf())` and DON'T use the returned path at all — for those, the change is transparent. Check each test carefully.

  **Must NOT do**:
  - Do NOT change the `NoopLoader`, `WARNING_SINK`, or `ensure_warning_registered()` helpers
  - Do NOT change test logic — only the fixture creation mechanism

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T12, T13 in Wave 5)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final wave
  - **Blocked By**: Tasks 2, 3

  **References**:
  - `tests/integration_version/mod.rs:66-123` — `write_bundle_manifest()` helper to rewrite
  - `tests/integration_version/mod.rs:125-599` — all test functions that call `write_bundle_manifest()`; check each call site for path usage

  **Acceptance Criteria**:
  - [ ] `cargo test --test integration_version` → all tests pass

  **QA Scenarios**:
  ```
  Scenario: All integration_version tests pass with directory bundles
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test --test integration_version 2>&1
    Expected Result: All tests pass, 0 failures
    Evidence: .sisyphus/evidence/task-11-version.txt
  ```

  **Commit**: YES (group with T10+T12+T13)
  - Files: `tests/integration_version/mod.rs`


- [x] 12. Update `integration_reload` tests — use `*_DIR` env vars for `load_bundle()`

  **What to do**:
  - Open `tests/integration_reload/mod.rs` (241 lines)
  - **Tests a–f, h**: These call `rt.load_bundle(Path::new(env!("RELOAD_PLUGIN_V1_SO")))`. After migration, `load_bundle()` requires a directory path. Change these to use `env!("RELOAD_PLUGIN_V1_DIR")` (emitted by build.rs in T5):
    ```rust
    rt.load_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_DIR"))).expect("load v1");
    ```
  - **`reload_bundle()` calls**: These still use `RELOAD_PLUGIN_V1_SO`/`V2_SO` env vars (reload_bundle accepts the `.so` path; the impl derives parent dir). NO CHANGE for reload_bundle calls.
  - **`test_e_cascade_reload`** (line 107): calls `rt.load_bundle(DEPENDER_PLUGIN_SO)` and `rt.load_bundle(RELOAD_PLUGIN_V1_SO)`. Change both load calls to use `DEPENDER_PLUGIN_DIR` and `RELOAD_PLUGIN_V1_DIR`.
  - **`test_g_file_watcher`** (`#[cfg(feature = "hot-reload")]`, line 174-210):
    - Currently copies `RELOAD_PLUGIN_V1_SO` and its companion `.manifest.toml` (via `path.with_extension("manifest.toml")`) to a tempdir.
    - After migration: copy the ENTIRE bundle directory (`RELOAD_PLUGIN_V1_DIR`) to a tempdir subdirectory. OR: create a bundle dir manually in tempdir:
      ```rust
      let bundle_dir: PathBuf = dir.path().join("reload_plugin_v1");
      fs::create_dir_all(&bundle_dir).expect("create bundle dir");
      fs::copy(env!("RELOAD_PLUGIN_V1_SO"), bundle_dir.join("libreload_plugin_v1.so")).expect("copy .so");
      // Also need manifest.toml inside the bundle dir.
      // Copy it from the fixture bundle dir created in T5:
      fs::copy(
          PathBuf::from(env!("RELOAD_PLUGIN_V1_DIR")).join("manifest.toml"),
          bundle_dir.join("manifest.toml"),
      ).expect("copy manifest");
      ```
    - Load using the bundle dir: `rt.load_bundle(bundle_dir.as_path()).expect("load from tmpdir")`.
    - Watch the PARENT dir: `Runtime::watch_plugin_dir(Arc::clone(&rt), dir.path()).expect("watch")`
    - Trigger reload by replacing the `.so` inside the bundle dir: copy `RELOAD_PLUGIN_V2_SO` to `bundle_dir/libreload_plugin_v1.so` (atomic rename as before, but inside the bundle subdir).
    - Remove lines that derive `manifest_src` via `path.with_extension("manifest.toml")` (lines 178-181) — no longer needed.

  **Must NOT do**:
  - Do NOT change `reload_bundle()` call sites to use DIR paths (reload_bundle still takes `.so` path)
  - Do NOT change `get_version_fn()`, quiescence tests, or assertion logic

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T11, T13 in Wave 5)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final wave
  - **Blocked By**: Tasks 3, 5, 6, 7

  **References**:
  - `tests/integration_reload/mod.rs:1-241` — full file to update
  - `tests/integration_reload/mod.rs:172-210` — `test_g_file_watcher` — most complex change
  - `crates/polyplug/build.rs` (after T5) — emits `RELOAD_PLUGIN_V1_DIR`, `RELOAD_PLUGIN_V2_DIR`, `DEPENDER_PLUGIN_DIR`

  **Acceptance Criteria**:
  - [ ] `cargo test --test integration_reload` → all 9 tests (a-i) pass
  - [ ] `cargo test --test integration_reload --features hot-reload` → test_g passes

  **QA Scenarios**:
  ```
  Scenario: All 9 integration_reload tests pass
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test --test integration_reload 2>&1
    Expected Result: 9 tests pass, 0 failures
    Evidence: .sisyphus/evidence/task-12-reload.txt
  ```

  **Commit**: YES (group with T10+T11+T13)
  - Files: `tests/integration_reload/mod.rs`


- [x] 13. Update `library_lifetime` test + delete flat fixture files

  **What to do**:

  **A. Update `tests/library_lifetime/mod.rs`:**
  - The test calls `load_bundle(path, &registry, host_vtable)` where `path = Path::new(env!("TEST_PLUGIN_SO"))` (a raw `.so` file).
  - After T3, `load_bundle()` requires a `ManifestData` parameter. The test must construct one:
    ```rust
    // Use parse_manifest to get the ManifestData from the bundle dir
    let plugin_dir: &Path = Path::new(env!("TEST_PLUGIN_DIR"));
    let mut manifest: polyplug::loader::manifest::ManifestData = polyplug::loader::parse_manifest(plugin_dir)
        .expect("parse_manifest for test_plugin_dir");
    manifest.bundle_id = polyplug::abi::bundle_id(&manifest.bundle_name);
    // The .so path inside the bundle dir:
    let so_path: std::path::PathBuf = plugin_dir.join(&manifest.file);
    load_bundle(&so_path, &manifest, &registry, host_vtable).expect("load_bundle must succeed");
    ```
  - Import `polyplug::loader::parse_manifest` at the top of the file.
  - The rest of the test (no assertions on registry, drop verification) stays unchanged.

  **B. Delete flat fixture files:**
  - Delete: `tests/fixtures/libreload_plugin_v1.manifest.toml`
  - Delete: `tests/fixtures/libreload_plugin_v2.manifest.toml` (if it exists; check `ls tests/fixtures/`)
  - Delete: `tests/fixtures/libdepender_plugin.manifest.toml`
  - Delete: `tests/fixtures/test_plugin.manifest.toml`
  - These are dead code after T5 creates the bundle directories programmatically.
  - DO NOT delete `tests/fixtures/test_plugin.lua` or `tests/fixtures/test_plugin.py` (they are not manifests).

  **Must NOT do**:
  - Do NOT delete `tests/fixtures/test_plugin_js/` or `tests/fixtures/test_plugin_js_deno/`
  - Do NOT delete any `.so` files from `tests/fixtures/`
  - Do NOT change the miri stub test in `library_lifetime`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T10, T11, T12 in Wave 5)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final wave
  - **Blocked By**: Tasks 3, 5

  **References**:
  - `tests/library_lifetime/mod.rs:82-116` — `library_handle_outlives_load_call` test to update
  - `crates/polyplug/src/loader/mod.rs` (after T3) — `load_bundle()` new signature: `(path, manifest, registry, host_vtable)`
  - `crates/polyplug/build.rs` (after T5) — emits `TEST_PLUGIN_DIR` env var
  - `tests/fixtures/libreload_plugin_v1.manifest.toml` — DELETE
  - `tests/fixtures/libreload_plugin_v2.manifest.toml` — DELETE (if exists)
  - `tests/fixtures/libdepender_plugin.manifest.toml` — DELETE
  - `tests/fixtures/test_plugin.manifest.toml` — DELETE

  **Acceptance Criteria**:
  - [ ] `cargo test --test library_lifetime` → PASS
  - [ ] `ls tests/fixtures/*.manifest.toml` → no such files (all deleted)

  **QA Scenarios**:
  ```
  Scenario: library_lifetime test passes with new load_bundle signature
    Tool: Bash (cargo test)
    Steps:
      1. Run: cargo test --test library_lifetime 2>&1
    Expected Result: 1 test passes
    Evidence: .sisyphus/evidence/task-13-library-lifetime.txt

  Scenario: All flat fixture manifests deleted
    Tool: Bash (ls)
    Steps:
      1. Run: ls tests/fixtures/*.manifest.toml 2>&1
    Expected Result: 'No such file or directory' or empty listing
    Evidence: .sisyphus/evidence/task-13-flat-files-deleted.txt
  ```

  **Commit**: YES (group with T10+T11+T12)
  - Message: `test: migrate integration tests to directory-bundle model; delete flat fixture files`
  - Files: `tests/library_lifetime/mod.rs`, `tests/integration_discovery/mod.rs` (T10), `tests/integration_version/mod.rs` (T11), `tests/integration_reload/mod.rs` (T12), deleted fixture .manifest.toml files
  - Pre-commit: `cargo test --workspace`


- [x] 14. Update `polyplug_prd.md` — sections 11 and 13

  **What to do**:
  - Open `polyplug_prd.md` and find sections 11 and 13.
  - Section 11 (Bundle Discovery / Loading): Update to document the directory-bundle model:
    - Every bundle is a directory containing `manifest.toml` + the plugin file(s)
    - `manifest.toml` is at `<bundle_dir>/manifest.toml`
    - The `file` field in `manifest.toml` is a relative path to the plugin file within the bundle dir
    - The scanner discovers bundles by scanning for directories with `manifest.toml` inside
    - Flat `.so + .manifest.toml` format is no longer supported
  - Section 13 (Hot-Reload): Update to reflect:
    - The file watcher now uses `RecursiveMode::Recursive` to detect `.so` changes inside bundle subdirs
    - `reload_bundle()` accepts the `.so` file path inside a bundle dir; the bundle dir is derived via `path.parent()`
    - Cascade reload uses `manifest.path` (stored as bundle dir) for re-loading dependents

  **Must NOT do**:
  - Do NOT restructure the PRD beyond sections 11 and 13
  - Do NOT add new sections
  - Do NOT change other sections

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (should be last, after all implementation is done)
  - **Parallel Group**: Wave 6 (after all implementation waves)
  - **Blocks**: Final wave
  - **Blocked By**: All previous tasks (conceptually — PRD describes the finished system)

  **References**:
  - `polyplug_prd.md` sections 11 and 13 (find by section heading)

  **Acceptance Criteria**:
  - [ ] `grep -A5 '## 11' polyplug_prd.md` → mentions "bundle directory" or "directory-based"
  - [ ] `grep -A5 '## 13' polyplug_prd.md` → mentions `RecursiveMode::Recursive` or `path.parent()`

  **QA Scenarios**:
  ```
  Scenario: PRD sections updated
    Tool: Bash (grep)
    Steps:
      1. Run: grep -i 'bundle directory\|directory-based\|flat bundle' polyplug_prd.md
    Expected Result: At least 2 matches mentioning directory-based bundle layout
    Evidence: .sisyphus/evidence/task-14-prd.txt
  ```

  **Commit**: YES
  - Message: `docs(prd): update sections 11 and 13 for bundle-as-directory model`
  - Files: `polyplug_prd.md`

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns (e.g. `with_extension("manifest.toml")`, `BundleFile::PerPlatform`, flat `.manifest.toml` files). Check evidence files in `.sisyphus/evidence/`.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings` + `cargo fmt --check`. Review changed files for: `.unwrap()` in production (outside `#[cfg(test)]`), `as any`/`#[allow(unused)]` without justification, missing `// SAFETY:` on new `unsafe` blocks, `use` inside functions (AGENTS.md Rule 2), bare inferred types (AGENTS.md Rule 3).
  Output: `Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | AGENTS.md violations [N] | VERDICT`

- [x] F3. **Real QA — Full cargo test** — `unspecified-high`
  Run `cargo test --workspace`. Capture full output. Verify all tests pass. Run `grep -r 'with_extension.*manifest.toml' crates/` to verify zero flat-file patterns remain.
  Output: `Tests [N pass / N fail] | Flat-file patterns [CLEAN/N remaining] | VERDICT`

- [x] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff. Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance. Flag any unaccounted changes.
  Output: `Tasks [N/N compliant] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- Wave 1 (T1+T2): `feat(loader): add BundleNotADirectory/ManifestMissingFile errors; parse_manifest reads dir/manifest.toml`
- Wave 2 (T3+T4): `refactor(loader): load_bundle accepts ManifestData; scanner drops flat-file branch`
- Wave 3 (T5+T6+T7): `feat(runtime): bundle dirs in build.rs; load_bundle_with dir support; recursive watcher; reload path.parent()`
- Wave 4 (T8+T9): `refactor(loader): remove dead is_dir() in js loaders; update generator manifest templates`
- Wave 5 (T10-T13): `test: migrate integration tests to directory-bundle model; delete flat fixture files`
- Wave 6 (T14): `docs(prd): update sections 11 and 13 for bundle-as-directory model`

## Success Criteria

```bash
cargo clippy -- -D warnings          # Expected: zero warnings
cargo fmt --check                    # Expected: clean
cargo test --workspace               # Expected: all pass
grep -r 'with_extension.*manifest.toml' crates/  # Expected: no output
ls tests/fixtures/*.manifest.toml    # Expected: no such files
```
