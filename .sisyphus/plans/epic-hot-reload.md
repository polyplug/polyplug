# Epic Hot-Reload (Epic 17 — Writer Path)

## TL;DR

> **Quick Summary**: Implement the hot-reload writer path for the polyplug runtime — atomic vtable swap via arc-swap, quiescence detection, deferred dlclose, cascading reload for opted-in dependents, opt-in OS file watcher (notify crate), and ReloadEvent callback. The arc-swap reader foundation is already in place from Epic 9.7; this epic adds zero overhead to the reader path.
>
> **Deliverables**:
> - `crates/polyplug/src/reload/mod.rs` — new module with `ReloadEvent`, core reload logic
> - `Runtime::reload_bundle(path)`, `Runtime::refresh_handle(handle)`, `RuntimeBuilder::on_reload(cb)`
> - `Runtime::watch_plugin_dir(dir)` — behind `hot-reload` Cargo feature
> - New error variants: `ReloadFailed`, `QuiescenceTimeout`, `WatcherFailed`
> - New `Registry` helpers: `swap_vtable()`, `find_slots_by_bundle()`
> - V1/V2 native test fixture bundles for reload integration tests
> - `tests/integration_reload/mod.rs` — 9 integration test groups (a–i)
> - `TRUST_MODEL.md` hot-reload safety section
> - `BENCHMARKS.md` reload latency entry
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 4 waves
> **Critical Path**: Task 1 → Task 5 → Task 6 → Task 8 → F1-F4

---

## Context

### Original Request
Implement the Epic 17 hot-reload writer path. Detection (file watcher + explicit API), atomic swap, quiescence spin, deferred dlclose, cascading reload, ReloadEvent callback, integration tests, TRUST_MODEL.md section, BENCHMARKS.md entry. All architectural decisions are pre-answered in the epic brief.

### Interview Summary
**Key Discussions**:
- Arc-swap reader foundation (Epic 9.7) is complete and must not be touched
- `ManifestData.needs_reinit_on_dep_reload` is already parsed — just needs wiring into cascade logic
- `Registry::loaded_libraries` uses a never-drop invariant — reloaded bundles must use a SEPARATE droppable collection (`Runtime::reload_libraries: Mutex<HashMap<u64, libloading::Library>>`)
- `Runtime` does not currently store `ManifestData` per bundle — needs `bundle_manifests: HashMap<String, ManifestData>` added
- `bundle_index` in Registry tracks only the FIRST slot per bundle — `find_slots_by_bundle()` method needed to find ALL slots
- Generation counter MUST be bumped on slot swap so stale handles are detected post-reload
- Quiescence spin needs a 5-second timeout returning `QuiescenceTimeout` error
- Non-native bundle reload (Python, Lua, JS, .NET) descoped to future epic — test 8.i covers native only; non-native loaders must be skipped with a warning
- `PluginVTableGuard` is `!Send` by design — integration tests holding guards must not transfer across threads
- `GLOBAL_REGISTRY` is `OnceLock` — reload modifies the existing Registry via interior mutability, never replaces it

**Research Findings**:
- `registry/mod.rs:64-110`: `RegistrySlot` and `Registry` structs — `vtable: Option<ArcSwap<VTableSlot>>` confirmed present
- `runtime/mod.rs:97-107`: `Runtime` struct fields — `registry: Arc<Registry>`, `loaders: HashMap<String, Box<dyn BundleLoader>>` available; `bundle_manifests` must be added
- `loader/mod.rs:237-390`: `load_bundle()` function — reuse this or factor out a shared helper that returns the library handle instead of calling `registry.push_library()`
- `error/mod.rs:8-40`: `RuntimeError` enum — add `ReloadFailed`, `QuiescenceTimeout`, `WatcherFailed` variants here; `PolyplugError = RuntimeError` alias stays
- `graph/mod.rs:154-171`: `topological_order()` — can be called on a freshly built graph from stored manifests for cascade ordering

### Metis Review
**Identified Gaps** (addressed):
- **C ABI raw pointer staleness after reload**: Resolved — bump `RegistrySlot.generation` on swap; document in TRUST_MODEL.md that raw C pointers from `host_resolve_plugin` must be re-obtained via `refresh_handle()` after reload.
- **Multi-contract bundles**: Resolved — add `Registry::find_slots_by_bundle()` returning `Vec<u32>` by scanning all slots.
- **Quiescence spin unboundedness**: Resolved — 5-second timeout, return `Err(PolyplugError::QuiescenceTimeout)`.
- **`reload_libraries` location**: Resolved — field goes on `Runtime` (not `Registry`) to avoid adding droppable state to the shared `Arc<Registry>`.
- **Test isolation with `OnceLock` globals**: Resolved — each integration test group uses its own `Runtime` instance; tests that create multiple runtimes must be in separate test binaries or use the same singleton — handled by test design.
- **Non-native reload descoped**: Resolved — test 8.i tests native bundle only; adapter-specific reload is future work. A warning is emitted if a non-native bundle path is passed to `reload_bundle()`.
- **`Arc::strong_count` ordering semantics**: Resolved — ArcSwap's `swap()` uses SeqCst fence internally; `strong_count == 1` after swap is sound. Add `// SAFETY:` comment explaining this.
- **Max cascade depth**: Resolved — add a limit of 16 cascade levels; return `Err(ReloadFailed)` if exceeded.

---

## Work Objectives

### Core Objective
Add the hot-reload writer path to the polyplug runtime: atomic vtable swap, quiescence detection, deferred dlclose, cascading reload for opted-in dependents, ReloadEvent callback, and opt-in OS file watcher — all with zero overhead added to the reader hot path.

### Concrete Deliverables
- `crates/polyplug/src/reload/mod.rs` (new file)
- `crates/polyplug/src/registry/mod.rs` (modified — 2 new methods)
- `crates/polyplug/src/runtime/mod.rs` (modified — new fields, builder method, 3 new Runtime methods)
- `crates/polyplug/src/error/mod.rs` (modified — 3 new variants)
- `crates/polyplug/src/lib.rs` (modified — `pub mod reload;`)
- `crates/polyplug/Cargo.toml` (modified — `[features]`, `notify` dep, new `[[test]]` entry)
- `tests/integration_reload/mod.rs` (new file)
- `tests/fixtures/reload_plugin_v1/` (new workspace member — Cargo.toml + src/lib.rs)
- `tests/fixtures/reload_plugin_v2/` (new workspace member — Cargo.toml + src/lib.rs)
- `TRUST_MODEL.md` (modified — new "Hot-Reload Safety Guarantees" section)
- `BENCHMARKS.md` (modified — reload latency row)
- `Cargo.toml` (workspace root — add new fixture members)

### Definition of Done
- [ ] `cargo test --workspace 2>&1 | tail -1` → `test result: ok`
- [ ] `cargo test --workspace --features hot-reload 2>&1 | tail -1` → `test result: ok`
- [ ] `cargo test --test integration_reload -- --nocapture 2>&1 | tail -5` shows all 9 test groups passing
- [ ] `cargo clippy --workspace --features hot-reload -- -D warnings 2>&1 | tail -3` → zero warnings
- [ ] `cargo fmt --check --all 2>&1` → exit 0, no output
- [ ] `TRUST_MODEL.md` contains "Hot-Reload Safety Guarantees" section
- [ ] `BENCHMARKS.md` contains reload latency row

### Must Have
- `reload_bundle(path)` implements all 5 steps (load → swap → quiescence → dlclose → cascade) atomically from the caller's perspective
- Arc-swap reader path untouched — `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()` have zero new branches
- `RegistrySlot.generation` bumped on every vtable swap
- 5-second quiescence timeout returning `Err(QuiescenceTimeout)`
- `hot-reload` Cargo feature gates ALL notify/watcher code
- `on_reload` callback fires AFTER swap, BEFORE dlclose
- `needs_reinit_on_dep_reload = true` bundles are re-initialized in topological order

### Must NOT Have (Guardrails)
- No modifications to frozen ABI structs: `HostVTable`, `PluginVTable`, `PluginHandle`, `StringView`, `Buffer`, `AbiError`
- No new code in `resolve_guard()`, `find_by_contract()`, `find_by_bundle()`, `find_all_by_contract()`, or `host_resolve_plugin()` — reader hot path is immutable
- No `.unwrap()` in production code anywhere
- No `use` statements inside functions or impl blocks
- No bare `filename.rs` module roots — only `filename/mod.rs`
- No `loaded_libraries` modification for reload — that vec is never-drop; use `Runtime::reload_libraries` instead
- No non-native bundle reload implementation in this epic — emit `tracing::warn!` and return `Err(ReloadFailed)` for non-native runtime types
- No test assertions requiring human intervention or visual confirmation
- No infinite spin — quiescence has 5-second timeout

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (`cargo test`, existing integration test pattern)
- **Automated tests**: Tests-after (integration tests in `tests/integration_reload/mod.rs`)
- **Framework**: `cargo test` (no additional framework)

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Library/Module**: Use Bash (`cargo test --test <name>`) — run, assert exit code + output
- **Build**: Use Bash (`cargo build`, `cargo clippy`, `cargo fmt --check`)
- **Quiescence**: Use Bash (test binary checks `Arc::strong_count` via test-only hook)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundations, all independent):
├── Task 1: Cargo.toml — feature gate + notify dep + test entry  [quick]
├── Task 2: error/mod.rs — 3 new error variants               [quick]
├── Task 3: Registry helpers (swap_vtable, find_slots_by_bundle) [quick]
└── Task 4: reload_plugin_v1 + reload_plugin_v2 fixture crates [quick]

Wave 2 (After Wave 1 — core runtime changes):
├── Task 5: runtime/mod.rs — bundle_manifests + reload_libraries + on_reload_cb + pub(crate) accessors [unspecified-high]
└── Task 6: reload/mod.rs — ReloadEvent + core reload_bundle() + refresh_handle() [deep]

Wave 3 (After Wave 2 — watcher + tests):
├── Task 7: reload/mod.rs (watch section) — watch_plugin_dir() + watcher thread + debounce [unspecified-high]
└── Task 8: tests/integration_reload/mod.rs — all 9 test groups (a–i) [deep]

Wave 4 (After Wave 3 — docs + final validation):
├── Task 9:  TRUST_MODEL.md hot-reload section [writing]
├── Task 10: BENCHMARKS.md reload latency row  [quick]
└── Task 11: lib.rs + cascade wiring audit     [unspecified-high]

Wave FINAL (After ALL tasks — independent review, parallel):
├── Task F1: Plan compliance audit          [oracle]
├── Task F2: Code quality review            [unspecified-high]
├── Task F3: Full integration test run      [unspecified-high]
└── Task F4: Scope fidelity check           [deep]
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|------------|--------|
| 1 | — | 5, 6, 7, 8 |
| 2 | — | 5, 6 |
| 3 | — | 6 |
| 4 | — | 8 |
| 5 | 1, 2 | 6, 7, 8 |
| 6 | 2, 3, 5 | 7, 8, 11 |
| 7 | 1, 5, 6 | 8 |
| 8 | 4, 5, 6, 7 | F1-F4 |
| 9 | 6 | F1 |
| 10 | 6 | F1 |
| 11 | 6 | F3 |
| F1-F4 | 8, 9, 10, 11 | — |

### Agent Dispatch Summary

- **Wave 1**: 4 tasks → `quick` ×4
- **Wave 2**: 2 tasks → `unspecified-high`, `deep`
- **Wave 3**: 2 tasks → `unspecified-high`, `deep`
- **Wave 4**: 3 tasks → `writing`, `quick`, `unspecified-high`
- **FINAL**: 4 tasks → `oracle`, `unspecified-high`, `unspecified-high`, `deep`

---

## TODOs

---

