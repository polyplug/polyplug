# Epic 9.7 — Full ABI Redesign: Dependencies, Multi-Impl, arc-swap, Hot-Reload Foundation

## TL;DR

> **Quick Summary**: Redesign the `polyplug` ABI to replace the single `find_plugin`/`call_plugin` model with a three-query API (`find_by_contract`, `find_by_bundle`, `find_all_by_contract`) backed by arc-swap vtable slots, multi-impl registry, declared dependency enforcement, and INIT_BUNDLE_ID thread-local for safety during bundle init.
>
> **Deliverables**:
> - New `HostVTable` (7 fn ptrs, 56 bytes) in `crates/polyplug/src/abi/mod.rs`
> - Multi-impl `Registry` with `ArcSwap<VTableSlot>` entries in `crates/polyplug/src/registry/mod.rs`
> - Dependency-enforcing runtime callbacks in `crates/polyplug/src/runtime/mod.rs`
> - New C exports (`polyplug_find_by_contract`, `polyplug_find_by_bundle`, `polyplug_find_all_by_contract`, `polyplug_resolve_plugin`) in `crates/polyplug/src/lib.rs`
> - Updated manifest parser + IR + all three generators (Rust, C++, C#) + Python/Lua skeleton stubs
> - Updated host-libs and guest-libs for Rust, C++, C#
> - 7-test cross-plugin integration suite
> - `TRUST_MODEL.md` at repo root
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 5 waves
> **Critical Path**: Task 1 → Task 5 → Task 6 → Task 7 → Task 8 → Task 14 → Task 20 → Task 21

---

## Context

### Original Request
Implement Epic 9.7 — Full ABI Redesign covering: declared dependency system, multi-impl registry, arc-swap vtable slots, hot-reload foundation, `INIT_BUNDLE_ID` thread-local, new C export surface, updated codegen, updated host/guest libs, integration tests, and `TRUST_MODEL.md`.

### Interview Summary
**Key Discussions**:
- Epic 9.5 (polyplug-dotnet hardening) already implemented — do not touch
- `find_all_by_contract` C ABI: caller-provides-buffer (`out: *mut PluginHandle, out_cap: usize`) returns count
- `PluginVTableGuard` is Rust-only; C ABI exposes raw `*const PluginVTable` from `resolve_plugin`
- `VTableSlot` + `PluginVTableGuard` belong in `registry/mod.rs`, not `abi/mod.rs`
- Python/Lua host/guest libs must NOT be created (Epic 10/11 scope)
- `DuplicateProvider` semantics change: same contract_id + same bundle_id = error; different bundles = allowed
- Thread-local `INIT_BUNDLE_ID` set/cleared by `NativeBundleLoader::load()` around `polyplug_init` call
- `find_by_contract` returns the first registered (index 0 of Vec)
- After this epic the ABI is re-frozen

**Research Findings**:
- `arc-swap = "1.7"` not yet in `Cargo.toml` — must be added
- `HostVTable` currently 40 bytes (5 fn ptrs) — must become 56 bytes (7 fn ptrs), layout test must change
- `contract_index` currently `HashMap<u64, u32>` — must become `HashMap<u64, Vec<u32>>`
- No `bundle_index` exists yet — must be added as `HashMap<u64, u32>`
- Python/Lua generator directories completely empty — create skeleton stubs only
- `polyplug_find_plugin` and `polyplug_call_plugin` C exports must be removed entirely

### Metis Review
**Identified Gaps** (addressed):
- `StaleHandle` collision: existing `RegistryError::StaleHandle` kept unchanged; no second top-level variant added
- `find_all_by_contract` ABI: confirmed caller-provides-buffer matches PRD §6
- `resolve_plugin` ABI return: confirmed raw `*const PluginVTable`, not `PluginVTableGuard`
- Python/Lua host/guest libs: confirmed out of scope
- Thread-local init guard: confirmed `INIT_BUNDLE_ID` set/cleared by loader, checked only during init phase

---

## Work Objectives

### Core Objective
Replace the monolithic `find_plugin`/`call_plugin` ABI with a three-query lookup API backed by arc-swap vtable slots and multi-impl registry, enforcing declared dependencies during bundle initialization.

### Concrete Deliverables
- `crates/polyplug/src/abi/mod.rs` — new `HostVTable` (7 fn ptrs, 56 bytes), `bundle_id()` fn, ABI freeze comment
- `crates/polyplug/src/registry/mod.rs` — `VTableSlot`, `PluginVTableGuard`, multi-impl `Registry`
- `crates/polyplug/src/runtime/mod.rs` — `INIT_BUNDLE_ID` thread-local, 4 new host callbacks
- `crates/polyplug/src/error/mod.rs` — 3 new `RuntimeError` variants
- `crates/polyplug/src/lib.rs` — 4 new C exports, 2 old C exports removed
- `crates/polyplug/src/loader/manifest/mod.rs` — `ManifestData` gains `bundle_id` + `dependencies`
- `crates/polyplug/src/loader/mod.rs` — set/clear `INIT_BUNDLE_ID`
- `crates/polyplug/Cargo.toml` — `arc-swap = "1.7"` added
- `crates/polyplugc/src/parser/mod.rs` — `[[dependency]]` parsing, `requires` removed
- `crates/polyplugc/src/ir/mod.rs` — `ResolvedDependency`, `ResolvedBundle` updated, `MY_BUNDLE_ID` const
- `crates/polyplugc/src/generators/rust/mod.rs` — guard-based dispatch, new manifest format
- `crates/polyplugc/src/generators/cpp/mod.rs` — guard-based dispatch
- `crates/polyplugc/src/generators/csharp/mod.rs` — updated API
- `crates/polyplugc/src/generators/python/mod.rs` — skeleton stub
- `crates/polyplugc/src/generators/lua/mod.rs` — skeleton stub
- `crates/polyplugc/src/generators/mod.rs` — register python + lua modules
- `host-libs/rust/src/lib/mod.rs` — new ABI wrappers
- `guest-libs/rust/src/lib/mod.rs` — remove call_plugin usage
- `host-libs/cpp/polyplug/abi.hpp` — updated HostVTable layout
- `guest-libs/cpp/polyplug/abi.hpp` — updated
- `host-libs/csharp/src/Abi.cs` — updated P/Invoke declarations
- `guest-libs/csharp/src/Abi.cs` — updated
- `tests/fixtures/test_plugin/src/lib.rs` — mirrored HostVTable updated
- `tests/integration_cross_plugin/mod.rs` — 7 new tests
- `crates/polyplug/benches/vtable_dispatch.rs` — updated stubs + new bench
- `TRUST_MODEL.md` — new file at repo root
- `AGENTS.md` — reference to TRUST_MODEL.md added

### Definition of Done
- [ ] `cargo build --workspace` exits 0
- [ ] `cargo test --workspace` exits 0, all tests pass
- [ ] `cargo clippy -- -D warnings` exits 0, zero warnings
- [ ] `cargo bench -p polyplug --bench vtable_dispatch --no-run` exits 0
- [ ] `grep -r "call_plugin\|find_plugin" crates/ --include="*.rs" | grep -v "_by_contract\|_by_bundle\|_all_by"` returns zero results
- [ ] `HostVTable` size is exactly 56 bytes (confirmed by layout test)
- [ ] All 7 cross-plugin integration tests pass

### Must Have
- `HostVTable` at exactly 56 bytes with exactly 7 fn ptrs in declaration order
- `ArcSwap<VTableSlot>` per registry slot (not raw pointer, not plain Arc)
- `INIT_BUNDLE_ID` thread-local set before and cleared after `polyplug_init` call
- Dependency enforcement: `host_find_by_contract`/`host_find_by_bundle` check `declared_deps` when `INIT_BUNDLE_ID != 0`
- `DuplicateProvider` returned only when same contract_id AND same bundle_id collide; different bundles are allowed
- `bundle_id()` in `abi/mod.rs` uses `fnv1a_64(name.as_bytes())`
- All 3 new `RuntimeError` variants
- `polyplug_find_plugin` and `polyplug_call_plugin` completely removed from `lib.rs`
- Python/Lua generators: skeleton stubs only, no real implementation
- ABI freeze comment block in `abi/mod.rs`

### Must NOT Have (Guardrails)
- `.unwrap()` anywhere in production code
- `.expect()` outside `#[cfg(test)]` blocks
- `use` statements inside functions, structs, or impl blocks
- Bare `filename.rs` as module root (use `filename/mod.rs`)
- Editing any generated file by hand (update the generator instead)
- Modifying Epic 9.5 code (polyplug-dotnet hardening)
- `unsafe` blocks without `// SAFETY:` comment
- Creating Python or Lua host-libs or guest-libs directories/files
- String-based error returns (`Err("...".to_string())`)
- Missing explicit type annotations (except the two permitted cases)
- `include!()` module patterns
- Second `StaleHandle` variant at top level of `RuntimeError` or `PolyplugError`
- Any change to `PluginHandle` struct layout or null sentinel
- Any change to `host_alloc`, `host_free`, `get_extension` signatures
- `call_plugin` or `find_plugin` remaining in any `.rs` file under `crates/`

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES
- **Automated tests**: Tests-after (integration tests in Wave 5)
- **Framework**: `cargo test`
- **TDD**: Not applicable — ABI changes require implementation before tests can be written

### QA Policy
Every task includes agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Build verification**: `cargo build --workspace`
- **Unit tests**: `cargo test --workspace`
- **Lint**: `cargo clippy -- -D warnings`
- **Absence checks**: `grep` for removed symbols
- **Layout checks**: layout test assertions in `abi/mod.rs`

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — no dependencies, start immediately):
├── Task 1:  Add arc-swap dependency to Cargo.toml [quick]
├── Task 2:  Add bundle_id() function to abi/mod.rs [quick]
├── Task 3:  Add 3 new RuntimeError variants to error/mod.rs [quick]
└── Task 4:  Add VTableSlot + PluginVTableGuard to registry/mod.rs [quick]

Wave 2 (Core ABI + Registry — depends on Wave 1):
├── Task 5:  Rewrite HostVTable in abi/mod.rs (depends: 1, 2) [unspecified-high]
├── Task 6:  Rewrite Registry in registry/mod.rs (depends: 1, 3, 4, 5) [deep]
├── Task 7:  Rewrite runtime callbacks in runtime/mod.rs (depends: 3, 5, 6) [deep]
├── Task 8:  Update lib.rs C exports (depends: 5, 6, 7) [unspecified-high]
├── Task 9:  Update loader/manifest/mod.rs (depends: 2) [quick]
├── Task 10: Update loader/mod.rs INIT_BUNDLE_ID + registrar_callback (depends: 6, 7, 9) [deep]

Wave 3 (Codegen — depends on Wave 2):
├── Task 11: Update parser/mod.rs — dependency parsing (depends: 9) [unspecified-high]
├── Task 12: Update ir/mod.rs — ResolvedDependency + MY_BUNDLE_ID (depends: 11) [unspecified-high]
├── Task 13: Update generators/mod.rs — register python + lua (independent) [quick]
├── Task 14: Update Rust generator (depends: 12) [deep]
├── Task 15: Update C++ generator (depends: 12) [unspecified-high]
├── Task 16: Update C# generator (depends: 12) [unspecified-high]
├── Task 17: Create Python generator skeleton (depends: 13) [quick]
└── Task 18: Create Lua generator skeleton (depends: 13) [quick]

Wave 4 (Consumer Updates — depends on Waves 2 & 3):
├── Task 19: Update host-libs/rust (depends: 5, 6, 7, 8) [unspecified-high]
├── Task 20: Update guest-libs/rust (depends: 5, 8) [quick]
├── Task 21: Update host-libs/cpp (depends: 5) [unspecified-high]
├── Task 22: Update guest-libs/cpp (depends: 5) [unspecified-high]
├── Task 23: Update host-libs/csharp (depends: 5) [unspecified-high]
├── Task 24: Update guest-libs/csharp (depends: 5) [quick]
├── Task 25: Update tests/fixtures/test_plugin (depends: 5) [quick]
├── Task 26: Update existing integration tests (depends: 6, 7, 8) [unspecified-high]
└── Task 27: Update benches/vtable_dispatch.rs (depends: 5, 6, 7, 8) [unspecified-high]

Wave 5 (Verification + Documentation — depends on Wave 4):
├── Task 28: Create tests/integration_cross_plugin/mod.rs (depends: 6, 7, 8, 26) [deep]
├── Task 29: Wire integration_cross_plugin into Cargo.toml [[test]] (depends: 28) [quick]
├── Task 30: Create TRUST_MODEL.md at repo root (independent) [writing]
├── Task 31: Update AGENTS.md with TRUST_MODEL.md reference (depends: 30) [quick]
└── Task 32: Add ABI freeze comment to abi/mod.rs (depends: 5) [quick]

Wave FINAL (After ALL tasks — independent parallel review):
├── F1: Plan Compliance Audit [oracle]
├── F2: Code Quality Review [unspecified-high]
├── F3: Real Manual QA [unspecified-high]
└── F4: Scope Fidelity Check [deep]

Critical Path: T1 → T5 → T6 → T7 → T8 → T26 → T28 → F1-F4
Parallel Speedup: ~65% faster than sequential
Max Concurrent: 8 (Wave 3)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1 | — | 5, 6 |
| 2 | — | 5, 9 |
| 3 | — | 6, 7 |
| 4 | — | 6 |
| 5 | 1, 2 | 6, 7, 8, 19, 20, 21, 22, 23, 24, 25, 27, 32 |
| 6 | 1, 3, 4, 5 | 7, 8, 19, 26, 27, 28 |
| 7 | 3, 5, 6 | 8, 10, 19, 26, 27, 28 |
| 8 | 5, 6, 7 | 19, 20, 26, 27, 28 |
| 9 | 2 | 10, 11 |
| 10 | 7, 9 | 26 |
| 11 | 9 | 12 |
| 12 | 11 | 14, 15, 16 |
| 13 | — | 17, 18 |
| 14 | 12 | — |
| 15 | 12 | — |
| 16 | 12 | — |
| 17 | 13 | — |
| 18 | 13 | — |
| 19 | 5, 6, 7, 8 | — |
| 20 | 5, 8 | — |
| 21 | 5 | — |
| 22 | 5 | — |
| 23 | 5 | — |
| 24 | 5 | — |
| 25 | 5 | — |
| 26 | 6, 7, 8 | 28 |
| 27 | 5, 6, 7, 8 | — |
| 28 | 6, 7, 8, 26 | 29 |
| 29 | 28 | — |
| 30 | — | 31 |
| 31 | 30 | — |
| 32 | 5 | — |

### Agent Dispatch Summary

