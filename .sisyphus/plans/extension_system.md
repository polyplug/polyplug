# Extension System Epic

## TL;DR

> **Quick Summary**: Implement the polyplug Extension System — a mechanism for the host to register vtable-based extensions (e.g., a trace/logging vtable) that guest plugins can optionally query at init time via `host_get_extension`. Includes the `Extension` trait, `TraceExtension`, `RuntimeBuilder` wiring, 7 generator updates, integration tests, and a benchmark addition.
>
> **Deliverables**:
> - `crates/polyplug/src/extensions/mod.rs` — `Extension` trait + `SendPtr` newtype
> - `crates/polyplug/src/extensions/trace/mod.rs` — `TraceExtension` + `TraceVTable`
> - `crates/polyplug/src/runtime/mod.rs` — `GLOBAL_EXTENSION_MAP`, real `host_get_extension`, `RuntimeBuilder::extension()`
> - `crates/polyplug/src/lib.rs` — `pub mod extensions;`, updated `polyplug_get_extension` delegate
> - 7 generator updates (Rust, C++, C#, Python, Lua, js-quickjs, js-deno) emitting extension query code
> - `tests/integration_extension/mod.rs` — integration tests (5 scenarios)
> - `crates/polyplug/Cargo.toml` — integration_extension test registration
> - `crates/polyplug/benches/vtable_dispatch.rs` — `bench_absent_extension_null_check` + criterion group update
> - `BENCHMARKS.md` — new row for extension null-check benchmark
>
> **Estimated Effort**: Medium
> **Parallel Execution**: YES — 3 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 11

---

## Context

### Original Request
Implement the Extension System epic per the polyplug PRD section 18 and session context from prior research. The `host_get_extension` vtable slot is already in the frozen ABI; this epic wires it up end-to-end.

### Interview Summary
**Key Discussions**:
- Architecture: `OnceLock<HashMap<u32, SendPtr>>` (not `*const ()` directly — `*const ()` is not `Send`)
- TraceExtension callback stored via `Box::leak` so vtable pointer is stable; `_extensions` in `Runtime` owns the `Box<dyn Extension>` heap objects
- `CounterExtension` is test-only — lives only in the integration test file
- No new crate dependencies
- Generator conditional: only emit extension query code when `optional` contains the extension name (e.g. `"trace"`)
- OnceLock cannot be reset — integration tests must be designed to tolerate one-time-set behavior

**Research Findings**:
- `GLOBAL_REGISTRY` pattern in `runtime/mod.rs` is the direct model for `GLOBAL_EXTENSION_MAP`
- `RuntimeBuilder` fields: `plugin_dirs`, `loaders` — add `extensions: Vec<Box<dyn Extension>>`
- `Runtime` struct: add `_extensions: Vec<Box<dyn Extension>>` for vtable lifetime
- `generate_guest_init_file` in Rust generator is the hook point; JS generators use `generate_init_ts`
- `compute_extension_id("trace")` → `0xC4EB9AEE_u32` (must be verified by a unit test)
- Workspace lints deny `unwrap_used` and `expect_used`; all test files need `#![allow(clippy::expect_used)]`

### Metis Review
**Identified Gaps** (addressed):
- `*const ()` is not `Send` → resolved via `SendPtr` newtype with `unsafe impl Send + Sync` in `extensions/mod.rs`
- Vtable lifetime: `_extensions: Vec<Box<dyn Extension>>` in `Runtime` keeps vtable memory alive; plan explicitly documents the never-drop invariant
- `OnceLock` not resettable: integration tests structured around this — each test uses `global_registry()` pattern; `build()` called at most once per process for extension tests
- Duplicate extension ID at `RuntimeBuilder::extension()` time: silently overwrite — document this in `extension()` docstring
- `ExtensionEntry` ABI struct: not used in new implementation (map stores raw `*const ()`); already `#[allow(dead_code)]` is not needed because it has public visibility

---

## Work Objectives

### Core Objective
Wire `host_get_extension` end-to-end: `RuntimeBuilder` accepts `Extension` impls → `build()` populates `GLOBAL_EXTENSION_MAP` → `host_get_extension` reads the map → generators emit opt-in query code in guest init.

### Concrete Deliverables
- `crates/polyplug/src/extensions/mod.rs` — `Extension` trait, `SendPtr` newtype
- `crates/polyplug/src/extensions/trace/mod.rs` — `TraceVTable`, `TraceExtension`
- Modified `crates/polyplug/src/runtime/mod.rs` — map static, real callback, builder method
- Modified `crates/polyplug/src/lib.rs` — module declaration + delegate
- 7 modified generator files
- `tests/integration_extension/mod.rs`
- Modified `crates/polyplug/Cargo.toml`
- Modified `crates/polyplug/benches/vtable_dispatch.rs`
- Modified `BENCHMARKS.md`

### Definition of Done
- [x] `cargo test -p polyplug --test integration_extension` → all tests pass
- [x] `cargo clippy -p polyplug -- -D warnings` → zero warnings
- [x] `cargo clippy -p polyplugc -- -D warnings` → zero warnings
- [x] `cargo fmt --check` → clean
- [x] `cargo bench -p polyplug --bench vtable_dispatch -- --test` → compiles and passes

### Must Have
- `host_get_extension(id)` returns a valid non-null pointer when TraceExtension registered
- `host_get_extension(id)` returns null for unknown extension IDs
- All generators emit extension query code ONLY when `optional` contains the extension name
- Empty `optional` list → no extension code emitted (unchanged generated output)
- `TraceExtension::new(callback)` — callback is `impl Fn(&str) + Send + Sync + 'static`
- Vtable pointer remains valid for the lifetime of the `Runtime`
- Integration test file has `#![allow(clippy::expect_used)]`

### Must NOT Have (Guardrails)
- No `.unwrap()` or `.expect()` in production code (workspace lint enforces this)
- No `extension.rs` or `trace.rs` as module roots (AGENTS.md Rule 1 — FORBIDDEN)
- No `use` inside function bodies or impl blocks (AGENTS.md Rule 2)
- No let bindings without explicit type annotation except struct construction and numeric casts (AGENTS.md Rule 3)
- No `unsafe { }` without a `// SAFETY:` comment on the immediately preceding line (AGENTS.md Rule 6)
- No new crate dependencies
- `CounterExtension` must not appear in production code — test file only
- Do not update `TRUST_MODEL.md`
- Do not add `remove_extension` or `get_extension<T>()` generic helper
- Do not touch `abi/mod.rs` (ABI is frozen)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (bun/cargo test)
- **Automated tests**: Tests-after (integration test file created in Task 11)
- **Framework**: `cargo test` (standard Rust integration tests)

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Unit/integration**: Use Bash (`cargo test`) — run specific test, assert pass
- **Compile check**: Use Bash (`cargo clippy`) — assert zero warnings
- **Benchmark**: Use Bash (`cargo bench -- --test`) — assert binary compiles

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundation):
├── Task 1: extensions/mod.rs — Extension trait + SendPtr newtype [quick]
├── Task 2: extensions/trace/mod.rs — TraceVTable + TraceExtension [quick]

Wave 2 (After Wave 1 — runtime wiring + generator updates, MAX PARALLEL):
├── Task 3: lib.rs — pub mod extensions + polyplug_get_extension delegate [quick]
├── Task 4: runtime/mod.rs — GLOBAL_EXTENSION_MAP + host_get_extension + RuntimeBuilder [unspecified-high]
├── Task 5: Rust generator — extension query in generate_guest_init_file [quick]
├── Task 6: C++ generator — extension query in generate_init_hpp [quick]
├── Task 7: C# generator — extension query in csharp generate_guest [quick]
├── Task 8: Python generator — extension query in python generate_guest [quick]
├── Task 9: Lua generator — extension query in lua generate_guest [quick]
├── Task 10a: js-quickjs generator — extension query in generate_init_ts [quick]
└── Task 10b: js-deno generator — extension query in generate_init_ts [quick]