- [ ] 1. `crates/polyplug/Cargo.toml` — add `hot-reload` Cargo feature, `notify` optional dep, `[[test]] integration_reload` entry; workspace root `Cargo.toml` — add V1/V2 fixture members

  **What to do**:
  - `crates/polyplug/Cargo.toml`: Add `[features]` section with `hot-reload = ["dep:notify"]` and `default = []`
  - `crates/polyplug/Cargo.toml`: In `[dependencies]` add `notify = { version = "6", optional = true }`
  - `crates/polyplug/Cargo.toml`: Add new `[[test]]` block at end of file:
    ```toml
    [[test]]
    name = "integration_reload"
    path = "../../tests/integration_reload.rs"
    ```
  - Workspace root `Cargo.toml` (`/mnt/data/Projects/Utils/polyplug/Cargo.toml`): add `"tests/fixtures/reload_plugin_v1"` and `"tests/fixtures/reload_plugin_v2"` to the `members` array on line 3

  **Must NOT do**:
  - Do not add `notify` to `[workspace.dependencies]` — it is only used in the `polyplug` crate
  - Do not add `hot-reload` to `default = []` — it must be opt-in (empty default)
  - Do not change any existing `[[test]]` entries
  - Do not add `notify` as a non-optional dependency

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: TOML file edits only, no Rust code
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 5, 6, 7, 8
  - **Blocked By**: None (can start immediately)

  **References**:
  - `crates/polyplug/Cargo.toml:12-22` — existing `[dependencies]` section; notify follows the same optional pattern
  - `crates/polyplug/Cargo.toml:23-145` — existing `[[test]]` entries; match exact TOML format
  - `Cargo.toml:3` — existing `members = [...]` line in workspace root

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Compiles without hot-reload feature
    Tool: Bash
    Preconditions: Cargo.toml modified
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0, no errors
    Expected Result: Build succeeds without feature
    Evidence: .sisyphus/evidence/task-1-build-no-feature.txt

  Scenario: Compiles with hot-reload feature
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug --features hot-reload 2>&1
      2. Assert: exit code 0, no 'notify' errors
    Expected Result: Build succeeds with feature
    Evidence: .sisyphus/evidence/task-1-build-with-feature.txt
  ```

  **Evidence to Capture**:
  - [ ] task-1-build-no-feature.txt
  - [ ] task-1-build-with-feature.txt

  **Commit**: YES (groups with Tasks 2, 3, 4)
  - Message: `feat(reload): add hot-reload Cargo feature gate, notify dep, fixture crates`
  - Files: `crates/polyplug/Cargo.toml`, `Cargo.toml`
  - Pre-commit: `cargo build -p polyplug && cargo build -p polyplug --features hot-reload`

---

- [ ] 2. `crates/polyplug/src/error/mod.rs` — add `ReloadFailed`, `QuiescenceTimeout`, `WatcherFailed` variants to `RuntimeError`

  **What to do**:
  - Open `crates/polyplug/src/error/mod.rs`. After the existing `BundleNotFound` variant (currently the last variant in `RuntimeError`), add exactly these three new variants:
    ```rust
    #[error("reload failed for bundle `{bundle}`: {reason}")]
    ReloadFailed { bundle: String, reason: String },

    #[error("quiescence timeout waiting for in-flight calls to complete for bundle `{bundle}`")]
    QuiescenceTimeout { bundle: String },

    #[cfg(feature = "hot-reload")]
    #[error("file watcher error: {reason}")]
    WatcherFailed { reason: String },
    ```
  - `PolyplugError = RuntimeError` alias on line 40 automatically covers these — no change needed
  - No other changes to this file

  **Must NOT do**:
  - Do not create a new enum or a new file
  - Do not change existing variants or the `PolyplugError` alias
  - Do not remove the `#[cfg(feature = "hot-reload")]` from `WatcherFailed` — this variant is only ever returned by the file watcher which only exists under that feature

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Pure enum extension, no logic, no unsafe
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Tasks 5, 6
  - **Blocked By**: None (can start immediately)

  **References**:
  - `crates/polyplug/src/error/mod.rs:8-37` — existing `RuntimeError` enum; match exact `#[error(...)]` / `#[derive(Debug, Error)]` attribute style
  - `crates/polyplug/src/error/mod.rs:21-30` — `DependencyNotFound`/`BundleNotFound` struct variant format to copy

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: New variants compile without errors
    Tool: Bash
    Preconditions: error/mod.rs modified
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0, no warnings about unused variants
      3. Run: cargo build -p polyplug --features hot-reload 2>&1
      4. Assert: exit code 0
    Expected Result: Both compile cleanly
    Evidence: .sisyphus/evidence/task-2-build-errors.txt
  ```

  **Evidence to Capture**:
  - [ ] task-2-build-errors.txt

  **Commit**: YES (groups with Task 1, 3, 4)
  - Message: `feat(reload): add hot-reload Cargo feature gate, notify dep, fixture crates`
  - Files: `crates/polyplug/src/error/mod.rs`
  - Pre-commit: `cargo build -p polyplug --features hot-reload`

---

- [ ] 3. `crates/polyplug/src/registry/mod.rs` — add `swap_vtable()` and `find_slots_by_bundle()` methods

  **What to do**:

  Add two new `pub` methods to the `impl Registry` block in `crates/polyplug/src/registry/mod.rs`.

  **`swap_vtable()` — atomically swap one slot's vtable, bump generation:**
  ```rust
  /// Atomically swap the vtable in slot `slot_index` with `new_vtable`.
  ///
  /// Returns the old `Arc<VTableSlot>` — the caller holds it alive during quiescence
  /// then drops it after `Arc::strong_count` reaches 1 (all in-flight calls done).
  /// Bumps `slot.generation` so stale `PluginHandle`s from before reload are detected.
  ///
  /// # Errors
  /// Returns `Err(RegistryError::StaleHandle)` if `slot_index` is out of bounds
  /// or the slot has no vtable.
  pub fn swap_vtable(
      &self,
      slot_index: u32,
      new_vtable: Arc<VTableSlot>,
  ) -> Result<Arc<VTableSlot>, RegistryError> {
      let mut slots: std::sync::RwLockWriteGuard<'_, Vec<RegistrySlot>> =
          self.slots.write().unwrap_or_else(|e| e.into_inner());
      let slot_idx: usize = slot_index as usize;
      if slot_idx >= slots.len() {
          return Err(RegistryError::StaleHandle {
              index: slot_index,
              expected: 0_u32,
              found: 0_u32,
          });
      }
      let slot: &mut RegistrySlot = &mut slots[slot_idx];
      match slot.vtable {
          Some(ref arc_swap) => {
              let old_arc: Arc<VTableSlot> = arc_swap.swap(new_vtable);
              // Bump generation so stale PluginHandles from before reload are detected.
              slot.generation = slot.generation.wrapping_add(1_u32);
              Ok(old_arc)
          }
          None => Err(RegistryError::StaleHandle {
              index: slot_index,
              expected: 0_u32,
              found: slot.generation,
          }),
      }
  }
  ```

  **`find_slots_by_bundle()` — return all slot indices for a bundle:**
  ```rust
  /// Find all slot indices that were registered by `bundle_id`.
  ///
  /// Returns an empty `Vec` if the bundle has no registered slots.
  /// Used by `reload_bundle()` to locate every vtable slot to swap.
  pub fn find_slots_by_bundle(&self, bundle_id: u64) -> Vec<u32> {
      let slots: std::sync::RwLockReadGuard<'_, Vec<RegistrySlot>> =
          self.slots.read().unwrap_or_else(|e| e.into_inner());
      let mut result: Vec<u32> = Vec::new();
      for (i, slot) in slots.iter().enumerate() {
          if let Some(ref entry) = slot.entry {
              if entry.bundle_id == bundle_id {
                  result.push(i as u32);
              }
          }
      }
      result
  }
  ```

  **Add unit tests** in the `#[cfg(test)]` block at the bottom of `registry/mod.rs`:
  ```rust
  #[test]
  fn swap_vtable_returns_old_arc_and_bumps_generation() {
      let registry: Registry = Registry::new();
      // register a vtable first
      let d: PluginDescriptor = make_descriptor("swap_test", "swap.contract");
      static VTABLE_SWAP: PluginVTable = PluginVTable {
          contract_id: 0xABCD_1234_5678_EF01,
          contract_version: (1 << 16) | 0,
          function_count: 0,
          functions: MOCK_FNS.as_ptr(),
      };
      // SAFETY: VTABLE_SWAP is 'static.
      let handle: PluginHandle = unsafe {
          registry.register(d, &VTABLE_SWAP, "swap.contract".to_owned(), 99_u64)
      }.expect("register should succeed");
      let gen_before: u32 = handle.generation;
      let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(&VTABLE_SWAP as *const PluginVTable));
      let old_arc: Arc<VTableSlot> = registry.swap_vtable(handle.index, new_arc).expect("swap should succeed");
      // old_arc still valid; generation bumped
      let slots = registry.slots.read().expect("lock");
      assert_eq!(slots[handle.index as usize].generation, gen_before.wrapping_add(1), "generation must be bumped");
      drop(old_arc);
  }

  #[test]
  fn find_slots_by_bundle_returns_all_slots() {
      let registry: Registry = Registry::new();
      static VTABLE_A: PluginVTable = PluginVTable { contract_id: 0x1111_0001_0002_0003, contract_version: 0, function_count: 0, functions: MOCK_FNS.as_ptr() };
      static VTABLE_B: PluginVTable = PluginVTable { contract_id: 0x1111_0001_0002_0004, contract_version: 0, function_count: 0, functions: MOCK_FNS.as_ptr() };
      let d1: PluginDescriptor = make_descriptor("p1", "c.a");
      let d2: PluginDescriptor = make_descriptor("p2", "c.b");
      let bid: u64 = 777_u64;
      // SAFETY: static vtables.
      unsafe { registry.register(d1, &VTABLE_A, "c.a".to_owned(), bid).expect("reg a"); }
      unsafe { registry.register(d2, &VTABLE_B, "c.b".to_owned(), bid).expect("reg b"); }
      let slots: Vec<u32> = registry.find_slots_by_bundle(bid);
      assert_eq!(slots.len(), 2, "must find both slots");
  }
  ```

  **Must NOT do**:
  - Do not change `bundle_index: RwLock<HashMap<u64, u32>>` type
  - Do not add new fields to `Registry`
  - Do not modify `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()`, `find_by_bundle()`, or `find()` — reader path is immutable
  - Do not add `use` statements inside methods

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Two Rust method additions with unit tests, no unsafe required
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Task 6
  - **Blocked By**: None (can start immediately)

  **References**:
  - `crates/polyplug/src/registry/mod.rs:163-242` — `register()` method — `RwLock::write().unwrap_or_else` pattern to copy exactly
  - `crates/polyplug/src/registry/mod.rs:371-404` — `find_all_by_contract()` — linear scan + Vec push pattern
  - `crates/polyplug/src/registry/mod.rs:406-443` — `resolve_guard()` — `slot.generation` access pattern
  - `crates/polyplug/src/registry/mod.rs:18-19` — `use arc_swap::ArcSwap;` and `use std::sync::Arc;` already present
  - `crates/polyplug/src/registry/mod.rs:470-684` — existing `#[cfg(test)]` block — copy `make_descriptor`, `MOCK_FNS`, `MOCK_VTABLE` patterns for unit tests

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: swap_vtable unit test passes
    Tool: Bash
    Preconditions: Methods and tests added
    Steps:
      1. Run: cargo test -p polyplug -- registry 2>&1
      2. Assert: exit code 0, lines matching 'swap_vtable' show 'ok'
    Expected Result: New unit tests pass, all existing tests unaffected
    Evidence: .sisyphus/evidence/task-3-registry-tests.txt

  Scenario: find_slots_by_bundle unit test passes
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug -- registry 2>&1 | grep -E 'find_slots|FAILED'
      2. Assert: 'find_slots_by_bundle_returns_all_slots ... ok'
    Expected Result: Test passes
    Evidence: .sisyphus/evidence/task-3-registry-unit.txt
  ```

  **Evidence to Capture**:
  - [ ] task-3-registry-tests.txt
  - [ ] task-3-registry-unit.txt

  **Commit**: YES (groups with Tasks 1, 2, 4)
  - Message: `feat(reload): add hot-reload Cargo feature gate, notify dep, fixture crates`
  - Files: `crates/polyplug/src/registry/mod.rs`
  - Pre-commit: `cargo test -p polyplug -- registry`

---

- [ ] 4. Create reload test fixture crates `tests/fixtures/reload_plugin_v1/` and `tests/fixtures/reload_plugin_v2/`

  **What to do**:

  Create two minimal `cdylib` workspace crates following the exact pattern of `tests/fixtures/test_plugin/`. The two crates are identical except V2 returns `200_u32` from `version_check()` while V1 returns `100_u32`.

  ### File structure to create:
  ```
  tests/fixtures/reload_plugin_v1/
  ├── Cargo.toml
  └── src/
      └── lib.rs
  tests/fixtures/reload_plugin_v2/
  ├── Cargo.toml
  └── src/
      └── lib.rs
  ```

  ### `tests/fixtures/reload_plugin_v1/Cargo.toml`:
  IMPORTANT: Do NOT add a `[dependencies]` section. Fixture crates cannot depend on `polyplug`
  (circular build-script dependency). Mirror ABI types inline, exactly as `tests/fixtures/test_plugin/` does.
  ```toml
  [package]
  name       = "reload_plugin_v1"
  version    = "0.1.0"
  edition.workspace     = true
  license.workspace     = true
  rust-version.workspace = true
  publish    = false

  [lib]
  name       = "reload_plugin_v1"
  crate-type = ["cdylib"]

  [lints]
  workspace = true
  ```

  ### `tests/fixtures/reload_plugin_v1/src/lib.rs`:
  IMPORTANT: Do NOT `use polyplug::...`. Mirror ABI types inline exactly as `tests/fixtures/test_plugin/src/lib.rs` does.
  Read that file first and copy the ABI type definitions section verbatim.
  Only the plugin-specific constants and functions differ:
  - `RELOAD_TEST_CONTRACT_ID`: compute `fnv1a_64(b"reload.test@1")` — run a scratch `#[test]` in any crate calling `polyplug::abi::contract_id("reload.test", 1)` and bake the hex literal. Both V1 and V2 must use the SAME value.
  - `FnPtr` wrapper must be used for the vtable functions array (matching `test_plugin` — `functions: *const FnPtr`, not `*const *const ()`)
  ```rust
  // THIS FILE IS INTENTIONALLY MINIMAL — mirrors ABI types from polyplug without depending on it.
  // See tests/fixtures/test_plugin/src/lib.rs for the full ABI mirroring pattern to copy.

  // --- copy all mirrored ABI types from test_plugin/src/lib.rs: ---
  // StringView, AbiError, PluginVTable, PluginDescriptor, PluginHandle,
  // HostVTable, PluginRegistrar, FnPtr, and their unsafe Send/Sync impls.

  // RELOAD_TEST_CONTRACT_ID = fnv1a_64(b"reload.test@1")
  // Executor: compute correct value and fill in.
  const RELOAD_TEST_CONTRACT_ID: u64 = 0x__FILL_IN_BY_EXECUTOR__u64;
  const POLYPLUG_ABI_VERSION: u32 = 1_u32;  // copy exact value from crates/polyplug/src/abi/mod.rs

  // SAFETY: version_check is a C-compatible static function.
  unsafe extern "C" fn version_check() -> u32 { 100_u32 }

  static VTABLE_FNS: [FnPtr; 1] = [FnPtr(version_check as *const ())];

  static VTABLE: PluginVTable = PluginVTable {
      contract_id:      RELOAD_TEST_CONTRACT_ID,
      contract_version: (1_u32 << 16) | 0_u32,
      function_count:   1_u32,
      functions:        VTABLE_FNS.as_ptr(),
  };

  static DESCRIPTOR: PluginDescriptor = PluginDescriptor {
      name:          StringView::from_static(b"reload_plugin_v1"),
      contract_name: StringView::from_static(b"reload.test"),
      version_major: 1_u32,
      version_minor: 0_u32,
      version_patch: 0_u32,
  };

  #[unsafe(no_mangle)]
  pub extern "C" fn polyplug_abi_version() -> u32 { POLYPLUG_ABI_VERSION }

  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_init(registrar: *mut PluginRegistrar) -> AbiError {
      if registrar.is_null() {
          return AbiError { code: 1_u32, message: StringView::null() };
      }
      // SAFETY: registrar is a valid non-null pointer from the host runtime, outlives this call.
      unsafe {
          ((*registrar).register_plugin)(
              registrar,
              &DESCRIPTOR as *const PluginDescriptor,
              &VTABLE as *const PluginVTable,
          );
      }
      AbiError::ok()
  }
  ```

  **Manifest files**: The manifest must be named to match the `.so` file produced by the build.
  The loader calls `path.with_extension("manifest.toml")` on the `.so` path.
  Since `build.rs` copies the `.so` to `tests/fixtures/libreload_plugin_v1.so`, the manifest must be:
  `tests/fixtures/libreload_plugin_v1.manifest.toml` (NOT `reload_plugin_v1.manifest.toml`).
  Reference schema from `crates/polyplug/src/loader/manifest/mod.rs` (`ManifestData` struct fields).
  Key fields for V1:
  ```toml
  bundle_name            = "reload_plugin_v1"
  version                = "1.0"
  runtime                = "native"
  file                   = "libreload_plugin_v1.so"
  needs_reinit_on_dep_reload = false
  provides               = ["reload.test@1"]

  [function_count]
  "reload.test@1" = 1
  ```
  V2 manifest: `tests/fixtures/libreload_plugin_v2.manifest.toml` — identical but `bundle_name = "reload_plugin_v2"`, `file = "libreload_plugin_v2.so"`.
  [function_count]
  "reload.test@1" = 1
  ```
  V2 manifest: identical but `bundle_name = "reload_plugin_v2"`, `file = "libreload_plugin_v2.so"`.

  ### `tests/fixtures/depender_plugin/` (required by Task 8 test group e):
  Create a third minimal `cdylib` fixture for cascade testing. This fixture:
  - Has `needs_reinit_on_dep_reload = true` in its manifest.
  - Declares a `ByBundle` dependency on `reload_plugin_v1` in its manifest.
  - Exports `init_count: extern "C" fn() -> u32` which returns how many times `polyplug_init` was called (tracked via a `static AtomicU32`).
  - Crate structure: `tests/fixtures/depender_plugin/Cargo.toml` + `tests/fixtures/depender_plugin/src/lib.rs`.
  - Add to workspace `members` in root `Cargo.toml`.
  - Add `DEPENDER_PLUGIN_SO` env var in `build.rs` following the same pattern as V1/V2.
  - Manifest `tests/fixtures/depender_plugin.manifest.toml`: use `[[dependency]]` table format (the actual TOML schema from `loader/manifest/mod.rs` uses `RawManifestDependency` with `kind`, `contract`, `min_version`, `bundle`, `bundle_id` fields):
    ```toml
    name                        = "depender_plugin"
    bundle_name                 = "depender_plugin"
    version                     = "1.0"
    runtime                     = "native"
    file                        = "libdepender_plugin.so"
    needs_reinit_on_dep_reload  = true
    provides                    = ["depender.test@1"]

    [[dependency]]
    kind        = "bundle"
    contract    = "reload.test@1"
    min_version = "1.0"
    bundle      = "reload_plugin_v1"
    bundle_id   = 0x__FILL_IN__  # fnv1a_64(b"reload_plugin_v1") — executor must compute
    ```
    **Note**: `bundle_id` must be the result of `fnv1a_64(b"reload_plugin_v1")`. Executor: compute via a scratch `#[test]` calling `polyplug::abi::bundle_id("reload_plugin_v1")` and bake the result.
    Manifest must be named `tests/fixtures/libdepender_plugin.manifest.toml` (matching the .so filename).
  - `src/lib.rs` sketch (no `polyplug` dep — mirror ABI types like V1/V2):
    ```rust
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;
    // --- copy mirrored ABI types from test_plugin/src/lib.rs ---
    static INIT_COUNT: AtomicU32 = AtomicU32::new(0_u32);
    // SAFETY: init_count_fn is a C-compatible function with no side effects.
    unsafe extern "C" fn init_count_fn() -> u32 { INIT_COUNT.load(Ordering::SeqCst) }
    // ... PluginVTable, DESCRIPTOR, polyplug_abi_version, polyplug_init as normal ...
    // In polyplug_init: INIT_COUNT.fetch_add(1_u32, Ordering::SeqCst);
    ```
  **Build.rs**: In `crates/polyplug/build.rs`, add env exports for `RELOAD_PLUGIN_V1_SO` and `RELOAD_PLUGIN_V2_SO` following the exact pattern of `TEST_PLUGIN_SO` and `MEMORY_PLUGIN_SO`. Read `crates/polyplug/build.rs` to understand the exact pattern.

  ### `tests/fixtures/reload_plugin_v2/src/lib.rs`:
  Identical to V1 except:
  - Package name: `reload_plugin_v2`
  - `version_check()` returns `200_u32`
  - `DESCRIPTOR.name` = `StringView::from_static(b"reload_plugin_v2")`
  - Manifest: `tests/fixtures/libreload_plugin_v2.manifest.toml`
  Identical to V1 except:
  - Package name: `reload_plugin_v2`
  - `version_check()` returns `200_u32`
  - `DESCRIPTOR.name` = `StringView::from_static(b"reload_plugin_v2")`

  **Must NOT do**:
  - Do NOT add `polyplug` as a dependency in any fixture crate's `Cargo.toml` — it creates a circular build-script dependency and will deadlock. Mirror ABI types inline.
  - Do not use `.unwrap()` anywhere
  - Do not omit `// SAFETY:` on unsafe blocks
  - Do not create `tests/fixtures/reload_plugin_v1.rs` as a bare module root — use `src/lib.rs`
  - Do not add any logic beyond `version_check()` returning a constant — keep fixtures minimal
  - Do NOT name manifests after the crate (e.g. `reload_plugin_v1.manifest.toml`) — name them after the .so (`libreload_plugin_v1.manifest.toml`)
  - Do not use `.unwrap()` anywhere
  - Do not omit `// SAFETY:` on unsafe blocks
  - Do not create `tests/fixtures/reload_plugin_v1.rs` as a bare module root — use `src/lib.rs`
  - Do not add any logic beyond `version_check()` returning a constant — keep fixtures minimal

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Boilerplate fixture crates following established pattern
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 8
  - **Blocked By**: None (can start immediately)

  **References**:
  - `tests/fixtures/test_plugin/src/lib.rs` — exact plugin boilerplate to copy and adapt
  - `tests/fixtures/test_plugin/Cargo.toml` — exact Cargo.toml structure
  - `crates/polyplug/src/loader/manifest/mod.rs` — `ManifestData` struct — schema reference for manifest fields (NOT `test_plugin.manifest.toml` which is a Lua manifest and unsuitable as a native template)
  - `crates/polyplug/build.rs` — env var export pattern for `.so` paths
  - `crates/polyplug/src/abi/mod.rs` — `contract_id()` and `bundle_id()` functions to compute correct constant values

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: V1 fixture builds successfully
    Tool: Bash
    Preconditions: reload_plugin_v1 added to workspace
    Steps:
      1. Run: cargo build -p reload_plugin_v1 2>&1
      2. Assert: exit code 0
    Expected Result: libreload_plugin_v1.so produced
    Evidence: .sisyphus/evidence/task-4-v1-build.txt

  Scenario: V2 fixture builds successfully
    Tool: Bash
    Steps:
      1. Run: cargo build -p reload_plugin_v2 2>&1
      2. Assert: exit code 0
    Expected Result: libreload_plugin_v2.so produced
    Evidence: .sisyphus/evidence/task-4-v2-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-4-v1-build.txt
  - [ ] task-4-v2-build.txt

  **Commit**: YES (groups with Tasks 1, 2, 3)
  - Message: `feat(reload): add hot-reload Cargo feature gate, notify dep, fixture crates`
  - Files: `tests/fixtures/reload_plugin_v1/`, `tests/fixtures/reload_plugin_v2/`, `crates/polyplug/build.rs`, `Cargo.toml`
  - Pre-commit: `cargo build -p reload_plugin_v1 && cargo build -p reload_plugin_v2`

