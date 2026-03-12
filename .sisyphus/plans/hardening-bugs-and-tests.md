# polyplug — Internal Hardening: Bug Fixes & Test Coverage

## TL;DR

> **Quick Summary**: Fix 3 active production bugs (UB from `from_utf8_unchecked` on plugin data,
> missing FFI null-pointer guards, undetected double-frees) and fill 5 test coverage gaps. No ABI
> changes, no public API changes — pure internal hardening.
>
> **Deliverables**:
> - `PolyplugError::InvalidUtf8 { context: String }` variant added to `RuntimeError`
> - All `from_utf8_unchecked` on plugin-provided data replaced with `from_utf8` + SAFETY comments on remaining host-owned sites
> - `polyplug_load_bundle`, `polyplug_reload_bundle`, `polyplug_rt_find_all_by_contract`, `polyplug_rt_resolve_plugin` null-guarded
> - `TrackingAllocator` panics on double-free in `#[cfg(debug_assertions)]` builds
> - 6 new test files + 1 new test added to existing file
> - 2 new fixture crates + 1 C fixture + build.rs/Cargo.toml wiring
> - `TRUST_MODEL.md` + PRD section 27 updated with crash-isolation non-goal
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 3 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 6 → Task 9 → Task 10

---

## Context

### Original Request
Fix 3 production bugs and add test coverage for 5 correctness gaps. All decisions pre-answered.
No public API changes. No ABI changes. Pure internal hardening.

### Key Discoveries from Codebase Analysis

**FIX 1 — from_utf8_unchecked audit findings:**
- `crates/polyplug/src/loader/mod.rs`: Does NOT use `from_utf8_unchecked` on plugin data — loader synthesises contract_name as `format!("contract_{:#x}", vtable_contract_id)`. ✅ No change needed.
- `crates/polyplug/src/ffi/mod.rs:94,134`: Already uses `from_utf8` for path bytes. ✅ Already correct.
- `crates/polyplug/src/abi/mod.rs:67`: `StringView::as_str()` — host-owned data, `from_utf8_unchecked` acceptable, has SAFETY comment. Needs comment improvement only.
- `crates/polyplug/src/extensions/trace/mod.rs:42`: `from_utf8_unchecked` on `msg` from plugin via trace thunk. Plugin-provided but ABI doc says "msg is valid UTF-8". Needs improved SAFETY comment clarifying the trust basis (ABI contract, not blind trust).
- `tests/` (11 files): `core::str::from_utf8_unchecked(bytes)` in `registry_register_callback` on `desc.contract_name`. These are test helpers with C-ABI callbacks — cannot return `Result`. These get `// SAFETY:` comments added explaining the fixture plugins use known-UTF-8 contract names.
- `crates/polyplug/benches/vtable_dispatch.rs:78`: `from_utf8_unchecked` in bench helper — add SAFETY comment.
- **`PolyplugError::InvalidUtf8 { context: String }`** does NOT exist — must be added to `RuntimeError` enum.

**FIX 2 — FFI null checks actual gaps (after reading ffi/mod.rs):**
- `polyplug_load_bundle`: null `rt` already handled ✅; `path.is_null()` NOT checked — **bug**
- `polyplug_reload_bundle`: null `rt` already handled ✅; `path.is_null()` NOT checked — **bug**
- `polyplug_rt_find_all_by_contract`: null `rt` handled ✅; `out.is_null() && out_cap > 0` NOT guarded — **bug** (UB write through null)
- `polyplug_rt_resolve_plugin`: null `rt` handled ✅ but sets `last_error("null runtime")` for null handle case — null handle should be silent return-null (not an error)
- All others already correct.

**FIX 3 — TrackingAllocator current state:**
- Only has `TLS_ALLOC_COUNT` / `TLS_FREE_COUNT` atomics. No `HashSet` of live addresses.
- Double-free goes undetected: `tracking_free` just calls `polyplug_host_free` again (real double-free into system allocator = UB/crash).
- `TLS_LIVE_ADDRS: RefCell<HashSet<usize>>` requires non-const init — cannot use `const {}` syntax unlike existing atomics.

**TEST 5 — production target:**
- The production loader never calls `from_utf8_unchecked` on `desc.contract_name`.
- The `from_utf8` path to exercise is in `ffi/mod.rs:94` — the path bytes UTF-8 check.
- TEST 5 therefore tests: passing non-UTF-8 bytes as the path to `polyplug_load_bundle` returns error, AND loading a plugin whose trace callback emits non-UTF-8 (if that path is exercised). Most concretely: test the `ffi/mod.rs` path validation path with raw `\xff\xfe` bytes.

**TEST 7 — PluginVTableGuard is !Send:**
- `PluginVTableGuard` has `PhantomData<Cell<()>>` — intentionally `!Send`.
- TEST 7 design: background thread receives `Arc<Registry>` + packed handle (both `Send`), calls `resolve_guard` itself inside that thread, holds the guard there. Main thread calls `reload_bundle`. Background thread drops guard after reload attempt returns.

**Error enum placement:**
- `RuntimeError` is the top-level enum (`PolyplugError = RuntimeError`). `InvalidUtf8` belongs directly on `RuntimeError` (not `LoaderError`) — it's an ABI-boundary concern, not a loader-phase concern.

**Workspace members:** root `Cargo.toml` line 3: `members = ["crates/*", "tests/fixtures/test_plugin", ...]` — new fixtures must be added here explicitly.

---

## Work Objectives

### Core Objective
Eliminate active undefined behaviour from plugin-provided data conversion, harden the C facade against null pointer arguments, and add double-free detection to the test-time allocator wrapper — then write the missing tests that verify each of these invariants.

### Concrete Deliverables
- `crates/polyplug/src/error/mod.rs` — new `InvalidUtf8` variant on `RuntimeError`
- `crates/polyplug/src/extensions/trace/mod.rs` — improved SAFETY comment on line 42
- `crates/polyplug/src/abi/mod.rs` — improved SAFETY comment on `StringView::as_str`
- `crates/polyplug/src/ffi/mod.rs` — null guards for `path`, `out`, and NULL_HANDLE
- `crates/polyplug/src/allocator/tracking/mod.rs` — `TLS_LIVE_ADDRS` + double-free panic
- `tests/integration_ffi_null/mod.rs` (new) — 9 null-pointer test cases
- `tests/integration_invalid_utf8/mod.rs` (new) — invalid UTF-8 path bytes test
- `tests/integration_malformed/mod.rs` (new) — 5 malformed-bundle test cases
- `tests/integration_quiescence/mod.rs` (new) — quiescence timeout test (#[ignore])
- `tests/integration_stringview_nulls/mod.rs` (new) — StringView embedded-null round-trip
- `tests/stress_memory/mod.rs` — 1 new `test_double_free_detected` test appended
- `tests/fixtures/no_init_plugin/` (new Rust cdylib) — exports `polyplug_abi_version` only
- `tests/fixtures/invalid_utf8_plugin.c` (new C source) — C file, compiled by build.rs via `cc`
- `crates/polyplug/build.rs` — build entries for 2 new fixtures
- `crates/polyplug/Cargo.toml` — 5 new `[[test]]` entries
- Root `Cargo.toml` — `no_init_plugin` added to workspace `members`
- `TRUST_MODEL.md` — new "Plugin crash isolation" section
- `polyplug_prd.md` section 27 — crash isolation non-goal added
- All 11 test files with `from_utf8_unchecked` — `// SAFETY:` comment added

### Definition of Done
- [ ] `grep -rn "from_utf8_unchecked" crates/ | grep -v "SAFETY"` → zero results
- [ ] `grep -rn "from_utf8_unchecked" crates/polyplug/src/loader/` → zero results (already true, stays true)
- [ ] `cargo clippy -- -D warnings` → zero warnings
- [ ] `cargo test --workspace` → all tests pass
- [ ] `cargo test --workspace -- --ignored` → quiescence test passes (~5 s)
- [ ] `PolyplugError::InvalidUtf8` variant exists in `error/mod.rs`

### Must Have
- `PolyplugError::InvalidUtf8 { context: String }` on `RuntimeError`
- `path.is_null()` guards in `polyplug_load_bundle` and `polyplug_reload_bundle`
- `out.is_null() && out_cap > 0` guard in `polyplug_rt_find_all_by_contract`
- NULL_HANDLE (u64::MAX) → silent null return in `polyplug_rt_resolve_plugin` (no `set_last_error`)
- `TLS_LIVE_ADDRS` double-free detection in `tracking_free` under `#[cfg(debug_assertions)]`
- All 5 new test files registered in `crates/polyplug/Cargo.toml`
- `no_init_plugin` in root workspace `Cargo.toml` members
- `TRUST_MODEL.md` crash-isolation section

### Must NOT Have (Guardrails)
- No `.unwrap()` added anywhere in production code
- No ABI struct changes (no `#[repr(C)]` field additions/reorderings)
- No public function signature changes
- No `use` statements inside functions or impl blocks (AGENTS.md Rule 2)
- No bare `filename.rs` module roots — all new test modules use `dirname/mod.rs` (AGENTS.md Rule 1)
- Do NOT change the logic in test-file `registry_register_callback` helpers — they get SAFETY comments only
- Do NOT add `InvalidUtf8` to `LoaderError` — it goes on `RuntimeError` only
- Do NOT make `PluginVTableGuard` `Send` — TEST 7 works around `!Send` by design
- Do NOT add `const {}` syntax to `TLS_LIVE_ADDRS` (HashSet is not const-constructible)
- Do NOT touch `integration_reload.rs` bare-file anomaly — out of scope

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (`cargo test`)
- **Automated tests**: Tests-after (existing test suite; new tests added)
- **Framework**: `cargo test` (built-in)
- **TDD**: Not applicable — hardening fixes with co-located tests

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — all independent, start immediately):
├── Task 1: Add InvalidUtf8 variant to RuntimeError           [quick]
├── Task 2: Fix SAFETY comments on from_utf8_unchecked sites  [quick]
└── Task 3: Add double-free detection to TrackingAllocator    [quick]