- **Wave 1** (4): T1 → `quick`, T2 → `quick`, T3 → `quick`, T4 → `quick`
- **Wave 2** (6): T5 → `unspecified-high`, T6 → `deep`, T7 → `deep`, T8 → `unspecified-high`, T9 → `quick`, T10 → `unspecified-high`
- **Wave 3** (8): T11 → `unspecified-high`, T12 → `unspecified-high`, T13 → `quick`, T14 → `deep`, T15 → `unspecified-high`, T16 → `unspecified-high`, T17 → `quick`, T18 → `quick`
- **Wave 4** (9): T19 → `unspecified-high`, T20 → `quick`, T21 → `unspecified-high`, T22 → `unspecified-high`, T23 → `unspecified-high`, T24 → `quick`, T25 → `quick`, T26 → `unspecified-high`, T27 → `unspecified-high`
- **Wave 5** (5): T28 → `deep`, T29 → `quick`, T30 → `writing`, T31 → `quick`, T32 → `quick`
- **FINAL** (4): F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [x] 1. Add `arc-swap = "1.7"` to `crates/polyplug/Cargo.toml`

  **What to do**:
  - Open `crates/polyplug/Cargo.toml`
  - In the `[dependencies]` section, add exactly: `arc-swap = "1.7"`
  - Do not add it under `[dev-dependencies]` — it is a production runtime dependency
  - Run `cargo fetch` to confirm the dependency resolves without error

  **Must NOT do**:
  - Do not change any other dependency versions
  - Do not add feature flags unless arc-swap requires them (it does not)

  **Recommended Agent Profile**:
  > Single-line TOML edit.
  - **Category**: `quick`
    - Reason: Single line addition to a config file, no logic involved
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)
  - **Blocks**: Tasks 5, 6
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/Cargo.toml:1-83` — existing dependency section, match formatting style
  - arc-swap crate: https://docs.rs/arc-swap/1.7.1/arc_swap/ — confirm `ArcSwap<T>` API

  **Acceptance Criteria**:
  - [ ] `crates/polyplug/Cargo.toml` contains line `arc-swap = "1.7"` under `[dependencies]`
  - [ ] `cargo fetch` exits 0
  - [ ] `cargo build -p polyplug` exits 0 (arc-swap compiles)

  **QA Scenarios**:
  ```
  Scenario: arc-swap dependency resolves and compiles
    Tool: Bash
    Preconditions: Cargo.toml edited, network available
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Assert: output does NOT contain "error" (case-insensitive)
    Expected Result: Clean build with arc-swap resolved
    Failure Indicators: "failed to select a version" or "unresolved import"
    Evidence: .sisyphus/evidence/task-1-arc-swap-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-1-arc-swap-build.txt — cargo build output

  **Commit**: YES (group with Tasks 2, 3, 4)
  - Message: `feat(deps): add arc-swap 1.7 to polyplug crate`
  - Files: `crates/polyplug/Cargo.toml`
  - Pre-commit: `cargo build -p polyplug`

- [x] 2. Add `bundle_id()` function to `crates/polyplug/src/abi/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/abi/mod.rs`
  - Locate the block of existing hash/ID functions: `contract_id()`, `extension_id()`, `fnv1a_64()`, `fnv1a_32()`
  - Add the following function AFTER `extension_id()` and BEFORE `fnv1a_64()`:
    ```rust
    pub fn bundle_id(name: &str) -> u64 {
        fnv1a_64(name.as_bytes())
    }
    ```
  - The function is `pub` (same visibility as `contract_id` and `extension_id`)
  - No additional logic — it delegates directly to `fnv1a_64`

  **Must NOT do**:
  - Do not change `contract_id()`, `extension_id()`, `fnv1a_64()`, or `fnv1a_32()`
  - Do not add `use` inside the function body
  - Do not add a `// SAFETY:` comment (this function has no unsafe code)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Adding a 3-line function that is a pure delegation — trivial
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)
  - **Blocks**: Tasks 5, 9
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/abi/mod.rs` — find `contract_id()` function for style reference; place `bundle_id()` after `extension_id()`, same pattern
  - `crates/polyplug/src/abi/mod.rs` — `fnv1a_64()` is the function being delegated to

  **Acceptance Criteria**:
  - [ ] `abi/mod.rs` exports `pub fn bundle_id(name: &str) -> u64`
  - [ ] `bundle_id("myapp") == fnv1a_64(b"myapp")` (same hash)
  - [ ] `cargo build -p polyplug` exits 0

  **QA Scenarios**:
  ```
  Scenario: bundle_id delegates to fnv1a_64
    Tool: Bash
    Preconditions: Task 2 complete
    Steps:
      1. Run: cargo test -p polyplug -- bundle_id 2>&1
      2. Assert: exit code 0 OR no test exists yet (function just must compile)
      3. Run: cargo build -p polyplug 2>&1 | grep -c error
      4. Assert: output is "0"
    Expected Result: function compiles without errors
    Failure Indicators: "error[E" in build output
    Evidence: .sisyphus/evidence/task-2-bundle-id-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-2-bundle-id-build.txt — cargo build output

  **Commit**: YES (group with Tasks 1, 3, 4)

- [x] 3. Add `UndeclaredDependency`, `DependencyNotFound`, `BundleNotFound` variants to `RuntimeError`

  **What to do**:
  - Open `crates/polyplug/src/error/mod.rs`
  - Locate `RuntimeError` enum (currently aliased as `PolyplugError`)
  - Add these three variants to `RuntimeError`, after the existing variants:
    ```rust
    #[error("undeclared dependency: bundle_id={bundle_id:#x} attempted to resolve contract_id={contract_id:#x} without declaring it")]
    UndeclaredDependency { bundle_id: u64, contract_id: u64 },

    #[error("dependency not found: contract={contract_name} min_version={min_version}")]
    DependencyNotFound { contract_name: String, min_version: u32 },

    #[error("bundle not found for contract: bundle={bundle_name} contract={contract_name}")]
    BundleNotFound { bundle_name: String, contract_name: String },
    ```
  - Use `thiserror::Error` derive — already present on the enum, no new imports needed
  - All fields use owned types (`u64`, `String`, `u32`) — no references, no lifetimes

  **Must NOT do**:
  - Do not modify existing variants (`ContractIdCollision`, `DuplicateProvider`, `PluginNotFound`)
  - Do not modify `RegistryError::StaleHandle { index, expected, found }` — leave it unchanged
  - Do not add a second `StaleHandle` variant at the `RuntimeError` level
  - Do not change the `PolyplugError = RuntimeError` type alias

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Adding enum variants with thiserror derive — mechanical, no logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)
  - **Blocks**: Tasks 6, 7
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/error/mod.rs:1-138` — existing enum structure and thiserror patterns to follow
  - `crates/polyplug/src/error/mod.rs` — `RegistryError::StaleHandle` is in a DIFFERENT enum; do not touch it

  **Acceptance Criteria**:
  - [ ] `RuntimeError::UndeclaredDependency { bundle_id: u64, contract_id: u64 }` exists
  - [ ] `RuntimeError::DependencyNotFound { contract_name: String, min_version: u32 }` exists
  - [ ] `RuntimeError::BundleNotFound { bundle_name: String, contract_name: String }` exists
  - [ ] `cargo build -p polyplug` exits 0

  **QA Scenarios**:
  ```
  Scenario: new error variants compile and format correctly
    Tool: Bash
    Preconditions: Task 3 complete
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: grep -n "UndeclaredDependency\|DependencyNotFound\|BundleNotFound" crates/polyplug/src/error/mod.rs
      4. Assert: exactly 3 matches found
    Expected Result: All 3 variants present, build clean
    Failure Indicators: build error or fewer than 3 grep matches
    Evidence: .sisyphus/evidence/task-3-error-variants-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-3-error-variants-build.txt — cargo build + grep output

  **Commit**: YES (group with Tasks 1, 2, 4)

- [x] 4. Add `VTableSlot` newtype and `PluginVTableGuard` to `crates/polyplug/src/registry/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/registry/mod.rs`
  - At the top of the file, AFTER the existing `use` statements and BEFORE the first struct/type definition, add:
    ```rust
    /// A `Send + Sync` wrapper around a raw vtable pointer.
    /// The pointer is guaranteed to point to `'static` data that is never mutated after registration.
    pub struct VTableSlot(pub *const PluginVTable);

    // SAFETY: *const PluginVTable points to 'static plugin data. Once registered, the data is never
    // mutated. The pointer remains valid for the lifetime of the loaded library. Aliasing is safe
    // because all access is read-only through PluginVTableGuard.
    unsafe impl Send for VTableSlot {}
    // SAFETY: Same reasoning as Send above — read-only access to 'static data.
    unsafe impl Sync for VTableSlot {}

    /// An Arc-backed guard that keeps a vtable slot alive.
    /// This is Rust-only and never crosses the C ABI boundary.
    /// Intentionally NOT Send: the guard must be used on the same thread that called
    /// resolve_guard(), or re-resolved per-call from a new thread.
    pub struct PluginVTableGuard {
        pub(crate) slot: Arc<VTableSlot>,
        /// Opt-out of Send. Cell<()> is !Send, so PluginVTableGuard becomes !Send automatically.
        _not_send: std::marker::PhantomData<std::cell::Cell<()>>,
    }

    impl PluginVTableGuard {
        /// Construct a new guard wrapping the given slot.
        pub(crate) fn new(slot: Arc<VTableSlot>) -> Self {
            Self { slot, _not_send: std::marker::PhantomData }
        }
        /// Returns the raw vtable pointer. The pointer is valid as long as this guard is alive.
        pub fn vtable(&self) -> *const PluginVTable {
            self.slot.0
        }
    }
    ```
  - `Arc` is `std::sync::Arc` — ensure it is imported at file top (add to existing `use std::sync::...` if needed)
  - `PluginVTable` is imported from `crate::abi` — must already be in scope (check existing imports)

  **Must NOT do**:
  - Do not place `VTableSlot` or `PluginVTableGuard` in `abi/mod.rs` — they belong ONLY in `registry/mod.rs`
  - Do not add `use` inside the struct/impl bodies
  - Do not add `unsafe impl Send for PluginVTableGuard` — the `PhantomData<Cell<()>>` field already makes it `!Send`. Adding an unsafe Send impl would override this and violate the thread-safety contract.
  - Do not omit the `// SAFETY:` comment on both `unsafe impl` blocks

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Defining two small structs with clear specs — no complex logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)
  - **Blocks**: Task 6
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/registry/mod.rs:1-50` — existing use statements and struct layout to match style
  - `crates/polyplug/src/abi/mod.rs` — `PluginVTable` struct definition (understand what we're wrapping)

  - [ ] `VTableSlot` struct exists in `registry/mod.rs` with `pub` field `*const PluginVTable`
  - [ ] `unsafe impl Send for VTableSlot` and `unsafe impl Sync for VTableSlot` present with `// SAFETY:` comments
  - [ ] `PluginVTableGuard` struct has `slot: Arc<VTableSlot>` and `_not_send: PhantomData<Cell<()>>` fields
  - [ ] `PluginVTableGuard::vtable()` method returns `*const PluginVTable`
  - [ ] `PluginVTableGuard` does NOT implement `Send` (enforced via `PhantomData<Cell<()>>`)
  - [ ] `cargo build -p polyplug` exits 0

  **QA Scenarios**:
  ```
  Scenario: VTableSlot and guard compile cleanly
    Tool: Bash
    Preconditions: Task 4 complete
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: grep -n "VTableSlot\|PluginVTableGuard" crates/polyplug/src/registry/mod.rs
      4. Assert: at least 4 lines match (struct defs + unsafe impls)
    Expected Result: Both types compile, unsafe impls present
    Failure Indicators: build error or fewer than 4 grep matches
    Evidence: .sisyphus/evidence/task-4-vtable-slot-build.txt

  Scenario: PluginVTableGuard is !Send via PhantomData
    Tool: Bash
    Preconditions: Task 4 complete
    Steps:
      1. Run: grep -n '_not_send.*PhantomData\|PhantomData.*Cell' crates/polyplug/src/registry/mod.rs
      2. Assert: at least 1 match (PhantomData<Cell<()>> present in struct)
      3. Run: grep -n 'unsafe impl Send for PluginVTableGuard' crates/polyplug/src/registry/mod.rs
      4. Assert: zero lines match (no manual Send impl that would override PhantomData's !Send)
      5. Run: cargo build -p polyplug 2>&1
      6. Assert: exit code 0
    Expected Result: PhantomData<Cell<()>> present; no Send override; builds cleanly
    Failure Indicators: PhantomData missing, or a Send impl found, or build error
    Evidence: .sisyphus/evidence/task-4-guard-not-send.txt
  ```

  **Evidence to Capture**:
  - [ ] task-4-vtable-slot-build.txt
  - [ ] task-4-guard-not-send.txt

  **Commit**: YES (group with Tasks 1, 2, 3)