---

- [ ] 5. `crates/polyplug/src/runtime/mod.rs` — add `bundle_manifests`, `reload_libraries`, `on_reload_cb` to `Runtime`; add `on_reload()` to `RuntimeBuilder`; add `pub(crate)` accessors; store manifests during `build()`

  **What to do**:

  **5a. New type alias at top of `runtime/mod.rs` (after existing type aliases):**
  ```rust
  type ReloadCb = Arc<dyn Fn(crate::reload::ReloadEvent) + Send + Sync>;
  ```

  **5b. Add to `Runtime` struct** (after `loaders` field):
  ```rust
  /// ManifestData for all loaded bundles, keyed by bundle_name.
  /// Used by reload_bundle() for cascade detection and by watch_plugin_dir().
  pub(crate) bundle_manifests: std::sync::Mutex<std::collections::HashMap<String, crate::loader::manifest::ManifestData>>,
  /// Library handles for reloaded native bundles — these ARE droppable (unlike loaded_libraries).
  /// Keyed by bundle_id. On each reload the old handle is removed and dropped after quiescence.
  pub(crate) reload_libraries: std::sync::Mutex<std::collections::HashMap<u64, libloading::Library>>,
  /// Optional callback fired after vtable swap, before dlclose.
  pub(crate) on_reload_cb: Option<ReloadCb>,
  ```

  **5c. Add `on_reload()` builder method to `RuntimeBuilder`:**
  - Add field to `RuntimeBuilder`: `on_reload_cb: Option<ReloadCb>`
  - Initialize to `None` in `RuntimeBuilder::new()`
  - Add the builder method:
  ```rust
  /// Register a callback fired after each successful vtable swap, before dlclose.
  ///
  /// The callback receives a `ReloadEvent` describing the reloaded bundle.
  pub fn on_reload(mut self, cb: impl Fn(crate::reload::ReloadEvent) + Send + Sync + 'static) -> RuntimeBuilder {
      self.on_reload_cb = Some(Arc::new(cb));
      self
  }
  ```

  **5d. In `RuntimeBuilder::build()`**, store manifests and initialize new fields:
  - After the `discovered` vec is populated but before the loading loop, snapshot manifests into `bundle_manifests`:
  ```rust
  let mut manifests_map: std::collections::HashMap<String, crate::loader::manifest::ManifestData> = std::collections::HashMap::new();
  for (_, manifest) in &discovered {
      manifests_map.insert(manifest.bundle_name.clone(), manifest.clone());
  }
  ```
  - Add new fields when constructing `Runtime` at the end of `build()`:
  ```rust
  Ok(Runtime {
      registry,
      _bundles: bundles,
      host_vtable,
      loaders: loader_map,
      _extensions: self.extensions,
      bundle_manifests: std::sync::Mutex::new(manifests_map),
      reload_libraries: std::sync::Mutex::new(std::collections::HashMap::new()),
      on_reload_cb: self.on_reload_cb,
  })
  ```

  **5e. Add `pub(crate)` accessor methods to `Runtime`:**
  ```rust
  pub(crate) fn registry(&self) -> &Arc<Registry> {
      &self.registry
  }

  pub(crate) fn host_vtable_ref(&self) -> &'static HostVTable {
      self.host_vtable
  }

  pub(crate) fn loaders(&self) -> &std::collections::HashMap<String, Box<dyn BundleLoader>> {
      &self.loaders
  }
  ```

  **5f. Add `pub mod reload;` to `crates/polyplug/src/lib.rs` AND create a stub `reload/mod.rs`.**
  - Add `pub mod reload;` after the existing module declarations.
  - ALSO create a STUB `crates/polyplug/src/reload/mod.rs` with the minimum content needed to compile the `ReloadCb` type alias in `runtime/mod.rs`:
    ```rust
    //! Reload — stub, full implementation in Task 6.
    /// Event delivered to the on_reload callback after each successful vtable swap.
    #[derive(Debug, Clone)]
    pub struct ReloadEvent {
        pub bundle_name: String,
        pub bundle_path: String,
        pub old_version: String,
        pub new_version: String,
        pub affected_contract_ids: Vec<u64>,
    }
    ```
  - Task 6 will replace this stub with the full implementation. The stub must define `ReloadEvent` exactly as shown above (same fields Task 6 uses) so Task 6 can expand it without merge conflicts.
  Add `pub mod reload;` after the existing module declarations. The `reload/mod.rs` file will be created in Task 6.

  **Must NOT do**:
  - Do not change the existing `_bundles` or `_extensions` fields
  - Do not change `GLOBAL_REGISTRY` or `GLOBAL_EXTENSION_MAP` — they remain `OnceLock`
  - Do not add `on_reload_cb` to `RuntimeBuilder` as an `OnceLock` — it must be `Option<ReloadCb>` (can change between builds in tests)
  - Do not use `.unwrap()` anywhere — use `unwrap_or_else(|e| e.into_inner())` for mutex guards
  - Do not add `use` inside functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multi-site modification to a complex file, must maintain existing logic, add fields carefully
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (sequential with Task 6)
  - **Blocks**: Tasks 6, 7, 8
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug/src/runtime/mod.rs:97-107` — `Runtime` struct — add new fields after `loaders`
  - `crates/polyplug/src/runtime/mod.rs:130-136` — `RuntimeBuilder` struct — add `on_reload_cb` field
  - `crates/polyplug/src/runtime/mod.rs:140-191` — `impl RuntimeBuilder` — add `on_reload()` method here
  - `crates/polyplug/src/runtime/mod.rs:198-346` — `build()` method — add manifest snapshotting and new fields in `Ok(Runtime { ... })`
  - `crates/polyplug/src/runtime/mod.rs:54` — `type WarningCb = Box<dyn Fn(&str) + Send + Sync>;` — follow this type alias pattern for `ReloadCb`
  - `crates/polyplug/src/lib.rs:7-15` — `pub mod ...` declarations — add `pub mod reload;` here
  - `crates/polyplug/src/error/mod.rs:40` — `PolyplugError = RuntimeError` — `ReloadFailed` etc. are already available

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: Runtime builds with new fields
    Tool: Bash
    Preconditions: runtime/mod.rs and lib.rs modified; reload/mod.rs stub created (at minimum pub mod declaration)
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0, no 'unknown field', 'missing field' errors
    Expected Result: Runtime struct compiles with new fields
    Evidence: .sisyphus/evidence/task-5-build.txt

  Scenario: on_reload builder method wires through correctly
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug -- runtime::tests 2>&1
      2. Assert: exit code 0
    Expected Result: Existing runtime unit tests still pass
    Evidence: .sisyphus/evidence/task-5-unit-tests.txt
  ```

  **Evidence to Capture**:
  - [ ] task-5-build.txt
  - [ ] task-5-unit-tests.txt

  **Commit**: YES (separate)
  - Message: `feat(reload): add bundle_manifests, reload_libraries, on_reload builder to Runtime`
  - Files: `crates/polyplug/src/runtime/mod.rs`, `crates/polyplug/src/lib.rs`
  - Pre-commit: `cargo build -p polyplug && cargo test -p polyplug -- runtime`