Wave 2 (After Wave 1 — fixes + fixtures):
├── Task 4: Fix FFI null pointer guards in ffi/mod.rs          [quick]    (needs Task 1)
├── Task 5: Build no_init_plugin fixture + build.rs wiring     [quick]    (independent)
├── Task 6: Build invalid_utf8_plugin.c fixture + build.rs     [quick]    (needs Task 1)
└── Task 7: Update TRUST_MODEL.md + PRD section 27             [writing]  (independent)

Wave 3 (After Wave 2 — all new test files):
├── Task 8:  tests/integration_ffi_null/mod.rs                 [quick]    (needs Task 4)
├── Task 9:  tests/integration_invalid_utf8/mod.rs             [quick]    (needs Task 6)
├── Task 10: tests/integration_malformed/mod.rs                [quick]    (needs Task 5)
├── Task 11: tests/integration_quiescence/mod.rs               [quick]    (needs Task 4)
├── Task 12: tests/integration_stringview_nulls/mod.rs         [quick]    (independent)
└── Task 13: Append test_double_free_detected to stress_memory [quick]    (needs Task 3)

Wave 4 (After Wave 3 — wiring):
└── Task 14: Cargo.toml + workspace members wiring             [quick]    (needs Tasks 8-13)

Wave FINAL (After ALL — verification):
├── Task F1: cargo clippy + cargo test --workspace             [unspecified-high]
└── Task F2: cargo test --workspace -- --ignored               [unspecified-high]
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| 1    | —         | 4, 6   |
| 2    | —         | —      |
| 3    | —         | 13     |
| 4    | 1         | 8, 11  |
| 5    | —         | 10     |
| 6    | 1         | 9      |
| 7    | —         | —      |
| 8    | 4         | 14     |
| 9    | 6         | 14     |
| 10   | 5         | 14     |
| 11   | 4         | 14     |
| 12   | —         | 14     |
| 13   | 3         | 14     |
| 14   | 8–13      | F1, F2 |
| F1   | 14        | —      |
| F2   | 14        | —      |

---

## TODOs

- [ ] 1. **Add `InvalidUtf8` variant to `RuntimeError`**

  **What to do**:
  - Open `crates/polyplug/src/error/mod.rs`
  - Add the following variant to `RuntimeError` (after `QuiescenceTimeout`, before `WatcherFailed`):
    ```rust
    #[error("invalid UTF-8 in plugin-provided data: context={context}")]
    InvalidUtf8 { context: String },
    ```
  - The variant has NO `#[source]` field — `context` is a human-readable description string (e.g. `"contract name"`, `"trace message"`)
  - Do NOT add this to `LoaderError` — it belongs on `RuntimeError` directly
  - No other changes to this file

  **Must NOT do**:
  - Do not add `InvalidUtf8` to `LoaderError` or any sub-enum
  - Do not add a `#[source] source: std::str::Utf8Error` field (not needed per spec)
  - Do not add `#[non_exhaustive]` to `RuntimeError`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 2, 3)
  - **Blocks**: Tasks 4, 6
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/error/mod.rs:8-47` — `RuntimeError` enum, insert after line 42 (`QuiescenceTimeout`)
  - `crates/polyplug/src/error/mod.rs:50` — `pub type PolyplugError = RuntimeError` (confirms alias)

  **Acceptance Criteria**:
  - [ ] `grep -n "InvalidUtf8" crates/polyplug/src/error/mod.rs` → one result on `RuntimeError`
  - [ ] `cargo build -p polyplug` compiles with zero errors

  **QA Scenarios**:
  ```
  Scenario: Variant compiles and is accessible as PolyplugError::InvalidUtf8
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert exit code 0
      3. Run: grep -n 'InvalidUtf8' crates/polyplug/src/error/mod.rs
      4. Assert exactly one line containing 'InvalidUtf8 { context: String }'
    Expected Result: Build succeeds, variant present
    Evidence: .sisyphus/evidence/task-1-build.txt
  ```

  **Commit**: YES (group with Tasks 2, 3, 4, 13)

---

- [ ] 2. **Add/improve SAFETY comments on all `from_utf8_unchecked` sites**

  **What to do**:
  This task adds `// SAFETY:` comments — NO logic changes anywhere.

  **Site A — `crates/polyplug/src/extensions/trace/mod.rs:42`**
  The existing comment on lines 39-40 says "ABI contract guarantees msg.ptr points to valid UTF-8".
  Improve it to make the trust basis explicit:
  ```rust
  // SAFETY: The ABI contract for TraceVTable::emit (see abi/mod.rs and TRUST_MODEL.md)
  // states that msg.ptr points to valid UTF-8 bytes for exactly msg.len bytes, and
  // remains valid for the duration of this call. Plugins that violate this contract
  // invoke undefined behaviour — enforcement is the caller's responsibility.
  ```

  **Site B — `crates/polyplug/src/abi/mod.rs` (`StringView::as_str`)** — line ~67
  Find the existing SAFETY comment. Replace/augment it to explicitly state the data is host-owned:
  ```rust
  // SAFETY: StringView::as_str is only called with host-owned StringViews created
  // via StringView::from_static or StringView::from_str_ref — both guarantee valid
  // UTF-8. Plugin-provided StringViews must never be passed to this method.
  ```

  **Site C — bench `crates/polyplug/benches/vtable_dispatch.rs:78`**
  Add SAFETY comment before the `from_utf8_unchecked` line:
  ```rust
  // SAFETY: desc.contract_name is set from a &'static str in the benchmark fixture.
  // The bytes are valid UTF-8 by construction.
  ```

  **Sites D — 11 test files** (all have identical pattern in `registry_register_callback`):
  Files:
  - `tests/integration_dispatch/mod.rs:49`
  - `tests/integration_graph/mod.rs:54`
  - `tests/integration_codegen_cpp/mod.rs:84`
  - `tests/stress_memory/mod.rs:157`
  - `tests/stress_error/mod.rs:142`
  - `tests/integration_dotnet/mod.rs:83`
  - `tests/integration_python/mod.rs:71`
  - `tests/integration_lua/mod.rs:60`
  - `tests/integration_js/mod.rs:129`
  - `tests/cross_language/mod.rs:132`
  - `tests/cross_language_deno/mod.rs:61`

  Each has a pattern like:
  ```rust
  core::str::from_utf8_unchecked(bytes)
  ```
  Add immediately before each call:
  ```rust
  // SAFETY: desc.contract_name is set by a test fixture plugin that uses a
  // &'static str contract name — guaranteed valid UTF-8 by construction.
  ```

  **Must NOT do**:
  - Do NOT change any logic — SAFETY comments only
  - Do NOT convert `from_utf8_unchecked` to `from_utf8` in these files (callbacks have C ABI, cannot return Result)
  - Do NOT touch `crates/polyplug/src/loader/mod.rs` (already clean)
  - Do NOT touch `crates/polyplug/src/ffi/mod.rs` (already uses `from_utf8`)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 3)
  - **Blocks**: nothing
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/extensions/trace/mod.rs:35-45` — full thunk function context
  - `crates/polyplug/src/abi/mod.rs:60-70` — `StringView::as_str` implementation
  - `TRUST_MODEL.md` — trust boundary documentation

  **Acceptance Criteria**:
  - [ ] `grep -rn "from_utf8_unchecked" crates/ | grep -v "SAFETY"` → zero results
  - [ ] `grep -rn "from_utf8_unchecked" tests/ | grep -v "SAFETY"` → zero results
  - [ ] `cargo build -p polyplug` compiles with zero errors

  **QA Scenarios**:
  ```
  Scenario: All unchecked calls have SAFETY annotation
    Tool: Bash
    Steps:
      1. Run: grep -rn "from_utf8_unchecked" crates/ | grep -v "// SAFETY"
      2. Assert output is empty (exit 1 from grep = no matches = pass)
      3. Run: grep -rn "from_utf8_unchecked" tests/ | grep -v "// SAFETY"
      4. Assert output is empty
    Expected Result: Zero unchecked calls without SAFETY annotation
    Evidence: .sisyphus/evidence/task-2-safety-grep.txt
  ```

  **Commit**: YES (group with Tasks 1, 3, 4, 13)

---

- [ ] 3. **Add double-free detection to `TrackingAllocator`**

  **What to do**:
  Modify `crates/polyplug/src/allocator/tracking/mod.rs`.

  **Step 1** — Add imports at file top (after existing `use` lines):
  ```rust
  #[cfg(debug_assertions)]
  use std::collections::HashSet;
  #[cfg(debug_assertions)]
  use std::cell::RefCell;
  ```

  **Step 2** — Add new thread-local after the existing `thread_local!` block (line 16):
  ```rust
  #[cfg(debug_assertions)]
  thread_local! {
      static TLS_LIVE_ADDRS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());
  }
  ```
  Note: Cannot use `const {}` syntax — `HashSet::new()` is not const-constructible. Use runtime init.

  **Step 3** — In `tracking_alloc`, add live-addr insert after the alloc call, before the return:
  ```rust
  unsafe extern "C" fn tracking_alloc(size: usize, align: usize) -> *mut u8 {
      TLS_ALLOC_COUNT.with(|c| c.fetch_add(1, Ordering::SeqCst));
      // SAFETY: Caller guarantees size > 0 and align is a power of two.
      let ptr: *mut u8 = polyplug_host_alloc(size, align);
      #[cfg(debug_assertions)]
      if !ptr.is_null() {
          TLS_LIVE_ADDRS.with(|s| {
              s.borrow_mut().insert(ptr as usize);
          });
      }
      ptr
  }
  ```

  **Step 4** — In `tracking_free`, add double-free check before forwarding:
  ```rust
  unsafe extern "C" fn tracking_free(ptr: *mut u8, size: usize, align: usize) {
      TLS_FREE_COUNT.with(|c| c.fetch_add(1, Ordering::SeqCst));
      #[cfg(debug_assertions)]
      {
          let addr: usize = ptr as usize;
          TLS_LIVE_ADDRS.with(|s| {
              if !s.borrow_mut().remove(&addr) {
                  panic!(
                      "TrackingAllocator: double-free detected at address {:#x}",
                      addr
                  );
              }
          });
      }
      // SAFETY: ptr was allocated by polyplug_host_alloc via tracking_alloc with this layout.
      unsafe { polyplug_host_free(ptr, size, align) }
  }
  ```

  **Step 5** — In `TrackingAllocator::new()`, reset `TLS_LIVE_ADDRS` alongside the counters:
  ```rust
  pub fn new() -> TrackingAllocator {
      TLS_ALLOC_COUNT.with(|c| c.store(0, Ordering::SeqCst));
      TLS_FREE_COUNT.with(|c| c.store(0, Ordering::SeqCst));
      #[cfg(debug_assertions)]
      TLS_LIVE_ADDRS.with(|s| s.borrow_mut().clear());
      TrackingAllocator
  }
  ```

  **Must NOT do**:
  - Do NOT use `const {}` for `TLS_LIVE_ADDRS` (won't compile)
  - Do NOT insert null pointer (addr == 0) into `TLS_LIVE_ADDRS`
  - Do NOT gate `TLS_LIVE_ADDRS` usage without `#[cfg(debug_assertions)]` on EVERY access
  - Do NOT remove the existing `TLS_ALLOC_COUNT` / `TLS_FREE_COUNT` logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with Tasks 1, 2)
  - **Blocks**: Task 13
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/allocator/tracking/mod.rs` — full file, replace both functions
  - `crates/polyplug/src/allocator/mod.rs` — `polyplug_host_alloc`/`polyplug_host_free` signatures

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` succeeds
  - [ ] `cargo test -p polyplug tracking` → existing tracking tests pass

  **QA Scenarios**:
  ```
  Scenario: Existing tracking tests still pass after modification
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug tracking_allocator 2>&1
      2. Assert exit code 0
      3. Assert output contains "test result: ok"
    Expected Result: Both existing tests pass
    Evidence: .sisyphus/evidence/task-3-tracking-tests.txt

  Scenario: Compile succeeds in release (cfg(debug_assertions) gates apply)
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug --release 2>&1
      2. Assert exit code 0
    Expected Result: Release build succeeds, no TLS_LIVE_ADDRS symbols in release
    Evidence: .sisyphus/evidence/task-3-release-build.txt
  ```

  **Commit**: YES (group with Tasks 1, 2, 4, 13)