- [x] 5. Rewrite `HostVTable` in `crates/polyplug/src/abi/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/abi/mod.rs`
  - Replace the existing `HostVTable` struct (currently 5 fn ptrs, 40 bytes) with the new 7-fn-ptr layout:
    ```rust
    #[repr(C)]
    pub struct HostVTable {
        pub alloc:                  unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
        pub free:                   unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
        pub find_by_contract:       unsafe extern "C" fn(contract_id: u64, min_version: u32) -> PluginHandle,
        pub find_by_bundle:         unsafe extern "C" fn(bundle_id: u64, contract_id: u64, min_version: u32) -> PluginHandle,
        pub find_all_by_contract:   unsafe extern "C" fn(contract_id: u64, min_version: u32, out: *mut PluginHandle, out_cap: usize) -> usize,
        pub resolve_plugin:         unsafe extern "C" fn(handle: PluginHandle) -> *const PluginVTable,
        pub get_extension:          unsafe extern "C" fn(extension_id: u32) -> *const (),
    }
    ```
  - Field order is EXACTLY as above. Do not reorder.
  - `PluginHandle` and `PluginVTable` are already defined in this file — no new imports needed
  - Remove fields: `find_plugin`, `call_plugin`
  - Keep fields: `alloc`, `free`, `get_extension` (signatures unchanged)
  - **Update the layout test** at the bottom of the file:
    - Change `assert_eq!(std::mem::size_of::<HostVTable>(), 40)` to `assert_eq!(std::mem::size_of::<HostVTable>(), 56)`
    - 7 fn ptrs × 8 bytes each = 56 bytes on 64-bit targets
  - Add a field-offset test for each new fn ptr to match the style of existing offset tests

  **Must NOT do**:
  - Do not change `PluginHandle { index: u32, generation: u32 }` struct
  - Do not change `PluginHandle::NULL` sentinel `{ index: u32::MAX, generation: 0 }`
  - Do not change `PluginVTable` struct
  - Do not change `alloc`, `free`, `get_extension` signatures
  - Do not leave the old layout test (40 bytes) — it must become 56
  - Do not add `find_plugin` or `call_plugin` in any form

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Struct rewrite with exact field-by-field specification and layout test update — requires careful attention to order and test arithmetic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 9, which only touches manifest)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 6, 7, 8, 19, 20, 21, 22, 23, 24, 25, 27, 32
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug/src/abi/mod.rs:1-474` — full file; find `HostVTable` struct and existing layout tests at bottom
  - `crates/polyplug/src/abi/mod.rs` line ~416 — existing `assert_eq!(size_of::<HostVTable>(), 40)` — must become 56
  - `crates/polyplug/src/abi/mod.rs` — `PluginHandle` null sentinel pattern for understanding the ABI context
  - `polyplug_prd.md §6` — canonical ABI spec for `find_all_by_contract` (caller-provides-buffer)

  **Acceptance Criteria**:
  - [ ] `HostVTable` has exactly 7 fields in the order: `alloc`, `free`, `find_by_contract`, `find_by_bundle`, `find_all_by_contract`, `resolve_plugin`, `get_extension`
  - [ ] Layout test asserts `size_of::<HostVTable>() == 56`
  - [ ] `cargo build -p polyplug` exits 0
  - [ ] `grep -n "find_plugin\|call_plugin" crates/polyplug/src/abi/mod.rs` returns zero results

  **QA Scenarios**:
  ```
  Scenario: HostVTable is exactly 56 bytes
    Tool: Bash
    Preconditions: Task 5 complete
    Steps:
      1. Run: cargo test -p polyplug -- layout 2>&1
      2. Assert: exit code 0
      3. Assert: test output contains "test abi::tests::test_host_vtable_layout ... ok" (or similar layout test name)
    Expected Result: Layout test passes confirming 56-byte size
    Failure Indicators: layout test failure with actual/expected size mismatch
    Evidence: .sisyphus/evidence/task-5-layout-test.txt

  Scenario: old find_plugin and call_plugin absent from abi module
    Tool: Bash
    Preconditions: Task 5 complete
    Steps:
      1. Run: grep -n "find_plugin\|call_plugin" crates/polyplug/src/abi/mod.rs 2>&1
      2. Assert: zero lines output
    Expected Result: No references to removed fields
    Failure Indicators: any grep match
    Evidence: .sisyphus/evidence/task-5-absence-check.txt
  ```

  **Evidence to Capture**:
  - [ ] task-5-layout-test.txt
  - [ ] task-5-absence-check.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 6. Rewrite `Registry` in `crates/polyplug/src/registry/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/registry/mod.rs`
  - Add `arc_swap` to imports at file top: `use arc_swap::ArcSwap;`
  - Add `std::collections::HashSet` to imports at file top
  - **Modify `RegistryEntry`** to add `bundle_id: u64` field:
    ```rust
    pub(crate) struct RegistryEntry {
        pub descriptor:    PluginDescriptor,
        pub contract_name: String,
        pub bundle_id:     u64,
    }
    ```
    Remove the `vtable: *const PluginVTable` field from `RegistryEntry` — the vtable moves to `RegistrySlot`
  - **Modify `RegistrySlot`** to use `ArcSwap<VTableSlot>` for the vtable:
    ```rust
    pub(crate) struct RegistrySlot {
        pub generation: u32,
        pub entry:      Option<RegistryEntry>,
        pub vtable:     Option<ArcSwap<VTableSlot>>,
    }
    ```
    The `vtable` field is `Option<ArcSwap<VTableSlot>>` — `None` for empty slots, `Some(...)` when occupied
  - **Modify `Registry` struct**:
    ```rust
    pub struct Registry {
        loaded_libraries: Mutex<Vec<Library>>,
        slots:            RwLock<Vec<RegistrySlot>>,
        contract_index:   RwLock<HashMap<u64, Vec<u32>>>,   // contract_id -> Vec<slot_index>
        bundle_index:     RwLock<HashMap<u64, u32>>,         // bundle_id -> slot_index (first slot for that bundle)
        declared_deps:    RwLock<HashMap<u64, HashSet<u64>>>, // bundle_id -> set of declared contract_ids
    }
    ```
  - **Update `Registry::new()`** to initialize all fields (empty vecs/maps)
  - **Update `Registry::register()`** method signature to add `bundle_id: u64` parameter:
    ```rust
    pub fn register(
        &self,
        descriptor: PluginDescriptor,
        vtable_ptr: *const PluginVTable,
        contract_name: String,
        bundle_id: u64,
    ) -> Result<PluginHandle, RegistryError>
    ```
    Inside `register()`:
    - Check `contract_index` for existing entries with same `contract_id`
    - For each existing slot index, check if its `bundle_id` matches — if YES, return `Err(RegistryError::DuplicateProvider)`
    - If no same-bundle collision, push a new slot, push slot index into `contract_index[contract_id]`, insert into `bundle_index[bundle_id]` (first slot wins for `find_by_bundle`)
    - Store `ArcSwap::new(Arc::new(VTableSlot(vtable_ptr)))` as the slot's vtable
  - **Add `Registry::declare_deps()`** method:
    ```rust
    pub fn declare_deps(&self, bundle_id: u64, contract_ids: Vec<u64>) -> Result<(), RegistryError>
    ```
    Inserts `contract_ids` into `declared_deps[bundle_id]`.
  - **Add `Registry::is_dependency_declared()`** method (used by runtime callbacks):
    ```rust
    pub(crate) fn is_dependency_declared(&self, bundle_id: u64, contract_id: u64) -> bool {
        match self.declared_deps.read() {
            Ok(guard) => guard.get(&bundle_id).map_or(false, |s| s.contains(&contract_id)),
            Err(_) => false, // poisoned lock — fail safe (deny access)
        }
    }
    ```
    This method is `pub(crate)` so `runtime/mod.rs` can call it without accessing the private `declared_deps` field directly.
  - **Add `Registry::find_by_contract()`** method:
    ```rust
    pub fn find_by_contract(&self, contract_id: u64, min_version: u32) -> Result<PluginHandle, RegistryError>
    ```
    - Lock `contract_index` read
    - Get `Vec<u32>` for `contract_id`, return `PluginNotFound` if absent
    - Iterate slot indices, find first slot where `descriptor.version >= min_version`
    - Return `PluginHandle { index: slot_index, generation: slot.generation }`
  - **Add `Registry::find_by_bundle()`** method:
    ```rust
    pub fn find_by_bundle(&self, bundle_id: u64, contract_id: u64, min_version: u32) -> Result<PluginHandle, RegistryError>
    ```
    - Lock `bundle_index` read, get slot index for `bundle_id`
    - Verify slot's `entry.bundle_id == bundle_id` AND `entry.descriptor.contract_id == contract_id` AND version >= min_version
    - Return handle or `BundleNotFound`
  - **Add `Registry::find_all_by_contract()`** method:
    ```rust
    pub fn find_all_by_contract(&self, contract_id: u64, min_version: u32) -> Vec<PluginHandle>
    ```
    - Returns ALL matching handles (version >= min_version), in registration order
  - **Add `Registry::resolve_guard()`** method:**
    ```rust
    pub fn resolve_guard(&self, handle: PluginHandle) -> Result<PluginVTableGuard, RegistryError>
    ```
    - Check slot at `handle.index`, verify `generation == handle.generation` (stale check via existing `RegistryError::StaleHandle`)
    - Load `Arc<VTableSlot>` from `ArcSwap` via `.load_full()` (returns `Arc<VTableSlot>`)
    - Return `PluginVTableGuard(arc)`
  - **Keep `Registry::find()` and `Registry::resolve()`** for backward compat with existing internal callers; update them to delegate to new methods where possible
    - `find()` can delegate to `find_by_contract()` using first result
    - `resolve()` can call `resolve_guard()` and return `guard.vtable()`

  **Must NOT do**:
  - Do not use `.unwrap()` anywhere — use `?` or explicit match
  - Do not use `use` inside method bodies
  - Do not omit `// SAFETY:` on any unsafe blocks
  - Do not break the `push_library()` method — keep it as-is
  - Do not remove `RegistryError::StaleHandle` — it is used in `resolve_guard()`

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Complex struct redesign with multiple interacting data structures (ArcSwap, multi-HashMap, generational handles) — requires careful reasoning about concurrency invariants
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 1, 3, 4, 5 all completing first)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 7, 8, 19, 26, 27, 28
  - **Blocked By**: Tasks 1, 3, 4, 5

  **References**:
  - `crates/polyplug/src/registry/mod.rs:1-390` — full existing implementation; understand all current methods before modifying
  - `crates/polyplug/src/abi/mod.rs` — `PluginDescriptor`, `PluginHandle`, `PluginVTable` types
  - `crates/polyplug/src/error/mod.rs` — `RegistryError` variants for error returns
  - arc-swap docs: https://docs.rs/arc-swap/1.7.1/arc_swap/struct.ArcSwap.html — `load_full()` returns `Arc<T>`

  **Acceptance Criteria**:
  - [ ] `Registry` struct has `contract_index: RwLock<HashMap<u64, Vec<u32>>>`, `bundle_index: RwLock<HashMap<u64, u32>>`, `declared_deps: RwLock<HashMap<u64, HashSet<u64>>>`
  - [ ] `RegistrySlot::vtable` is `Option<ArcSwap<VTableSlot>>`
  - [ ] `register()` accepts `bundle_id: u64` parameter
  - [ ] `DuplicateProvider` returned only on same bundle_id + same contract_id collision
  - [ ] `find_by_contract()`, `find_by_bundle()`, `find_all_by_contract()`, `resolve_guard()`, `declare_deps()`, `is_dependency_declared()` all present
  - [ ] `cargo build -p polyplug` exits 0
  - [ ] No `.unwrap()` in registry/mod.rs

  **QA Scenarios**:
  ```
  Scenario: multi-impl allows two plugins for same contract from different bundles
    Tool: Bash
    Preconditions: Task 6 complete, unit test written in registry/mod.rs #[cfg(test)] block
    Steps:
      1. In test: register plugin A (contract_id=1, bundle_id=100), then register plugin B (contract_id=1, bundle_id=200)
      2. Assert: both register() calls return Ok(handle)
      3. Call find_all_by_contract(1, 0), assert: returns Vec of length 2
      4. Run: cargo test -p polyplug -- registry 2>&1
      5. Assert: all registry tests pass
    Expected Result: Two handles returned for same contract from different bundles
    Failure Indicators: second register() returns Err(DuplicateProvider), or find_all returns len 1
    Evidence: .sisyphus/evidence/task-6-multi-impl-test.txt

  Scenario: DuplicateProvider on same bundle_id + contract_id
    Tool: Bash
    Preconditions: Task 6 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: register plugin A (contract_id=1, bundle_id=100)
      2. Register plugin A again (same contract_id=1, same bundle_id=100)
      3. Assert: second register() returns Err(RegistryError::DuplicateProvider)
      4. Run: cargo test -p polyplug -- registry_duplicate 2>&1
      5. Assert: test passes
    Expected Result: Duplicate rejected only when bundle_id matches
    Failure Indicators: returns Ok(handle) on second registration
    Evidence: .sisyphus/evidence/task-6-duplicate-provider-test.txt
  ```

  **Evidence to Capture**:
  - [ ] task-6-multi-impl-test.txt
  - [ ] task-6-duplicate-provider-test.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 7. Rewrite runtime callbacks in `crates/polyplug/src/runtime/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/runtime/mod.rs`
  - Add `INIT_BUNDLE_ID` thread-local at file top (after `use` statements, before any fn/struct):
    ```rust
    thread_local! {
        pub(crate) static INIT_BUNDLE_ID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }
    ```
  - Remove the existing `host_find_plugin` and `host_call_plugin` unsafe extern "C" functions entirely
  - Add four new callback functions:

    ```rust
    pub(crate) unsafe extern "C" fn host_find_by_contract(
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle {
        let registry: &Registry = /* get from GLOBAL_REGISTRY */;
        let caller_bundle_id: u64 = INIT_BUNDLE_ID.with(|c| c.get());
        if caller_bundle_id != 0 {
            if !registry.is_dependency_declared(caller_bundle_id, contract_id) {
                // undeclared dependency — return null handle (error logging is acceptable here)
        }
        match registry.find_by_contract(contract_id, min_version) {
            Ok(h) => h,
            Err(_) => PluginHandle::NULL,
        }
    }
    ```
    - IMPORTANT: In `unsafe extern "C"` functions, `?` cannot be used. Errors must be handled with match/if-let returning `PluginHandle::NULL` or 0/null.

    ```rust
    pub(crate) unsafe extern "C" fn host_find_by_bundle(
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle {
        // Same dependency enforcement pattern as host_find_by_contract
        // Then: registry.find_by_bundle(bundle_id, contract_id, min_version).unwrap_or(PluginHandle::NULL)
        // NOTE: use match not .unwrap_or — .unwrap_or with fallback value is OK here since PluginHandle::NULL is the fallback
        // Actually: use match to avoid .unwrap() style
    }
    ```

    ```rust
    pub(crate) unsafe extern "C" fn host_find_all_by_contract(
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize {
        // No dependency enforcement — enumeration is allowed freely
        // Get all handles from registry.find_all_by_contract(contract_id, min_version)
        // Copy min(handles.len(), out_cap) handles into the out buffer
        // SAFETY: out is valid for out_cap elements (ABI contract: caller allocates)
        // Return the number of handles written
    }
    ```

    ```rust
    pub(crate) unsafe extern "C" fn host_resolve_plugin(
        handle: PluginHandle,
    ) -> *const PluginVTable {
        // Call registry.resolve_guard(handle)
        // On Ok: return guard.vtable() — note: guard drops here, but vtable ptr is still valid ('static data)
        // On Err: return std::ptr::null()
        // SAFETY: The returned pointer is valid as long as the plugin library is loaded.
        //         The host guarantees it does not unload libraries during active dispatch.
    }
    ```

  - Update `HostVTable` construction in `Runtime::new()` (or wherever the vtable is built) to use the new 7-field layout:
    ```rust
    let vtable: HostVTable = HostVTable {
        alloc:                  host_alloc,
        free:                   host_free,
        find_by_contract:       host_find_by_contract,
        find_by_bundle:         host_find_by_bundle,
        find_all_by_contract:   host_find_all_by_contract,
        resolve_plugin:         host_resolve_plugin,
        get_extension:          host_get_extension,
    };
    ```
  - Remove `Runtime::find_plugin()` and `Runtime::call_plugin()` public methods if they exist; add corresponding `find_by_contract()`, `find_by_bundle()`, `find_all_by_contract()`, `resolve_plugin()` Rust-safe wrappers that call the registry

  **Must NOT do**:
  - Do not use `.unwrap()` in any production code path (use match, map_or, or `if let`)
  - Do not use `.expect()` outside tests
  - Do not capture lock guards across await points (no async here, but still — drop guards promptly)
  - Do not enforce dependency check in `host_find_all_by_contract` — only in `host_find_by_contract` and `host_find_by_bundle`

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Implementing unsafe extern "C" callbacks with dependency enforcement logic, thread-local state, and error-to-null conversion — requires careful reasoning about lock scoping and ABI constraints
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 3, 5, 6)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 8, 10, 19, 26, 27, 28
  - **Blocked By**: Tasks 3, 5, 6

  **References**:
  - `crates/polyplug/src/runtime/mod.rs:1-355` — full existing file; understand GLOBAL_REGISTRY usage and existing callback pattern
  - `crates/polyplug/src/registry/mod.rs` — new method signatures from Task 6
  - `crates/polyplug/src/abi/mod.rs` — `HostVTable` new struct (from Task 5), `PluginHandle::NULL` sentinel
  - `crates/polyplug/src/error/mod.rs` — new error variants for logging/debug

  **Acceptance Criteria**:
  - [ ] `INIT_BUNDLE_ID` thread-local present in `runtime/mod.rs`
  - [ ] `host_find_by_contract`, `host_find_by_bundle`, `host_find_all_by_contract`, `host_resolve_plugin` all present as `unsafe extern "C"` fns
  - [ ] `host_find_plugin` and `host_call_plugin` absent from file
  - [ ] `HostVTable` construction uses 7 fields in correct order
  - [ ] Dependency enforcement: `find_by_contract`/`find_by_bundle` call `registry.is_dependency_declared(bundle_id, contract_id)` and return `PluginHandle::NULL` when `INIT_BUNDLE_ID != 0` and the call returns false
  - [ ] `cargo build -p polyplug` exits 0

  **QA Scenarios**:
  ```
  Scenario: dependency enforcement blocks undeclared access during init
    Tool: Bash
    Preconditions: Task 7 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: set INIT_BUNDLE_ID to 999 (simulating bundle 999 in init phase)
      2. Call registry.find_by_contract() for a contract not declared by bundle 999
      3. Assert: returns PluginHandle::NULL
      4. Set INIT_BUNDLE_ID back to 0
      5. Call registry.find_by_contract() for same contract
      6. Assert: returns Ok(handle) or PluginNotFound (no enforcement when INIT_BUNDLE_ID=0)
      7. Run: cargo test -p polyplug -- runtime 2>&1
    Expected Result: enforcement only active when INIT_BUNDLE_ID != 0
    Failure Indicators: blocking when INIT_BUNDLE_ID=0, or not blocking when INIT_BUNDLE_ID!=0
    Evidence: .sisyphus/evidence/task-7-dep-enforcement-test.txt

  Scenario: host_find_all_by_contract skips dependency check
    Tool: Bash
    Preconditions: Task 7 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: set INIT_BUNDLE_ID to 999 (no declared deps for bundle 999)
      2. Register two plugins for contract_id=42
      3. Call host_find_all_by_contract via the C callback (or Rust equivalent)
      4. Assert: both handles returned regardless of INIT_BUNDLE_ID state
    Expected Result: find_all bypasses dependency enforcement
    Failure Indicators: returns empty or 1 result instead of 2
    Evidence: .sisyphus/evidence/task-7-find-all-no-enforce.txt
  ```

  **Evidence to Capture**:
  - [ ] task-7-dep-enforcement-test.txt
  - [ ] task-7-find-all-no-enforce.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 8. Update `crates/polyplug/src/lib.rs` C exports

  **What to do**:
  - Open `crates/polyplug/src/lib.rs`
  - Remove the two existing `#[no_mangle]` C exports:
    - `polyplug_find_plugin`
    - `polyplug_call_plugin`
  - Add four new `#[no_mangle]` C exports:
    ```rust
    #[no_mangle]
    pub unsafe extern "C" fn polyplug_find_by_contract(
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle {
        // delegate to runtime callback: host_find_by_contract(contract_id, min_version)
        crate::runtime::host_find_by_contract(contract_id, min_version)
    }

    #[no_mangle]
    pub unsafe extern "C" fn polyplug_find_by_bundle(
        bundle_id: u64,
        contract_id: u64,
        min_version: u32,
    ) -> PluginHandle {
        crate::runtime::host_find_by_bundle(bundle_id, contract_id, min_version)
    }

    #[no_mangle]
    pub unsafe extern "C" fn polyplug_find_all_by_contract(
        contract_id: u64,
        min_version: u32,
        out: *mut PluginHandle,
        out_cap: usize,
    ) -> usize {
        crate::runtime::host_find_all_by_contract(contract_id, min_version, out, out_cap)
    }

    #[no_mangle]
    pub unsafe extern "C" fn polyplug_resolve_plugin(
        handle: PluginHandle,
    ) -> *const PluginVTable {
        crate::runtime::host_resolve_plugin(handle)
    }
    ```
  - Imports at file top: ensure `PluginHandle` and `PluginVTable` are imported from `crate::abi`
  - All four fns are `unsafe extern "C"` — these cross the C ABI boundary

  **Must NOT do**:
  - Do not keep `polyplug_find_plugin` or `polyplug_call_plugin` in any form
  - Do not add new logic in these wrapper functions — they delegate to runtime callbacks only
  - Do not use `.unwrap()` here

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: C ABI function wrappers requiring exact signature matching — mechanical but must be precise
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 5, 6, 7)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 19, 20, 26, 27, 28
  - **Blocked By**: Tasks 5, 6, 7

  **References**:
  - `crates/polyplug/src/lib.rs:1-121` — full existing file; follow existing export pattern
  - `crates/polyplug/src/runtime/mod.rs` — new callback function names from Task 7
  - `crates/polyplug/src/abi/mod.rs` — `PluginHandle`, `PluginVTable` type definitions

  **Acceptance Criteria**:
  - [ ] `polyplug_find_by_contract`, `polyplug_find_by_bundle`, `polyplug_find_all_by_contract`, `polyplug_resolve_plugin` all present with `#[no_mangle]`
  - [ ] `polyplug_find_plugin` and `polyplug_call_plugin` absent from file
  - [ ] `cargo build -p polyplug` exits 0
  - [ ] `grep -n "polyplug_find_plugin\|polyplug_call_plugin" crates/polyplug/src/lib.rs` returns zero results

  **QA Scenarios**:
  ```
  Scenario: new C exports compile and link correctly
    Tool: Bash
    Preconditions: Task 8 complete
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: nm target/debug/libpolyplug.so 2>/dev/null | grep -E "polyplug_find_by|polyplug_resolve" || cargo build -p polyplug 2>&1 | tail -5
      4. Assert: at minimum, build succeeds
    Expected Result: all 4 new exports compile cleanly
    Failure Indicators: linker errors or missing symbol errors
    Evidence: .sisyphus/evidence/task-8-c-exports-build.txt

  Scenario: old exports absent from binary
    Tool: Bash
    Preconditions: Task 8 complete
    Steps:
      1. Run: grep -n "polyplug_find_plugin\|polyplug_call_plugin" crates/polyplug/src/lib.rs
      2. Assert: zero lines output
    Expected Result: No references to removed exports
    Failure Indicators: any grep match
    Evidence: .sisyphus/evidence/task-8-old-exports-absent.txt
  ```

  **Evidence to Capture**:
  - [ ] task-8-c-exports-build.txt
  - [ ] task-8-old-exports-absent.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 9. Update `crates/polyplug/src/loader/manifest/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/loader/manifest/mod.rs`
  - The current `ManifestData` is `pub struct ManifestData { pub runtime: String }` with `#[derive(Deserialize)]` (23 lines total)
  - **KEEP** the `serde::Deserialize` derive — `ManifestData` is parsed from `manifest.toml` at runtime using `toml::from_str()`
  - Add a raw TOML-parseable `RawManifestDependency` struct for serde parsing:
    ```rust
    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct RawManifestDependency {
        pub kind:        String,         // "contract" or "bundle"
        pub contract:    String,         // contract name (will be hashed to contract_id by loader)
        pub min_version: String,         // e.g. "1.0"
        /// Only present for kind == "bundle"
        #[serde(default)]
        pub bundle:      Option<String>,
    }
    ```
  - Extend `ManifestData` to add `bundle_name`, `dependencies`, and computed `bundle_id`:
    ```rust
    #[derive(Debug, Clone, serde::Deserialize)]
    pub struct ManifestData {
        pub runtime:      String,
        /// Bundle name — used by the loader to compute bundle_id via abi::bundle_id()
        #[serde(default)]
        pub bundle_name:  String,
        /// Raw dependency declarations from [[dependency]] table in manifest.toml
        #[serde(default, rename = "dependency")]
        pub dependencies: Vec<RawManifestDependency>,
        /// Computed from bundle_name by the loader after parsing; NOT in the TOML
        #[serde(skip)]
        pub bundle_id:    u64,
    }
    ```
  - After `toml::from_str()` parses the manifest, the loader (Task 10) will compute:
    ```rust
    data.bundle_id = crate::abi::bundle_id(&data.bundle_name);
    ```
  - The TOML `manifest.toml` format for dependencies:
    ```toml
    bundle_name = "audio-engine"
    runtime = "native"

    [[dependency]]
    kind = "contract"
    contract = "audio-decoder"
    min_version = "1.0"
    ```

  **Must NOT do**:
  - Do not remove `#[derive(serde::Deserialize)]` from `ManifestData` — it IS parsed from TOML at runtime
  - Do not remove the `runtime: String` field
  - Do not add `bundle_id` to the TOML schema directly — it is computed from `bundle_name` after parsing
  - Do not use `use` inside any method body

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Adding two simple structs/enums with known field types
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 5, 6, 7, 8 — no shared dependencies)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 10, 11
  - **Blocked By**: Task 2 (needs `bundle_id` type, but `u64` is stdlib — effectively no blocker)

  **References**:
  - `crates/polyplug/src/loader/manifest/mod.rs:1-23` — full existing file (23 lines, add to it)
  - `crates/polyplug/src/abi/mod.rs` — `bundle_id()` fn reference for understanding what `bundle_id: u64` represents

  **Acceptance Criteria**:
  - [ ] `ManifestDependency` enum with `ByContract` and `ByBundle` variants present
  - [ ] `ManifestData` has `bundle_id: u64` and `dependencies: Vec<ManifestDependency>` fields
  - [ ] `cargo build -p polyplug` exits 0

  **QA Scenarios**:
  ```
  Scenario: ManifestData struct compiles with new fields
    Tool: Bash
    Preconditions: Task 9 complete
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: grep -n "ManifestDependency\|bundle_id\|dependencies" crates/polyplug/src/loader/manifest/mod.rs
      4. Assert: at least 4 lines match
    Expected Result: new types present and compile cleanly
    Failure Indicators: build error or grep finds fewer than 4 matches
    Evidence: .sisyphus/evidence/task-9-manifest-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-9-manifest-build.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 10. Update `crates/polyplug/src/loader/mod.rs` — `INIT_BUNDLE_ID`, `registrar_callback`, `bundle_id` computation

  **What to do**:
  - Open `crates/polyplug/src/loader/mod.rs` (350 lines)
  - **Part A — Compute `bundle_id` after manifest parse**:
    In `load_bundle()` (or `parse_manifest()`), after parsing `ManifestData`, compute and store the bundle_id:
    ```rust
    let mut manifest: ManifestData = parse_manifest(path)?;
    // Compute bundle_id from bundle_name field in manifest
    manifest.bundle_id = crate::abi::bundle_id(&manifest.bundle_name);
    ```
    If `bundle_name` is empty (bundle didn't set it in manifest.toml), `bundle_id` defaults to 0 (anonymous).
  - **Part B — `declare_deps` before init**:
    After computing `bundle_id`, resolve manifest dependencies to contract_ids and call:
    ```rust
    let contract_ids: Vec<u64> = manifest.dependencies.iter().map(|dep| {
        crate::abi::contract_id(&dep.contract)
    }).collect::<Vec<u64>>();
    registry.declare_deps(manifest.bundle_id, contract_ids)?;
    ```
  - **Part C — Thread-local INIT_BUNDLE_ID with RAII guard**:
    Add import at file top: `use crate::runtime::INIT_BUNDLE_ID;`
    In `load_bundle()`, before calling `init_fn_ptr`:
    ```rust
    struct BundleInitGuard;
    impl Drop for BundleInitGuard {
        fn drop(&mut self) {
            INIT_BUNDLE_ID.with(|c| c.set(0));
        }
    }
    INIT_BUNDLE_ID.with(|c| c.set(manifest.bundle_id));
    let _bundle_guard: BundleInitGuard = BundleInitGuard;
    // call polyplug_init here — _bundle_guard clears INIT_BUNDLE_ID on drop even on panic
    ```
  - **Part D — Implement `registrar_callback` (currently a TODO stub)**:
    The current `registrar_callback` returns `AbiError::ok()` without registering anything.
    The code comments already identify the solution: use thread-local for state passing.
    Add a new thread-local for registrar state:
    ```rust
    use std::cell::Cell;
    thread_local! {
        static REGISTRAR_BUNDLE_ID: Cell<u64> = const { Cell::new(0) };
        static REGISTRAR_REGISTRY_PTR: Cell<*const Registry> = const { Cell::new(std::ptr::null()) };
    }
    ```
    Before calling `init_fn_ptr`, set both thread-locals:
    ```rust
    REGISTRAR_REGISTRY_PTR.with(|c| c.set(registry as *const Registry));
    REGISTRAR_BUNDLE_ID.with(|c| c.set(manifest.bundle_id));
    ```
    After `init_fn_ptr` returns, clear both:
    ```rust
    REGISTRAR_REGISTRY_PTR.with(|c| c.set(std::ptr::null()));
    REGISTRAR_BUNDLE_ID.with(|c| c.set(0));
    ```
    Implement `registrar_callback` to use these thread-locals:
    ```rust
    extern "C" fn registrar_callback(
        _registrar: *mut PluginRegistrar,
        descriptor: *const PluginDescriptor,
        vtable: *const PluginVTable,
    ) -> AbiError {
        let registry_ptr: *const Registry = REGISTRAR_REGISTRY_PTR.with(|c| c.get());
        let bundle_id: u64 = REGISTRAR_BUNDLE_ID.with(|c| c.get());
        if registry_ptr.is_null() {
            return AbiError::from_code(1); // no registry context — should never happen
        }
        // SAFETY: registry_ptr was set to a valid &Registry reference immediately before
        // polyplug_init was called on this thread. The Registry outlives this call.
        // This callback is only ever called synchronously during init, never after.
        let registry: &Registry = unsafe { &*registry_ptr };
        let desc: PluginDescriptor = unsafe { *descriptor };
        let contract_name: String = /* derive from descriptor.contract_id or pass empty for now */
            format!("contract_{:#x}", desc.contract_id);
        match registry.register(desc, vtable, contract_name, bundle_id) {
            Ok(_) => AbiError::ok(),
            Err(_) => AbiError::from_code(1),
        }
    }
    ```
    - NOTE: `// SAFETY:` comment is REQUIRED on the unsafe block inside registrar_callback
    - NOTE: `AbiError::from_code(1)` is the failure sentinel — verify the actual error code constant exists (it may be `AbiError { code: 1, message: StringView::null() }` constructed manually)

  **Must NOT do**:
  - Do not leave `registrar_callback` as a TODO stub — it MUST be implemented
  - Do not leave `INIT_BUNDLE_ID` set to non-zero after the init call returns (RAII guard handles this)
  - Do not use `.unwrap()` in production code paths
  - Do not skip `declare_deps()` — deps must be declared before INIT_BUNDLE_ID is set
  - Do not add `use` statements inside function bodies

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Implementing thread-local registrar state, RAII guard pattern, and completing a TODO callback across an FFI boundary — requires careful reasoning about ownership and call order
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 6, 7, and 9)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 26 (existing integration tests load bundles via this path)
  - **Blocked By**: Tasks 6, 7, 9

  **References**:
  - `crates/polyplug/src/loader/mod.rs:1-350` — full file; find `registrar_callback` (line ~328), `RegistrarState` struct, and `load_bundle()` call to `init_fn_ptr`
  - `crates/polyplug/src/loader/mod.rs` — existing `RegistrarState` struct and comments about thread-local approach (lines 318-337)
  - `crates/polyplug/src/registry/mod.rs` — `register()` method signature with `bundle_id` param (from Task 6)
  - `crates/polyplug/src/runtime/mod.rs` — `INIT_BUNDLE_ID` thread-local (from Task 7) — can share the same guard or have two separate guards
  - `crates/polyplug/src/abi/mod.rs` — `PluginDescriptor`, `PluginRegistrar`, `AbiError`, `contract_id()`, `bundle_id()`
  - `crates/polyplug/src/loader/manifest/mod.rs` — `ManifestData` with `bundle_name` and `bundle_id` fields from Task 9

  **Acceptance Criteria**:
  - [ ] `manifest.bundle_id` is computed from `manifest.bundle_name` via `abi::bundle_id()` in loader
  - [ ] `registry.declare_deps()` called with manifest dependency contract_ids before init
  - [ ] `INIT_BUNDLE_ID` set before and cleared after `polyplug_init` call (via RAII)
  - [ ] `registrar_callback` no longer returns `AbiError::ok()` as stub; it calls `registry.register()`
  - [ ] All `unsafe` blocks in `registrar_callback` have `// SAFETY:` comments
  - [ ] `cargo build -p polyplug` exits 0
  - [ ] `cargo test --workspace` exits 0 (existing integration tests pass)

  **QA Scenarios**:
  ```
  Scenario: INIT_BUNDLE_ID is cleared after bundle load completes
    Tool: Bash
    Preconditions: Task 10 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: after calling load(), check INIT_BUNDLE_ID via INIT_BUNDLE_ID.with(|c| c.get())
      2. Assert: value is 0 after load() returns (whether success or error)
      3. Run: cargo test -p polyplug -- loader 2>&1
      4. Assert: all loader tests pass
    Expected Result: thread-local reset to 0 after init
    Failure Indicators: value remains non-zero after load()
    Evidence: .sisyphus/evidence/task-10-init-bundle-id-cleared.txt

  Scenario: registrar_callback registers vtables into Registry
    Tool: Bash
    Preconditions: Task 10 complete, integration test using actual .so fixture
    Steps:
      1. Run: cargo test --workspace -- integration_load 2>&1
      2. Assert: exit code 0
      3. Assert: integration_load tests show 'ok' (they exercise bundle loading + registration)
    Expected Result: vtables are registered during polyplug_init; registry is non-empty after load
    Failure Indicators: integration_load tests fail or no plugins found after load
    Evidence: .sisyphus/evidence/task-10-registrar-callback-test.txt
  ```

  **Evidence to Capture**:
  - [ ] task-10-init-bundle-id-cleared.txt
  - [ ] task-10-registrar-callback-test.txt

  **Commit**: YES (Wave 1-2 group)