---

- [ ] 6. `crates/polyplug/src/reload/mod.rs` — `ReloadEvent`, `reload_bundle()`, `refresh_handle()`

  **What to do**:

  Create the file `crates/polyplug/src/reload/mod.rs`. This is the core of the epic.

  ### File header and imports:
  ```rust
  //! Reload — hot-reload writer path for polyplug.
  //!
  //! This module implements the 5-step reload path:
  //!  1. Load new bundle via correct loader, capture new vtable ptr.
  //!  2. Atomically swap arc-swap slot: slot.vtable.store(Arc::new(VTableSlot(new_ptr))).
  //!  3. Hold old_arc. Spin (with 5-second timeout) until strong_count == 1.
  //!  4. Drop old_arc, drop old library handle (calls dlclose).
  //!  5. Walk dependency graph for cascade re-init.
  //!
  //! The on_reload callback fires AFTER step 2 (swap) and BEFORE step 4 (dlclose).
  //! All new calls see the new vtable from the moment step 2 completes.
  
  use std::collections::HashMap;
  use std::path::Path;
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::time::Duration;
  use std::time::Instant;
  
  use crate::abi::bundle_id as compute_bundle_id;
  use crate::error::PolyplugError;
  use crate::error::RuntimeError;
  use crate::loader::manifest::ManifestData;
  use crate::registry::VTableSlot;
  use crate::runtime::Runtime;
  ```

  ### `ReloadEvent` struct:
  ```rust
  /// Event delivered to the on_reload callback after each successful vtable swap.
  ///
  /// Fires AFTER the new vtable is visible to all callers and BEFORE dlclose of
  /// the old library. All new calls use the new vtable when the callback fires.
  #[derive(Debug, Clone)]
  pub struct ReloadEvent {
      pub bundle_name: String,
      pub bundle_path: String,
      pub old_version: String,
      pub new_version: String,
      /// Contract IDs of all vtable slots swapped during this reload.
      pub affected_contract_ids: Vec<u64>,
  }
  ```

  ### Constants:
  ```rust
  /// Maximum time to wait for in-flight callers to release their vtable guards.
  const QUIESCENCE_TIMEOUT: Duration = Duration::from_secs(5);

  /// Maximum cascade depth to prevent runaway recursive re-initialization.
  const MAX_CASCADE_DEPTH: usize = 16_usize;
  ```

  ### `reload_bundle()` — add as a method on `Runtime` via an `impl Runtime` block in this file:
  ```rust
  impl Runtime {
      /// Reload a native plugin bundle at `path`.
      ///
      /// Implements the 5-step reload path: load new → swap arc-swap → quiescence → dlclose → cascade.
      /// Returns `Err(QuiescenceTimeout)` if in-flight callers do not release their guards
      /// within 5 seconds.
      /// Returns `Err(ReloadFailed)` for non-native bundle runtimes (non-native reload is future work).
      ///
      /// **Thread safety**: Only one reload at a time per bundle is safe. Calling reload_bundle()
      /// concurrently on the same bundle from multiple threads is NOT supported and produces
      /// undefined behaviour.
      pub fn reload_bundle(&self, path: &Path) -> Result<(), PolyplugError> {
          crate::reload::reload_bundle_impl(self, path, 0_usize)
      }
  
      /// Refresh a plugin handle after a reload.
      ///
      /// App developers who cache `PluginHandle` or `*const PluginVTable` directly
      /// must call this after a reload event to get a fresh guard. Generated host
      /// callers do this automatically via resolve_plugin on every use.
      pub fn refresh_handle(
          &self,
          contract_id: u64,
          min_version: u32,
      ) -> Result<crate::abi::PluginHandle, crate::error::RegistryError> {
          self.registry().find_by_contract(contract_id, min_version)
      }
  }
  ```

  ### `reload_bundle_impl()` — the internal implementation (free function, `pub(crate)`):
  Full pseudocode — executor must translate to correct Rust with explicit types, no `.unwrap()`, all SAFETY comments:

  ```rust
  pub(crate) fn reload_bundle_impl(
      runtime: &Runtime,
      path: &Path,
      cascade_depth: usize,
  ) -> Result<(), PolyplugError> {
  ```

  **Step 1: Validate path and parse manifest:**
  ```rust
  if cascade_depth >= MAX_CASCADE_DEPTH {
      return Err(PolyplugError::ReloadFailed {
          bundle: path.display().to_string(),
          reason: format!("cascade depth limit ({MAX_CASCADE_DEPTH}) exceeded"),
      });
  }
  let manifest: ManifestData = crate::loader::parse_manifest(path)
      .map_err(|e: crate::error::LoaderError| PolyplugError::Loader(e))?;
  // Only native bundles are supported in this epic.
  if manifest.runtime != "native" {
      crate::runtime::emit_warning(&format!(
          "reload_bundle: non-native runtime '{}' for bundle '{}' is not supported in this version; skipping",
          manifest.runtime, manifest.bundle_name,
      ));
      return Err(PolyplugError::ReloadFailed {
          bundle: manifest.bundle_name.clone(),
          reason: format!("non-native runtime '{}' is not supported", manifest.runtime),
      });
  }
  ```

  **Step 2: Compute bundle_id and find all existing slots:**
  ```rust
  let bundle_id: u64 = compute_bundle_id(&manifest.bundle_name);
  let slot_indices: Vec<u32> = runtime.registry().find_slots_by_bundle(bundle_id);
  if slot_indices.is_empty() {
      return Err(PolyplugError::ReloadFailed {
          bundle: manifest.bundle_name.clone(),
          reason: "bundle is not loaded; cannot reload an unloaded bundle".to_owned(),
      });
  }
  ```

  **Step 3: Load new bundle, capture new library handle and vtable ptrs:**
  This is the tricky part. The normal `load_bundle()` path pushes the library into `registry.loaded_libraries` (never-drop). For reload, we need to intercept it differently.

  **Approach**: Use a modified load sequence that:
  - Opens the library with `libloading::Library::new(path)` directly (same as `load_bundle` does)
  - Resolves `polyplug_abi_version` and verifies it
  - Resolves `polyplug_init` fn ptr
  - Creates a TEMP `PluginRegistrar` that captures the registered vtable ptrs into a `Vec<*const PluginVTable>` instead of writing to `registry` — use a separate thread-local or a wrapper
  - Calls `polyplug_init` with the capturing registrar
  - The new vtable ptrs are now in the capture buffer
  - The new library handle is held locally — NOT pushed to `registry.loaded_libraries`

  **Implementation detail for capturing registrar**:
  Use a thread-local `Vec` to capture vtable ptrs during the reload init call. Define a new `pub(crate)` `unsafe extern "C"` callback:
  ```rust
  thread_local! {
      static RELOAD_CAPTURED_VTABLES: core::cell::RefCell<Vec<*const crate::abi::PluginVTable>> =
          const { core::cell::RefCell::new(Vec::new()) };
  }

  // SAFETY: Called by plugin init during reload. Captures vtable ptrs for reload_bundle_impl.
  pub(crate) unsafe extern "C" fn reload_registrar_callback(
      _registrar: *mut crate::abi::PluginRegistrar,
      _descriptor: *const crate::abi::PluginDescriptor,
      vtable: *const crate::abi::PluginVTable,
  ) -> crate::abi::AbiError {
      if !vtable.is_null() {
          RELOAD_CAPTURED_VTABLES.with(|v| v.borrow_mut().push(vtable));
      }
      crate::abi::AbiError::ok()
  }
  ```

  Then in reload_bundle_impl, before calling init:
  ```rust
  RELOAD_CAPTURED_VTABLES.with(|v| v.borrow_mut().clear());
  // ... open library, resolve init_fn_ptr ...
  let mut reload_registrar: crate::abi::PluginRegistrar = crate::abi::PluginRegistrar {
      register_plugin: reload_registrar_callback,
      host: runtime.host_vtable_ref() as *const crate::abi::HostVTable,
  };
  // SAFETY: init_fn_ptr is resolved from the new library and valid for this call.
  let init_result: crate::abi::AbiError = unsafe { init_fn_ptr(&mut reload_registrar) };
  if init_result.code != crate::abi::ABI_OK {
      return Err(PolyplugError::ReloadFailed {
          bundle: manifest.bundle_name.clone(),
          reason: format!("new bundle init() failed with code {}", init_result.code),
      });
  }
  let captured_vtables: Vec<*const crate::abi::PluginVTable> =
      RELOAD_CAPTURED_VTABLES.with(|v| v.borrow().clone());
  ```

  **Step 4: Build new Arc<VTableSlot>s — one per captured vtable:**
  ```rust
  // Build new arc vtables. One per slot_index (matched by contract_id ordering).
  // Since a bundle may provide multiple contracts, we match new vtable to old slot by contract_id.
  // Build a map: contract_id -> new *const PluginVTable
  let mut new_vtable_map: HashMap<u64, *const crate::abi::PluginVTable> = HashMap::new();
  for &vt_ptr in &captured_vtables {
      // SAFETY: vt_ptr was just returned by the new init(), lives in the new library.
      let contract_id: u64 = unsafe { (*vt_ptr).contract_id };
      new_vtable_map.insert(contract_id, vt_ptr);
  }
  ```

  **Step 5: Atomically swap each slot:**
  ```rust
  let mut old_arcs: Vec<Arc<VTableSlot>> = Vec::new();
  for slot_idx in &slot_indices {
      // Get contract_id for this slot from registry
      // Use find_slots_by_bundle result — we know slot_idx, we need contract_id.
      // Approach: read contract_id from the current arc_swap in that slot.
      let current_vtable_ptr: *const crate::abi::PluginVTable = {
          let guard = runtime.registry().slots
              .read().unwrap_or_else(|e| e.into_inner());
          let slot = &guard[*slot_idx as usize];
          match slot.vtable {
              Some(ref arc_swap) => arc_swap.load().0,
              None => continue,
          }
      };
      // SAFETY: current_vtable_ptr is 'static, valid from prior load.
      let contract_id: u64 = unsafe { (*current_vtable_ptr).contract_id };
      let new_vt_ptr: *const crate::abi::PluginVTable = match new_vtable_map.get(&contract_id) {
          Some(&ptr) => ptr,
          None => continue, // bundle dropped this contract in V2 — leave old slot intact
      };
      let new_arc: Arc<VTableSlot> = Arc::new(VTableSlot(new_vt_ptr));
      let old_arc: Arc<VTableSlot> = runtime
          .registry()
          .swap_vtable(*slot_idx, new_arc)
          .map_err(|e: crate::error::RegistryError| PolyplugError::Registry(e))?;
      old_arcs.push(old_arc);
  }
  ```

  **Note on `registry().slots` access**: `slots` is `pub(crate)` on `RegistrySlot` but `slots` field itself is private `RwLock<Vec<RegistrySlot>>`. Add `pub(crate) fn slots_read(&self) -> std::sync::RwLockReadGuard<'_, Vec<crate::registry::RegistrySlot>>` method to `Registry`, OR expose the contract_id differently. **Simplest approach**: Add a helper `pub(crate) fn get_slot_contract_id(&self, slot_index: u32) -> Option<u64>` to `Registry` that reads the arc_swap and returns the contract_id.

  **Step 6: Fire on_reload callback (after swap, before dlclose):**
  ```rust
  let old_version: String = {
      let manifests = runtime.bundle_manifests.lock().unwrap_or_else(|e| e.into_inner());
      manifests.get(&manifest.bundle_name).map(|m| m.version.clone()).unwrap_or_default()
  };
  let event: ReloadEvent = ReloadEvent {
      bundle_name: manifest.bundle_name.clone(),
      bundle_path: path.display().to_string(),
      old_version,
      new_version: manifest.version.clone(),
      affected_contract_ids: new_vtable_map.keys().copied().collect(),
  };
  if let Some(ref cb) = runtime.on_reload_cb {
      cb(event);
  }
  ```

  **Step 7: Wait for quiescence:**
  ```rust
  let quiescence_start: Instant = Instant::now();
  for old_arc in &old_arcs {
      loop {
          // SAFETY: ArcSwap::swap() used SeqCst fence internally.
          // After swap, new load() calls return the new Arc. The strong_count of
          // old_arc will reach 1 (only our reference) once all in-flight guards
          // that loaded the old Arc have been dropped. This is sound because:
          // 1. ArcSwap guarantees no new references to old_arc after swap.
          // 2. Arc::strong_count uses Acquire ordering — safe for polling.
          if Arc::strong_count(old_arc) == 1_usize {
              break;
          }
          if quiescence_start.elapsed() > QUIESCENCE_TIMEOUT {
              return Err(PolyplugError::QuiescenceTimeout {
                  bundle: manifest.bundle_name.clone(),
              });
          }
          // Yield the CPU briefly to avoid burning a core for up to 5 seconds.
          // A 1ms sleep is acceptable: reload is an infrequent writer-path operation.
          std::thread::sleep(std::time::Duration::from_millis(1));
          std::hint::spin_loop();
      }
  }
  ```

  **Step 8: dlclose — drop old library handle:**
  ```rust
  // Drop old_arcs first (decrements Arc strong_count to 0, freeing VTableSlot).
  drop(old_arcs);
  // Remove old library handle from reload_libraries and drop it (calls dlclose).
  let old_library: Option<libloading::Library> = runtime
      .reload_libraries
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .remove(&bundle_id);
  drop(old_library); // dlclose fires here if Some
  ```

  **Step 9: Insert new library handle and update manifests:**
  ```rust
  runtime
      .reload_libraries
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .insert(bundle_id, new_library);
  runtime
      .bundle_manifests
      .lock()
      .unwrap_or_else(|e| e.into_inner())
      .insert(manifest.bundle_name.clone(), manifest.clone());
  ```

  **Step 10: Cascade reload for dependents with `needs_reinit_on_dep_reload = true`:**
  ```rust
  let dependents: Vec<(String, PathBuf)> = {
      let manifests = runtime.bundle_manifests.lock().unwrap_or_else(|e| e.into_inner());
      find_cascade_targets(&manifests, &manifest.bundle_name)
  };
  for (dep_name, dep_path) in dependents {
      crate::runtime::emit_warning(&format!(
          "reload cascade: re-initializing '{}' (needs_reinit_on_dep_reload = true)",
          dep_name
      ));
      reload_bundle_impl(runtime, &dep_path, cascade_depth + 1_usize)?;
  }
  Ok(())
  }
  ```

  ### `find_cascade_targets()` helper:
  ```rust
  /// Find bundles that need re-initialization after `reloaded_bundle` was reloaded.
  ///
  /// Returns `(bundle_name, bundle_path)` pairs in topological order (dependencies first).
  /// Only includes bundles where `needs_reinit_on_dep_reload = true` AND that have a
  /// dependency on `reloaded_bundle_name`.
  pub(crate) fn find_cascade_targets(
      manifests: &HashMap<String, ManifestData>,
      reloaded_bundle_name: &str,
  ) -> Vec<(String, PathBuf)> {
      let mut targets: Vec<(String, PathBuf)> = Vec::new();
      for (name, manifest) in manifests {
          if !manifest.needs_reinit_on_dep_reload {
              continue;
          }
          // Check if this bundle depends on reloaded_bundle_name
          let depends: bool = manifest.resolved_dependencies().iter().any(|dep| {
              match dep {
                  crate::loader::manifest::ManifestDependency::ByBundle { bundle, .. } => {
                      bundle == reloaded_bundle_name
                  }
                  crate::loader::manifest::ManifestDependency::ByContract { .. } => false,
              }
          });
          if depends {
              // Reconstruct path from manifest.file and bundle_name
              // Note: ManifestData does not store the path; use the bundle's .so name from manifest.file.
              // For cascade, we need the path. Store it as part of ManifestData OR use a convention:
              // the path is stored in a new field added to ManifestData: `pub path: PathBuf`.
              // Add `pub path: PathBuf` (serde skip) to ManifestData and populate it during discovery.
              targets.push((name.clone(), PathBuf::from(&manifest.file)));
          }
      }
      targets
  }
  ```

  **Executor note on cascade path**: `ManifestData` does not currently store the full path to the bundle file. Add `#[serde(skip)] pub path: PathBuf` to `ManifestData` in `loader/manifest/mod.rs`, following the existing `#[serde(skip)] pub bundle_id: u64` pattern. Then populate it at ALL 4 of the following call sites:
  1. `loader/mod.rs:parse_manifest()` — set `manifest.path = manifest_toml_path.to_path_buf()` after deserialization
  2. `loader/mod.rs:load_bundle()` — already calls `parse_manifest()`; set `manifest.path` on the returned value before returning
  3. `runtime/mod.rs:load_bundle_with()` — calls `parse_manifest()` or `load_bundle()`; ensure `path` is propagated
  4. `runtime/mod.rs:build()` — in the `manifests_map` construction loop, ensure each `ManifestData` has `path` set from the discovery path
  The executor must verify all 4 sites in Task 11 (audit task).

  ### Add `get_slot_contract_id()` to Registry (needed by reload_bundle_impl):
  ```rust
  /// Get the contract_id for the vtable currently stored in `slot_index`.
  /// Returns None if the slot is empty or has no vtable.
  pub(crate) fn get_slot_contract_id(&self, slot_index: u32) -> Option<u64> {
      let slots: std::sync::RwLockReadGuard<'_, Vec<crate::registry::RegistrySlot>> =
          self.slots.read().unwrap_or_else(|e| e.into_inner());
      let slot: &crate::registry::RegistrySlot = slots.get(slot_index as usize)?;
      let arc_swap: &arc_swap::ArcSwap<VTableSlot> = slot.vtable.as_ref()?;
      let guard: arc_swap::Guard<Arc<VTableSlot>> = arc_swap.load();
      // SAFETY: VTableSlot.0 is a valid 'static PluginVTable written at registration.
      Some(unsafe { (*guard.0).contract_id })
  }
  ```
  Add this to `impl Registry` in `crates/polyplug/src/registry/mod.rs`.

  **Must NOT do**:
  - Do not use `.unwrap()` anywhere
  - Do not add new code to `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()`
  - Do not push `new_library` to `registry.loaded_libraries` — it must go to `runtime.reload_libraries`
  - Do not call `INIT_BUNDLE_ID` thread-local manually — for reload init path, `INIT_BUNDLE_ID` does NOT need to be set because we're not declaring deps (the bundle was already loaded; deps were declared on first load). The reload registrar captures vtables only.
  - Do not add bare `filename.rs` module root — use `reload/mod.rs`

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Core reload logic, multi-step unsafe Rust with strict correctness requirements, complex state management across Registry + Runtime + thread-locals
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 5)
  - **Parallel Group**: Wave 2 (sequential after Task 5)
  - **Blocks**: Tasks 7, 8, 11
  - **Blocked By**: Tasks 2, 3, 5

  **References**:
  - `crates/polyplug/src/loader/mod.rs:237-390` — `load_bundle()` — the exact native load pattern to replicate/adapt for the reload init path (ABI version check, symbol resolution, init call)
  - `crates/polyplug/src/loader/mod.rs:425-457` — `registrar_callback()` — copy pattern for `reload_registrar_callback()`
  - `crates/polyplug/src/registry/mod.rs:64-71` — `RegistrySlot` struct — `vtable: Option<ArcSwap<VTableSlot>>`
  - `crates/polyplug/src/registry/mod.rs:27-34` — `VTableSlot` and its `Send+Sync` impls
  - `crates/polyplug/src/loader/manifest/mod.rs:79-113` — `ManifestData` struct — add `pub path: PathBuf` with `#[serde(skip)]` following `bundle_id` pattern on line 93
  - `crates/polyplug/src/abi/mod.rs` — `AbiError::ok()`, `ABI_OK`, `PluginRegistrar` struct
  - `crates/polyplug/src/runtime/mod.rs:64` — `emit_warning()` function

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: reload_bundle module compiles (no tests yet — test in Task 8)
    Tool: Bash
    Preconditions: reload/mod.rs created, Task 5 complete
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: cargo build -p polyplug --features hot-reload 2>&1
      4. Assert: exit code 0
    Expected Result: Module compiles with and without hot-reload feature
    Evidence: .sisyphus/evidence/task-6-build.txt

  Scenario: clippy passes on reload module
    Tool: Bash
    Steps:
      1. Run: cargo clippy -p polyplug --features hot-reload -- -D warnings 2>&1
      2. Assert: exit code 0, zero warnings
    Expected Result: Zero warnings
    Evidence: .sisyphus/evidence/task-6-clippy.txt
  ```

  **Evidence to Capture**:
  - [ ] task-6-build.txt
  - [ ] task-6-clippy.txt

  **Commit**: YES (separate)
  - Message: `feat(reload): implement core reload_bundle(), refresh_handle(), ReloadEvent, cascade`
  - Files: `crates/polyplug/src/reload/mod.rs`, `crates/polyplug/src/loader/manifest/mod.rs`, `crates/polyplug/src/registry/mod.rs`
  - Pre-commit: `cargo build -p polyplug --features hot-reload && cargo clippy -p polyplug --features hot-reload -- -D warnings`

- [ ] 7. `watch_plugin_dir()` — file watcher background thread (hot-reload feature gate)

  **What to do**:
  - All code in this task is behind `#[cfg(feature = "hot-reload")]`. Do NOT add any watcher logic outside the feature gate.
  - Add a `watcher_thread: Option<std::thread::JoinHandle<()>>` field to the `Runtime` struct (also gated). Add a `watcher_stop: Option<Arc<std::sync::atomic::AtomicBool>>` alongside it.
  - Implement `Runtime::watch_plugin_dir(&self, dir: &Path) -> Result<(), PolyplugError>` behind `#[cfg(feature = "hot-reload")]`:
    1. Resolve `dir` to an absolute canonical path: `let canonical_dir: PathBuf = dir.canonicalize().map_err(|e| PolyplugError::WatcherFailed { reason: e.to_string() })?;`
    2. Create a `notify::recommended_watcher()` using the callback described below.
    3. Call `watcher.watch(&canonical_dir, notify::RecursiveMode::NonRecursive)` — non-recursive, one level only.
    4. Spawn a background thread via `std::thread::spawn`. The thread owns the watcher (keeping it alive). The thread loops on `stop_flag.load(Ordering::Relaxed)` with `std::thread::sleep(Duration::from_millis(10))` between polls.
    5. Store the `JoinHandle` in `self.watcher_thread` and the `stop_flag` `Arc` in `self.watcher_stop`.
  - The watcher callback signature (passed to `recommended_watcher`):
    ```rust
    move |res: notify::Result<notify::Event>| {
        let event: notify::Event = match res { Ok(e) => e, Err(_) => return };
        if !matches!(event.kind, notify::EventKind::Modify(_) | notify::EventKind::Create(_)) { return; }
        for path in &event.paths {
            let ext: &str = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            if !matches!(ext, "so" | "dll" | "dylib") { continue; }
            // Debounce: only fire if > 100ms since last event for this path
            let mut debounce_map = debounce.lock().unwrap_or_else(|e| e.into_inner());
            let now: std::time::Instant = std::time::Instant::now();
            let last: std::time::Instant = *debounce_map.get(path).unwrap_or(&std::time::Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or(now));
            if now.duration_since(last) < Duration::from_millis(100) { continue; }
            debounce_map.insert(path.clone(), now);
            drop(debounce_map);
            // Only reload if path matches a known bundle
            let bundle_path: String = path.to_string_lossy().into_owned();
            let rt_clone = runtime_weak.upgrade();
            if let Some(rt) = rt_clone {
                match crate::reload::reload_bundle_impl(&rt, std::path::Path::new(&bundle_path), 0_usize) {
                    Ok(_) => {},
                    Err(e) => { tracing::warn!("hot-reload: auto-reload failed for {bundle_path}: {e}"); }
                }
            }
        }
    }
    ```
  - `debounce` is an `Arc<Mutex<HashMap<PathBuf, std::time::Instant>>>` captured by the closure.
  - `runtime_weak` is a `std::sync::Weak<Runtime>` (requires `Runtime` to be in an `Arc`).
    **Resolved design**: `watch_plugin_dir()` takes `self_arc: Arc<Self>` as the first argument (instead of `&self`) when called from `#[cfg(feature = "hot-reload")]` context. `build()` return type stays `Result<Runtime, RuntimeError>` — the caller wraps in `Arc<Runtime>` before calling `watch_plugin_dir`. Do NOT add `self_weak` as a stored field to `Runtime`. Do NOT change `build()` return type. Instead:
    ```rust
    #[cfg(feature = "hot-reload")]
    impl Runtime {
        pub fn watch_plugin_dir(self_arc: Arc<Runtime>, dir: &Path) -> Result<(), PolyplugError> {
            // self_arc is the Arc wrapping this Runtime; downgrade for watcher closure
            let runtime_weak: std::sync::Weak<Runtime> = Arc::downgrade(&self_arc);
            // ... rest of watcher setup ...
            // Store JoinHandle and stop_flag in self_arc's fields (interior mutability via Mutex):
            // watcher_thread: Mutex<Option<JoinHandle<()>>>
            // watcher_stop:   Mutex<Option<Arc<AtomicBool>>>
            Ok(())
        }
    }
    ```
    Change field types in `Runtime` (feature-gated) to `Mutex<Option<...>>` to allow mutation via `&self`:
    ```rust
    #[cfg(feature = "hot-reload")]
    watcher_thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    #[cfg(feature = "hot-reload")]
    watcher_stop: std::sync::Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    ```
  - Implement `Drop` for `Runtime` (cannot be feature-gated as a separate `impl Drop` — there can only be one). Add a SINGLE unconditional `impl Drop for Runtime` block with the cleanup code conditionally compiled inside it:
    ```rust
    impl Drop for Runtime {
        fn drop(&mut self) {
            #[cfg(feature = "hot-reload")]
            {
                if let Ok(mut guard) = self.watcher_stop.lock() {
                    if let Some(flag) = guard.take() {
                        flag.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                if let Ok(mut guard) = self.watcher_thread.lock() {
                    if let Some(handle) = guard.take() {
                        let _: std::thread::Result<()> = handle.join();
                    }
                }
            }
        }
    }
    ```
  - The watcher thread, once spawned, stores the `JoinHandle` into `self_arc.watcher_thread.lock()...insert(handle)` and the stop flag into `self_arc.watcher_stop.lock()...insert(flag_clone)`.
  - Note: `notify = "6"` is already added to `Cargo.toml` in Task 1 behind `hot-reload` feature.

  **Must NOT do**:
  - Do NOT add watcher fields or logic outside `#[cfg(feature = "hot-reload")]` — non-feature builds must compile identically.
  - Do NOT use `notify::RecursiveMode::Recursive` — one level only.
  - Do NOT call `watcher.watch()` on individual files — watch the directory.
  - Do NOT `.unwrap()` in production code paths. The `unwrap_or_else` on the `Mutex` lock in the closure is the one exception pattern permitted (panic poison recovery).
  - Do NOT block the watcher callback — all heavy work (reload) must be off-thread.
  - Do NOT break the `Runtime` `Send + Sync` bounds — ensure all new fields are `Send + Sync` or wrapped appropriately.

  **Recommended Agent Profile**:
  > Moderate Rust async/threading task; no novel architecture.
  - **Category**: `unspecified-high`
    - Reason: Rust background threading with `notify` crate, `Arc<AtomicBool>` stop flag, feature-gating, Drop impl.
  - **Skills**: []
    - No special skills needed — pure Rust std + notify crate.
  - **Skills Evaluated but Omitted**:
    - `git-master`: Not needed here.

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 — depends on Tasks 2, 5, 6 completing first
  - **Blocks**: Task 8 (integration test for watcher)
  - **Blocked By**: Tasks 2, 5, 6

  **References**:
  - `crates/polyplug/src/runtime/mod.rs` — `Runtime` struct — add `watcher_thread: Mutex<Option<JoinHandle<()>>>` and `watcher_stop: Mutex<Option<Arc<AtomicBool>>>` feature-gated fields; add `impl Drop for Runtime` with `#[cfg(feature = "hot-reload")]` block inside
  - `crates/polyplug/src/reload/mod.rs` (Task 6 output) — `reload_bundle_impl()` — called from watcher callback
  - `crates/polyplug/Cargo.toml` (Task 1 output) — `notify = "6"` dep under `hot-reload` feature gate
  - `notify` crate docs: `https://docs.rs/notify/6/notify/` — `recommended_watcher`, `Event`, `EventKind`, `RecursiveMode`

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: watch_plugin_dir compiles under hot-reload feature
    Tool: Bash
    Preconditions: Task 5 + 6 complete, notify in Cargo.toml
    Steps:
      1. Run: cargo build -p polyplug --features hot-reload 2>&1
      2. Assert: exit code 0
      3. Run: cargo build -p polyplug 2>&1  (no feature)
      4. Assert: exit code 0 — watcher fields absent without feature
    Expected Result: Both builds succeed
    Evidence: .sisyphus/evidence/task-7-build.txt

  Scenario: Drop impl stops watcher thread (no hang)
    Tool: Bash
    Preconditions: hot-reload feature enabled, test binary built
    Steps:
      1. Run: cargo test --features hot-reload -p polyplug -- watcher_drops_cleanly --nocapture 2>&1
      2. Assert: exit code 0, test output contains "ok"
    Expected Result: Runtime with watcher drops without hanging
    Evidence: .sisyphus/evidence/task-7-drop.txt
  ```

  **Evidence to Capture**:
  - [ ] task-7-build.txt
  - [ ] task-7-drop.txt

  **Commit**: YES (groups with Task 8)
  - Message: `feat(reload): add watch_plugin_dir() hot-reload file watcher`
  - Files: `crates/polyplug/src/runtime/mod.rs`
  - Pre-commit: `cargo build -p polyplug --features hot-reload && cargo clippy -p polyplug --features hot-reload -- -D warnings`


- [ ] 8. `tests/integration_reload/mod.rs` — 9 integration test groups (a–i)

  **What to do**:
  - Create `tests/integration_reload/mod.rs` (follow existing `tests/integration_load/mod.rs` as structural template).
  - Add an entry point in `tests/integration_reload.rs` (just `mod integration_reload;`) and register it in `Cargo.toml` as `[[test]] name = "integration_reload" path = "tests/integration_reload.rs"`.
  - All 9 test groups use the fixture crates built in Task 4 (`reload_plugin_v1`, `reload_plugin_v2`). Access paths via the env vars set in `build.rs` (Task 4): `env!("RELOAD_PLUGIN_V1_SO")` and `env!("RELOAD_PLUGIN_V2_SO")`.
  - Contract ID: `polyplug::abi::contract_id("reload.test", 1)` — use this exact call everywhere.
  - Each test must call `Runtime::builder().build()` fresh to avoid state leakage between tests. Tests are NOT `#[serial]` but each creates its own isolated `Runtime`.

  **Test group a — basic_reload**:
  ```rust
  // Helper used across test groups — resolves the 1st function ptr from the vtable for a contract.
  fn get_version_fn(rt: &Runtime, contract_id: u64) -> Option<extern "C" fn() -> u32> {
      let handle: polyplug::registry::PluginHandle = rt.registry().find_by_contract(contract_id, 0).ok()?;
      let guard: PluginVTableGuard = rt.registry().resolve_guard(handle).ok()?;
      // SAFETY: vtable pointer is valid while guard is alive. Function slot 0 is version_check.
      let fn_ptr: extern "C" fn() -> u32 = unsafe {
          let vt: *const polyplug::abi::PluginVTable = guard.vtable();
          let fns: *const *const () = (*vt).functions;
          std::mem::transmute(*fns)
      };
      // guard is moved out of scope here — drop before calling fn_ptr to release Arc guard.
      Some(fn_ptr)
  }

  #[test]
  fn test_a_basic_reload() {
      let v1_path: &str = env!("RELOAD_PLUGIN_V1_SO");
      let v2_path: &str = env!("RELOAD_PLUGIN_V2_SO");
      let rt: Runtime = Runtime::builder().build().expect("build");
      rt.load_bundle(v1_path).expect("load v1");
      let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
      let version_fn: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v1");
      assert_eq!(version_fn(), 100_u32, "v1 should return 100");
      rt.reload_bundle(std::path::Path::new(v2_path)).expect("reload v2");
      let version_fn2: extern "C" fn() -> u32 = get_version_fn(&rt, contract_id).expect("resolve v2");
      assert_eq!(version_fn2(), 200_u32, "v2 should return 200");
  }
  ```

  **Test group b — in_flight_safety** (concurrent reload — no crash/UAF):
  ```rust
  #[test]
  fn test_b_in_flight_safety() {
      // Spawn tight-loop caller thread, call reload 20 times concurrently.
      // PluginVTableGuard is !Send, so we cannot pass it across threads.
      // Instead, resolve per-call on the caller thread (normal usage pattern).
      let rt: std::sync::Arc<Runtime> = std::sync::Arc::new(Runtime::builder().build().expect("build"));
      rt.load_bundle(env!("RELOAD_PLUGIN_V1_SO")).expect("load v1");
      let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
      let rt_clone: std::sync::Arc<Runtime> = std::sync::Arc::clone(&rt);
      let caller = std::thread::spawn(move || {
          for _ in 0..1000_u32 {
              // Resolve on this thread (PluginVTableGuard is !Send)
              let handle_result = rt_clone.registry().find_by_contract(contract_id, 0);
              if let Ok(handle) = handle_result {
                  if let Ok(guard) = rt_clone.registry().resolve_guard(handle) {
                      let _: u32 = unsafe {
                          let vt: *const polyplug::abi::PluginVTable = guard.vtable();
                          let f: extern "C" fn() -> u32 = std::mem::transmute(*(*vt).functions);
                          f()
                      };
                  }
              }
          }
      });
      for _ in 0..20_u32 {
          let _ = rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")));
          let _ = rt.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V1_SO")));
      }
      caller.join().expect("caller thread panicked");
  }
  ```

  **Test group c — quiescence_arc_count** (`Arc::strong_count == 1` before dlclose):
  - Requires a `#[cfg(test)]` hook in `reload_bundle_impl` that captures the strong_count before the `drop(old_library)` line and writes it to an `Arc<Mutex<Option<usize>>>` passed via a thread-local. Expose a `set_quiescence_spy(spy: ...)` function gated on `#[cfg(test)]`.
  - Assert that the captured count is `1` (only `reload_libraries` map holds it).

  **Test group d — dlclose_timing** (old handle not freed while call in flight):
  - Hold a `PluginVTableGuard` (bumps `Arc::strong_count`) then call `reload_bundle()` in another thread.
  - Assert that `reload_bundle()` blocks (waits in quiescence loop) while guard is held.
  - Drop guard, then assert reload completes within 500ms.
  - Note: `PluginVTableGuard` is `!Send`. It must be held on the SAME thread as the test. The test below holds it on the main test thread while the reload runs in a spawned thread.
  ```rust
  let rt: std::sync::Arc<Runtime> = std::sync::Arc::new(Runtime::builder().build().expect("build"));
  rt.load_bundle(env!("RELOAD_PLUGIN_V1_SO")).expect("load");
  let contract_id: u64 = polyplug::abi::contract_id("reload.test", 1);
  let handle: polyplug::registry::PluginHandle = rt.registry().find_by_contract(contract_id, 0).expect("find");
  let guard: PluginVTableGuard = rt.registry().resolve_guard(handle).expect("guard");
  let rt2: std::sync::Arc<Runtime> = std::sync::Arc::clone(&rt);
  let reload_thread = std::thread::spawn(move || {
      rt2.reload_bundle(std::path::Path::new(env!("RELOAD_PLUGIN_V2_SO")))
  });
  std::thread::sleep(std::time::Duration::from_millis(200));
  // reload_thread is blocked in quiescence loop — guard still alive on this thread
  assert!(!reload_thread.is_finished(), "should be waiting for quiescence");
  drop(guard);  // release the guard — Arc::strong_count drops to 1
  let result: Result<(), PolyplugError> = reload_thread.join().expect("join");
  assert!(result.is_ok(), "reload should succeed after guard dropped");
  ```

  **Test group e — cascade_reload** (`needs_reinit_on_dep_reload = true` bundle):
  - Load two bundles: `depender` (has `needs_reinit_on_dep_reload = true` and `ByBundle` dep on `reload.test`) and `reload_plugin_v1`.
  - Reload `reload_plugin_v1`.
  - Assert `depender` was re-initialized (track via a static counter in `depender` fixture or via the on_reload callback).
  - **NOTE**: This test requires a third fixture `depender_plugin`. Add it to Task 4 as a sub-item. The fixture exports `init_count: extern "C" fn() -> u32` which returns how many times `polyplug_init` was called.

  **Test group f — callback_fires**:
  ```rust
  let fired: Arc<Mutex<Option<ReloadEvent>>> = Arc::new(Mutex::new(None));
  let fired_clone = Arc::clone(&fired);
  let rt: Runtime = Runtime::builder()
      .on_reload(move |ev: ReloadEvent| { *fired_clone.lock().unwrap() = Some(ev); })
      .build().expect("build");
  rt.load_bundle(env!("RELOAD_PLUGIN_V1_SO")).expect("load");
  rt.reload_bundle(env!("RELOAD_PLUGIN_V2_SO")).expect("reload");
  let ev: ReloadEvent = fired.lock().unwrap().take().expect("callback must fire");
  assert_eq!(ev.bundle_path, env!("RELOAD_PLUGIN_V2_SO"));
  assert!(ev.affected_contract_ids.contains(&polyplug::abi::contract_id("reload.test", 1)));
  ```

  **Test group g — file_watcher** (behind `#[cfg(feature = "hot-reload")]`):
  ```rust
  #[cfg(feature = "hot-reload")]
  #[test]
  fn test_g_file_watcher() {
      use std::time::Duration;
      let dir: tempfile::TempDir = tempfile::tempdir().expect("tempdir");
      let so_dest: PathBuf = dir.path().join("reload_plugin.so");
      std::fs::copy(env!("RELOAD_PLUGIN_V1_SO"), &so_dest).expect("copy v1");
      let fired: Arc<std::sync::atomic::AtomicBool> = Arc::new(std::sync::atomic::AtomicBool::new(false));
      let fired_clone = Arc::clone(&fired);
      let rt: Arc<Runtime> = Arc::new(Runtime::builder()
          .on_reload(move |_| { fired_clone.store(true, std::sync::atomic::Ordering::Relaxed); })
          .build().expect("build"));
      rt.load_bundle(so_dest.to_str().expect("valid utf8 path")).expect("load");
      Runtime::watch_plugin_dir(Arc::clone(&rt), dir.path()).expect("watch");
      // Replace with v2
      std::fs::copy(env!("RELOAD_PLUGIN_V2_SO"), &so_dest).expect("copy v2");
      // Wait up to 500ms for debounce + watcher to fire
      for _ in 0..50 {
          if fired.load(std::sync::atomic::Ordering::Relaxed) { break; }
          std::thread::sleep(Duration::from_millis(10));
      }
      assert!(fired.load(std::sync::atomic::Ordering::Relaxed), "watcher must have fired reload");
  }
  ```
  - Note: `tempfile = "3"` is ALREADY in `[dev-dependencies]` of `crates/polyplug/Cargo.toml` — do NOT re-add it.

  **Test group h — multiple_reloads** (50x, no leak):
  ```rust
  #[test]
  fn test_h_multiple_reloads() {
      let rt: Runtime = Runtime::builder().build().expect("build");
      rt.load_bundle(env!("RELOAD_PLUGIN_V1_SO")).expect("load");
      for i in 0..50_u32 {
          let so: &str = if i % 2 == 0 { env!("RELOAD_PLUGIN_V2_SO") } else { env!("RELOAD_PLUGIN_V1_SO") };
          rt.reload_bundle(std::path::Path::new(so)).expect("reload");
      }
      // reload_libraries should have exactly 1 entry (the last loaded)
      // Expose via #[cfg(test)] accessor: rt.test_reload_libraries_count()
      assert_eq!(rt.test_reload_libraries_count(), 1, "should only keep latest");
  }
  ```
  - After each successful reload, the previous `reload_libraries` entry for that `bundle_id` must be dropped (dlclose'd). Only the current live version stays in `reload_libraries`.

  **Test group i — non_native_returns_error**:
  ```rust
  #[test]
  fn test_i_non_native_returns_error() {
      // Simulate a non-native bundle path (e.g. a .py or .lua extension)
      let rt: Runtime = Runtime::builder().build().expect("build");
      // Inject a fake manifest entry for a python bundle
      // (or use a test helper to register a non-native bundle_id)
      let result = rt.reload_bundle("fake_plugin.py");
      match result {
          Err(PolyplugError::ReloadFailed { reason }) => {
              assert!(reason.contains("non-native"), "error must mention non-native: {reason}");
          }
          other => panic!("expected ReloadFailed for non-native, got: {other:?}"),
      }
  }
  ```

  **Must NOT do**:
  - Do NOT use `#[serial]` or test mutexes across tests — each test creates its own `Runtime`.
  - Do NOT share static state between test groups.
  - Do NOT `.unwrap()` on `reload_bundle()` results except in test assertions with `.expect("message")`.
  - Do NOT add test-only code paths to production modules except for explicitly `#[cfg(test)]`-gated hooks.
  - Do NOT skip test group e (cascade) by omitting the `depender_plugin` fixture — it must be created.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: 9 test groups with edge cases, concurrent tests, quiescence hooks, and a third fixture to create.
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `playwright`: Not applicable (no UI).

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 6 and 7)
  - **Parallel Group**: Wave 3 (sequential after Tasks 6 and 7)
  - **Blocks**: Final Verification Wave
  - **Blocked By**: Tasks 6, 7

  **References**:
  - `tests/integration_load/mod.rs` — structural template: test entry wiring, `Runtime::builder().build()`, `load_bundle()` pattern
  - `crates/polyplug/src/reload/mod.rs` (Task 6 output) — `ReloadEvent`, `reload_bundle_impl()`, quiescence spy hook
  - `crates/polyplug/src/runtime/mod.rs` (Task 5 output) — `Runtime::reload_bundle()`, `on_reload()` builder
  - `crates/polyplug/src/registry/mod.rs` — `PluginVTableGuard`, `resolve_guard()`, `fn_ptr()` slot indexing
  - Task 4 fixtures: `tests/fixtures/reload_plugin_v1/`, `tests/fixtures/reload_plugin_v2/` — V1 returns 100, V2 returns 200 from `version_check()`
  - `tempfile` crate docs: `https://docs.rs/tempfile/3/tempfile/` — for test group g

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: all test groups pass without hot-reload feature
    Tool: Bash
    Preconditions: Tasks 4, 6 complete; fixture .so files built
    Steps:
      1. Run: cargo test --test integration_reload -- --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test test_a_basic_reload ... ok"
      4. Assert: output contains "test test_b_in_flight_safety ... ok"
      5. Assert: output contains "test test_c_quiescence_arc_count ... ok"
      6. Assert: output contains "test test_d_dlclose_timing ... ok"
      7. Assert: output contains "test test_e_cascade_reload ... ok"
      8. Assert: output contains "test test_f_callback_fires ... ok"
      9. Assert: output contains "test test_h_multiple_reloads ... ok"
      10. Assert: output contains "test test_i_non_native_returns_error ... ok"
    Expected Result: 8 test groups pass (g is feature-gated)
    Evidence: .sisyphus/evidence/task-8-tests-no-feature.txt

  Scenario: file watcher test passes with hot-reload feature
    Tool: Bash
    Preconditions: Task 7 complete
    Steps:
      1. Run: cargo test --test integration_reload --features hot-reload -- test_g_file_watcher --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: output contains "test test_g_file_watcher ... ok"
    Expected Result: watcher test passes
    Evidence: .sisyphus/evidence/task-8-test-g-watcher.txt

  Scenario: concurrent in-flight test does not crash under ThreadSanitizer (advisory)
    Tool: Bash
    Steps:
      1. Run: RUSTFLAGS="-Z sanitizer=thread" cargo +nightly test --test integration_reload -- test_b_in_flight_safety --nocapture 2>&1 || true
      2. Check output for "ThreadSanitizer" data race warnings
    Expected Result: no data race reported (advisory — failure is a warning, not hard block)
    Evidence: .sisyphus/evidence/task-8-tsan.txt
  ```

  **Evidence to Capture**:
  - [ ] task-8-tests-no-feature.txt
  - [ ] task-8-test-g-watcher.txt
  - [ ] task-8-tsan.txt

  **Commit**: YES (groups with Task 7)
  - Message: `test(reload): add integration_reload test suite (9 groups a–i)`
  - Files: `tests/integration_reload/mod.rs`, `tests/integration_reload.rs`, `crates/polyplug/Cargo.toml` (tempfile dev-dep)
  - Pre-commit: `cargo test --test integration_reload && cargo clippy --workspace -- -D warnings`


- [ ] 9. TRUST_MODEL.md — hot-reload safety guarantees section

  **What to do**:
  - Open `TRUST_MODEL.md` and add a new section titled `## Hot-Reload Safety Guarantees` after the existing `## ABI Freeze` section (or at the end of the file if no such section exists — find the right placement by reading the file first).
  - The section must contain **exactly these 5 bullets**, verbatim:
    1. `vtable swaps are atomic at the ArcSwap level — readers always see a consistent VTableSlot`
    2. `old library handles are held alive by Arc reference counting until all in-flight calls release their PluginVTableGuard`
    3. `the quiescence spin is bounded to 5 seconds; if the bound is exceeded the reload fails with QuiescenceTimeout without touching the live vtable`
    4. `cascade reload depth is capped at 16 levels; deeper cascades fail with ReloadFailed and leave all plugins in their pre-reload state`
    5. `non-native language bundles (Python, Lua, JS, .NET) are explicitly not reloadable in this version; reload_bundle() returns ReloadFailed with a clear reason string`
  - Do NOT reword or paraphrase these bullets. Copy them exactly.

  **Must NOT do**:
  - Do NOT modify any other section of `TRUST_MODEL.md`.
  - Do NOT change the ABI freeze section.

  **Recommended Agent Profile**:
  - **Category**: `writing`
    - Reason: Documentation-only task — append a markdown section to an existing doc.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10 and 11)
  - **Parallel Group**: Wave 4 (with Tasks 10, 11)
  - **Blocks**: Final Verification Wave
  - **Blocked By**: Tasks 6, 7 (defines the guarantees being documented)

  **References**:
  - `TRUST_MODEL.md` — read full file first to find placement and avoid duplicate sections

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: TRUST_MODEL.md contains all 5 hot-reload bullets
    Tool: Bash
    Preconditions: TRUST_MODEL.md updated
    Steps:
      1. Run: grep -c 'vtable swaps are atomic' TRUST_MODEL.md
      2. Assert: output is 1
      3. Run: grep -c 'quiescence spin is bounded to 5 seconds' TRUST_MODEL.md
      4. Assert: output is 1
      5. Run: grep -c 'cascade reload depth is capped at 16' TRUST_MODEL.md
      6. Assert: output is 1
      7. Run: grep -c 'non-native language bundles' TRUST_MODEL.md
      8. Assert: output is 1
      9. Run: grep -c 'old library handles are held alive' TRUST_MODEL.md
      10. Assert: output is 1
    Expected Result: All 5 bullets present
    Evidence: .sisyphus/evidence/task-9-trust-model.txt
  ```

  **Evidence to Capture**:
  - [ ] task-9-trust-model.txt (grep output confirming all 5 bullets)

  **Commit**: YES (groups with Tasks 10, 11)
  - Message: `docs(reload): add hot-reload safety guarantees to TRUST_MODEL.md`
  - Files: `TRUST_MODEL.md`
  - Pre-commit: none

- [ ] 10. BENCHMARKS.md — reload latency row

  **What to do**:
  - Open `BENCHMARKS.md` and read the existing table structure to find where to insert the new row.
  - Add a new row to the existing benchmark table for `reload_bundle() cold path`:
    - **Benchmark**: `reload_bundle() cold path`
    - **Description**: Full vtable-swap reload cycle (quiescence wait + ArcSwap + notify callback)
    - **Steady-state overhead**: `identical to baseline (no overhead when hot-reload feature disabled; zero branches added to reader path)`
    - **Reload latency (native, no contention)**: `< 1ms p50, < 5ms p99`
    - **Note**: `Quiescence timeout cap: 5s. Measurement excludes dlopen() I/O.`
  - Also add a note at the bottom of the file: `Hot-reload feature flag: the hot-reload feature adds no overhead to the reader path (resolve_guard, find_by_contract). All watcher and reload code paths are invoked only when reload_bundle() is called explicitly.`

  **Must NOT do**:
  - Do NOT modify existing rows.
  - Do NOT change the table format — match existing column structure exactly. If the existing columns are `Benchmark | Mean (ns) | Std Dev (ns) | Notes | Epic`, adapt the reload row data to fit: use `"< 1,000,000"` for Mean (ns) p50, `"< 5,000,000"` for Std Dev (ns) p99, and put `"Quiescence timeout: 5s; I/O excluded"` in Notes. The exact column names are in `BENCHMARKS.md` — read the file before inserting.
  - Do NOT add columns that don't exist in the table.

  **Recommended Agent Profile**:
  - **Category**: `writing`
    - Reason: Documentation-only task.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9 and 11)
  - **Parallel Group**: Wave 4
  - **Blocks**: Final Verification Wave
  - **Blocked By**: Tasks 6, 7

  **References**:
  - `BENCHMARKS.md` — read full file first to match table format

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: BENCHMARKS.md contains reload row
    Tool: Bash
    Steps:
      1. Run: grep -c 'reload_bundle() cold path' BENCHMARKS.md
      2. Assert: output is 1
      3. Run: grep -c 'identical to baseline' BENCHMARKS.md
      4. Assert: output is 1
    Expected Result: Reload row present
    Evidence: .sisyphus/evidence/task-10-benchmarks.txt
  ```

  **Evidence to Capture**:
  - [ ] task-10-benchmarks.txt

  **Commit**: YES (groups with Tasks 9, 11)
  - Message: `docs(reload): add reload latency row to BENCHMARKS.md`
  - Files: `BENCHMARKS.md`
  - Pre-commit: none