---

## Final Verification Wave

- [ ] F1. **Build + Test** — `unspecified-high`
  Run `cargo clippy -- -D warnings` from workspace root. Zero warnings = pass.
  Run `cargo test --workspace`. All tests pass = pass.
  Run `grep -rn "from_utf8_unchecked" crates/ | grep -v "SAFETY"` → zero results.
  Output: `Clippy [PASS/FAIL] | Tests [N pass / N fail] | SAFETY grep [CLEAN/issues] | VERDICT`

- [ ] F2. **Ignored tests** — `unspecified-high`
  Run `cargo test --workspace -- --ignored`.
  Assert quiescence test (`integration_quiescence::test_quiescence_timeout`) passes.
  Assert runtime healthy after (next reload succeeds).
  Output: `Ignored tests [N pass / N fail] | Quiescence [PASS/FAIL] | VERDICT`

---

## Commit Strategy

- [ ] 4. **Fix FFI null pointer guards in `crates/polyplug/src/ffi/mod.rs`**

  **What to do**:
  Four targeted edits to `ffi/mod.rs`. All other functions are already correct.

  **Edit A — `polyplug_load_bundle`**: Add `path.is_null()` check.
  After the existing `if rt.is_null()` block (which returns `1u32`), add:
  ```rust
  if path.is_null() {
      set_last_error("null path pointer in polyplug_load_bundle");
      return 1u32;
  }
  ```
  This goes immediately before the `core::slice::from_raw_parts(path, path_len)` line.

  **Edit B — `polyplug_reload_bundle`**: Same pattern, same placement:
  ```rust
  if path.is_null() {
      set_last_error("null path pointer in polyplug_reload_bundle");
      return 1u32;
  }
  ```

  **Edit C — `polyplug_rt_find_all_by_contract`**: Add `out.is_null() && out_cap > 0` guard.
  After the existing `if rt.is_null() { return 0usize; }` block, add:
  ```rust
  if out.is_null() && out_cap > 0 {
      set_last_error("null output buffer with non-zero capacity in polyplug_rt_find_all_by_contract");
      return 0usize;
  }
  ```
  Note: `out.is_null() && out_cap == 0` is the valid "probe for count" pattern — this guard
  correctly allows that case through.

  **Edit D — `polyplug_rt_resolve_plugin`**: NULL_HANDLE (packed == u64::MAX) → silent null return.
  Add check before the `unpack_handle` call:
  ```rust
  const NULL_HANDLE: u64 = u64::MAX;
  if packed_handle == NULL_HANDLE {
      // Null handle — return null guard without setting last_error.
      // Callers that receive NULL_HANDLE back from find functions use this as a sentinel.
      return core::ptr::null_mut();
  }
  ```
  This goes immediately after the `if rt.is_null()` block in `polyplug_rt_resolve_plugin`.

  **Must NOT do**:
  - Do NOT change any function that is already correct (`polyplug_runtime_free`, `polyplug_rt_find_by_contract`, etc.)
  - Do NOT change function signatures
  - Do NOT add `.unwrap()` anywhere

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO — Wave 2 (after Task 1 for `InvalidUtf8` variant)
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7)
  - **Blocks**: Tasks 8, 11
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplug/src/ffi/mod.rs:80-115` — `polyplug_load_bundle` full implementation
  - `crates/polyplug/src/ffi/mod.rs:117-155` — `polyplug_reload_bundle` full implementation
  - `crates/polyplug/src/ffi/mod.rs:220-242` — `polyplug_rt_find_all_by_contract`
  - `crates/polyplug/src/ffi/mod.rs:244-270` — `polyplug_rt_resolve_plugin`
  - `crates/polyplug/src/ffi/mod.rs:1-30` — `set_last_error` helper and `NULL_HANDLE` constant (check if already defined)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` succeeds
  - [ ] `grep -n 'path.is_null()' crates/polyplug/src/ffi/mod.rs` → 2 results
  - [ ] `grep -n 'out.is_null()' crates/polyplug/src/ffi/mod.rs` → 1 result
  - [ ] `grep -n 'NULL_HANDLE' crates/polyplug/src/ffi/mod.rs` → at least 1 result

  **QA Scenarios**:
  ```
  Scenario: Build succeeds with null guards added
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert exit code 0
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-4-build.txt
  ```

  **Commit**: YES (group with Tasks 1, 2, 3, 13)

---