- [x] 11. Update `crates/polyplugc/src/parser/mod.rs` — `[[dependency]]` table, remove `requires`

  **What to do**:
  - Open `crates/polyplugc/src/parser/mod.rs`
  - Remove `requires: Vec<String>` field from `RawPlugin` struct
  - Remove any code that parses `requires` from `[[plugin]]` table entries
  - Add a `RawDependency` struct for parsing `[[dependency]]` table entries:
    ```rust
    #[derive(Debug, serde::Deserialize)]
    pub(crate) struct RawDependency {
        /// Either 'contract' or 'bundle' depending on resolution strategy
        pub kind:        String,         // "contract" or "bundle"
        pub contract:    String,         // contract name
        pub min_version: String,         // e.g. "1.0"
        /// Optional: only for kind == "bundle"
        pub bundle:      Option<String>,
    }
    ```
  - Add `dependencies: Vec<RawDependency>` field to `RawBundleSchema` struct
  - Update `lower_bundle()` function (which produces `ResolvedBundle`) to:
    - Pass `dependencies` through to the IR (Task 12 will handle full lowering)
    - Remove `requires` from `ResolvedPlugin` construction
  - The TOML format for a dependency table entry:
    ```toml
    [[dependency]]
    kind = "contract"
    contract = "audio-decoder"
    min_version = "1.0"

    [[dependency]]
    kind = "bundle"
    bundle = "audio-engine"
    contract = "audio-decoder"
    min_version = "1.0"
    ```

  **Must NOT do**:
  - Do not remove `[[plugin]]` tables or any other `RawPlugin` fields except `requires`
  - Do not break existing valid bundle.toml files (dependencies are optional — empty Vec if absent)
  - Do not use `serde` `#[serde(rename)]` unless it already exists in the file style

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: TOML parser struct modification with serde derives — must match existing patterns exactly
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 9 for `ManifestDependency` understanding)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 12
  - **Blocked By**: Task 9

  **References**:
  - `crates/polyplugc/src/parser/mod.rs:1-302` — full existing parser; find `RawBundleSchema`, `RawPlugin`, `lower_bundle()`
  - `crates/polyplug/src/loader/manifest/mod.rs` — `ManifestDependency` enum (the runtime representation being targeted)

  **Acceptance Criteria**:
  - [ ] `RawPlugin` has no `requires` field
  - [ ] `RawDependency` struct exists with `kind`, `contract`, `min_version`, `bundle: Option<String>`
  - [ ] `RawBundleSchema` has `dependencies: Vec<RawDependency>`
  - [ ] `cargo build -p polyplugc` exits 0
  - [ ] `grep -n "requires" crates/polyplugc/src/parser/mod.rs | grep -v "//"` returns zero production code matches

  **QA Scenarios**:
  ```
  Scenario: bundle.toml with [[dependency]] entries parses correctly
    Tool: Bash
    Preconditions: Task 11 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: parse a TOML string with two [[dependency]] entries (one by-contract, one by-bundle)
      2. Assert: RawBundleSchema.dependencies.len() == 2
      3. Assert: first dependency has kind == "contract", contract == expected name
      4. Run: cargo test -p polyplugc -- parser 2>&1
      5. Assert: all parser tests pass
    Expected Result: dependency table parses into Vec<RawDependency>
    Failure Indicators: parse error, empty Vec, or wrong field values
    Evidence: .sisyphus/evidence/task-11-parser-deps-test.txt
  ```

  **Evidence to Capture**:
  - [ ] task-11-parser-deps-test.txt

  **Commit**: YES (Wave 3 group)