Wave 3 (After Wave 2 — tests + bench + docs):
├── Task 11: integration_extension/mod.rs + Cargo.toml registration [unspecified-high]
├── Task 12: bench vtable_dispatch.rs — new bench + criterion_group update [quick]
└── Task 13: BENCHMARKS.md — new row [quick]
```

### Dependency Matrix

- **1**: None → 2, 3, 4
- **2**: 1 → 4, 11
- **3**: 1 → (done)
- **4**: 1, 2 → 11, 12
- **5**: 1 → 11
- **6–10b**: 1 → 11
- **11**: 1, 2, 4, 5, 6, 7, 8, 9, 10a, 10b → Final
- **12**: 4 → 13, Final
- **13**: 12 → Final

### Agent Dispatch Summary

- **Wave 1**: Task 1, 2 → `quick`
- **Wave 2**: Task 3, 5–10b → `quick`; Task 4 → `unspecified-high`
- **Wave 3**: Task 11 → `unspecified-high`; Tasks 12, 13 → `quick`

---

## TODOs


- [x] 1. Create `crates/polyplug/src/extensions/mod.rs` — `Extension` trait and `SendPtr` newtype

  **What to do**:
  - Create the directory `crates/polyplug/src/extensions/` and the file `crates/polyplug/src/extensions/mod.rs`.
  - **FORBIDDEN**: Do NOT create `src/extensions.rs`. The module root MUST be `src/extensions/mod.rs` (AGENTS.md Rule 1).
  - Declare a module: `pub mod trace;` (for the sub-module created in Task 2).
  - Define a `SendPtr` newtype to make `*const ()` usable in `OnceLock<HashMap<...>>` (raw pointers are not `Send`):
    ```rust
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SendPtr(pub(crate) *const ());
    // SAFETY: SendPtr wraps a raw pointer to a 'static extension vtable.
    // Extension vtables are written once during RuntimeBuilder::build() and never mutated.
    // All accesses are read-only after initialization. The pointed-to data outlives any
    // thread that reads this pointer (vtable lifetime is Runtime lifetime).
    unsafe impl Send for SendPtr {}
    // SAFETY: Same reasoning as Send — concurrent reads of a static vtable pointer are safe.
    unsafe impl Sync for SendPtr {}
    ```
  - Define the `Extension` trait:
    ```rust
    pub trait Extension: Send + Sync {
        /// Returns the FNV-1a 32-bit extension ID (e.g. fnv1a_32(b"trace")).
        fn extension_id(&self) -> u32;
        /// Returns a raw pointer to the extension's C-ABI vtable struct.
        /// The pointer MUST remain valid for the entire lifetime of the Runtime.
        fn vtable_ptr(&self) -> *const ();
    }
    ```
  - File-level doc comment: `//! Extensions — Extension trait and SendPtr helper for the extension system.`
  - All `use` statements at file top only (AGENTS.md Rule 2). No imports needed beyond `std` for this file.
  - Every `let` binding must have explicit type annotation (AGENTS.md Rule 3).

  **Must NOT do**:
  - Do NOT create `src/extensions.rs` (module root naming violation)
  - Do NOT add any functionality beyond `Extension` trait + `SendPtr` + `pub mod trace;` + doc comment
  - Do NOT add `get_extension<T>()` generic helper
  - Do NOT touch `abi/mod.rs`

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Pure new-file creation, well-specified, no dependencies on other in-progress work
  - **Skills**: []
    - No skills needed — straightforward Rust type definitions
  - **Skills Evaluated but Omitted**:
    - None applicable

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Task 2)
  - **Blocks**: Tasks 2, 3, 4, 5, 6, 7, 8, 9, 10a, 10b, 11
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplug/src/registry/mod.rs` — module organization pattern (doc comment at top, pub types, no inline use)
  - `crates/polyplug/src/abi/mod.rs:62-64` — `unsafe impl Send/Sync` pattern with `// SAFETY:` justifications

  **Acceptance Criteria**:
  - [ ] File exists at `crates/polyplug/src/extensions/mod.rs`
  - [ ] `pub trait Extension` is defined with `extension_id(&self) -> u32` and `vtable_ptr(&self) -> *const ()`
  - [ ] `pub(crate) struct SendPtr(pub(crate) *const ())` with `unsafe impl Send` and `unsafe impl Sync`
  - [ ] Each `unsafe impl` block has a `// SAFETY:` comment
  - [ ] `pub mod trace;` declared

  **QA Scenarios**:
  ```
  Scenario: Module compiles without warnings
    Tool: Bash (cargo)
    Preconditions: Task 1 file created; lib.rs has NOT yet been updated (Task 3 not done)
    Steps:
      1. Run: cargo check -p polyplug 2>&1 | grep -E '(error|warning)'
         (This will error that `extensions` module not found until Task 3, but the file itself must be syntactically valid)
      2. Verify the file parses: rustfmt --check crates/polyplug/src/extensions/mod.rs
    Expected Result: rustfmt exits 0 (file is valid Rust, properly formatted)
    Failure Indicators: rustfmt exits non-zero; syntax errors in the file
    Evidence: .sisyphus/evidence/task-1-fmt-check.txt
  ```

  **Commit**: YES (groups with Tasks 2, 3, 4 in Commit A)

---

- [x] 2. Create `crates/polyplug/src/extensions/trace/mod.rs` — `TraceVTable` + `TraceExtension`

  **What to do**:
  - Create `crates/polyplug/src/extensions/trace/mod.rs`.
  - **FORBIDDEN**: Do NOT create `src/extensions/trace.rs`. The module root MUST be `src/extensions/trace/mod.rs` (AGENTS.md Rule 1).
  - `use` statements at file top only (AGENTS.md Rule 2):
    ```rust
    use crate::abi::StringView;
    use crate::extensions::Extension;
    ```
  - Define `TraceVTable` (C-ABI vtable struct). **Critical:** `emit` takes TWO arguments: `msg: StringView` and `state: *const ()`. The `state` field stores the opaque `TraceState` pointer so the thunk can call the right callback without global state:
    ```rust
    /// C-ABI vtable for the trace extension. Passed to plugins as a *const TraceVTable.
    #[repr(C)]
    pub struct TraceVTable {
        /// Emit a trace message. msg is valid UTF-8 for call duration.
        /// state is the opaque TraceState pointer (same as this vtable's state field).
        pub emit: unsafe extern "C" fn(msg: StringView, state: *const ()),
        /// Opaque pointer to the heap-allocated TraceState (leaked, never freed).
        pub state: *const (),
    }
    // SAFETY: TraceVTable fields are a function pointer and a *const () to a leaked allocation.
    // Function pointers are thread-safe. The state pointer is immutable after construction.
    unsafe impl Send for TraceVTable {}
    // SAFETY: Same reasoning — no mutable state, concurrent reads are safe.
    unsafe impl Sync for TraceVTable {}
    ```
  - Define `TraceExtension`:
    ```rust
    /// Host-side trace extension. Wraps a callback and exposes it via a C-ABI vtable.
    pub struct TraceExtension {
        /// Leaked TraceVTable — stable pointer for the lifetime of the Runtime.
        vtable: *const TraceVTable,
    }
    // SAFETY: TraceExtension holds only a pointer to a leaked (never-freed) TraceVTable.
    // The callback inside is Send + Sync. The vtable pointer is valid until process exit.
    unsafe impl Send for TraceExtension {}
    // SAFETY: Same — no mutable state; vtable pointer is read-only after construction.
    unsafe impl Sync for TraceExtension {}
    ```
  - Define `TraceExtension::new(callback: impl Fn(&str) + Send + Sync + 'static) -> TraceExtension`:
    - Define a `TraceState` struct (non-`repr(C)`) at module level:
      `struct TraceState { callback: Box<dyn Fn(&str) + Send + Sync + 'static> }`
    - Define a module-level `unsafe extern "C" fn trace_emit_thunk(msg: StringView, state: *const ())`:
      1. SAFETY comment: state is a non-null *const TraceState leaked in TraceExtension::new
      2. Cast: `let ts: *const TraceState = state as *const TraceState;`
      3. Reconstruct &str from msg.ptr and msg.len (SAFETY: ABI contract guarantees valid UTF-8)
      4. Call `(*ts).callback(s);`
    - In `TraceExtension::new`:
      1. `let state: Box<TraceState> = Box::new(TraceState { callback: Box::new(callback) });`
      2. `let state_ptr: *const TraceState = Box::into_raw(state);`
      3. `let vtable: Box<TraceVTable> = Box::new(TraceVTable { emit: trace_emit_thunk, state: state_ptr as *const () });`
      4. `let vtable_ptr: *const TraceVTable = Box::into_raw(vtable);`
      5. Return `TraceExtension { vtable: vtable_ptr }`
    - AGENTS.md Rule 6: Every unsafe block must have a preceding // SAFETY: comment.
  - Add a public constant for the extension ID (before the TraceVTable definition):
    `pub const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;`
    Doc comment: `/// FNV-1a 32-bit hash of b"trace". Verified by unit test.`
  - Implement `Extension for TraceExtension` using the constant:
    `fn extension_id(&self) -> u32 { EXT_TRACE_ID }`
    `fn vtable_ptr(&self) -> *const () { self.vtable as *const () }`
  - Add `#[cfg(test)]` unit test: `assert_eq!(TraceExtension::new(|_| {}).extension_id(), crate::abi::extension_id("trace"));`

  **Must NOT do**:
  - Do NOT create `trace.rs` as module root
  - Do NOT add a `#[repr(C)]` field for the callback directly (fat pointer is not FFI-safe) — use the `TraceState` pattern above
  - Do NOT call `.unwrap()` or `.expect()` in non-test code
  - Do NOT add imports inside `impl` blocks

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Well-specified struct + impl, clear unsafe patterns to follow
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - None applicable

  **Parallelization**:
  - **Can Run In Parallel**: YES (parallel with Task 1)
  - **Parallel Group**: Wave 1 (with Task 1)
  - **Blocks**: Tasks 4, 11
  - **Blocked By**: Task 1 (needs `Extension` trait definition)

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplug/src/abi/mod.rs:62-65` — `unsafe impl Send/Sync` with SAFETY comments
  - `crates/polyplug/src/abi/mod.rs:179-207` — `#[repr(C)]` struct definitions with function pointer fields
  - `crates/polyplug/src/allocator/mod.rs` — `Box::leak` pattern for long-lived allocations

  **API/Type References**:
  - `crates/polyplug/src/abi/mod.rs:33-56` — `StringView` type used in `TraceVTable.emit` parameter
  - `crates/polyplug/src/abi/mod.rs:310-312` — `extension_id(name: &str) -> u32` — use to verify EXT_TRACE_ID

  **Acceptance Criteria**:
  - [ ] File exists at `crates/polyplug/src/extensions/trace/mod.rs`
  - [ ] `pub const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;` is declared and exported
  - [ ] `TraceVTable` is `#[repr(C)]` with field `emit: unsafe extern "C" fn(msg: StringView, state: *const ())` AND field `state: *const ()`
  - [ ] `TraceExtension::new(callback)` compiles with callback type `impl Fn(&str) + Send + Sync + 'static`
  - [ ] `Extension` impl returns `EXT_TRACE_ID` from `extension_id()`
  - [ ] Unit test passes: `assert_eq!(TraceExtension::new(|_| {}).extension_id(), crate::abi::extension_id("trace"))`
  - [ ] All unsafe blocks have `// SAFETY:` comments on the immediately preceding line

  **QA Scenarios**:
  ```
  Scenario: TraceExtension extension_id matches runtime hash
    Tool: Bash (cargo test)
    Preconditions: Task 1 and Task 2 files created; lib.rs updated with pub mod extensions (Task 3)
    Steps:
      1. Run: cargo test -p polyplug -- extensions::trace::tests 2>&1
      2. Check output contains 'test extensions::trace::tests::... ok'
    Expected Result: All unit tests in trace/mod.rs pass
    Failure Indicators: 'FAILED' in output; extension_id mismatch assertion
    Evidence: .sisyphus/evidence/task-2-unit-tests.txt

  Scenario: TraceVTable state pointer wiring (callback invoked)
    Tool: Bash (cargo test)
    Preconditions: All Wave 1 tasks done; Task 4 done (so host_get_extension works)
    Steps:
      1. Run integration test: cargo test -p polyplug --test integration_extension -- trace_callback_invoked 2>&1
      2. Check 'test trace_callback_invoked ... ok'
    Expected Result: Callback receives the message string correctly
    Failure Indicators: Test FAILED; message not received
    Evidence: .sisyphus/evidence/task-2-callback-roundtrip.txt
  ```

  **Commit**: YES (groups with Tasks 1, 3, 4 in Commit A)