- [ ] 5. **Build `no_init_plugin` fixture + add build.rs/Cargo.toml wiring**

  **What to do**:
  Create a new minimal Rust cdylib fixture that exports `polyplug_abi_version` only.
  This tests `LoaderError::MissingSymbol { symbol: "polyplug_init" }`.

  **Step 1** — Create `tests/fixtures/no_init_plugin/Cargo.toml`:
  ```toml
  [package]
  name    = "no_init_plugin"
  version = "0.1.0"
  edition = "2021"

  [lib]
  crate-type = ["cdylib"]

  [dependencies]
  ```

  **Step 2** — Create `tests/fixtures/no_init_plugin/src/lib.rs`:
  ```rust
  //! Minimal plugin fixture that exports polyplug_abi_version but NOT polyplug_init.
  //! Used to test LoaderError::MissingSymbol { symbol: "polyplug_init" }.

  /// ABI version constant — makes this a recognisable polyplug plugin binary.
  #[unsafe(no_mangle)]
  pub extern "C" fn polyplug_abi_version() -> u32 {
      1_u32
  }
  ```
  Note: `src/lib.rs` is a bare file — acceptable here because `lib.rs` is the Cargo convention
  for the crate root (not a module root), and AGENTS.md Rule 1 applies to `mod` declarations.

  **Step 3** — Add to root `Cargo.toml` workspace members:
  Open root `Cargo.toml` (not `crates/polyplug/Cargo.toml`). Line 3 has the `members` array.
  Append `"tests/fixtures/no_init_plugin"` to the members list.

  **Step 4** — Add build.rs entry in `crates/polyplug/build.rs`.
  Follow the exact pattern used for `test_plugin`, `memory_plugin`, etc.
  Add a new section that:
  - Adds `println!("cargo:rerun-if-changed=tests/fixtures/no_init_plugin/src/lib.rs")`
  - Runs `cargo build -p no_init_plugin --release --target-dir &plugin_target_dir`
  - Finds the resulting `.so` (`.dylib` on macOS, `.dll` on Windows)
  - Copies it to `fixtures_dir/no_init_plugin_dir/`
  - Writes a `manifest.toml` in that dir:
    ```toml
    bundle_name = "no_init_plugin"
    runtime     = "rust"
    file        = "libno_init_plugin.so"
    ```
  - Sets env var `NO_INIT_PLUGIN_DIR` pointing to `no_init_plugin_dir`

  **Must NOT do**:
  - Do NOT export `polyplug_init` — the point is that it is absent
  - Do NOT add any business logic to the fixture
  - Do NOT create a `src/lib/mod.rs` directory (lib.rs is the crate root, not a module)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 6, 7)
  - **Blocks**: Task 10
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/build.rs:25-130` — `test_plugin` build pattern to copy exactly
  - Root `Cargo.toml:3` — workspace members line
  - `crates/polyplug/src/error/mod.rs:69-70` — `MissingSymbol` error variant

  **Acceptance Criteria**:
  - [ ] `tests/fixtures/no_init_plugin/Cargo.toml` exists
  - [ ] `tests/fixtures/no_init_plugin/src/lib.rs` exists
  - [ ] Root `Cargo.toml` members includes `"tests/fixtures/no_init_plugin"`
  - [ ] `cargo build -p polyplug` triggers no_init_plugin build without error
  - [ ] Env var `NO_INIT_PLUGIN_DIR` is set after build

  **QA Scenarios**:
  ```
  Scenario: no_init_plugin builds and env var is set
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert exit code 0
      3. Run: ls tests/fixtures/no_init_plugin/src/lib.rs
      4. Assert file exists
    Expected Result: Build succeeds, fixture file present
    Evidence: .sisyphus/evidence/task-5-build.txt
  ```

  **Commit**: YES (group with Tasks 8–12, 14)

---

- [ ] 6. **Build `invalid_utf8_plugin.c` fixture + build.rs wiring**

  **What to do**:
  Create a C source fixture that, when loaded by the test, exposes a scenario where
  non-UTF-8 bytes are passed as a path to `polyplug_load_bundle`. The simplest path:
  the fixture is NOT loaded as a plugin — instead, TEST 5 calls `polyplug_load_bundle`
  with a byte slice containing `\xff\xfe` bytes as the path argument and asserts an error.
  No C fixture is needed for this.

  **However**, to also test that the `ffi/mod.rs` `from_utf8` path error is returned as
  `PolyplugError` (not a panic), we need the test to access the Rust API directly.
  The test uses the Rust `Runtime::load_bundle` API with an invalid UTF-8 string. Since
  Rust strings are always UTF-8, the test must go through the C facade FFI path.

  **Actual fixture needed**: Create `tests/fixtures/invalid_utf8_bundle/manifest.toml`
  at test time (in tmpdir) — no build.rs compilation needed.
  The test writes a directory with a manifest pointing to a non-existent path and
  also calls `polyplug_load_bundle` with raw non-UTF-8 bytes (via the C ABI).

  **This task is therefore**: Create the test infrastructure only:
  - No C file to compile
  - No build.rs change needed for this task
  - The test itself (Task 9) will use `polyplug_load_bundle` raw FFI with `\xff\xfe` path bytes

  **What to actually do in this task**:
  Verify the `InvalidUtf8` variant added in Task 1 is used in `ffi/mod.rs` path validation.
  Currently, `polyplug_load_bundle` already calls `core::str::from_utf8(bytes)` and on error
  calls `set_last_error(e.to_string())` and returns `1u32`. This is correct BUT does not
  return `PolyplugError::InvalidUtf8` — it returns a UTF-8 error string only.

  **Decision** (per spec): The C facade sets last_error. The Rust `Runtime::load_bundle` API
  never receives invalid UTF-8 (Rust strings are valid UTF-8). TEST 5 therefore tests the
  C facade path — it verifies `polyplug_load_bundle` returns non-zero and `polyplug_last_error`
  contains a UTF-8 error message. `PolyplugError::InvalidUtf8` is available for future use.

  **This task is now a no-op for code changes** — Task 1 creates the variant, TEST 5 (Task 9)
  exercises it via the C facade. Mark Task 6 as: verify Task 1 result is accessible and note
  that no C fixture compilation is needed.

  **Must NOT do**:
  - Do NOT write a C file that won't be used
  - Do NOT add a build.rs compilation step for a C fixture we don't need

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 7)
  - **Blocks**: Task 9
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplug/src/ffi/mod.rs:94-105` — existing `from_utf8` path validation
  - `crates/polyplug/src/error/mod.rs` — `InvalidUtf8` variant (added in Task 1)

  **Acceptance Criteria**:
  - [ ] `PolyplugError::InvalidUtf8` exists (from Task 1)
  - [ ] `polyplug_load_bundle` already handles non-UTF-8 path bytes via `from_utf8`
  - [ ] No build.rs changes needed

  **QA Scenarios**:
  ```
  Scenario: Verify ffi already handles non-UTF-8 path bytes
    Tool: Bash
    Steps:
      1. Run: grep -n 'from_utf8' crates/polyplug/src/ffi/mod.rs
      2. Assert at least 2 results (load_bundle and reload_bundle)
    Expected Result: Both functions already use from_utf8
    Evidence: .sisyphus/evidence/task-6-ffi-check.txt
  ```

  **Commit**: N/A (no changes)

---
All tasks commit as one logical unit:
- `fix(hardening): replace from_utf8_unchecked on plugin data, add FFI null guards, double-free detection` — Tasks 1–4, 13
- `test(hardening): add ffi-null, invalid-utf8, malformed, quiescence, stringview-null tests` — Tasks 5–12, 14
- `docs(trust-model): document plugin crash isolation non-goal` — Task 7

- [ ] 7. **Update `TRUST_MODEL.md` and PRD section 27 with crash-isolation non-goal**

  **What to do**:

  **Edit A — `TRUST_MODEL.md`**:
  Append a new section at the end of the file:
  ```markdown
  ## Plugin crash isolation

  Plugins run in-process. A plugin that dereferences a null pointer, causes a stack
  overflow, or triggers any hardware exception (SIGSEGV, SIGBUS, SIGILL) takes down
  the entire host process. **This is expected and intentional behaviour.**

  Isolating plugin crashes would require either:
  - Out-of-process execution with IPC — violates the zero-overhead hot-path goal
  - OS-level sandboxing (seccomp, pledge) — platform-specific, adds significant complexity

  Neither is acceptable for v1. See PRD section 27 (Non-Goals).

  App developers who need crash isolation should run plugins in a separate worker process
  and communicate via IPC. polyplug does not provide this facility.
  ```

  **Edit B — `polyplug_prd.md` section 27**:
  Find section 27 (Non-Goals). Append to the non-goals list:
  ```
  - Plugin crash isolation: a SIGSEGV in a plugin kills the host process. This is by
    design. See TRUST_MODEL.md “Plugin crash isolation” for rationale.
  ```

  **Must NOT do**:
  - Do NOT change anything else in TRUST_MODEL.md or the PRD
  - Do NOT invent new non-goals beyond crash isolation

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 6)
  - **Blocks**: nothing
  - **Blocked By**: None

  **References**:
  - `TRUST_MODEL.md` — append after last section
  - `polyplug_prd.md` section 27 — find and append to non-goals list

  **Acceptance Criteria**:
  - [ ] `grep -n 'Plugin crash isolation' TRUST_MODEL.md` → at least 1 result
  - [ ] `grep -n 'crash isolation' polyplug_prd.md` → at least 1 result

  **QA Scenarios**:
  ```
  Scenario: Both documents updated
    Tool: Bash
    Steps:
      1. Run: grep -n 'Plugin crash isolation' TRUST_MODEL.md
      2. Assert output is non-empty
      3. Run: grep -n 'crash isolation' polyplug_prd.md
      4. Assert output is non-empty
    Expected Result: Both docs contain crash isolation text
    Evidence: .sisyphus/evidence/task-7-docs.txt
  ```

  **Commit**: YES (separate doc commit)