- [x] 12. Update `crates/polyplugc/src/ir/mod.rs` — `ResolvedDependency`, `ResolvedBundle`, `MY_BUNDLE_ID`

  **What to do**:
  - Open `crates/polyplugc/src/ir/mod.rs`
  - Remove `requires: Vec<String>` from `ResolvedPlugin` struct
  - Add `ResolvedDependency` enum:
    ```rust
    #[derive(Debug, Clone)]
    pub enum ResolvedDependency {
        ByContract {
            contract:    String,
            contract_id: u64,
            min_version: u32,
        },
        ByBundle {
            bundle:      String,
            bundle_id:   u64,
            contract:    String,
            contract_id: u64,
            min_version: u32,
        },
    }
    ```
  - Add `dependencies: Vec<ResolvedDependency>` and `bundle_id: u64` to `ResolvedBundle`:
    ```rust
    pub struct ResolvedBundle {
        pub name:         String,
        pub version:      String,
        pub bundle_id:    u64,
        pub plugins:      Vec<ResolvedPlugin>,
        pub dependencies: Vec<ResolvedDependency>,
    }
    ```
  - Update `lower_bundle()` (in `parser/mod.rs` or `ir/mod.rs`, wherever it lives) to:
    - Compute `bundle_id = polyplug::abi::bundle_id(&bundle.name)` — OR use `fnv1a_64(name.as_bytes())` directly if the polyplug crate isn't linked from polyplugc. The codegen tool has its own copy of the hash function.
    - Check: does `polyplugc` depend on `polyplug`? If yes, use `polyplug::abi::bundle_id()`. If no, duplicate the hash inline in `ir/mod.rs`:
      ```rust
      fn fnv1a_64_local(data: &[u8]) -> u64 {
          let mut hash: u64 = 14695981039346656037;
          for byte in data {
              hash ^= u64::from(*byte);
              hash = hash.wrapping_mul(1099511628211);
          }
          hash
      }
      pub fn bundle_id_from_name(name: &str) -> u64 { fnv1a_64_local(name.as_bytes()) }
      ```
    - Lower `RawDependency` to `ResolvedDependency` by resolving contract/bundle names to their IDs
    - Parse `min_version` string (e.g. "1.0") to `u32`: take major version only (`"1.0".split('.').next().parse::<u32>()`)
  - Add a `MY_BUNDLE_ID` constant emission in the bundle IR (codegen reads this per-bundle)
    - Store as `bundle_id: u64` on `ResolvedBundle` — already done above

  **Must NOT do**:
  - Do not change `ResolvedPlugin` in ways that break codegen (only remove `requires`)
  - Do not use string-based errors
  - Do not add `use` inside functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: IR lowering with hash computation and version parsing — moderate complexity
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 11)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 14, 15, 16
  - **Blocked By**: Task 11

  **References**:
  - `crates/polyplugc/src/ir/mod.rs:1-320` — full existing IR; understand `ResolvedPlugin`, `ResolvedBundle`, `lower_bundle()`
  - `crates/polyplug/src/abi/mod.rs` — `fnv1a_64()` implementation to duplicate/reference
  - `crates/polyplugc/Cargo.toml` — check if `polyplug` is a dependency

  **Acceptance Criteria**:
  - [ ] `ResolvedPlugin` has no `requires` field
  - [ ] `ResolvedDependency` enum exists with `ByContract` and `ByBundle` variants
  - [ ] `ResolvedBundle` has `bundle_id: u64` and `dependencies: Vec<ResolvedDependency>`
  - [ ] `lower_bundle()` correctly computes `bundle_id` from bundle name using FNV1a-64
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: lower_bundle computes correct bundle_id
    Tool: Bash
    Preconditions: Task 12 complete, unit test in #[cfg(test)] block
    Steps:
      1. In test: call lower_bundle() with bundle name "test-bundle"
      2. Assert: bundle_id == fnv1a_64(b"test-bundle") (hard-code expected value in test)
      3. Run: cargo test -p polyplugc -- ir 2>&1
      4. Assert: all IR tests pass
    Expected Result: bundle_id matches FNV1a-64 of bundle name
    Failure Indicators: incorrect hash value or test failure
    Evidence: .sisyphus/evidence/task-12-bundle-id-hash-test.txt
  ```

  **Evidence to Capture**:
  - [ ] task-12-bundle-id-hash-test.txt

  **Commit**: YES (Wave 3 group)

- [x] 13. Update `crates/polyplugc/src/generators/mod.rs` — register Python and Lua modules

  **What to do**:
  - Open `crates/polyplugc/src/generators/mod.rs`
  - Current content declares only `cpp`, `csharp`, `rust` submodules
  - Add:
    ```rust
    pub(crate) mod python;
    pub(crate) mod lua;
    ```
  - If `mod.rs` has a generator dispatch enum or match arm, do NOT add Python or Lua dispatch — the skeleton stubs (Tasks 17-18) return `Err` immediately and will be added to dispatch only when fully implemented (Epic 10/11)
  - If `mod.rs` has a `Language` enum, add `Python` and `Lua` variants that map to the stub generators

  **Must NOT do**:
  - Do not implement any codegen logic here
  - Do not add Python or Lua to any working dispatch path

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Adding two `mod` declarations — trivial
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 11, 12 — independent)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 17, 18
  - **Blocked By**: None (but do after Wave 2 completes for safety)

  **References**:
  - `crates/polyplugc/src/generators/mod.rs:1-46` — full existing file

  **Acceptance Criteria**:
  - [ ] `pub(crate) mod python;` present in `generators/mod.rs`
  - [ ] `pub(crate) mod lua;` present in `generators/mod.rs`
  - [ ] `cargo build -p polyplugc` exits 0 (after stubs are created in Tasks 17-18)

  **QA Scenarios**:
  ```
  Scenario: python and lua modules declared
    Tool: Bash
    Preconditions: Tasks 13, 17, 18 complete
    Steps:
      1. Run: cargo build -p polyplugc 2>&1
      2. Assert: exit code 0
      3. Run: grep -n "mod python\|mod lua" crates/polyplugc/src/generators/mod.rs
      4. Assert: exactly 2 matches
    Expected Result: both stub modules declared and compilable
    Failure Indicators: build error or missing mod declarations
    Evidence: .sisyphus/evidence/task-13-generators-mod-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-13-generators-mod-build.txt

  **Commit**: YES (Wave 3 group)