---

- [x] 3. Update `crates/polyplug/src/lib.rs` — declare `extensions` module + delegate `polyplug_get_extension`

  **What to do**:
  - Add `pub mod extensions;` to `crates/polyplug/src/lib.rs` alongside the other existing module declarations (after `pub mod runtime;`).
  - Update `polyplug_get_extension` to delegate to the real `host_get_extension`. The current stub body is:
    ```rust
    // MVP: no extension registry
    core::ptr::null()
    ```
    Replace it with:
    ```rust
    // SAFETY: host_get_extension reads from GLOBAL_EXTENSION_MAP (OnceLock, read-only after init).
    // No pointer dereferences; safe to call from any thread.
    unsafe { crate::runtime::host_get_extension(_extension_id) }
    ```
  - The `_runtime` parameter stays unchanged (unused, kept for ABI compat).
  - Keep the existing `#[unsafe(no_mangle)]` attribute and safety doc comment.
  - No other changes to `lib.rs`.

  **Must NOT do**:
  - Do NOT remove the `_runtime` parameter
  - Do NOT change the function signature (ABI frozen)
  - Do NOT add any new pub use re-exports for extensions types
  - Do NOT touch any other functions in lib.rs

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Two small surgical edits to an existing file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (parallel with Task 4 and Tasks 5–10b)
  - **Parallel Group**: Wave 2
  - **Blocks**: Nothing critical (Task 2's unit test needs this to run)
  - **Blocked By**: Task 1 (`extensions/mod.rs` must exist before the module can be declared) and Task 4 (`host_get_extension` must be `pub(crate)` before `lib.rs` can call it — both are Wave 2 tasks; final `cargo check` runs after all of Wave 2)

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplug/src/lib.rs:7-13` — module declaration block to add `pub mod extensions;` to
  - `crates/polyplug/src/lib.rs:73-74` — `polyplug_find_by_contract` delegation pattern to follow
  - `crates/polyplug/src/lib.rs:127-133` — exact current `polyplug_get_extension` body to replace

  **Acceptance Criteria**:
  - [ ] `pub mod extensions;` appears in `lib.rs`
  - [ ] `polyplug_get_extension` body delegates to `crate::runtime::host_get_extension(_extension_id)`
  - [ ] `// SAFETY:` comment present before the `unsafe` call
  - [ ] `cargo check -p polyplug` exits 0 after this change (requires Tasks 1, 2 also done)

  **QA Scenarios**:
  ```
  Scenario: lib.rs compiles with extensions module declared
    Tool: Bash (cargo check)
    Preconditions: Tasks 1, 2, 3 done
    Steps:
      1. Run: cargo check -p polyplug 2>&1
      2. Assert: exit code 0, no errors about missing modules
    Expected Result: cargo check exits 0
    Failure Indicators: 'error[E0583]: file not found for module `extensions`'
    Evidence: .sisyphus/evidence/task-3-check.txt
  ```

  **Commit**: YES (groups with Tasks 1, 2, 4 in Commit A)

---

- [x] 4. Update `crates/polyplug/src/runtime/mod.rs` — `GLOBAL_EXTENSION_MAP`, real `host_get_extension`, `RuntimeBuilder::extension()`, `Runtime::_extensions`

  **What to do**:
  This is the most complex task. Follow the existing `GLOBAL_REGISTRY` / `set_global_registry` / `global_registry()` pattern exactly.

  **Step 4a — Add import at file top** (AGENTS.md Rule 2 — `use` at file top only):
  ```rust
  use crate::extensions::Extension;
  use crate::extensions::SendPtr;
  ```

  **Step 4b — Add `GLOBAL_EXTENSION_MAP` static** (after the `GLOBAL_REGISTRY` declaration, around line 43):
  ```rust
  /// Extension map: extension_id -> raw vtable pointer.
  /// Set once during RuntimeBuilder::build(). Immutable after that.
  static GLOBAL_EXTENSION_MAP: OnceLock<HashMap<u32, SendPtr>> = OnceLock::new();
  ```

  **Step 4c — Add `RuntimeBuilder.extensions` field** (in the `RuntimeBuilder` struct, after `loaders` field):
  ```rust
  extensions: Vec<Box<dyn Extension>>,
  ```
  Update `RuntimeBuilder::new()` to initialize `extensions: Vec::new()`.
  Update `RuntimeBuilder::default()` is auto-derived, no change needed.

  **Step 4d — Add `RuntimeBuilder::extension()` method** (in `impl RuntimeBuilder`, after `loader()`):
  ```rust
  /// Register an extension. If two extensions share the same extension_id, the last one wins.
  ///
  /// Extensions provide optional host-side vtables queryable by plugins at init time.
  pub fn extension(mut self, ext: Box<dyn Extension>) -> RuntimeBuilder {
      self.extensions.push(ext);
      self
  }
  ```

  **Step 4e — Add `Runtime::_extensions` field** (in the `Runtime` struct, after `_bundles`):
  ```rust
  /// Extension impls. Never dropped — keeps vtable memory alive for the Runtime's lifetime.
  _extensions: Vec<Box<dyn Extension>>,
  ```
  Update `impl Send/Sync` safety comments on `Runtime` to mention extensions (they are Send+Sync by trait bound).

  **Step 4f — Populate the map in `build()`** (in `RuntimeBuilder::build()`, after `set_global_registry(...)` at line ~138 and before constructing `HostVTable`):
  ```rust
  // Build extension map: extension_id -> vtable pointer.
  // If GLOBAL_EXTENSION_MAP is already set (e.g., second build() call in tests), silently skip.
  let mut ext_map: HashMap<u32, SendPtr> = HashMap::new();
  for ext in &self.extensions {
      let id: u32 = ext.extension_id();
      let ptr: *const () = ext.vtable_ptr();
      ext_map.insert(id, SendPtr(ptr));
  }
  // OnceLock::set returns Err(value) when already set — expected after first build().
  let _: Result<(), HashMap<u32, SendPtr>> = GLOBAL_EXTENSION_MAP.set(ext_map);
  ```
  Move `self.extensions` into the `Runtime` struct at the end of `build()`:
  ```rust
  Ok(Runtime {
      registry,
      _bundles: bundles,
      host_vtable,
      loaders: loader_map,
      _extensions: self.extensions,  // <— NEW FIELD
  })
  ```

  **Step 4g — Replace `host_get_extension` stub** (around line 444):
  Replace the current stub body with:
  ```rust
  // SAFETY: GLOBAL_EXTENSION_MAP is initialized during RuntimeBuilder::build() and
  // never mutated after that. Reading from OnceLock::get() is lock-free and safe
  // from any thread. SendPtr wraps a *const () to a 'static extension vtable.
  pub(crate) unsafe extern "C" fn host_get_extension(extension_id: u32) -> *const () {
      match GLOBAL_EXTENSION_MAP.get() {
          Some(map) => match map.get(&extension_id) {
              Some(ptr) => ptr.0,
              None => core::ptr::null(),
          },
          None => core::ptr::null(),
      }
  }
  ```
  Note: the function changes from `unsafe extern "C" fn host_get_extension(_extension_id: u32)` to `pub(crate) unsafe extern "C" fn host_get_extension(extension_id: u32)` (remove underscore prefix, add `pub(crate)` for use in `lib.rs`).

  **Step 4h — Add a unit test** in the existing `#[cfg(test)]` block:
  ```rust
  #[test]
  fn host_get_extension_returns_null_for_unknown_id() {
      // SAFETY: host_get_extension reads from OnceLock; no pointer preconditions.
      let ptr: *const () = unsafe { host_get_extension(0xDEAD_BEEF_u32) };
      assert!(ptr.is_null(), "unknown extension_id must return null");
  }
  ```

  **Must NOT do**:
  - Do NOT use `.unwrap()` on `GLOBAL_EXTENSION_MAP.set(...)` — use `let _: Result<...> = ...` pattern (matches existing `set_global_registry` pattern at line 59)
  - Do NOT add explicit type annotation exceptions beyond struct construction/numeric cast
  - Do NOT change `HostVTable` (ABI frozen)
  - Do NOT change `host_get_extension`'s function signature (ABI frozen)
  - All `use` statements MUST remain at file top

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multiple interlocking changes to a critical file; requires careful ordering and pattern-matching
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (parallel with Tasks 3, 5–10b)
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 11, 12
  - **Blocked By**: Tasks 1, 2

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplug/src/runtime/mod.rs:43-65` — `GLOBAL_REGISTRY`, `set_global_registry`, `global_registry()` pattern to mirror exactly for `GLOBAL_EXTENSION_MAP`
  - `crates/polyplug/src/runtime/mod.rs:96-99` — `RuntimeBuilder` struct field declarations
  - `crates/polyplug/src/runtime/mod.rs:103-108` — `RuntimeBuilder::new()` initialization pattern
  - `crates/polyplug/src/runtime/mod.rs:124-127` — `loader()` builder method pattern to follow for `extension()`
  - `crates/polyplug/src/runtime/mod.rs:134-248` — `build()` function — insert extension map population after line ~138
  - `crates/polyplug/src/runtime/mod.rs:439-447` — current `host_get_extension` stub to replace

  **Acceptance Criteria**:
  - [ ] `GLOBAL_EXTENSION_MAP: OnceLock<HashMap<u32, SendPtr>>` static declared
  - [ ] `RuntimeBuilder.extensions: Vec<Box<dyn Extension>>` field exists
  - [ ] `RuntimeBuilder::extension(ext: Box<dyn Extension>) -> RuntimeBuilder` method compiles
  - [ ] `Runtime._extensions: Vec<Box<dyn Extension>>` field exists
  - [ ] `build()` populates the map and moves extensions into Runtime
  - [ ] `host_get_extension` is `pub(crate)` and reads from map (non-stub)
  - [ ] Unit test `host_get_extension_returns_null_for_unknown_id` passes
  - [ ] `cargo test -p polyplug` passes (no regressions in existing tests)

  **QA Scenarios**:
  ```
  Scenario: Existing tests pass with runtime changes
    Tool: Bash (cargo test)
    Preconditions: Tasks 1, 2, 3, 4 all done
    Steps:
      1. Run: cargo test -p polyplug 2>&1
      2. Assert: exit code 0, no FAILED
    Expected Result: All existing tests pass; new unit test passes
    Failure Indicators: Any 'FAILED' in output
    Evidence: .sisyphus/evidence/task-4-cargo-test.txt

  Scenario: host_get_extension returns null for unknown ID
    Tool: Bash (cargo test)
    Preconditions: Tasks 1, 2, 3, 4 done
    Steps:
      1. Run: cargo test -p polyplug -- runtime::tests::host_get_extension_returns_null_for_unknown_id 2>&1
      2. Assert: 'test runtime::tests::host_get_extension_returns_null_for_unknown_id ... ok'
    Expected Result: Test passes
    Failure Indicators: Test FAILED or not found
    Evidence: .sisyphus/evidence/task-4-null-check-unit.txt
  ```

  **Commit**: YES (groups with Tasks 1, 2, 3 in Commit A)

---

- [x] 5. Update Rust generator — emit extension query code in `generate_guest_init_file`

  **What to do**:
  File: `crates/polyplugc/src/generators/rust/mod.rs`

  The function `generate_guest_init_file(out: &mut String, ir: &ValidatedIr)` (starts at line 630) generates the `polyplug_init` function body. Extend it to:
  1. Check if any plugin in the IR's bundle declares `"trace"` as optional:
     ```rust
     let has_trace: bool = ir.bundle.as_ref().map_or(false, |b: &crate::ir::ResolvedBundle| {
         b.plugins.iter().any(|p: &crate::ir::ResolvedPlugin| {
             p.optional.contains(&"trace".to_owned())
         })
     });
     ```
  2. If `has_trace` is true, emit at the TOP of `polyplug_init` (before the per-contract loop), after the registrar null-check:
     ```rust
     if has_trace {
         out.push_str("    // Optional: trace extension\n");
         out.push_str("    const EXT_TRACE_ID: u32 = 0xC4EB9AEE_u32;\n");
         // SAFETY: host.get_extension returns a *const TraceVTable or null.
         // The vtable is owned by the host runtime and valid for the plugin lifetime.
         out.push_str("    // SAFETY: reg.host is a valid HostVTable pointer set by the host.\n");
         out.push_str("    let trace_vtable_ptr: *const () = unsafe { ((*reg.host).get_extension)(EXT_TRACE_ID) };\n");
         out.push_str("    if trace_vtable_ptr.is_null() {\n");
         out.push_str("        // Trace extension not available — continue without tracing.\n");
         out.push_str("    }\n\n");
     }
     ```
  3. If `has_trace` is false, emit nothing extra (unchanged behavior for all existing tests).

  **Must NOT do**:
  - Do NOT change the generated output when `optional` is empty (must not break existing codegen tests)
  - Do NOT add `use` statements inside the `generate_guest_init_file` function body (AGENTS.md Rule 2)
  - Do NOT emit code for any extension name other than `"trace"` in this task

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small conditional block added to one existing function
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2, parallel with Tasks 3, 4, 6–10b)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 11 (integration test needs correct generator output)
  - **Blocked By**: Task 1 (needs IR types, but these are already in scope via existing imports)

  **References**:

  **Pattern References**:
  - `crates/polyplugc/src/generators/rust/mod.rs:630-700` — `generate_guest_init_file` function to extend
  - `crates/polyplugc/src/generators/rust/mod.rs:59-78` — `if let Some(ref bundle) = ir.bundle` pattern for optional access
  - `crates/polyplugc/src/ir/mod.rs:196-219` — `ResolvedPlugin.optional` field and `ResolvedBundle.plugins`

  **Acceptance Criteria**:
  - [ ] When IR bundle has a plugin with `optional: ["trace"]`, generated `init.rs` contains `EXT_TRACE_ID` constant and `get_extension` call
  - [ ] When IR bundle has NO `optional` or empty `optional`, generated `init.rs` is unchanged (no extension code)
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0
  - [ ] Existing codegen tests pass: `cargo test -p polyplug --test integration_codegen_rust`

  **QA Scenarios**:
  ```
  Scenario: Generator emits trace code when optional contains 'trace'
    Tool: Bash (cargo test)
    Preconditions: Task 5 done
    Steps:
      1. Run: cargo test -p polyplug --test integration_extension -- codegen_rust_trace_emitted 2>&1
      2. Assert output contains 'ok'
    Expected Result: Codegen test confirms EXT_TRACE_ID present in generated output
    Failure Indicators: Test FAILED; generated code missing EXT_TRACE_ID
    Evidence: .sisyphus/evidence/task-5-codegen-rust.txt

  Scenario: Existing codegen tests unaffected
    Tool: Bash (cargo test)
    Preconditions: Task 5 done
    Steps:
      1. Run: cargo test -p polyplug --test integration_codegen_rust 2>&1
      2. Assert: exit 0, no FAILED
    Expected Result: All existing codegen_rust tests pass
    Failure Indicators: Any test FAILED
    Evidence: .sisyphus/evidence/task-5-codegen-rust-existing.txt
  ```

  **Commit**: YES (groups with Tasks 6–10b in Commit B)

---

- [x] 6. Update C++ generator — emit extension query code in `generate_init_hpp`

  **What to do**:
  File: `crates/polyplugc/src/generators/cpp/mod.rs`

  Find the `generate_init_hpp(ir: &ValidatedIr)` function (around line 86 in `generate_guest`, called for `guest/init.hpp`). Apply the same conditional logic as Task 5:
  1. Check `has_trace` using the same `ir.bundle` access pattern.
  2. If `has_trace`, emit at the top of the generated `polyplug_init` C function (after null-check, before per-contract registrations):
     ```cpp
     static constexpr uint32_t EXT_TRACE_ID = 0xC4EB9AEEu;
     // Optional: trace extension
     const void* trace_vtable_ptr = host->get_extension(EXT_TRACE_ID);
     // trace_vtable_ptr is null if the host does not provide the trace extension.
     (void)trace_vtable_ptr;  // suppress unused warning if not used further
     ```
  3. If `has_trace` is false, emit nothing extra.

  **Must NOT do**:
  - Do NOT change generated output when `optional` is empty
  - Do NOT add `use` inside functions

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Parallel Group**: Wave 2 (with Tasks 3, 4, 5, 7–10b)
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/cpp/mod.rs:59-103` — `generate_guest` function, `generate_init_hpp` call at line 86
  - `crates/polyplugc/src/generators/cpp/mod.rs` offset ~86–200 — `generate_init_hpp` function definition
  - `crates/polyplugc/src/ir/mod.rs:196-219` — `ResolvedPlugin.optional` access pattern

  **Acceptance Criteria**:
  - [ ] `generate_init_hpp` emits `EXT_TRACE_ID` constant + `get_extension` call when `has_trace`
  - [ ] Empty optional → no change to generated output
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0
  - [ ] Existing codegen cpp tests pass: `cargo test -p polyplug --test integration_codegen_cpp`

  **QA Scenarios**:
  ```
  Scenario: C++ generator emits trace code
    Tool: Bash (cargo test)
    Preconditions: Task 6 done
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_cpp_trace_emitted 2>&1
      2. Assert 'ok'
    Expected Result: Generated init.hpp contains EXT_TRACE_ID
    Evidence: .sisyphus/evidence/task-6-codegen-cpp.txt
  ```

  **Commit**: YES (groups with Tasks 5, 7–10b in Commit B)

---

- [x] 7. Update C# generator — emit extension query code

  **What to do**:
  File: `crates/polyplugc/src/generators/csharp/mod.rs`

  Find the guest init file generation (analogous to `generate_guest_init_file` in other generators). Locate the function that generates the guest plugin init code and apply the same conditional:
  1. Check `has_trace` from `ir.bundle`.
  2. If `has_trace`, emit in the generated init method:
     ```csharp
     const uint ExtTraceId = 0xC4EB9AEEu;
     // Optional: trace extension
     IntPtr traceVtablePtr = PolyplugHost.GetExtension(ExtTraceId);
     // traceVtablePtr is IntPtr.Zero if trace extension not available
     ```
  3. If false, no emission.

  **Must NOT do**:
  - Do NOT change other generator output
  - Do NOT add `use` inside functions (AGENTS.md Rule 2)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/csharp/mod.rs` — find `generate_guest` and the guest init generation function; read file before editing
  - `crates/polyplugc/src/ir/mod.rs:196-219` — `ResolvedPlugin.optional` pattern

  **Acceptance Criteria**:
  - [ ] C# guest init emits `ExtTraceId` constant + `GetExtension` call when `has_trace`
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: C# generator emits trace code
    Tool: Bash (cargo test)
    Preconditions: Task 7 done
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_csharp_trace_emitted 2>&1
    Expected Result: Generated C# init contains ExtTraceId
    Evidence: .sisyphus/evidence/task-7-codegen-csharp.txt
  ```

  **Commit**: YES (Commit B)

---

- [x] 8. Update Python generator — emit extension query code

  **What to do**:
  File: `crates/polyplugc/src/generators/python/mod.rs`

  **Read the file before editing** to understand how `generate_guest()` is structured. Python currently generates `types.py` and `contracts.py` but has NO init file.

  - Add a `generate_init_py(ir: &ValidatedIr) -> String` function (or inline in `generate_guest()`).
  - Call it from `generate_guest()` and write the result to `out_dir/guest/init.py`.
  - Compute `has_trace` using the same ir.bundle pattern as all other generators.
  - When `has_trace` is true, emit:
  ```python
  EXT_TRACE_ID = 0xC4EB9AEE
  # Optional: trace extension
  trace_vtable_ptr = polyplug.get_extension(EXT_TRACE_ID)
  # trace_vtable_ptr is None/0 if not available
  ```
  - When `has_trace` is false, emit a minimal file with just a comment: `# No optional extensions requested.`
  - The file must always be generated (even when empty) so it is available for import.

  Same AGENTS.md constraints as all other tasks.

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/python/mod.rs` — read before editing; find `generate_guest()` and any existing init-file pattern
  - `crates/polyplugc/src/generators/rust/mod.rs:630-700` — `generate_guest_init_file` as the reference model
  - `crates/polyplugc/src/ir/mod.rs:196-219` — `ResolvedPlugin.optional` access pattern

  **Acceptance Criteria**:
  - [ ] `guest/init.py` is written by `generate_guest()`
  - [ ] When `optional = ["trace"]`: file contains `EXT_TRACE_ID` and `get_extension` call
  - [ ] When `optional = []`: file contains only the empty-comment placeholder
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: Python generator emits trace code
    Tool: Bash (cargo test)
    Preconditions: Task 8 done
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_python_trace_emitted 2>&1 | tee .sisyphus/evidence/task-8-codegen-python.txt
    Expected Result: Generated Python init.py contains EXT_TRACE_ID
    Failure Indicators: file not created, or EXT_TRACE_ID missing from content
    Evidence: .sisyphus/evidence/task-8-codegen-python.txt
  ```

  **Commit**: YES (Commit B)

---

- [x] 9. Update Lua generator — emit extension query code

  **What to do**:
  File: `crates/polyplugc/src/generators/lua/mod.rs`

  **Read the file before editing** to understand how `generate_guest()` is structured. Lua currently generates `types.lua` and `contracts.lua` but has NO init file.

  - Add a `generate_init_lua(ir: &ValidatedIr) -> String` function (or inline in `generate_guest()`).
  - Call it from `generate_guest()` and write the result to `out_dir/guest/init.lua`.
  - Compute `has_trace` using the ir.bundle pattern (same as all other generators).
  - When `has_trace` is true, emit:
  ```lua
  local EXT_TRACE_ID = 0xC4EB9AEE
  -- Optional: trace extension
  local trace_vtable_ptr = polyplug.get_extension(EXT_TRACE_ID)
  -- trace_vtable_ptr is nil/0 if not available
  ```
  - When `has_trace` is false, emit a minimal file: `-- No optional extensions requested.`
  - The file must always be generated.

  Same AGENTS.md constraints as all other tasks.

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/lua/mod.rs` — read before editing; find `generate_guest()` and how output files are written
  - `crates/polyplugc/src/generators/rust/mod.rs:630-700` — `generate_guest_init_file` as reference model
  - `crates/polyplugc/src/ir/mod.rs:196-219` — `ResolvedPlugin.optional` access pattern

  **Acceptance Criteria**:
  - [ ] `guest/init.lua` is written by `generate_guest()`
  - [ ] When `optional = ["trace"]`: file contains `EXT_TRACE_ID` and `get_extension` call
  - [ ] When `optional = []`: file contains only the empty-comment placeholder
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: Lua generator emits trace code
    Tool: Bash (cargo test)
    Preconditions: Task 9 done
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_lua_trace_emitted 2>&1 | tee .sisyphus/evidence/task-9-codegen-lua.txt
    Expected Result: Generated Lua init.lua contains EXT_TRACE_ID
    Failure Indicators: file not created, or EXT_TRACE_ID missing from content
    Evidence: .sisyphus/evidence/task-9-codegen-lua.txt
  ```

  **Commit**: YES (Commit B)

---

- [x] 10a. Update js-quickjs generator — emit extension query code in `generate_init_ts`

  **What to do**:
  File: `crates/polyplugc/src/generators/js_quickjs/mod.rs`

  The current `generate_init_ts(ir: &ValidatedIr)` function (line 194) is a stub. Update it to:
  1. Check `has_trace` from `ir.bundle`.
  2. Keep the existing stub structure but add trace code when `has_trace`:
     ```typescript
     const EXT_TRACE_ID: number = 0xC4EB9AEE;
     // Optional: trace extension
     // QuickJS uses lo/hi u32 split for pointer-sized values
     const { lo: traceLo, hi: traceHi } = polyplug.getExtension(EXT_TRACE_ID);
     // traceLo and traceHi are both 0 if extension not available
     ```
  3. The `polyplug.getExtension` call returns `{ lo: number; hi: number }` per QuickJS's lo/hi ABI convention.
  4. Remove the line `let _: &ValidatedIr = ir;` (no longer needed when ir is used).

  **Must NOT do**:
  - Do NOT design a full TS vtable binding system — only emit the query and null-check
  - Do NOT remove the existing `// Dependency resolution...` comments

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/js_quickjs/mod.rs:194-208` — `generate_init_ts` stub to update
  - `crates/polyplugc/src/generators/js_quickjs/mod.rs:91-113` — lo/hi type conventions in this generator

  **Acceptance Criteria**:
  - [ ] `generate_init_ts` uses `ir` rather than ignoring it
  - [ ] When `has_trace`, emits `EXT_TRACE_ID` + `getExtension` call
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0 (no unused variable warnings)

  **QA Scenarios**:
  ```
  Scenario: js-quickjs generator emits trace code
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_js_quickjs_trace_emitted 2>&1
    Expected Result: Generated init.ts contains EXT_TRACE_ID
    Evidence: .sisyphus/evidence/task-10a-codegen-js-quickjs.txt
  ```

  **Commit**: YES (Commit B)

---

- [x] 10b. Update js-deno generator — emit extension query code in `generate_init_ts`

  **What to do**:
  File: `crates/polyplugc/src/generators/js_deno/mod.rs`

  Same pattern as Task 10a, but Deno uses `Deno.core.ops.op_get_extension(EXT_TRACE_ID)` and returns a `BigInt` (64-bit result, not lo/hi split):
  ```typescript
  const EXT_TRACE_ID: number = 0xC4EB9AEE;
  // Optional: trace extension
  // Deno returns BigInt for pointer-sized values
  const traceVtablePtr: bigint = Deno.core.ops.op_get_extension(EXT_TRACE_ID);
  // traceVtablePtr is 0n if extension not available
  ```
  Read the js-deno generator file before editing to confirm the analogous `generate_init_ts` structure.

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 2)
  - **Blocks**: Task 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/js_deno/mod.rs` — read before editing; find `generate_init_ts`
  - `crates/polyplugc/src/generators/js_quickjs/mod.rs:194-208` — reference pattern (same structure, different API call)

  **Acceptance Criteria**:
  - [ ] `generate_init_ts` uses `ir` when `has_trace`
  - [ ] Emits `Deno.core.ops.op_get_extension(EXT_TRACE_ID)` with `bigint` return type
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: js-deno generator emits trace code
    Tool: Bash (cargo test)
    Steps:
      1. cargo test -p polyplug --test integration_extension -- codegen_js_deno_trace_emitted 2>&1
    Expected Result: Generated init.ts contains EXT_TRACE_ID and bigint usage
    Evidence: .sisyphus/evidence/task-10b-codegen-js-deno.txt
  ```

  **Commit**: YES (Commit B)

---

- [x] 11. Create `tests/integration_extension/mod.rs` and register in `crates/polyplug/Cargo.toml`

  **What to do**:
  - Create the new file `tests/integration_extension/mod.rs` as the crate root (AGENTS.md Rule 1).
  - Add the file-level header comment marking it as an integration test crate root.
  - Add `#![allow(clippy::expect_used)]` at the top — required by AGENTS.md for test-only `.expect()` usage.
  - Declare all `use` imports at file top (AGENTS.md Rule 2). Needed imports:
    - `polyplug::extensions::Extension`
    - `polyplug::extensions::trace::EXT_TRACE_ID`
    - `polyplug::extensions::trace::TraceExtension`
    - `polyplug::extensions::trace::TraceVTable`
    - `polyplug::abi::StringView`
    - `polyplug::polyplug_get_extension` (the public extern C function, used instead of the pub(crate) host_get_extension)
    - `polyplug::runtime::Runtime`
    - `std::sync::Arc`
    - `std::sync::OnceLock`
    - `std::sync::atomic::AtomicBool`
    - `std::sync::atomic::Ordering`
    - `std::path::Path`
    - `std::path::PathBuf`
    - `std::process::Command`
    - `std::process::Output`
  - Define `CounterExtension` as a TEST-ONLY struct that implements `Extension`:
    ```rust
    struct CounterExtension {
        id: u32,
        vtable_ptr: *const (),
    }
    // SAFETY: CounterExtension holds a *const () pointing to a static vtable.
    // The pointer never changes after construction, so Send + Sync are safe.
    unsafe impl Send for CounterExtension {}
    unsafe impl Sync for CounterExtension {}
    impl Extension for CounterExtension {
        fn extension_id(&self) -> u32 { self.id }
        fn vtable_ptr(&self) -> *const () { self.vtable_ptr }
    }
    ```
    The `vtable_ptr` should point to a static unit value (`&() as *const () as *const _`) — its value is not called, only its non-null-ness is asserted.
  - Define a module-level `OnceLock`-guarded `ensure_runtime_built()` helper and a companion `CALLBACK_FLAG` static. Exact design:
    ```rust
    static CALLBACK_FLAG: OnceLock<Arc<AtomicBool>> = OnceLock::new();
    fn ensure_runtime_built() {
        static SETUP: OnceLock<()> = OnceLock::new();
        SETUP.get_or_init(|| {
            let flag: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
            let _ = CALLBACK_FLAG.set(flag.clone());
            let cb_flag: Arc<AtomicBool> = flag;
            Runtime::builder()
                .extension(Box::new(TraceExtension::new(move |_msg: &str| {
                    cb_flag.store(true, Ordering::SeqCst);
                })))
                .build()
                .expect("runtime build must succeed");
        });
    }
    ```
    This ensures `GLOBAL_EXTENSION_MAP` is populated exactly once, and `CALLBACK_FLAG` is accessible to the `trace_callback_invoked` test.
    Additional imports needed at file top: `use std::sync::Arc;`, `use std::sync::atomic::AtomicBool;`, `use std::sync::atomic::Ordering;`.
  - Write the following 5 `#[test]` functions:
    1. `extension_registered_returns_non_null`:
       - Call `ensure_runtime_built()`.
       - Use the public C-ABI function (NOT the `pub(crate)` internal): `let ptr: *const () = unsafe { polyplug::polyplug_get_extension(core::ptr::null(), EXT_TRACE_ID) };`
       - Assert the returned pointer is non-null.
    2. `extension_absent_returns_null`:
       - Use a **different** test binary strategy — this test cannot share `GLOBAL_EXTENSION_MAP` state with test 1. Since `OnceLock` is set-once per process, create a subprocess or accept that this scenario is verified by unit tests in `runtime/mod.rs` instead. Document this limitation with a `#[ignore]` attribute and a comment explaining why.
       - Alternatively: assert that calling `polyplug::polyplug_get_extension(core::ptr::null(), 0xDEAD_0000_u32)` returns null even after the map is set — because the ID won't match any registered extension.
    3. `trace_callback_invoked`:
       - Call `ensure_runtime_built()` (populates GLOBAL_EXTENSION_MAP if not already done).
       - Get the pointer using the public API: `let ptr: *const () = unsafe { polyplug::polyplug_get_extension(core::ptr::null(), EXT_TRACE_ID) };`
       - Assert `!ptr.is_null()`.
       - Cast it: `let vtable: *const TraceVTable = ptr as *const TraceVTable;`
       - Call emit passing both msg AND state (the state field from the vtable):
         `// SAFETY: ptr non-null, vtable valid for Runtime lifetime; state is leaked TraceState`
         `unsafe { ((*vtable).emit)(StringView::from_static(b"hello"), (*vtable).state) };`
       - Read `CALLBACK_FLAG.get().expect("flag set in ensure_runtime_built").load(Ordering::SeqCst)` and assert it is `true`.
    4. `counter_extension_custom`:
       - This test cannot share the same runtime as tests 1/3 (OnceLock is already set).
       - Instead: assert a `CounterExtension` instance correctly reports its `extension_id()` and `vtable_ptr()` — this validates the trait implementation without requiring a new Runtime.
       - Construct `CounterExtension { id: 0xAABB_CCDD_u32, vtable_ptr: &() as *const () as *const _ }` and assert `ce.extension_id() == 0xAABB_CCDD_u32` and `!ce.vtable_ptr().is_null()`.
    5. Codegen assertion tests (one per generator, 7 total):
       - For each generator: `codegen_rust_trace_emitted`, `codegen_cpp_trace_emitted`, `codegen_csharp_trace_emitted`, `codegen_python_trace_emitted`, `codegen_lua_trace_emitted`, `codegen_js_quickjs_trace_emitted`, `codegen_js_deno_trace_emitted`.
       - **DO NOT call generator Rust functions directly** (would require `polyplugc` as library dep). Instead, use the binary approach:
         1. Write a minimal `bundle.toml` with `optional = ["trace"]` to a temp dir (use `tempfile` or `std::env::temp_dir()`).
         2. Call `Command::new(env!("CARGO_BIN_EXE_polyplugc")).arg("generate").arg("--api").arg(...).arg("--lang").arg("rust").arg("--out").arg(temp_dir).output().expect("polyplugc failed")`.
         3. Read the generated init file from the output dir.
         4. Assert the file content contains `"EXT_TRACE_ID"` (or language-appropriate constant).
       - Reuse the `run_polyplugc` helper pattern from `tests/integration_codegen_rust/mod.rs:47-58` — the file already shows the exact binary invocation pattern.
       - The `bundle.toml` used must be a minimal fixture with one plugin whose `optional` array contains `"trace"`; all other fields can use minimal valid values.
  - Register the new test binary in `crates/polyplug/Cargo.toml` by appending:
    ```toml
    [[test]]
    name = "integration_extension"
    path = "../../tests/integration_extension/mod.rs"
    ```

  **Must NOT do**:
  - Do NOT place `CounterExtension` anywhere outside of `tests/` or test-gated code.
  - Do NOT call `.unwrap()` on `OnceLock::set()` — use `.ok()` to discard the `Err` when already set is expected.
  - Do NOT modify `abi/mod.rs`.
  - Do NOT add `polyplugc` as a library dev-dependency to `crates/polyplug/Cargo.toml`. Instead, the 7 codegen assertion tests MUST invoke `polyplugc` as a binary using `Command::new(env!("CARGO_BIN_EXE_polyplugc"))` — the same pattern used in `tests/integration_codegen_rust/mod.rs`. This avoids a circular crate dependency and matches existing conventions.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Requires coordinating multiple test scenarios, OnceLock isolation strategy, and unsafe casting to TraceVTable — not trivial.
  - **Skills**: []
  - **Skills Evaluated but Omitted**:
    - `playwright`: No UI. Pure Rust test code.

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (sequential after Wave 2)
  - **Blocks**: F1, F2, F3
  - **Blocked By**: Tasks 1–10b (needs Extension trait, EXT_TRACE_ID, TraceExtension, TraceVTable, Runtime::builder, and all generators)

  **References**:

  **Pattern References** (existing code to follow):
  - `tests/integration_load/mod.rs:1-60` — File header, `#![allow(clippy::expect_used)]`, import style, test structure
  - `tests/integration_codegen_rust/mod.rs:1-60` — How to construct `ValidatedIr` and call a generator for codegen assertion tests
  - `crates/polyplug/src/runtime/mod.rs:503-540` — The `ensure_test_plugin_registered` / `OnceLock::get_or_init` pattern to replicate for `ensure_runtime_built`

  **API/Type References** (contracts to implement against):
  - `crates/polyplug/src/extensions/mod.rs` (created in Task 1) — `Extension` trait: `extension_id() -> u32`, `vtable_ptr() -> *const ()`
  - `crates/polyplug/src/extensions/trace/mod.rs` (created in Task 2) — `EXT_TRACE_ID: u32`, `TraceVTable { emit: unsafe extern "C" fn(StringView), state: *const () }`, `TraceExtension::new(impl Fn(&str) + Send + Sync + 'static)`
  - `crates/polyplug/src/runtime/mod.rs` (modified in Task 4) — `Runtime::builder().with_extension(Box<dyn Extension>).build()`, `polyplug::runtime::host_get_extension(u32) -> *const ()`
  - `crates/polyplug/Cargo.toml:90-104` — Existing `[[test]]` entries to replicate the registration format

  **Test References**:
  - `tests/integration_load/mod.rs` — Complete test file to understand ABI pattern
  - `crates/polyplug/src/runtime/mod.rs:449-593` — Unit tests including `ensure_test_plugin_registered` OnceLock pattern

  **Acceptance Criteria**:
  - [ ] `tests/integration_extension/mod.rs` created
  - [ ] `crates/polyplug/Cargo.toml` has `[[test]] name = "integration_extension"` entry
  - [ ] `cargo test -p polyplug --test integration_extension` exits 0 with all non-ignored tests passing
  - [ ] `cargo clippy -p polyplug -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: Happy path — registered extension is reachable
    Tool: Bash (cargo test)
    Preconditions: Tasks 1–4 complete (Extension trait + Runtime wiring done)
    Steps:
      1. Run: cargo test -p polyplug --test integration_extension -- extension_registered_returns_non_null 2>&1 | tee .sisyphus/evidence/task-11-ext-registered.txt
      2. Assert: output contains "test extension_registered_returns_non_null ... ok"
    Expected Result: Test passes, pointer returned is non-null
    Failure Indicators: "FAILED" in output, or "null pointer" assertion error
    Evidence: .sisyphus/evidence/task-11-ext-registered.txt

  Scenario: Trace callback is invoked through vtable
    Tool: Bash (cargo test)
    Preconditions: Tasks 1–4 complete
    Steps:
      1. Run: cargo test -p polyplug --test integration_extension -- trace_callback_invoked 2>&1 | tee .sisyphus/evidence/task-11-trace-callback.txt
      2. Assert: output contains "test trace_callback_invoked ... ok"
    Expected Result: Test passes, callback flag/value was set
    Failure Indicators: "FAILED", "assertion failed", or panic in unsafe cast
    Evidence: .sisyphus/evidence/task-11-trace-callback.txt

  Scenario: Codegen trace assertions — all 7 generators
    Tool: Bash (cargo test)
    Preconditions: Tasks 5–10b complete (all generators updated)
    Steps:
      1. Run: cargo test -p polyplug --test integration_extension -- codegen 2>&1 | tee .sisyphus/evidence/task-11-codegen-all.txt
      2. Assert: all 7 codegen_*_trace_emitted tests show "ok"
    Expected Result: 7/7 tests pass
    Failure Indicators: Any "FAILED" line, or "does not contain" assertion
    Evidence: .sisyphus/evidence/task-11-codegen-all.txt

  Scenario: Failure case — unknown extension ID returns null
    Tool: Bash (cargo test)
    Preconditions: Tasks 1–4 complete
    Steps:
      1. Run: cargo test -p polyplug --test integration_extension -- extension_absent_returns_null 2>&1 | tee .sisyphus/evidence/task-11-ext-absent.txt
      2. Assert: output contains "ok" (even if test is #[ignore], document why)
    Expected Result: Unknown ID 0xDEAD_0000 returns null pointer from host_get_extension
    Failure Indicators: Test panics or returns non-null for unknown ID
    Evidence: .sisyphus/evidence/task-11-ext-absent.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-11-ext-registered.txt` — stdout of the registration test run
  - [ ] `.sisyphus/evidence/task-11-trace-callback.txt` — stdout of the callback test run
  - [ ] `.sisyphus/evidence/task-11-codegen-all.txt` — stdout of all codegen tests run
  - [ ] `.sisyphus/evidence/task-11-ext-absent.txt` — stdout of the absent-ID test run

  **Commit**: YES (Commit C)
  - Message: `test(extension): add integration_extension test binary`
  - Files: `tests/integration_extension/mod.rs`, `crates/polyplug/Cargo.toml`
  - Pre-commit: `cargo test -p polyplug --test integration_extension`

---

- [x] 12. Add `bench_absent_extension_null_check` to `crates/polyplug/benches/vtable_dispatch.rs`

  **What to do**:
  - Open `crates/polyplug/benches/vtable_dispatch.rs` and read it fully before editing.
  - Add a new benchmark function `bench_absent_extension_null_check` after the last existing benchmark function (before the `criterion_group!` macro). Follow the same structure as `bench_dispatch_noop`:
    - Reset `BENCH_REGISTRY` at the start.
    - Load the test plugin with `load_and_init_plugin(TEST_PLUGIN_SO)` — this is needed so a Runtime-equivalent state is established.
    - Create a `Criterion` benchmark group named `"dispatch"`.
    - Set `group.throughput(Throughput::Elements(1))`.
    - Inside `group.bench_function(BenchmarkId::new("absent_extension_null_check", "unknown_id"), |b| { ... })`:
      - Call `black_box(unsafe { bench_get_extension(black_box(0xDEAD_0000_u32)) })` in the iter loop.
      - **Note:** `bench_get_extension` is the local bench stub (line 158) that always returns null. This benchmark measures the overhead of a function pointer call to a null-returning function — the minimum floor cost for extension queries. It does NOT exercise the real `GLOBAL_EXTENSION_MAP`; that would require a separate bench setup.
      - This measures the cost of `host_get_extension` with an ID not in the map — exercises the `OnceLock` read path and HashMap miss.
    - Call `group.finish()` and `core::mem::forget(_library)`.
    - Add doc-comment: `/// Measures host_get_extension overhead when the requested extension ID is not registered.`
    - All `unsafe` blocks must have `// SAFETY:` comments. The benchmark calls `bench_get_extension` which is `unsafe extern "C"` — wrap with SAFETY comment.
  - Add `bench_absent_extension_null_check` to the `criterion_group!` macro list.
  - Explicit type annotations required on all `let` bindings (AGENTS.md Rule 3).

  **Must NOT do**:
  - Do NOT import `polyplug::extensions` or `TraceExtension` — this bench file tests the ABI-level `bench_get_extension` stub, not the high-level extension API.
  - Do NOT add new dependencies to `Cargo.toml`.
  - Do NOT modify any existing benchmark function.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file, clearly defined pattern to follow, mechanical addition.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, alongside Task 11)
  - **Parallel Group**: Wave 3 (with Task 13)
  - **Blocks**: F1, F2
  - **Blocked By**: Tasks 1–4 (needs `bench_get_extension` stub in vtable, set by Task 4's HostVTable wiring)

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplug/benches/vtable_dispatch.rs:215-250` — `bench_dispatch_noop` function: exact structure to replicate
  - `crates/polyplug/benches/vtable_dispatch.rs:154-160` — `bench_get_extension` stub: the function being benchmarked
  - `crates/polyplug/benches/vtable_dispatch.rs:444-450` — `criterion_group!` macro: add the new function name here

  **Acceptance Criteria**:
  - [ ] `bench_absent_extension_null_check` function exists in the file
  - [ ] Function is listed in `criterion_group!(benches, ...)`
  - [ ] `cargo bench -p polyplug --bench vtable_dispatch -- --test` exits 0 (compiles and runs in test mode)
  - [ ] `cargo clippy -p polyplug -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: Benchmark compiles and runs without panic
    Tool: Bash
    Preconditions: Task 4 done (bench_get_extension stub wired into HostVTable)
    Steps:
      1. Run: cargo bench -p polyplug --bench vtable_dispatch -- --test 2>&1 | tee .sisyphus/evidence/task-12-bench-compile.txt
      2. Assert: output contains "absent_extension_null_check" and "ok"
    Expected Result: Benchmark compiles, the new bench function runs without panic
    Failure Indicators: compile error, missing symbol, or "FAILED" line
    Evidence: .sisyphus/evidence/task-12-bench-compile.txt

  Scenario: Failure — absent ID does not panic or UB
    Tool: Bash
    Preconditions: Same as above
    Steps:
      1. Run: cargo bench -p polyplug --bench vtable_dispatch -- absent_extension_null_check 2>&1 | tee .sisyphus/evidence/task-12-bench-run.txt
      2. Assert: No "SIGSEGV", no "thread panicked", output shows timing line
    Expected Result: Benchmark runs cleanly and reports a time measurement
    Failure Indicators: crash, panic, or timeout
    Evidence: .sisyphus/evidence/task-12-bench-run.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-12-bench-compile.txt` — stdout of `-- --test` run
  - [ ] `.sisyphus/evidence/task-12-bench-run.txt` — stdout of full bench run

  **Commit**: YES (Commit C, groups with Task 11 and 13)
  - Message: `bench(extension): add absent_extension_null_check benchmark`
  - Files: `crates/polyplug/benches/vtable_dispatch.rs`
  - Pre-commit: `cargo bench -p polyplug --bench vtable_dispatch -- --test`

---

- [x] 13. Update `BENCHMARKS.md` with new benchmark row and Epic History entry

  **What to do**:
  - Open `BENCHMARKS.md` and read it fully before editing.
  - In the **Results table**, add a new row after the last existing row (`dispatch/cross_plugin`):
    ```markdown
    | dispatch/absent_extension_null_check | TBD | TBD | `bench_get_extension` stub null-return — floor cost of a function pointer call via HostVTable.get_extension | NO |
    ```
    - Mean and Std Dev are `TBD` because the benchmark hasn't been run in a baseline environment yet.
    - "Epic 6 Baseline" column is `NO` — this bench was added in the extension epic, not Epic 6.
  - In the **Epic History table**, add a new row after the `Epic 6` row:
    ```markdown
    | Extension System | 2026-03-10 | Extension trait, TraceExtension, GLOBAL_EXTENSION_MAP wiring, all 7 generators updated, integration tests added |
    ```
    Use today's actual date if different from `2026-03-10`.
  - Do not touch any other section of `BENCHMARKS.md`.

  **Must NOT do**:
  - Do NOT change existing benchmark rows or their values.
  - Do NOT change the Methodology section.
  - Do NOT rename columns.

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Pure markdown table edit, two rows added, no code involved.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3, alongside Tasks 11 and 12)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1, F3
  - **Blocked By**: Task 12 (must know the benchmark exists before documenting it)

  **References**:

  **Pattern References** (existing content to follow):
  - `BENCHMARKS.md:13-19` — Results table header and existing rows: exact column order and pipe formatting to match
  - `BENCHMARKS.md:29-31` — Epic History table: exact column order and row format to match

  **Acceptance Criteria**:
  - [ ] Results table has 5 rows (was 4, now 5)
  - [ ] New row mentions `absent_extension_null_check` and `TBD` values
  - [ ] Epic History table has 2 rows (was 1, now 2)
  - [ ] New history row mentions Extension System
  - [ ] `cargo fmt --check` still exits 0 (Markdown not affected, but verifies nothing else broke)

  **QA Scenarios**:
  ```
  Scenario: Happy path — new rows present
    Tool: Bash (grep)
    Preconditions: File edited
    Steps:
      1. Run: grep -c 'absent_extension_null_check' BENCHMARKS.md 2>&1 | tee .sisyphus/evidence/task-13-bench-row.txt
      2. Assert: output is "1" (exactly one match)
      3. Run: grep -c 'Extension System' BENCHMARKS.md 2>&1 >> .sisyphus/evidence/task-13-bench-row.txt
      4. Assert: output is "1"
    Expected Result: Both new rows are present exactly once
    Failure Indicators: grep returns 0 (missing) or 2+ (duplicated)
    Evidence: .sisyphus/evidence/task-13-bench-row.txt

  Scenario: Failure case — existing rows not modified
    Tool: Bash (grep)
    Preconditions: File edited
    Steps:
      1. Run: grep -c 'dispatch/cross_plugin' BENCHMARKS.md 2>&1 | tee .sisyphus/evidence/task-13-existing-rows.txt
      2. Assert: output is "1" (existing row untouched)
      3. Run: grep -c 'Epic 6' BENCHMARKS.md 2>&1 >> .sisyphus/evidence/task-13-existing-rows.txt
      4. Assert: output is "1" (existing Epic 6 row untouched)
    Expected Result: All pre-existing rows are still present with their original content
    Failure Indicators: count is 0 (row was deleted or renamed)
    Evidence: .sisyphus/evidence/task-13-existing-rows.txt
  ```

  **Evidence to Capture**:
  - [ ] `.sisyphus/evidence/task-13-bench-row.txt` — grep output confirming new rows
  - [ ] `.sisyphus/evidence/task-13-existing-rows.txt` — grep output confirming old rows intact

  **Commit**: YES (Commit C, groups with Tasks 11 and 12)
  - Message: `docs(benchmarks): add absent_extension_null_check row and extension epic history`
  - Files: `BENCHMARKS.md`
  - Pre-commit: none (docs only)

---

## Final Verification Wave

> 3 review agents run in PARALLEL. ALL must APPROVE.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read files, run `cargo test`). For each "Must NOT Have": search for forbidden patterns (`grep -r "\.unwrap()" crates/polyplug/src/extensions/`, `grep -r "\.rs\b" crates/polyplug/src/ | grep -v "mod\.rs"`). Check evidence files exist in `.sisyphus/evidence/`.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -p polyplug -- -D warnings`, `cargo clippy -p polyplugc -- -D warnings`, `cargo fmt --check`, `cargo test -p polyplug --test integration_extension`. Check all new files for AGENTS.md violations: `use` inside functions, missing type annotations, unwrap/expect in production, unsafe without SAFETY comments, module root naming violations.
  Output: `Clippy [PASS/FAIL] | Fmt [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [x] F3. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual git diff. Verify 1:1 — nothing missing, nothing extra. Flag: TRUST_MODEL.md touched? New crate deps added? `CounterExtension` in production code? `extensions.rs` or `trace.rs` created instead of `mod.rs`? `unsafe` without SAFETY? Cross-task file contamination?
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- **Commit A** (after Tasks 1–4): `feat(extensions): add Extension trait, TraceExtension, and GLOBAL_EXTENSION_MAP wiring`
  - Files: `crates/polyplug/src/extensions/mod.rs`, `crates/polyplug/src/extensions/trace/mod.rs`, `crates/polyplug/src/runtime/mod.rs`, `crates/polyplug/src/lib.rs`
  - Pre-commit: `cargo clippy -p polyplug -- -D warnings && cargo fmt --check`

- **Commit B** (after Tasks 5–10b): `feat(codegen): emit optional extension query code in all 7 generators`
  - Files: all 7 generator `mod.rs` files
  - Pre-commit: `cargo clippy -p polyplugc -- -D warnings && cargo fmt --check`

- **Commit C** (after Tasks 11–13): `test(extension): add integration tests and vtable dispatch benchmark`
  - Files: `tests/integration_extension/mod.rs`, `crates/polyplug/Cargo.toml`, `crates/polyplug/benches/vtable_dispatch.rs`, `BENCHMARKS.md`
  - Pre-commit: `cargo test -p polyplug --test integration_extension && cargo bench -p polyplug --bench vtable_dispatch -- --test`

---

## Success Criteria

### Verification Commands
```bash
cargo test -p polyplug --test integration_extension    # Expected: all tests pass
cargo clippy -p polyplug -- -D warnings               # Expected: zero warnings
cargo clippy -p polyplugc -- -D warnings              # Expected: zero warnings
cargo fmt --check                                      # Expected: no diffs
cargo bench -p polyplug --bench vtable_dispatch -- --test  # Expected: compiles + exits 0
```

### Final Checklist
- [x] All "Must Have" present
- [x] All "Must NOT Have" absent
- [x] All integration tests pass
- [x] Clippy clean on polyplug and polyplugc
- [x] Bench compiles and runs