---

- [ ] 8. **Create `tests/integration_ffi_null/mod.rs`**

  **What to do**:
  Create new file `tests/integration_ffi_null/mod.rs` with 9 test functions.
  Each test calls a C facade function via `unsafe extern "C"` declarations at the top of the file.

  **File structure**:
  ```rust
  //! Integration tests: null pointer safety of all C facade FFI functions.
  //! Every function that takes a pointer must handle null without panicking.

  use polyplug::ffi::*; // or declare extern "C" fns directly
  ```

  **Important**: The FFI functions use concrete opaque types (`OpaqueRuntime`, `OpaqueGuard`).
  Use `extern "C"` declarations with `*mut core::ffi::c_void` as the opaque pointer type.
  This avoids importing internal types and works correctly for C ABI calls:
  ```rust
  use core::ffi::c_void;

  unsafe extern "C" {
      fn polyplug_runtime_new() -> *mut c_void;
      fn polyplug_runtime_free(rt: *mut c_void);
      fn polyplug_load_bundle(rt: *mut c_void, path: *const u8, path_len: usize) -> u32;
      fn polyplug_reload_bundle(rt: *mut c_void, path: *const u8, path_len: usize) -> u32;
      fn polyplug_rt_find_all_by_contract(
          rt: *const c_void, contract_id: u64, min_version: u32,
          out: *mut u64, out_cap: usize,
      ) -> usize;
      fn polyplug_rt_resolve_plugin(rt: *const c_void, packed_handle: u64) -> *mut c_void;
      fn polyplug_guard_free(guard: *mut c_void);
      fn polyplug_get_vtable(guard: *const c_void) -> *const c_void;
      fn polyplug_last_error(buf: *mut u8, buf_len: usize) -> usize;
  }
  ```

  **Test cases**:
  ```rust
  #[test]
  fn test_runtime_free_null() {
      // polyplug_runtime_free(null) must be a no-op, not a crash
      unsafe { polyplug_runtime_free(core::ptr::null_mut()) };
  }

  #[test]
  fn test_load_bundle_null_rt() {
      let path = b"/some/path";
      let rc: u32 = unsafe { polyplug_load_bundle(core::ptr::null_mut(), path.as_ptr(), path.len()) };
      assert_ne!(rc, 0, "load_bundle(null rt) must return non-zero");
  }

  #[test]
  fn test_load_bundle_null_path() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let rc: u32 = unsafe { polyplug_load_bundle(rt, core::ptr::null(), 0) };
      assert_ne!(rc, 0, "load_bundle(null path) must return non-zero");
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_find_all_null_out_zero_cap() {
      // out=null, cap=0 is the 'probe for count' pattern — must return 0, no error
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let count: usize = unsafe {
          polyplug_rt_find_all_by_contract(rt as *const (), 0xDEADBEEF_u64, 0_u32, core::ptr::null_mut(), 0)
      };
      // No plugins loaded, so count == 0. Point is: no crash, no panic.
      let _ = count;
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_find_all_null_out_nonzero_cap() {
      // out=null, cap=5 — must set last_error and return 0 (not UB write through null)
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let rc: usize = unsafe {
          polyplug_rt_find_all_by_contract(rt as *const (), 0xDEADBEEF_u64, 0_u32, core::ptr::null_mut(), 5)
      };
      assert_eq!(rc, 0, "find_all with null out + cap=5 must return 0");
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_resolve_plugin_null_handle() {
      // NULL_HANDLE (u64::MAX) — must return null ptr, must NOT set last_error
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let guard: *mut () = unsafe { polyplug_rt_resolve_plugin(rt as *const (), u64::MAX) };
      assert!(guard.is_null(), "resolve_plugin(NULL_HANDLE) must return null");
      // Verify no last_error was set
      let mut buf: [u8; 256] = [0_u8; 256];
      let n: usize = unsafe { polyplug_last_error(buf.as_mut_ptr(), buf.len()) };
      assert_eq!(n, 0, "last_error must be empty after NULL_HANDLE resolve");
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_guard_free_null() {
      // polyplug_guard_free(null) must be a no-op
      unsafe { polyplug_guard_free(core::ptr::null_mut()) };
  }

  #[test]
  fn test_get_vtable_null_guard() {
      // polyplug_get_vtable(null) must return null, not crash
      let vtable: *const () = unsafe { polyplug_get_vtable(core::ptr::null()) };
      assert!(vtable.is_null(), "get_vtable(null) must return null");
  }

  #[test]
  fn test_last_error_null_buf() {
      // polyplug_last_error(null, 0) must return 0, not crash
      let n: usize = unsafe { polyplug_last_error(core::ptr::null_mut(), 0) };
      assert_eq!(n, 0, "last_error(null buf) must return 0");
  }
  ```

  **Must NOT do**:
  - Do NOT use `.unwrap()` in tests (use `.expect()` with descriptive message)
  - Do NOT leave the runtime pointer leaked — always call `polyplug_runtime_free` at end of each test
  - Do NOT use `integration_reload.rs` bare-file style — this file is `mod.rs` in a directory

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9–12)
  - **Blocks**: Task 14
  - **Blocked By**: Task 4

  **References**:
  - `crates/polyplug/src/ffi/mod.rs` — all function signatures
  - `tests/integration_dispatch/mod.rs` — test file structure and polyplug import pattern
  - `crates/polyplug/Cargo.toml:28-35` — existing `[[test]]` entry format

  **Acceptance Criteria**:
  - [ ] `tests/integration_ffi_null/mod.rs` exists
  - [ ] File compiles as part of `cargo test -p polyplug integration_ffi_null`
  - [ ] All 9 test functions present and named as above

  **QA Scenarios**:
  ```
  Scenario: All null-pointer tests pass without panic
    Tool: Bash (after Task 14 wires Cargo.toml)
    Steps:
      1. Run: cargo test -p polyplug integration_ffi_null 2>&1
      2. Assert exit code 0
      3. Assert output contains "9 passed" or "test result: ok. 9 passed"
    Expected Result: All 9 tests pass
    Evidence: .sisyphus/evidence/task-8-ffi-null-tests.txt
  ```

  **Commit**: YES (group with Tasks 9–12, 14)

---