- [x] 14. Update `crates/polyplugc/src/generators/rust/mod.rs` — guard-based dispatch, new manifest format

  **What to do**:
  - Open `crates/polyplugc/src/generators/rust/mod.rs`
  - **Remove** emission of `requires = [...]` from generated `manifest.toml` (the old plugin-level requires)
  - **Add** emission of `[[dependency]]` table entries in the generated `manifest.toml`, sourced from `ResolvedBundle.dependencies`
  - **Add** emission of `MY_BUNDLE_ID: u64` constant in generated Rust code:
    ```rust
    pub const MY_BUNDLE_ID: u64 = {bundle_id_value};
    ```
  - **Add** emission of dependency contract ID constants, one per dependency:
    ```rust
    pub const DEP_{CONTRACT_UPPER}: u64 = {contract_id};
    pub const DEP_{CONTRACT_UPPER}_MIN_VERSION: u32 = {min_version};
    ```
  - **Update host callers** (generated code that calls other plugins): change from:
    ```rust
    self.runtime.call_plugin(handle, input)
    ```
    to guard-based dispatch:
    ```rust
    let vtable: *const PluginVTable = (self.vtable.resolve_plugin)(handle);
    // SAFETY: vtable is valid for the duration of this call. The host guarantees
    //         no library unloading during active dispatch.
    let result = unsafe { ((*vtable).call)(input_ptr, input_len, out_ptr, out_len) };
    ```
    Actually: the generated code does not manage the guard — it calls `resolve_plugin` from `HostVTable` and uses the raw `*const PluginVTable` immediately. No `PluginVTableGuard` in generated Rust code.
  - **Update host find calls** in generated code: change from:
    ```rust
    (self.vtable.find_plugin)(contract_id, 0)
    ```
    to:
    ```rust
    (self.vtable.find_by_contract)(contract_id, MIN_VERSION)
    ```

  **Must NOT do**:
  - Do not modify the generator in a way that breaks existing valid bundles (no-dep bundles must still generate correctly)
  - Do not add `use` inside generated function bodies
  - Do not emit `call_plugin` in generated code

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Templated code emission in a 1029-line generator — requires careful reading of all generation paths and template strings to update each consistently
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 15, 16 — all generators are independent)
  - **Parallel Group**: Wave 3
  - **Blocks**: None (consumed by integration tests in Wave 5)
  - **Blocked By**: Task 12

  **References**:
  - `crates/polyplugc/src/generators/rust/mod.rs:1-1029` — full generator; search for `call_plugin`, `find_plugin`, `requires` emission to find all sites needing update
  - `crates/polyplugc/src/ir/mod.rs` — `ResolvedBundle`, `ResolvedDependency` from Task 12
  - `crates/polyplug/src/abi/mod.rs` — `HostVTable` new fields (from Task 5) for generated struct usage

  **Acceptance Criteria**:
  - [ ] Generated `manifest.toml` emits `[[dependency]]` tables (when deps exist)
  - [ ] Generated Rust code emits `MY_BUNDLE_ID: u64` constant
  - [ ] Generated host caller code uses `find_by_contract` not `find_plugin`
  - [ ] Generated host caller code uses `resolve_plugin` + raw vtable, not `call_plugin`
  - [ ] `grep -n "call_plugin\|find_plugin" crates/polyplugc/src/generators/rust/mod.rs` returns zero results (except in comments)
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: Rust generator emits correct manifest and constants for a bundle with dependencies
    Tool: Bash
    Preconditions: Task 14 complete, unit test in generator #[cfg(test)]
    Steps:
      1. In test: create a ResolvedBundle with one ByContract dependency
      2. Run the Rust generator
      3. Assert: output contains MY_BUNDLE_ID, DEP_* constants, [[dependency]] in manifest
      4. Assert: output does NOT contain "call_plugin" or "find_plugin"
      5. Run: cargo test -p polyplugc -- rust_generator 2>&1
    Expected Result: all assertions pass
    Failure Indicators: missing constants, old API calls found in output
    Evidence: .sisyphus/evidence/task-14-rust-gen-output.txt
  ```

  **Evidence to Capture**:
  - [ ] task-14-rust-gen-output.txt

  **Commit**: YES (Wave 3 group)

- [x] 15. Update `crates/polyplugc/src/generators/cpp/mod.rs` — guard-based dispatch, new manifest format

  **What to do**:
  - Open `crates/polyplugc/src/generators/cpp/mod.rs`
  - Apply the same conceptual changes as Task 14 but for C++ generated code:
  - **Remove** `requires = [...]` from generated `manifest.toml` emission
  - **Add** `[[dependency]]` table emission in manifest
  - **Add** emission of `MY_BUNDLE_ID` constant in generated C++ header:
    ```cpp
    static constexpr uint64_t MY_BUNDLE_ID = {bundle_id_value}ULL;
    ```
  - **Update host callers** in generated C++ code: change from:
    ```cpp
    (host_->call_plugin)(handle, ...)
    ```
    to:
    ```cpp
    const PluginVTable* vtable = (host_->resolve_plugin)(handle);
    // vtable->call(...)  — or whatever the C++ dispatch pattern is in this generator
    ```
  - **Update find calls** in generated C++: change `(host_->find_plugin)(...)` to `(host_->find_by_contract)(...)`

  **Must NOT do**:
  - Do not change C++ code that does not use the ABI fns being removed
  - Do not emit Python or Lua artifacts from this generator

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 888-line generator update with similar pattern to Task 14 but for C++ output
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 14, 16)
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 12

  **References**:
  - `crates/polyplugc/src/generators/cpp/mod.rs:1-888` — full generator
  - `crates/polyplugc/src/ir/mod.rs` — `ResolvedBundle`, `ResolvedDependency`
  - `host-libs/cpp/polyplug/abi.hpp` — C++ HostVTable layout (Task 21 will update this; generators may reference it)

  **Acceptance Criteria**:
  - [ ] Generated C++ manifest emits `[[dependency]]` tables
  - [ ] Generated C++ code emits `MY_BUNDLE_ID` constant
  - [ ] `grep -n "call_plugin\|find_plugin" crates/polyplugc/src/generators/cpp/mod.rs` returns zero results
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: C++ generator produces no call_plugin references
    Tool: Bash
    Preconditions: Task 15 complete
    Steps:
      1. Run: grep -n 'call_plugin\|find_plugin' crates/polyplugc/src/generators/cpp/mod.rs
      2. Assert: zero matches
      3. Run: cargo build -p polyplugc 2>&1
      4. Assert: exit code 0
    Expected Result: old ABI references purged from C++ generator
    Failure Indicators: any match or build error
    Evidence: .sisyphus/evidence/task-15-cpp-gen-absence.txt
  ```

  **Evidence to Capture**:
  - [ ] task-15-cpp-gen-absence.txt

  **Commit**: YES (Wave 3 group)

- [x] 16. Update `crates/polyplugc/src/generators/csharp/mod.rs` — updated API

  **What to do**:
  - Open `crates/polyplugc/src/generators/csharp/mod.rs`
  - Apply the same pattern changes as Tasks 14-15 for C# generated code:
  - **Remove** `requires` from generated manifest
  - **Add** `[[dependency]]` table emission
  - **Add** `MY_BUNDLE_ID` constant in generated C# code:
    ```csharp
    public static readonly ulong MyBundleId = {bundle_id_value}UL;
    ```
  - **Update host caller stubs**: change `FindPlugin`/`CallPlugin` references to `FindByContract`/`ResolvePlugin`
  - The C# generator currently has stubs — update those stubs to reflect new API

  **Must NOT do**:
  - Do not implement full C# generation logic (Epic 9.5 handles C# hardening — do not regress)
  - Do not change any C# dotnet-specific hardening added in Epic 9.5

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 483-line generator update for C# output
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 14, 15)
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 12

  **References**:
  - `crates/polyplugc/src/generators/csharp/mod.rs:1-483` — full generator
  - `crates/polyplugc/src/ir/mod.rs` — `ResolvedBundle` from Task 12
  - `host-libs/csharp/` and `guest-libs/csharp/` — C# library files for context (Tasks 23, 24 will update them)

  **Acceptance Criteria**:
  - [ ] `grep -n "call_plugin\|find_plugin\|CallPlugin\|FindPlugin" crates/polyplugc/src/generators/csharp/mod.rs` returns zero results (except new names with "by_contract" etc.)
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: C# generator purged of old API references
    Tool: Bash
    Preconditions: Task 16 complete
    Steps:
      1. Run: grep -in 'call_plugin\|CallPlugin\|find_plugin\|FindPlugin' crates/polyplugc/src/generators/csharp/mod.rs | grep -v 'by_contract\|by_bundle\|all_by'
      2. Assert: zero lines output
      3. Run: cargo build -p polyplugc 2>&1 | tail -3
      4. Assert: exit code 0
    Expected Result: old API names absent, build clean
    Evidence: .sisyphus/evidence/task-16-csharp-gen-absence.txt
  ```

  **Evidence to Capture**:
  - [ ] task-16-csharp-gen-absence.txt

  **Commit**: YES (Wave 3 group)

- [x] 17. Create Python generator skeleton `crates/polyplugc/src/generators/python/mod.rs`

  **What to do**:
  - Create directory `crates/polyplugc/src/generators/python/` if it does not exist
  - Create `crates/polyplugc/src/generators/python/mod.rs` with a skeleton stub:
    ```rust
    //! Python code generator skeleton.
    //! Full implementation is planned for Epic 10.
    //! This stub exists to register the module and allow the Language enum to compile.
    
    use crate::generators::CodeGenerator;
    use crate::ir::ResolvedBundle;

    pub struct PythonGenerator;

    impl CodeGenerator for PythonGenerator {
        fn generate(&self, _bundle: &ResolvedBundle) -> Result<Vec<crate::generators::GeneratedFile>, crate::error::CodegenError> {
            Err(crate::error::CodegenError::ValidationFailed {
                message: "Python generator not yet implemented (planned for Epic 10)".into(),
            })
        }
    }
    ```
  - Adapt the exact trait and error type names to match what `CodeGenerator` trait and `CodegenError` actually look like in this codebase (read `generators/mod.rs` for the trait definition)

  **Must NOT do**:
  - Do not implement any real Python code generation
  - Do not create `host-libs/python/` or `guest-libs/python/` directories
  - Do not create any Python test files

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Skeleton stub with a single Err return — trivial
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 18)
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 13 (module declaration)

  **References**:
  - `crates/polyplugc/src/generators/mod.rs` — `CodeGenerator` trait definition and `CodegenError` type
  - `crates/polyplugc/src/generators/lua/mod.rs` — mirror this stub (Task 18 creates it)
  - AGENTS.md rule 1: `python/mod.rs` MUST be `generators/python/mod.rs` (directory/mod.rs pattern)

  **Acceptance Criteria**:
  - [ ] `crates/polyplugc/src/generators/python/mod.rs` exists
  - [ ] `PythonGenerator` struct implements `CodeGenerator` trait returning `Err(...)`
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: Python generator compiles as an Err stub
    Tool: Bash
    Preconditions: Tasks 13 and 17 complete
    Steps:
      1. Run: cargo build -p polyplugc 2>&1
      2. Assert: exit code 0
      3. Run: grep -n 'PythonGenerator' crates/polyplugc/src/generators/python/mod.rs
      4. Assert: at least 1 match
    Expected Result: stub compiles cleanly
    Evidence: .sisyphus/evidence/task-17-python-stub-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-17-python-stub-build.txt

  **Commit**: YES (Wave 3 group)

- [x] 18. Create Lua generator skeleton `crates/polyplugc/src/generators/lua/mod.rs`

  **What to do**:
  - Create directory `crates/polyplugc/src/generators/lua/` if it does not exist
  - Create `crates/polyplugc/src/generators/lua/mod.rs` — identical pattern to Task 17 but for Lua:
    ```rust
    //! Lua code generator skeleton.
    //! Full implementation is planned for Epic 11.
    
    use crate::generators::CodeGenerator;
    use crate::ir::ResolvedBundle;

    pub struct LuaGenerator;

    impl CodeGenerator for LuaGenerator {
        fn generate(&self, _bundle: &ResolvedBundle) -> Result<Vec<crate::generators::GeneratedFile>, crate::error::CodegenError> {
            Err(crate::error::CodegenError::ValidationFailed {
                message: "Lua generator not yet implemented (planned for Epic 11)".into(),
            })
        }
    }
    ```

  **Must NOT do**:
  - Do not implement any real Lua code generation
  - Do not create `host-libs/lua/` or `guest-libs/lua/` files

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Trivial skeleton stub
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 17)
  - **Parallel Group**: Wave 3
  - **Blocks**: None
  - **Blocked By**: Task 13

  **References**:
  - `crates/polyplugc/src/generators/python/mod.rs` — mirror the Python stub from Task 17
  - `crates/polyplugc/src/generators/mod.rs` — `CodeGenerator` trait

  **Acceptance Criteria**:
  - [ ] `crates/polyplugc/src/generators/lua/mod.rs` exists
  - [ ] `LuaGenerator` struct implements `CodeGenerator` trait returning `Err(...)`
  - [ ] `cargo build -p polyplugc` exits 0

  **QA Scenarios**:
  ```
  Scenario: Lua generator compiles as an Err stub
    Tool: Bash
    Preconditions: Tasks 13 and 18 complete
    Steps:
      1. Run: cargo build -p polyplugc 2>&1
      2. Assert: exit code 0
    Expected Result: stub compiles cleanly
    Evidence: .sisyphus/evidence/task-18-lua-stub-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-18-lua-stub-build.txt

  **Commit**: YES (Wave 3 group)