- [ ] 11. `lib.rs` + cascade wiring audit

  **What to do**:
  - This is an audit task. Read the following files and verify the stated conditions. Fix anything that fails.
  - **lib.rs check**: Verify `pub mod reload;` is present in `crates/polyplug/src/lib.rs`. If missing, add it. Also verify `pub use reload::ReloadEvent;` is present (so users can `use polyplug::ReloadEvent`). If missing, add it after the `pub mod reload;` line.
  - **`reload_bundle` re-export**: Verify that `Runtime::reload_bundle()` is accessible as `polyplug::Runtime::reload_bundle()`. Since `Runtime` is already `pub` in `lib.rs`, this is automatic. No additional re-export needed for the method itself.
  - **ManifestData.path population check**: Search all call sites that construct or return a `ManifestData` value (likely in `loader/manifest/mod.rs` and `loader/mod.rs`). Verify that `path` is always populated with the source `.toml` path (or `.so` path as appropriate) in every code path, not left as `PathBuf::new()` or a default. Fix any unpopulated sites.
  - **Reader-path zero-change check**: Read `registry/mod.rs` functions `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()`, and `host_resolve_plugin()`. Verify that Tasks 3, 5, 6 introduced zero new branches (no `if`, `match`, `?` added to these functions). If any are found, file this as a VIOLATION and escalate to the human — do NOT silently fix.
  - **Cascade wiring check**: In `reload/mod.rs`, verify `find_cascade_targets()` reads `ManifestData.path` for each candidate (to load the bundle for re-init). Verify `ManifestData.needs_reinit_on_dep_reload` is actually checked (not commented out or always-false).
  - **pub use re-exports check**: Verify `ReloadEvent` and `reload_bundle` are re-exported from `lib.rs` at the crate root level (so users can `use polyplug::ReloadEvent`).

  **Must NOT do**:
  - Do NOT add new logic to reader-path functions — only audit.
  - Do NOT change `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()`, `host_resolve_plugin()` for any reason.
  - Do NOT add new test coverage here — that's Task 8.

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Multi-file audit requiring careful cross-reference between loader, registry, runtime, and lib.rs. Must escalate violations rather than silently fix.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9 and 10)
  - **Parallel Group**: Wave 4
  - **Blocks**: Final Verification Wave
  - **Blocked By**: Tasks 5, 6

  **References**:
  - `crates/polyplug/src/lib.rs` — verify `pub mod reload;` exists
  - `crates/polyplug/src/loader/manifest/mod.rs` — all `ManifestData` construction sites — verify `path` set
  - `crates/polyplug/src/loader/mod.rs` — `load_bundle()` return path — verify `ManifestData.path` populated
  - `crates/polyplug/src/registry/mod.rs` — `resolve_guard()`, `find_by_contract()`, `find_all_by_contract()`, `host_resolve_plugin()` — zero new branches allowed
  - `crates/polyplug/src/reload/mod.rs` (Task 6 output) — `find_cascade_targets()` — verify path + needs_reinit_on_dep_reload usage

  **Acceptance Criteria**:

  **QA Scenarios (MANDATORY)**:
  ```
  Scenario: lib.rs exports reload module and ReloadEvent
    Tool: Bash
    Steps:
      1. Run: grep 'pub mod reload' crates/polyplug/src/lib.rs
      2. Assert: match found
      3. Run: grep 'ReloadEvent' crates/polyplug/src/lib.rs
      4. Assert: match found (re-export or pub use)
    Expected Result: Both present
    Evidence: .sisyphus/evidence/task-11-lib-exports.txt

  Scenario: reader-path functions have zero new branches
    Tool: Bash
    Steps:
      1. Run: git diff main -- crates/polyplug/src/registry/mod.rs | grep '^+' | grep -E '\bif\b|\bmatch\b' | grep -v '^\+\+\+'
      2. Assert: output is empty (no new conditional branches in registry reader functions)
    Expected Result: Zero new branches
    Evidence: .sisyphus/evidence/task-11-reader-path.txt

  Scenario: ManifestData.path populated in all load paths
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace -- manifest_path_populated --nocapture 2>&1
      2. Assert: test passes (add a simple #[cfg(test)] test in loader/manifest/mod.rs asserting path != PathBuf::new())
    Expected Result: path is non-empty after load_bundle()
    Evidence: .sisyphus/evidence/task-11-manifest-path.txt
  ```

  **Evidence to Capture**:
  - [ ] task-11-lib-exports.txt
  - [ ] task-11-reader-path.txt
  - [ ] task-11-manifest-path.txt

  **Commit**: YES (groups with Tasks 9, 10)
  - Message: `fix(reload): wire ManifestData.path, pub mod reload, cascade audit`
  - Files: `crates/polyplug/src/lib.rs`, `crates/polyplug/src/loader/manifest/mod.rs`, `crates/polyplug/src/loader/mod.rs` (if path missing)
  - Pre-commit: `cargo build --workspace && cargo clippy --workspace -- -D warnings`


## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read source files, run `cargo test`). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found (e.g., `.unwrap()` in production code, new branches in `resolve_guard()`). Check evidence files exist in `.sisyphus/evidence/`. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy --workspace --features hot-reload -- -D warnings` + `cargo fmt --check --all` + `cargo test --workspace`. Review all changed files for: `.unwrap()` / `.expect()` in production code, `use` inside functions, bare `filename.rs` module roots, `unsafe` blocks without `// SAFETY:` comments, missing explicit type annotations. Check AI slop patterns.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [ ] F3. **Full Integration Test Run** — `unspecified-high`
  From a clean state, run:
  1. `cargo test --workspace 2>&1` — assert exit 0, grep for failures
  2. `cargo test --workspace --features hot-reload 2>&1` — assert exit 0
  3. `cargo test --test integration_reload -- --nocapture 2>&1` — assert all 9 test groups pass
  4. `cargo test --test integration_reload --features hot-reload -- --nocapture 2>&1` — assert watcher test passes
  Capture all output to `.sisyphus/evidence/final-qa/full-test-run.txt`.
  Output: `Tests [N/N pass] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", check git diff for that file scope. Verify nothing beyond spec was built (no creep into non-native reload, no new C ABI exports, no reader-path changes). Grep `resolve_guard`, `find_by_contract`, `find_all_by_contract`, `host_resolve_plugin` — confirm zero new branches added.
  Output: `Tasks [N/N compliant] | Reader-path [CLEAN] | ABI [FROZEN] | VERDICT`

---

## Commit Strategy

- **Wave 1**: `feat(reload): add Cargo feature gate, error variants, Registry helpers, V1/V2 fixtures`
- **Wave 2**: `feat(reload): add bundle_manifests storage, reload_libraries, on_reload callback, core reload_bundle()`
- **Wave 3**: `feat(reload): add watch_plugin_dir() file watcher and integration tests`
- **Wave 4**: `docs(reload): add TRUST_MODEL.md section and BENCHMARKS.md reload latency`
- Pre-commit: `cargo test --workspace && cargo clippy --workspace --features hot-reload -- -D warnings`

---

## Success Criteria

### Verification Commands
```bash
# Core test suite (no feature)
cargo test --workspace 2>&1 | tail -3
# Expected: "test result: ok. N passed; 0 failed"