- [ ] 9. **Create `tests/integration_invalid_utf8/mod.rs`**

  **What to do**:
  Create `tests/integration_invalid_utf8/mod.rs` testing the C facade's UTF-8 validation.

  ```rust
  //! Integration tests: non-UTF-8 bytes passed to polyplug_load_bundle / polyplug_reload_bundle
  //! must produce a non-zero return code and a last_error message, not a panic or UB.

  unsafe extern "C" {
      fn polyplug_runtime_new() -> *mut ();
      fn polyplug_runtime_free(rt: *mut ());
      fn polyplug_load_bundle(rt: *mut (), path: *const u8, path_len: usize) -> u32;
      fn polyplug_reload_bundle(rt: *mut (), path: *const u8, path_len: usize) -> u32;
      fn polyplug_last_error(buf: *mut u8, buf_len: usize) -> usize;
  }

  /// Helper: read last_error into a String.
  fn read_last_error() -> String {
      let mut buf: Vec<u8> = vec![0_u8; 512];
      let n: usize = unsafe { polyplug_last_error(buf.as_mut_ptr(), buf.len()) };
      String::from_utf8_lossy(&buf[..n]).into_owned()
  }

  #[test]
  fn test_load_bundle_invalid_utf8_path() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null(), "runtime_new must succeed");
      // Construct a path with invalid UTF-8: \xff\xfe are invalid UTF-8 lead bytes
      let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'a', b't', b'h'];
      let rc: u32 = unsafe {
          polyplug_load_bundle(rt, bad_path.as_ptr(), bad_path.len())
      };
      assert_ne!(rc, 0, "load_bundle with invalid UTF-8 path must return non-zero");
      let err: String = read_last_error();
      assert!(!err.is_empty(), "last_error must be set after invalid UTF-8 path");
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_reload_bundle_invalid_utf8_path() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null(), "runtime_new must succeed");
      let bad_path: &[u8] = &[0xff_u8, 0xfe_u8, b'/', b'p', b'l', b'u', b'g'];
      let rc: u32 = unsafe {
          polyplug_reload_bundle(rt, bad_path.as_ptr(), bad_path.len())
      };
      assert_ne!(rc, 0, "reload_bundle with invalid UTF-8 path must return non-zero");
      let err: String = read_last_error();
      assert!(!err.is_empty(), "last_error must be set after invalid UTF-8 path");
      unsafe { polyplug_runtime_free(rt) };
  }

  #[test]
  fn test_runtime_healthy_after_invalid_utf8() {
      // After a failed load, runtime must still accept a valid load attempt.
      // We test this by attempting a second load with a valid (but non-existent) ASCII path.
      // The second call should fail with a 'file not found' error, NOT a panic.
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let bad_path: &[u8] = &[0xff_u8, 0xfe_u8];
      let _ = unsafe { polyplug_load_bundle(rt, bad_path.as_ptr(), bad_path.len()) };
      // Now try a valid ASCII path (non-existent file is OK — just proves runtime didn't break)
      let good_path: &[u8] = b"/tmp/nonexistent_plugin_dir";
      let rc2: u32 = unsafe {
          polyplug_load_bundle(rt, good_path.as_ptr(), good_path.len())
      };
      // We expect a 'path not found' error, not a panic. rc2 != 0 is expected.
      let err2: String = read_last_error();
      assert!(!err2.is_empty(), "runtime must be healthy and set last_error on second call");
      let _ = rc2;
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Must NOT do**:
  - Do NOT use `.unwrap()` in production paths
  - Do NOT attempt to load an actual invalid-UTF-8 plugin — test the path validation layer only

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8, 10–12)
  - **Blocks**: Task 14
  - **Blocked By**: Task 6 (conceptually, but Task 6 has no code changes so Task 9 is really unblocked)

  **References**:
  - `crates/polyplug/src/ffi/mod.rs:88-115` — `polyplug_load_bundle` from_utf8 path
  - `tests/integration_dispatch/mod.rs` — test file structure

  **Acceptance Criteria**:
  - [ ] `tests/integration_invalid_utf8/mod.rs` exists with 3 test functions
  - [ ] After Task 14 wires Cargo.toml: `cargo test -p polyplug integration_invalid_utf8` → 3 passed

  **QA Scenarios**:
  ```
  Scenario: All 3 invalid UTF-8 tests pass
    Tool: Bash (after Task 14)
    Steps:
      1. Run: cargo test -p polyplug integration_invalid_utf8 2>&1
      2. Assert exit code 0
      3. Assert output contains "3 passed"
    Expected Result: All 3 tests pass
    Evidence: .sisyphus/evidence/task-9-invalid-utf8-tests.txt
  ```

  **Commit**: YES (group with Tasks 8, 10–12, 14)

---

- [ ] 10. **Create `tests/integration_malformed/mod.rs`**

  **What to do**:
  Create `tests/integration_malformed/mod.rs` with 5 test cases for malformed bundle inputs.
  Uses `std::fs` and `tempfile` crate (or manual tmpdir) to write test fixtures at runtime.

  **Check if `tempfile` is a dev-dependency**: run `grep 'tempfile' crates/polyplug/Cargo.toml`.
  If not present, use `std::env::temp_dir()` + unique subdirectory via `std::time::SystemTime`.
  If it is present, use `tempfile::TempDir`.

  **File structure**:
  ```rust
  //! Integration tests: malformed bundle inputs must return clean Err, never panic.

  use std::fs;
  use std::path::PathBuf;

  // Declare FFI functions used
  unsafe extern "C" {
      fn polyplug_runtime_new() -> *mut ();
      fn polyplug_runtime_free(rt: *mut ());
      fn polyplug_load_bundle(rt: *mut (), path: *const u8, path_len: usize) -> u32;
  }

  fn load_bundle_path(rt: *mut (), dir: &str) -> u32 {
      let bytes: &[u8] = dir.as_bytes();
      unsafe { polyplug_load_bundle(rt, bytes.as_ptr(), bytes.len()) }
  }

  fn make_tmpdir(name: &str) -> PathBuf {
      let base: PathBuf = std::env::temp_dir().join(format!("polyplug_test_{}", name));
      fs::create_dir_all(&base).expect("create tmpdir");
      base
  }

  fn cleanup(dir: &PathBuf) {
      let _ = fs::remove_dir_all(dir);
  }
  ```

  **Test a: Truncated .so**
  ```rust
  #[test]
  fn test_truncated_so() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let dir: PathBuf = make_tmpdir("truncated");
      // Write a truncated .so: valid ELF magic + 508 zero bytes (truncated body)
      let mut so: Vec<u8> = vec![0x7f_u8, b'E', b'L', b'F'];
      so.extend_from_slice(&[0u8; 508]);
      fs::write(dir.join("libtruncated.so"), &so).expect("write truncated so");
      fs::write(dir.join("manifest.toml"),
          b"bundle_name = \"truncated\"\nruntime = \"rust\"\nfile = \"libtruncated.so\"\n",
      ).expect("write manifest");
      let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8 path"));
      assert_ne!(rc, 0, "truncated .so must produce non-zero return");
      cleanup(&dir);
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Test b: Wrong magic bytes**
  ```rust
  #[test]
  fn test_wrong_magic_bytes() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let dir: PathBuf = make_tmpdir("wrong_magic");
      let garbage: Vec<u8> = b"NOTANELF\x00".iter().cycle().take(512).cloned().collect();
      fs::write(dir.join("libwrong.so"), &garbage).expect("write garbage");
      fs::write(dir.join("manifest.toml"),
          b"bundle_name = \"wrong_magic\"\nruntime = \"rust\"\nfile = \"libwrong.so\"\n",
      ).expect("write manifest");
      let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
      assert_ne!(rc, 0, "wrong magic bytes must produce non-zero return");
      cleanup(&dir);
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Test c: Missing init symbol (`no_init_plugin` fixture)**
  ```rust
  #[test]
  fn test_missing_init_symbol() {
      // Uses the no_init_plugin fixture built by build.rs (Task 5).
      // NO_INIT_PLUGIN_DIR env var is set by build.rs.
      let dir: &str = env!("NO_INIT_PLUGIN_DIR");
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let rc: u32 = load_bundle_path(rt, dir);
      assert_ne!(rc, 0, "plugin missing polyplug_init must produce non-zero return");
      // Verify error message mentions the missing symbol
      let mut buf: [u8; 256] = [0u8; 256];
      let n: usize = unsafe {
          // last_error declared inline since it's only needed here
          extern "C" { fn polyplug_last_error(b: *mut u8, l: usize) -> usize; }
          polyplug_last_error(buf.as_mut_ptr(), buf.len())
      };
      let msg: &str = core::str::from_utf8(&buf[..n]).expect("last_error is valid utf8");
      assert!(
          msg.contains("polyplug_init") || msg.contains("symbol") || msg.contains("init"),
          "error message should mention missing symbol, got: {}", msg
      );
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Test d: Sub-case d is documented as skipped (ABI mismatch is untestable safely)**
  ```rust
  // Test d (ABI mismatch) is intentionally omitted.
  // A plugin that exports init() with a wrong signature causes undefined behaviour
  // at the call site. This is documented in polyplug_prd.md section 27 as out-of-scope.
  // There is no safe way to test this in-process.
  ```

  **Test e: Bundle directory exists but .so file is missing**
  ```rust
  #[test]
  fn test_so_file_missing_from_bundle() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let dir: PathBuf = make_tmpdir("missing_so");
      // Write manifest pointing to a nonexistent .so
      fs::write(dir.join("manifest.toml"),
          b"bundle_name = \"missing_so\"\nruntime = \"rust\"\nfile = \"nonexistent.so\"\n",
      ).expect("write manifest");
      // Do NOT create nonexistent.so
      let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
      assert_ne!(rc, 0, "missing .so file must produce non-zero return");
      cleanup(&dir);
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Test f: Unknown runtime in manifest**
  ```rust
  #[test]
  fn test_unknown_runtime() {
      let rt: *mut () = unsafe { polyplug_runtime_new() };
      assert!(!rt.is_null());
      let dir: PathBuf = make_tmpdir("unknown_runtime");
      // Create a dummy .so file so the manifest parse succeeds
      fs::write(dir.join("dummy.so"), b"notareal").expect("write dummy");
      fs::write(dir.join("manifest.toml"),
          b"bundle_name = \"unknown_runtime\"\nruntime = \"cobol\"\nfile = \"dummy.so\"\n",
      ).expect("write manifest");
      let rc: u32 = load_bundle_path(rt, dir.to_str().expect("valid utf8"));
      assert_ne!(rc, 0, "unknown runtime must produce non-zero return");
      cleanup(&dir);
      unsafe { polyplug_runtime_free(rt) };
  }
  ```

  **Must NOT do**:
  - Do NOT use `.unwrap()` in the `load_bundle_path` helper or FFI calls
  - Do NOT rely on file system state outside `temp_dir()`
  - Do NOT implement sub-case d

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8, 9, 11, 12)
  - **Blocks**: Task 14
  - **Blocked By**: Task 5 (needs `NO_INIT_PLUGIN_DIR` env var from build.rs)

  **References**:
  - `tests/integration_reload/mod.rs` — `env!()` macro usage pattern for fixture env vars
  - `crates/polyplug/src/error/mod.rs:69-70,90-93` — `MissingSymbol` and `NoLoaderForRuntime` variants
  - `crates/polyplug/build.rs:115-130` — bundle dir + manifest.toml creation pattern

  **Acceptance Criteria**:
  - [ ] `tests/integration_malformed/mod.rs` exists with 5 test functions (a,b,c,e,f; d is skipped with comment)
  - [ ] After Task 14: `cargo test -p polyplug integration_malformed` → 5 passed

  **QA Scenarios**:
  ```
  Scenario: All 5 malformed bundle tests pass
    Tool: Bash (after Task 14)
    Steps:
      1. Run: cargo test -p polyplug integration_malformed 2>&1
      2. Assert exit code 0
      3. Assert output contains "5 passed"
    Expected Result: All 5 tests pass, no panics
    Evidence: .sisyphus/evidence/task-10-malformed-tests.txt
  ```

  **Commit**: YES (group with Tasks 8, 9, 11, 12, 14)

---

- [ ] 11. **Create `tests/integration_quiescence/mod.rs`**

  **What to do**:
  Create `tests/integration_quiescence/mod.rs` with one `#[ignore]` test.
  The test holds a `PluginVTableGuard` on a background thread while the main thread
  attempts a hot-reload. `PluginVTableGuard` is `!Send`, so the guard is created AND held
  entirely on the background thread.

  **Implementation**:
  ```rust
  //! Integration test: hot-reload quiescence timeout.
  //! Verifies that reload_bundle returns Err(QuiescenceTimeout) when an in-flight
  //! guard is held past the timeout window.
  //!
  //! This test takes ~5 seconds. Run with: cargo test -- --ignored

  use polyplug::error::PolyplugError;
  use polyplug::runtime::Runtime;
  use polyplug::registry::Registry;
  use polyplug::abi::PluginHandle;

  #[test]
  #[ignore] // Takes ~5s — run with `cargo test -- --ignored`
  fn test_quiescence_timeout() {
      // Build runtime and load v1
      let rt: Runtime = Runtime::builder()
          .build()
          .expect("runtime build must succeed");
      let v1_dir: &str = env!("RELOAD_PLUGIN_V1_DIR");
      rt.load_bundle(std::path::Path::new(v1_dir))
          .expect("load v1 must succeed");

      // Get contract_id for the reload fixture from build.rs env var.
      // build.rs must set: println!("cargo:rustc-env=RELOAD_PLUGIN_CONTRACT_ID={}", contract_id)
      // See References for how to add this to build.rs.
      let contract_id_str: &str = env!("RELOAD_PLUGIN_CONTRACT_ID");
      let contract_id: u64 = contract_id_str.parse::<u64>()
          .expect("RELOAD_PLUGIN_CONTRACT_ID must be a valid u64");

      // Find a handle for the loaded plugin
      let handles: Vec<PluginHandle> = rt.find_all_by_contract(contract_id, 0_u32);
      assert!(!handles.is_empty(), "must find at least one plugin for contract_id={:#x}", contract_id);
      let handle: PluginHandle = handles[0];

      // Pass PluginHandle fields (both Copy u32) to background thread.
      // PluginHandle is a plain struct {index: u32, generation: u32} — both are Send.
      let index: u32 = handle.index;
      let generation: u32 = handle.generation;

      // Clone registry Arc for the background thread (Arc<Registry> is Send).
      let registry_arc: std::sync::Arc<Registry> = rt.registry().clone();

      let hold_thread = std::thread::spawn(move || {
          // Reconstruct handle on this thread
          let h: PluginHandle = PluginHandle { index, generation };
          // Resolve guard HERE — PluginVTableGuard is !Send, stays on this thread
          let guard = registry_arc
              .resolve_guard(h)
              .expect("resolve_guard must succeed for loaded plugin");
          // Hold for 7s — longer than the 5s QUIESCENCE_TIMEOUT
          std::thread::sleep(std::time::Duration::from_secs(7_u64));
          drop(guard);
      });

      // Give the background thread time to acquire the guard before attempting reload
      std::thread::sleep(std::time::Duration::from_millis(100));

      let v2_dir: &str = env!("RELOAD_PLUGIN_V2_DIR");
      let result = rt.reload_bundle(std::path::Path::new(v2_dir));

      // Join the background thread (it will finish after QUIESCENCE_TIMEOUT fires)
      hold_thread.join().expect("hold thread must not panic");

      match result {
          Err(PolyplugError::QuiescenceTimeout { .. }) => {
              // Expected — test passes
          }
          Err(e) => panic!("Expected QuiescenceTimeout, got: {:?}", e),
          Ok(()) => panic!("Expected QuiescenceTimeout, got Ok(())"),
      }

      // Verify runtime is healthy: retry reload now that guard is dropped
      let result2 = rt.reload_bundle(std::path::Path::new(v2_dir));
      assert!(result2.is_ok(), "second reload must succeed after guard is released: {:?}", result2);
  }
  ```

  **Visibility confirmed**: `polyplug::runtime::Runtime` is `pub struct`. `polyplug::registry::Registry` is
  accessible via `polyplug::registry::Registry`. `polyplug::abi::PluginHandle` is `pub struct`.
  `pack_handle`/`unpack_handle` are private `fn` in `ffi/mod.rs` — do NOT use them.
  Pass `handle.index: u32` and `handle.generation: u32` to the background thread instead.

  **`RELOAD_PLUGIN_CONTRACT_ID` env var**: This must be added to `crates/polyplug/build.rs`.
  After building `reload_plugin_v1`, the build.rs must emit:
  `println!("cargo:rustc-env=RELOAD_PLUGIN_CONTRACT_ID={}", contract_id)`.
  The contract_id is the FNV-1a 64-bit hash of the contract name string used in the reload fixture.
  To find it: open `tests/fixtures/reload_plugin_v1/src/lib.rs` and read the `polyplug_init` impl
  to find what string is used as the contract name; then compute `fnv1a_64(name.as_bytes())`.
  OR: export `pub const RELOAD_CONTRACT_ID: u64 = ...;` from the fixture (as a non-ABI symbol)
  and read it from the built .so via `libloading` in build.rs.
  The simplest approach: read the contract name from the fixture source, compute the hash in build.rs,
  and emit the env var. The implementation agent must look at the reload fixture source.

  **Must NOT do**:
  - Do NOT make `PluginVTableGuard` `Send`
  - The test MUST be marked `#[ignore]`
  - Do NOT hardcode a 5-second sleep that's shorter than `QUIESCENCE_TIMEOUT`
  - Do NOT use `pack_handle` or `unpack_handle` (they are private `fn` in `ffi/mod.rs`)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8–10, 12)
  - **Blocks**: Task 14
  - **Blocked By**: Task 4 (needs `reload_bundle` to work correctly)

  **References**:
  - `tests/integration_reload/mod.rs` — full reload test showing `RELOAD_PLUGIN_V1_DIR` / `V2_DIR` env vars and fixture loading
  - `crates/polyplug/src/reload/mod.rs:14` — `QUIESCENCE_TIMEOUT = Duration::from_secs(5)`
  - `crates/polyplug/src/registry/mod.rs:40-45` — `PluginVTableGuard` struct and `!Send` annotation
  - `crates/polyplug/src/registry/mod.rs:401-426` — `resolve_guard` implementation
  - `crates/polyplug/src/error/mod.rs:41-42` — `QuiescenceTimeout { bundle: String }` variant
  - `crates/polyplug/build.rs` — check/add `RELOAD_PLUGIN_CONTRACT_ID` env var

  **Acceptance Criteria**:
  - [ ] `tests/integration_quiescence/mod.rs` exists with `test_quiescence_timeout` function
  - [ ] Test is marked `#[ignore]`
  - [ ] `cargo test -p polyplug integration_quiescence -- --ignored` → 1 passed (~5s)

  **QA Scenarios**:
  ```
  Scenario: Quiescence timeout test passes when run with --ignored
    Tool: Bash (after Task 14)
    Steps:
      1. Run: cargo test -p polyplug test_quiescence_timeout -- --ignored 2>&1
      2. Assert exit code 0
      3. Assert output contains "1 passed"
    Expected Result: Test passes in ~5-7 seconds
    Evidence: .sisyphus/evidence/task-11-quiescence.txt
  ```

  **Commit**: YES (group with Tasks 8– 10, 12, 14)