- [ ] 19. Update `host-libs/rust/src/lib/mod.rs` — new ABI function wrappers

  **What to do**:
  - Open `host-libs/rust/src/lib/mod.rs`
  - Currently re-exports only (13 lines)
  - Add wrapper functions for the new C ABI surface. These are thin wrappers for host code to call:
    ```rust
    use crate::abi::{HostVTable, PluginHandle, PluginVTable};

    /// Look up the first provider of a contract at or above min_version.
    /// Returns PluginHandle::NULL if not found.
    pub unsafe fn find_by_contract(vtable: &HostVTable, contract_id: u64, min_version: u32) -> PluginHandle {
        (vtable.find_by_contract)(contract_id, min_version)
    }

    /// Look up a specific bundle's implementation of a contract.
    pub unsafe fn find_by_bundle(vtable: &HostVTable, bundle_id: u64, contract_id: u64, min_version: u32) -> PluginHandle {
        (vtable.find_by_bundle)(bundle_id, contract_id, min_version)
    }

    /// Enumerate all providers of a contract into caller-provided buffer.
    /// Returns the number of handles written.
    pub unsafe fn find_all_by_contract(
        vtable: &HostVTable,
        contract_id: u64,
        min_version: u32,
        out: &mut [PluginHandle],
    ) -> usize {
        (vtable.find_all_by_contract)(contract_id, min_version, out.as_mut_ptr(), out.len())
    }

    /// Resolve a handle to a raw vtable pointer.
    /// # Safety
    /// The returned pointer is valid as long as the plugin library is loaded.
    pub unsafe fn resolve_plugin(vtable: &HostVTable, handle: PluginHandle) -> *const PluginVTable {
        (vtable.resolve_plugin)(handle)
    }
    ```
  - Adapt imports to match what is actually re-exported from this lib's crate root

  **Must NOT do**:
  - Do not add wrappers for removed fns (`find_plugin`, `call_plugin`)
  - Do not add `use` inside function bodies

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Adding safe wrapper functions with correct signatures and safety docs
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 20-27)
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Tasks 5, 6, 7, 8

  **References**:
  - `host-libs/rust/src/lib/mod.rs:1-13` — full existing file
  - `crates/polyplug/src/abi/mod.rs` — `HostVTable` new layout (from Task 5)

  **Acceptance Criteria**:
  - [ ] `find_by_contract`, `find_by_bundle`, `find_all_by_contract`, `resolve_plugin` wrappers present
  - [ ] No `find_plugin` or `call_plugin` wrappers
  - [ ] `cargo build` for the host-libs/rust package exits 0

  **QA Scenarios**:
  ```
  Scenario: new wrapper functions compile cleanly
    Tool: Bash
    Preconditions: Task 19 complete
    Steps:
      1. Run: cargo build 2>&1 (from host-libs/rust or workspace)
      2. Assert: exit code 0
      3. Run: grep -n 'find_by_contract\|find_by_bundle\|find_all_by_contract\|resolve_plugin' host-libs/rust/src/lib/mod.rs
      4. Assert: at least 4 matches
    Expected Result: all 4 wrappers present and compilable
    Evidence: .sisyphus/evidence/task-19-host-rust-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-19-host-rust-build.txt

  **Commit**: YES (Wave 4 group)

- [x] 20. Update `guest-libs/rust/src/lib/mod.rs` — remove call_plugin usage

  **What to do**:
  - Open `guest-libs/rust/src/lib/mod.rs` (43 lines, re-exports + `PluginError` struct)
  - Search for any reference to `call_plugin` or `find_plugin` in the file
  - Remove or update any such reference to use `find_by_contract` / `resolve_plugin` instead
  - Guest libs typically don't call `find_plugin` directly (plugins don't find themselves), but may have stubs or imports — clean any up
  - If no references exist, verify the file still compiles correctly after `HostVTable` layout change

  **Must NOT do**:
  - Do not change `PluginError` struct
  - Do not break the re-export surface

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Likely a small cleanup; file is only 43 lines
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 19, 21-27)
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Tasks 5, 8

  **References**:
  - `guest-libs/rust/src/lib/mod.rs:1-43` — full file
  - `crates/polyplug/src/abi/mod.rs` — new `HostVTable` layout

  **Acceptance Criteria**:
  - [ ] No `call_plugin` or `find_plugin` references in `guest-libs/rust/src/lib/mod.rs`
  - [ ] `cargo build` for the guest-libs/rust exits 0

  **QA Scenarios**:
  ```
  Scenario: guest-rust lib compiles after ABI change
    Tool: Bash
    Preconditions: Task 20 complete
    Steps:
      1. Run: cargo build --workspace 2>&1 | grep -E 'error|warning.*guest'
      2. Assert: zero error lines
    Expected Result: clean compile
    Evidence: .sisyphus/evidence/task-20-guest-rust-build.txt
  ```

  **Evidence to Capture**:
  - [ ] task-20-guest-rust-build.txt

  **Commit**: YES (Wave 4 group)

- [x] 21. Update `host-libs/cpp/` — new HostVTable layout

  **What to do**:
  - Open `host-libs/cpp/polyplug/abi.hpp` (and any other relevant C++ header files in this directory)
  - Find the `HostVTable` C struct definition (currently has `find_plugin` and `call_plugin` fn ptrs)
  - Replace with the new 7-field layout matching `crates/polyplug/src/abi/mod.rs` Task 5:
    ```cpp
    struct PolyplugHostVTable {
        void* (*alloc)(size_t size, size_t align);
        void  (*free)(void* ptr, size_t size, size_t align);
        PluginHandle (*find_by_contract)(uint64_t contract_id, uint32_t min_version);
        PluginHandle (*find_by_bundle)(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
        size_t (*find_all_by_contract)(uint64_t contract_id, uint32_t min_version, PluginHandle* out, size_t out_cap);
        const PluginVTable* (*resolve_plugin)(PluginHandle handle);
        const void* (*get_extension)(uint64_t extension_id);
    };
    ```
  - Remove `find_plugin` and `call_plugin` fn ptr fields
  - Add `MY_BUNDLE_ID` constant pattern documentation comment if present

  **Must NOT do**:
  - Do not change `PluginHandle` struct layout
  - Do not create `host-libs/python/` or `host-libs/lua/` directories

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: C++ header update with exact field order requirement
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 19, 20, 22-27)
  - **Parallel Group**: Wave 4
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `host-libs/cpp/polyplug/abi.hpp` — full file (read first to understand current layout)
  - `crates/polyplug/src/abi/mod.rs` — Rust `HostVTable` definition as the ground truth

  **Acceptance Criteria**:
  - [ ] C++ `PolyplugHostVTable` has 7 fields in correct order
  - [ ] `find_plugin` and `call_plugin` absent from C++ header
  - [ ] File compiles cleanly (if a C++ test or build exists)

  **QA Scenarios**:
  ```
  Scenario: C++ host header has no old API fields
    Tool: Bash
    Preconditions: Task 21 complete
    Steps:
      1. Run: grep -n 'find_plugin\|call_plugin' host-libs/cpp/polyplug/abi.hpp
      2. Assert: zero lines output
    Expected Result: old fields absent
    Evidence: .sisyphus/evidence/task-21-cpp-host-absence.txt
  ```

  **Evidence to Capture**:
  - [ ] task-21-cpp-host-absence.txt

  **Commit**: YES (Wave 4 group)

- [x] 22. Update `guest-libs/cpp/` — remove old API references

  **What to do**:
  - Open `guest-libs/cpp/polyplug/abi.hpp` (and related files)
  - Remove any `find_plugin` or `call_plugin` fn ptr references
  - Update `HostVTable` mirror struct if it exists in this file to match new 7-field layout
  - If the guest lib provides a `call()` helper that was implemented via `call_plugin`, update it to use `resolve_plugin` + direct vtable dispatch

  **Must NOT do**:
  - Do not create `guest-libs/python/` or `guest-libs/lua/` files

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: C++ guest lib update mirroring host lib changes
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4)
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `guest-libs/cpp/polyplug/abi.hpp` — full file
  - `host-libs/cpp/polyplug/abi.hpp` — updated in Task 21 as reference

  **Acceptance Criteria**:
  - [ ] No `find_plugin`/`call_plugin` in guest-libs/cpp headers
  - [ ] File compiles (or at least no syntax errors visible)

  **QA Scenarios**:
  ```
  Scenario: C++ guest header purged of old API
    Tool: Bash
    Steps:
      1. Run: grep -n 'find_plugin\|call_plugin' guest-libs/cpp/polyplug/abi.hpp 2>/dev/null || echo 'FILE_NOT_FOUND'
      2. Assert: zero matches (or FILE_NOT_FOUND means no such file, also acceptable)
    Evidence: .sisyphus/evidence/task-22-cpp-guest-absence.txt
  ```

  **Evidence to Capture**:
  - [ ] task-22-cpp-guest-absence.txt

  **Commit**: YES (Wave 4 group)

- [x] 23. Update `host-libs/csharp/` — updated P/Invoke declarations

  **What to do**:
  - Open `host-libs/csharp/src/Abi.cs` (and any related `.cs` files)
  - Find P/Invoke declarations for `polyplug_find_plugin` and `polyplug_call_plugin`
  - Remove them
  - Add new P/Invoke declarations:
    ```csharp
    [DllImport("polyplug")]
    public static extern PluginHandle polyplug_find_by_contract(ulong contractId, uint minVersion);

    [DllImport("polyplug")]
    public static extern PluginHandle polyplug_find_by_bundle(ulong bundleId, ulong contractId, uint minVersion);

    [DllImport("polyplug")]
    public static extern UIntPtr polyplug_find_all_by_contract(ulong contractId, uint minVersion, IntPtr outHandles, UIntPtr outCap);

    [DllImport("polyplug")]
    public static extern IntPtr polyplug_resolve_plugin(PluginHandle handle);
    ```
  - Adapt exact `DllImport` attribute style to match existing file conventions (CallingConvention, EntryPoint, etc.)

  **Must NOT do**:
  - Do not touch Epic 9.5 hardening code
  - Do not use P/Invoke for removed functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: C# P/Invoke update with exact signature matching
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4)
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `host-libs/csharp/src/Abi.cs` — full file; find `[DllImport]` patterns to follow
  - `crates/polyplug/src/lib.rs` — new C export signatures from Task 8 as source of truth

  **Acceptance Criteria**:
  - [ ] `polyplug_find_by_contract`, `polyplug_find_by_bundle`, `polyplug_find_all_by_contract`, `polyplug_resolve_plugin` P/Invoke declarations present
  - [ ] `polyplug_find_plugin` and `polyplug_call_plugin` absent
  - [ ] `dotnet build` or equivalent exits 0 (if a C# build is configured in workspace)

  **QA Scenarios**:
  ```
  Scenario: C# host P/Invokes updated
    Tool: Bash
    Steps:
      1. Run: grep -in 'find_plugin\|call_plugin' host-libs/csharp/src/Abi.cs
      2. Assert: zero matches
      3. Run: grep -in 'find_by_contract\|find_by_bundle\|resolve_plugin' host-libs/csharp/src/Abi.cs
      4. Assert: at least 3 matches
    Evidence: .sisyphus/evidence/task-23-csharp-host-pinvoke.txt
  ```

  **Evidence to Capture**:
  - [ ] task-23-csharp-host-pinvoke.txt

  **Commit**: YES (Wave 4 group)

- [x] 24. Update `guest-libs/csharp/` — remove old API stubs

  **What to do**:
  - Open `guest-libs/csharp/src/Abi.cs` (and related files)
  - Remove references to `call_plugin` / `find_plugin`
  - Update any stubs to reference new API names
  - Guest libs rarely call `find_plugin` directly, but may import it for reference

  **Must NOT do**:
  - Do not touch Epic 9.5 hardening code

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small cleanup, mostly removing obsolete references
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4)
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `guest-libs/csharp/src/Abi.cs` — full file
  - `host-libs/csharp/src/Abi.cs` — updated in Task 23 as reference

  **Acceptance Criteria**:
  - [ ] No `call_plugin` or `find_plugin` in guest-libs/csharp files

  **QA Scenarios**:
  ```
  Scenario: C# guest lib purged of old API
    Tool: Bash
    Steps:
      1. Run: grep -in 'call_plugin\|find_plugin' guest-libs/csharp/src/Abi.cs 2>/dev/null || echo 'NONE'
      2. Assert: output is 'NONE' (no such file) or zero matches
    Evidence: .sisyphus/evidence/task-24-csharp-guest-absence.txt
  ```

  **Evidence to Capture**:
  - [ ] task-24-csharp-guest-absence.txt

  **Commit**: YES (Wave 4 group)

- [x] 25. Update `tests/fixtures/test_plugin/src/lib.rs` — mirrored HostVTable layout

  **What to do**:
  - Open `tests/fixtures/test_plugin/src/lib.rs` (249 lines)
  - Find the `HostVTable` struct definition mirrored in this file (it manually mirrors `polyplug::abi::HostVTable`)
  - Replace the 5-field definition with the new 7-field definition matching Task 5:
    ```rust
    #[repr(C)]
    pub struct HostVTable {
        pub alloc:                  unsafe extern "C" fn(size: usize, align: usize) -> *mut u8,
        pub free:                   unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize),
        pub find_by_contract:       unsafe extern "C" fn(contract_id: u64, min_version: u32) -> PluginHandle,
        pub find_by_bundle:         unsafe extern "C" fn(bundle_id: u64, contract_id: u64, min_version: u32) -> PluginHandle,
        pub find_all_by_contract:   unsafe extern "C" fn(contract_id: u64, min_version: u32, out: *mut PluginHandle, out_cap: usize) -> usize,
        pub resolve_plugin:         unsafe extern "C" fn(handle: PluginHandle) -> *const PluginVTable,
        pub get_extension:          unsafe extern "C" fn(extension_id: u32) -> *const (),
    }
    ```
  - Update any code in the test plugin that called `vtable.find_plugin` or `vtable.call_plugin` to use the new methods
  - If the test plugin's `polyplug_init` callback calls into the host vtable, update those calls to use `find_by_contract` instead

  **Must NOT do**:
  - Do not add test assertions in this file (tests belong in the integration test suite)
  - Do not add any logic that doesn't belong in a minimal test fixture

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Struct update + call site fixes in a test fixture — mechanical
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4)
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `tests/fixtures/test_plugin/src/lib.rs:1-249` — full file; find the mirrored `HostVTable` definition and all vtable call sites
  - `crates/polyplug/src/abi/mod.rs` — new `HostVTable` layout from Task 5 as source of truth

  **Acceptance Criteria**:
  - [ ] Test fixture `HostVTable` has 7 fields in exact order
  - [ ] No `find_plugin` or `call_plugin` call sites in the fixture
  - [ ] `cargo test --workspace` exits 0

  **QA Scenarios**:
  ```
  Scenario: test fixture compiles with updated HostVTable
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace 2>&1 | head -20
      2. Assert: no compile errors for test_plugin fixture
    Evidence: .sisyphus/evidence/task-25-fixture-compile.txt
  ```

  **Evidence to Capture**:
  - [ ] task-25-fixture-compile.txt

  **Commit**: YES (Wave 4 group)

- [x] 26. Update existing integration tests to use new registry API

  **What to do**:
  - Open `tests/integration_dispatch/mod.rs` (the existing integration test file)
  - Find all calls to `runtime.find_plugin()`, `runtime.call_plugin()`, `registry.find()`, `registry.resolve()`
  - Update them to use the new API:
    - `runtime.find_plugin(contract_id)` — change to `runtime.find_by_contract(contract_id, 0)`
    - `runtime.call_plugin(handle, ...)` — remove; use `runtime.resolve_plugin(handle)` then call vtable directly
    - `registry.find(contract_id)` — change to `registry.find_by_contract(contract_id, 0)`
    - `registry.register(descriptor, vtable, name)` — add `bundle_id: 0` (or a test bundle id) as fourth param
  - Where tests construct `HostVTable` manually, update to 7-field layout
  - All tests must continue to PASS after these updates

  **Must NOT do**:
  - Do not delete existing passing tests — update them to compile and pass
  - Do not add new test assertions here (new tests go in Task 28)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Updating call sites across an integration test file — requires understanding test intent to update correctly
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 6, 7, 8, 10)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 28
  - **Blocked By**: Tasks 6, 7, 8

  **References**:
  - `tests/integration_dispatch/mod.rs` — full existing test file
  - `crates/polyplug/src/runtime/mod.rs` — new public API methods (from Task 7)
  - `crates/polyplug/src/registry/mod.rs` — updated `register()` signature with `bundle_id` param (from Task 6)

  **Acceptance Criteria**:
  - [ ] All existing integration tests compile
  - [ ] `cargo test --workspace` exits 0
  - [ ] No calls to removed API (`find_plugin`, `call_plugin`, old `register()` without `bundle_id`)

  **QA Scenarios**:
  ```
  Scenario: existing integration tests pass after API update
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace 2>&1
      2. Assert: exit code 0
      3. Assert: output shows all integration_dispatch tests as 'ok'
    Evidence: .sisyphus/evidence/task-26-integration-tests-pass.txt
  ```

  **Evidence to Capture**:
  - [ ] task-26-integration-tests-pass.txt

  **Commit**: YES (Wave 4 group)

- [x] 27. Update `crates/polyplug/benches/vtable_dispatch.rs` — new bench fns

  **What to do**:
  - Open `crates/polyplug/benches/vtable_dispatch.rs` (441 lines)
  - Remove `bench_find_plugin` and `bench_call_plugin` functions
  - Add replacement benchmarks:
    ```rust
    fn bench_find_by_contract(c: &mut Criterion) {
        // Setup: create registry with one registered plugin
        // Benchmark: repeated find_by_contract(contract_id, 0) calls
    }

    fn bench_resolve_plugin(c: &mut Criterion) {
        // Setup: create registry, get a PluginHandle
        // Benchmark: repeated resolve_plugin(handle) calls
    }

    fn bench_cross_plugin_guard(c: &mut Criterion) {
        // Setup: create registry with one plugin, get handle
        // Benchmark: find_by_contract + resolve_plugin + vtable->call chain (full hot path)
    }
    ```
  - Register all new benchmarks in the `criterion_group!` macro
  - Remove old benchmark group entries

  **Must NOT do**:
  - Do not use `.unwrap()` in bench setup code (use `.expect()` is allowed in test/bench context)
  - Do not leave benchmark functions that reference removed API

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Bench rewrite requiring understanding of criterion patterns and new API setup
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 4, once Tasks 5, 6, 7, 8 complete)
  - **Blocks**: None
  - **Blocked By**: Tasks 5, 6, 7, 8

  **References**:
  - `crates/polyplug/benches/vtable_dispatch.rs:1-441` — full bench file; understand criterion usage pattern
  - `crates/polyplug/src/registry/mod.rs` — new API methods from Task 6
  - `crates/polyplug/src/runtime/mod.rs` — new methods from Task 7

  **Acceptance Criteria**:
  - [ ] `bench_find_by_contract`, `bench_resolve_plugin`, `bench_cross_plugin_guard` present
  - [ ] `bench_find_plugin`, `bench_call_plugin` absent
  - [ ] `cargo bench -p polyplug --bench vtable_dispatch --no-run` exits 0

  **QA Scenarios**:
  ```
  Scenario: bench file compiles with new API
    Tool: Bash
    Steps:
      1. Run: cargo bench -p polyplug --bench vtable_dispatch --no-run 2>&1
      2. Assert: exit code 0
      3. Run: grep -n 'find_plugin\|call_plugin' crates/polyplug/benches/vtable_dispatch.rs
      4. Assert: zero matches
    Evidence: .sisyphus/evidence/task-27-bench-compile.txt
  ```

  **Evidence to Capture**:
  - [ ] task-27-bench-compile.txt

  **Commit**: YES (Wave 4 group)

- [ ] 28. Create `tests/integration_cross_plugin/mod.rs` — 7 cross-plugin integration tests

  **What to do**:
  - Create directory `tests/integration_cross_plugin/` if it does not exist
  - Create `tests/integration_cross_plugin/mod.rs` with the following 7 test functions:

  **Test a — Single plugin find-by-contract**:
  - Register a plugin with `contract_id = contract_id("audio.Decoder")` and `bundle_id = bundle_id("audio-engine")`
  - Call `registry.find_by_contract(contract_id("audio.Decoder"), 0)`
  - Assert: returns `Ok(handle)` where `handle != PluginHandle::NULL`

  **Test b — Multi-impl: two plugins, same contract, different bundles**:
  - Register plugin A with `contract_id = contract_id("audio.Decoder")`, `bundle_id = bundle_id("bundle-a")`
  - Register plugin B with same `contract_id`, `bundle_id = bundle_id("bundle-b")`
  - Call `registry.find_all_by_contract(contract_id("audio.Decoder"), 0)`
  - Assert: returns `Vec` of length 2

  **Test c — find-by-bundle specificity**:
  - Register two plugins (same contract, different bundles) as in test b
  - Call `registry.find_by_bundle(bundle_id("bundle-b"), contract_id("audio.Decoder"), 0)`
  - Assert: returns the handle for plugin B specifically (verify by resolving and checking)

  **Test d — Stale handle rejected by resolve_guard**:
  - Register a plugin, get a handle, then increment its generation (simulate a reload by manipulating slot)
  - OR: Create a handle with a wrong generation directly: `PluginHandle { index: 0, generation: 99 }`
  - Call `registry.resolve_guard(stale_handle)`
  - Assert: returns `Err(RegistryError::StaleHandle { ... })`

  **Test e — Dependency enforcement during simulated init**:
  - Set `INIT_BUNDLE_ID` via `INIT_BUNDLE_ID.with(|c| c.set(bundle_id("caller-bundle")))`
  - Do NOT call `declare_deps` for this bundle
  - Call `host_find_by_contract(contract_id("audio.Decoder"), 0)` (the runtime callback)
  - Assert: returns `PluginHandle::NULL`
  - Set `INIT_BUNDLE_ID` back to 0

  **Test f — Declared dependency passes enforcement**:
  - Register plugin with `contract_id = contract_id("audio.Decoder")`, `bundle_id = bundle_id("provider-bundle")`
  - Call `registry.declare_deps(bundle_id("caller-bundle"), vec![contract_id("audio.Decoder")])`
  - Set `INIT_BUNDLE_ID.with(|c| c.set(bundle_id("caller-bundle")))`
  - Call `host_find_by_contract(contract_id("audio.Decoder"), 0)`
  - Assert: returns a valid (non-null) handle
  - Set `INIT_BUNDLE_ID` back to 0

  **Test g — find_all skips dependency enforcement**:
  - Register two plugins for same contract
  - Set `INIT_BUNDLE_ID` to a bundle with NO declared deps
  - Call `host_find_all_by_contract(contract_id, 0, &mut buf, buf.len())`
  - Assert: returns 2 (both plugins found despite no declared dep)
  - Set `INIT_BUNDLE_ID` back to 0

  All 7 tests live in a `#[cfg(test)]` module. Each test is `#[test]` annotated.
  Use `use crate::abi::{contract_id, bundle_id, PluginHandle};` and `use crate::registry::Registry;` at file top.

  **Must NOT do**:
  - Do not use `.unwrap()` — use `.expect("msg")` which is allowed in `#[cfg(test)]` context
  - Do not test hot-reload (that is Epic 10 scope)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Writing 7 integration tests that exercise dependency enforcement, multi-impl, stale-handle detection, and find_all bypass — requires thorough understanding of all prior tasks
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 6, 7, 8, 26)
  - **Parallel Group**: Wave 5
  - **Blocks**: Task 29
  - **Blocked By**: Tasks 6, 7, 8, 26

  **References**:
  - `crates/polyplug/src/registry/mod.rs` — `Registry` API from Task 6
  - `crates/polyplug/src/runtime/mod.rs` — `INIT_BUNDLE_ID`, `host_find_by_contract`, `host_find_by_bundle`, `host_find_all_by_contract` from Task 7
  - `crates/polyplug/src/abi/mod.rs` — `contract_id()`, `bundle_id()`, `PluginHandle::NULL`
  - `tests/integration_dispatch/mod.rs` — existing test patterns to follow for style

  **Acceptance Criteria**:
  - [ ] All 7 tests (a-g) present in `tests/integration_cross_plugin/mod.rs`
  - [ ] `cargo test --workspace` exits 0
  - [ ] All 7 tests show as `ok` in test output

  **QA Scenarios**:
  ```
  Scenario: all 7 integration tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace -- integration_cross_plugin 2>&1
      2. Assert: exit code 0
      3. Assert: output contains 'test ... ok' for all 7 tests (a through g)
    Expected Result: 7/7 pass
    Failure Indicators: any test shows FAILED or does not appear in output
    Evidence: .sisyphus/evidence/task-28-cross-plugin-tests.txt
  ```

  **Evidence to Capture**:
  - [ ] task-28-cross-plugin-tests.txt

  **Commit**: YES (Wave 4-5 group)