# Hot-reload feature enabled
cargo test --workspace --features hot-reload 2>&1 | tail -3
# Expected: "test result: ok. N passed; 0 failed"

# Reload-specific test group
cargo test --test integration_reload -- --nocapture 2>&1 | tail -5
# Expected: all test groups pass

# Lint
cargo clippy --workspace --features hot-reload -- -D warnings 2>&1 | tail -2
# Expected: no warnings

# Format
cargo fmt --check --all 2>&1
# Expected: no output (exit 0)
```

### Final Checklist
- [ ] `reload_bundle()` 5-step path implemented and tested
- [ ] In-flight safety verified (quiescence test passes)
- [ ] `Arc::strong_count == 1` before dlclose (test hook confirms)
- [ ] dlclose timing: old handle not freed while call in flight
- [ ] Cascade reload: needs_reinit_on_dep_reload = true bundle re-initialized
- [ ] Callback fires after swap, before dlclose
- [ ] File watcher test passes (hot-reload feature)
- [ ] Multiple reload (50x): no memory leak, no leaked handles
- [ ] Reader-path benchmarks unchanged (within 10% of baseline)
- [ ] `needs_reinit_on_dep_reload` in ManifestData wired into cascade
- [ ] TRUST_MODEL.md section exists
- [ ] No `.unwrap()` in production code
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo test --workspace --features hot-reload` passes