---

- [ ] 12. **Create `tests/integration_stringview_nulls/mod.rs`**

  **What to do**:
  Create `tests/integration_stringview_nulls/mod.rs` testing that `StringView` with embedded
  null bytes round-trips correctly. `StringView` is a ptr+len pair — embedded nulls must
  never truncate the data.

  ```rust
  //! Integration tests: StringView with embedded null bytes.
  //! polyplug never treats StringView as null-terminated — embedded \x00 bytes must
  //! be preserved through any polyplug-internal API that processes StringViews.

  use polyplug::abi::StringView;

  #[test]
  fn test_stringview_embedded_null_length() {
      // StringView with embedded null — len must be 11, not 5
      let data: &[u8] = b"hello\x00world";
      let sv: StringView = StringView {
          ptr: data.as_ptr(),
          len: data.len(),
      };
      assert_eq!(sv.len, 11_usize, "StringView len must not be truncated at null byte");
  }

  #[test]
  fn test_stringview_roundtrip_through_host_alloc() {
      // Allocate host memory for data with embedded null, write to it,
      // read it back, assert no truncation.
      use polyplug::ffi::{polyplug_host_alloc, polyplug_host_free};
      // OR declare inline:
      unsafe extern "C" {
          fn polyplug_host_alloc(size: usize, align: usize) -> *mut u8;
          fn polyplug_host_free(ptr: *mut u8, size: usize, align: usize);
      }

      let data: &[u8] = b"hello\x00world";
      let size: usize = data.len();
      // SAFETY: size > 0, align = 1 is a power of two.
      let ptr: *mut u8 = unsafe { polyplug_host_alloc(size, 1_usize) };
      assert!(!ptr.is_null(), "host_alloc must succeed for size=11");
      // SAFETY: ptr is non-null and valid for `size` bytes.
      unsafe { core::ptr::copy_nonoverlapping(data.as_ptr(), ptr, size) };
      // Read back the bytes
      // SAFETY: ptr valid for size bytes, just written.
      let read_back: &[u8] = unsafe { core::slice::from_raw_parts(ptr, size) };
      assert_eq!(read_back, data, "data with embedded null must round-trip unchanged");
      // SAFETY: ptr was allocated with size=11, align=1.
      unsafe { polyplug_host_free(ptr, size, 1_usize) };
  }

  #[test]
  fn test_stringview_from_static_with_embedded_null() {
      // StringView::from_static does NOT exist — StringView is a raw struct.
      // Test that constructing a StringView pointing to static data with embedded null
      // preserves the full length.
      let data: &'static [u8] = b"poly\x00plug";
      let sv: StringView = StringView {
          ptr: data.as_ptr(),
          len: data.len(), // 9
      };
      assert_eq!(sv.len, 9_usize, "static StringView with embedded null must have full len");
      // Verify as_bytes (if available) or raw slice reconstruction preserves null
      // SAFETY: sv.ptr is a valid pointer to sv.len bytes of static data.
      let slice: &[u8] = unsafe { core::slice::from_raw_parts(sv.ptr, sv.len) };
      assert_eq!(slice[4], 0_u8, "byte at index 4 must be null");
      assert_eq!(slice[5], b'p', "byte at index 5 must be 'p'");
  }
  ```

  **NOTE on `StringView` visibility**: Check `crates/polyplug/src/abi/mod.rs` — if `StringView`
  is not re-exported from the crate root, use `polyplug::abi::StringView` or declare the struct
  inline with `#[repr(C)]`. Implementation agent must check `src/lib.rs` exports.

  **Must NOT do**:
  - Do NOT change `StringView` to be null-terminated
  - Do NOT test paths that involve converting StringView to CString (there shouldn't be any)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 8–11)
  - **Blocks**: Task 14
  - **Blocked By**: None

  **References**:
  - `crates/polyplug/src/abi/mod.rs` — `StringView` struct definition
  - `crates/polyplug/src/allocator/mod.rs` — `polyplug_host_alloc`/`polyplug_host_free` signatures

  **Acceptance Criteria**:
  - [ ] `tests/integration_stringview_nulls/mod.rs` exists with 3 test functions
  - [ ] After Task 14: `cargo test -p polyplug integration_stringview_nulls` → 3 passed

  **QA Scenarios**:
  ```
  Scenario: All 3 StringView null tests pass
    Tool: Bash (after Task 14)
    Steps:
      1. Run: cargo test -p polyplug integration_stringview_nulls 2>&1
      2. Assert exit code 0
      3. Assert output contains "3 passed"
    Expected Result: All 3 tests pass
    Evidence: .sisyphus/evidence/task-12-stringview-tests.txt
  ```

  **Commit**: YES (group with Tasks 8–11, 14)

---

- [ ] 13. **Append `test_double_free_detected` to `tests/stress_memory/mod.rs`**

  **What to do**:
  Append one new test function to the END of `tests/stress_memory/mod.rs`.

  ```rust
  #[test]
  #[should_panic(expected = "double-free")]
  #[cfg(debug_assertions)]
  fn test_double_free_detected() {
      // Allocate via tracking_alloc, free twice, assert double-free panic.
      // Uses TrackingAllocator which wraps polyplug_host_alloc/free.
      let tracker: TrackingAllocator = TrackingAllocator::new();
      let alloc: unsafe extern "C" fn(usize, usize) -> *mut u8 = tracker.alloc_fn();
      let free_fn: unsafe extern "C" fn(*mut u8, usize, usize) = tracker.free_fn();
      // SAFETY: size=64, align=1 is a valid layout.
      let ptr: *mut u8 = unsafe { alloc(64_usize, 1_usize) };
      assert!(!ptr.is_null(), "alloc must succeed");
      // SAFETY: ptr was just allocated with size=64, align=1.
      unsafe { free_fn(ptr, 64_usize, 1_usize) };
      // Second free on same pointer — must panic with "double-free" in debug builds.
      // SAFETY: This is intentionally invalid — test verifies the panic fires.
      unsafe { free_fn(ptr, 64_usize, 1_usize) };
  }
  ```

  **Placement**: Append after the last existing test function in the file.
  The `TrackingAllocator` import is already present at the top of the file.

  **Must NOT do**:
  - Do NOT remove `#[cfg(debug_assertions)]` — the test only runs in debug builds
  - Do NOT add `.unwrap()` anywhere
  - Do NOT change existing tests in the file

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 3 (after Task 3)
  - **Blocks**: Task 14 (indirectly)
  - **Blocked By**: Task 3

  **References**:
  - `tests/stress_memory/mod.rs` — append at end; `TrackingAllocator` import already at top
  - `crates/polyplug/src/allocator/tracking/mod.rs` — `alloc_fn()`/`free_fn()` API (Task 3 output)

  **Acceptance Criteria**:
  - [ ] `grep -n 'test_double_free_detected' tests/stress_memory/mod.rs` → 1 result
  - [ ] `cargo test -p polyplug stress_memory::test_double_free_detected` → PASSED (panics as expected)

  **QA Scenarios**:
  ```
  Scenario: Double-free test panics with correct message in debug build
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug test_double_free_detected 2>&1
      2. Assert exit code 0 (test passes because should_panic caught the panic)
      3. Assert output contains "test result: ok" or "passed"
    Expected Result: Test passes (should_panic matching "double-free")
    Evidence: .sisyphus/evidence/task-13-double-free-test.txt

  Scenario: Double-free test is absent in release build
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug --release test_double_free_detected 2>&1
      2. Assert output contains "0 tests" or the test is not found
    Expected Result: Test does not appear in release binary (cfg(debug_assertions) gates it)
    Evidence: .sisyphus/evidence/task-13-release-absent.txt
  ```

  **Commit**: YES (group with Tasks 1, 2, 4)

---

- [ ] 14. **Wire all new tests in `crates/polyplug/Cargo.toml` + workspace members**

  **What to do**:

  **Edit A — `crates/polyplug/Cargo.toml`**: Add 5 new `[[test]]` entries.
  Insert after the last existing `[[test]]` entry (currently `integration_reload` at line ~153):
  ```toml
  [[test]]
  name = "integration_ffi_null"
  path = "../../tests/integration_ffi_null/mod.rs"

  [[test]]
  name = "integration_invalid_utf8"
  path = "../../tests/integration_invalid_utf8/mod.rs"

  [[test]]
  name = "integration_malformed"
  path = "../../tests/integration_malformed/mod.rs"

  [[test]]
  name = "integration_quiescence"
  path = "../../tests/integration_quiescence/mod.rs"

  [[test]]
  name = "integration_stringview_nulls"
  path = "../../tests/integration_stringview_nulls/mod.rs"
  ```
  Note: `stress_memory` already has an entry — do NOT add a duplicate for Task 13.

  **Edit B — Root `Cargo.toml` workspace members** (line 3):
  Add `"tests/fixtures/no_init_plugin"` to the `members` array.
  Current line: `members = ["crates/*", "tests/fixtures/test_plugin", ...]`
  Add `"tests/fixtures/no_init_plugin"` to the end of the array.

  **Must NOT do**:
  - Do NOT duplicate existing test entries
  - Do NOT use bare `.rs` file paths — all new entries use `mod.rs` directory pattern
  - Do NOT add `stress_memory` again (already exists)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (after all test files exist)
  - **Blocks**: F1, F2
  - **Blocked By**: Tasks 8–13

  **References**:
  - `crates/polyplug/Cargo.toml:28-154` — all existing `[[test]]` entries
  - Root `Cargo.toml:3` — workspace `members` line

  **Acceptance Criteria**:
  - [ ] `grep -c '\[\[test\]\]' crates/polyplug/Cargo.toml` → count increases by 5
  - [ ] `grep 'no_init_plugin' Cargo.toml` (root) → 1 result
  - [ ] `cargo test --workspace` compiles all new test binaries

  **QA Scenarios**:
  ```
  Scenario: cargo test compiles all new test binaries
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace 2>&1
      2. Assert exit code 0
      3. Assert output mentions integration_ffi_null, integration_invalid_utf8,
         integration_malformed, integration_stringview_nulls
    Expected Result: All new tests compile and run
    Evidence: .sisyphus/evidence/task-14-cargo-test.txt
  ```

  **Commit**: YES (group with Tasks 8–12)
## Success Criteria

```bash
grep -rn "from_utf8_unchecked" crates/ | grep -v "SAFETY"   # Expected: (empty)
cargo clippy -- -D warnings                                   # Expected: no warnings
cargo test --workspace                                        # Expected: all pass
cargo test --workspace -- --ignored                          # Expected: all pass
```