- [ ] 29. Wire `integration_cross_plugin` into `crates/polyplug/Cargo.toml` `[[test]]`

  **What to do**:
  - Open `crates/polyplug/Cargo.toml`
  - Add a `[[test]]` entry for the new integration test:
    ```toml
    [[test]]
    name = "integration_cross_plugin"
    path = "../../tests/integration_cross_plugin/mod.rs"
    ```
    Adapt the path to match where `tests/` lives relative to `crates/polyplug/Cargo.toml`
  - If the existing `integration_dispatch` test is wired similarly, follow that exact pattern

  **Must NOT do**:
  - Do not remove the `integration_dispatch` test entry

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single TOML entry addition
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 28)
  - **Parallel Group**: Wave 5
  - **Blocks**: None
  - **Blocked By**: Task 28

  **References**:
  - `crates/polyplug/Cargo.toml` — find existing `[[test]]` section for integration_dispatch to follow pattern

  **Acceptance Criteria**:
  - [ ] `[[test]]` entry for `integration_cross_plugin` present in `Cargo.toml`
  - [ ] `cargo test --workspace -- integration_cross_plugin` exits 0

  **QA Scenarios**:
  ```
  Scenario: new test target registered and discoverable by cargo
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace -- integration_cross_plugin 2>&1
      2. Assert: exit code 0 and tests run (not zero tests)
    Evidence: .sisyphus/evidence/task-29-test-wired.txt
  ```

  **Evidence to Capture**:
  - [ ] task-29-test-wired.txt

  **Commit**: YES (Wave 4-5 group)

- [ ] 30. Create `TRUST_MODEL.md` at repo root

  **What to do**:
  - Create a new file `TRUST_MODEL.md` at the repository root (same level as `AGENTS.md`)
  - Content must cover:
    1. **Overview**: What trust model means in the context of polyplug
    2. **Bundle Identity**: How `bundle_id` (FNV1a-64 of bundle name) is computed and used
    3. **Declared Dependencies**: Meaning of `[[dependency]]` in bundle.toml, how they are enforced during init
    4. **Enforcement Window**: Dependency enforcement ONLY applies during `polyplug_init()` (when `INIT_BUNDLE_ID != 0`). Hot-path calls are NOT enforced.
    5. **Multi-impl**: How multiple bundles can provide the same contract; resolution order
    6. **Threat Model**: What this protects against (accidental undeclared dep access) and what it does NOT protect against (malicious plugins, runtime tampering)
    7. **ABI Freeze Notice**: The ABI was re-frozen at Epic 9.7; extension mechanism is the approved path for new functionality
    8. **Future Work**: Hot-reload (Epic 10), Python/Lua bindings (Epics 10/11)
  - Use clear headings, concise prose. No fluff. Aim for 200-400 lines.

  **Must NOT do**:
  - Do not make security guarantees the system cannot deliver
  - Do not describe unimplemented features as implemented

  **Recommended Agent Profile**:
  - **Category**: `writing`
    - Reason: Technical documentation
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (independent of all code tasks)
  - **Parallel Group**: Wave 5
  - **Blocks**: Task 31
  - **Blocked By**: None

  **References**:
  - `AGENTS.md` — project identity and rules section for context
  - `polyplug_prd.md` §6, §7, §8 — ABI design rationale
  - `epics.md` Epic 9.7 — epic description and intent

  **Acceptance Criteria**:
  - [ ] `TRUST_MODEL.md` exists at repo root
  - [ ] Covers all 8 content areas listed above
  - [ ] Does not contain `.unwrap()` or code samples that violate AGENTS.md rules

  **QA Scenarios**:
  ```
  Scenario: TRUST_MODEL.md exists and has required sections
    Tool: Bash
    Steps:
      1. Run: ls TRUST_MODEL.md
      2. Assert: file exists (exit code 0)
      3. Run: grep -c '## ' TRUST_MODEL.md
      4. Assert: at least 6 section headings
    Evidence: .sisyphus/evidence/task-30-trust-model-exists.txt
  ```

  **Evidence to Capture**:
  - [ ] task-30-trust-model-exists.txt

  **Commit**: YES (Wave 4-5 group)

- [ ] 31. Update `AGENTS.md` with reference to `TRUST_MODEL.md`

  **What to do**:
  - Open `AGENTS.md` at repo root
  - In the **Project Identity** section, add a reference line:
    ```markdown
    - **Trust model**: See `TRUST_MODEL.md` for bundle identity, declared dependencies, and ABI freeze details.
    ```
  - Do not restructure or rewrite any existing content — insert only this reference

  **Must NOT do**:
  - Do not change any existing rule in `AGENTS.md`
  - Do not add any rule changes

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single bullet point addition
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 30 existing)
  - **Parallel Group**: Wave 5
  - **Blocks**: None
  - **Blocked By**: Task 30

  **References**:
  - `AGENTS.md` — Project Identity section (top of file)
  - `TRUST_MODEL.md` — created in Task 30

  **Acceptance Criteria**:
  - [ ] `AGENTS.md` contains a reference line pointing to `TRUST_MODEL.md`
  - [ ] No other content in `AGENTS.md` changed

  **QA Scenarios**:
  ```
  Scenario: AGENTS.md references TRUST_MODEL.md
    Tool: Bash
    Steps:
      1. Run: grep -n 'TRUST_MODEL' AGENTS.md
      2. Assert: exactly 1 match
    Evidence: .sisyphus/evidence/task-31-agents-md-ref.txt
  ```

  **Evidence to Capture**:
  - [ ] task-31-agents-md-ref.txt

  **Commit**: YES (Wave 4-5 group)

- [ ] 32. Add ABI freeze comment block to `crates/polyplug/src/abi/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/abi/mod.rs`
  - At the very top of the file (before `#![doc]` or after the first doc comment if one exists, before any `use` statements), add:
    ```rust
    // =============================================================================
    // ABI FROZEN AS OF EPIC 9.7
    // =============================================================================
    //
    // The following types and function signatures constitute the frozen polyplug ABI.
    // NO CHANGES to #[repr(C)] structs, function pointer signatures, or the field
    // order of HostVTable are permitted after this point.
    //
    // All new functionality must go through the extension mechanism (get_extension).
    // For rationale and trust model, see TRUST_MODEL.md.
    // =============================================================================
    ```
  - This is a comment block only — no code changes

  **Must NOT do**:
  - Do not change any code in the file
  - Do not add `use` statements

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Adding a comment block — trivial
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 5 to ensure the ABI is fully updated first)
  - **Parallel Group**: Wave 5
  - **Blocks**: None
  - **Blocked By**: Task 5

  **References**:
  - `crates/polyplug/src/abi/mod.rs` — top of file

  **Acceptance Criteria**:
  - [ ] ABI freeze comment block present at top of `abi/mod.rs`
  - [ ] `cargo build -p polyplug` still exits 0

  **QA Scenarios**:
  ```
  Scenario: freeze comment present in abi/mod.rs
    Tool: Bash
    Steps:
      1. Run: grep -n 'ABI FROZEN AS OF EPIC 9.7' crates/polyplug/src/abi/mod.rs
      2. Assert: exactly 1 match
    Evidence: .sisyphus/evidence/task-32-abi-freeze-comment.txt
  ```

  **Evidence to Capture**:
  - [ ] task-32-abi-freeze-comment.txt

  **Commit**: YES (Wave 4-5 group)

---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in `.sisyphus/evidence/`. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings` + `cargo test --workspace`. Review all changed files for: `.unwrap()`, `.expect()` in production, `use` inside functions/impl, bare `filename.rs` module roots, missing `// SAFETY:` on unsafe blocks, string error returns. Check AI slop: excessive comments, over-abstraction, generic names.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Manual QA** — `unspecified-high`
  From clean state: run all cargo commands, verify grep absence check, inspect HostVTable layout test assertion, verify all 7 cross-plugin integration tests pass, inspect arc-swap slot usage in registry. Save evidence to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [N/N] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual file diff. Verify 1:1 — everything in spec was built, nothing beyond spec. Check "Must NOT do" compliance per task. Flag any Epic 9.5 code touched. Detect cross-task contamination. Flag unaccounted changes.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

- **Wave 1-2**: `feat(abi): add arc-swap, bundle_id, new error variants, VTableSlot, HostVTable redesign, multi-impl registry, INIT_BUNDLE_ID runtime`
  - Files: `Cargo.toml`, `abi/mod.rs`, `error/mod.rs`, `registry/mod.rs`, `runtime/mod.rs`, `lib.rs`, `loader/manifest/mod.rs`, `loader/mod.rs`
  - Pre-commit: `cargo build -p polyplug`
- **Wave 3**: `feat(codegen): update parser, IR, all generators for new ABI; add Python/Lua stubs`
  - Files: `parser/mod.rs`, `ir/mod.rs`, `generators/mod.rs`, `generators/rust/mod.rs`, `generators/cpp/mod.rs`, `generators/csharp/mod.rs`, `generators/python/mod.rs`, `generators/lua/mod.rs`
  - Pre-commit: `cargo build -p polyplugc`
- **Wave 4-5**: `feat(consumers): update host/guest libs, fixtures, integration tests, docs`
  - Files: all host-libs, guest-libs, test fixtures, integration tests, benches, `TRUST_MODEL.md`, `AGENTS.md`
  - Pre-commit: `cargo test --workspace && cargo clippy -- -D warnings`

---

## Success Criteria

### Verification Commands
```bash
cargo build --workspace                          # Expected: exit 0, zero errors
cargo test --workspace                           # Expected: exit 0, all tests pass
cargo clippy -- -D warnings                      # Expected: exit 0, zero warnings
cargo bench -p polyplug --bench vtable_dispatch --no-run  # Expected: exit 0
grep -r "call_plugin\|find_plugin" crates/ --include="*.rs" | grep -v "_by_contract\|_by_bundle\|_all_by"  # Expected: zero output
```

### Final Checklist
- [ ] All "Must Have" present and verified
- [ ] All "Must NOT Have" absent (grep-verified)
- [ ] `HostVTable` size exactly 56 bytes
- [ ] All 7 cross-plugin integration tests pass
- [ ] `TRUST_MODEL.md` exists at repo root
- [ ] `AGENTS.md` references `TRUST_MODEL.md`
- [ ] ABI freeze comment present in `abi/mod.rs`
- [ ] Python/Lua generators compile but return `Err` immediately
- [ ] No `.unwrap()` in any production file changed by this epic
- [ ] All `unsafe` blocks have `// SAFETY:` comments
