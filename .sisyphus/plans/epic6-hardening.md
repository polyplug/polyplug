# Epic 6 — Memory and Error Model Hardening

## TL;DR

> **Quick Summary**: Three-phase hardening epic. Phase 1 gates everything: smoke-test that Epic 5 codegen and all existing tests actually run clean. Phase 2 adds memory and error model stress tests with a custom tracking allocator that proves zero leaks. Phase 3 adds criterion benchmarks and a populated BENCHMARKS.md baseline.
>
> **Deliverables**:
> - `tests/smoke/mod.rs` — Phase 1 gate tests
> - `tests/fixtures/memory_plugin/` — hand-written cdylib for Buffer/StringView stress
> - `tests/fixtures/error_plugin/` — hand-written cdylib for error codes + panic
> - `tests/stress_memory/mod.rs` — memory model stress tests
> - `tests/stress_error/mod.rs` — error model stress tests
> - `crates/polyplug-runtime/src/allocator/tracking/mod.rs` — TrackingAllocator
> - `crates/polyplug-runtime/benches/vtable_dispatch.rs` — criterion benchmarks
> - `BENCHMARKS.md` — at workspace root with real numbers
> - Dispatcher implementation (wiring `host_find_plugin` + `host_call_plugin`)
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 3 waves within Phase 2; Phase 1 and 3 sequential
> **Critical Path**: Phase 1 smoke → dispatcher impl → memory_plugin + error_plugin → stress tests → benchmarks → BENCHMARKS.md

---

## Context

### Original Request
Epic 6: harden the memory and error model before adding more language generators. No new features. Fix what's broken.

### Interview Summary
**Key Discussions**:
- Phase structure: user required Phase 1 smoke tests as a hard gate — Epic 5 was implemented but never run
- Dispatcher: implement the real dispatcher (OnceLock<Arc<Registry>>) before cross-plugin benchmark
- New plugins: hand-written cdylibs (memory_plugin, error_plugin) following test_plugin pattern
- Concurrency: std::thread::scope, 8 threads, no async
- TrackingAllocator: wraps polyplug_host_alloc/free call-counting — NOT GlobalAlloc (wrong abstraction)
- Benchmarks: [[bench]] in polyplug-runtime/Cargo.toml, criterion 0.8
- Leak detection: tracking allocator only, no valgrind/ASAN

**Research Findings (from Metis)**:
- criterion 0.8 is latest stable — use `{ version = "0.8", features = ["html_reports"] }`
- TrackingAllocator MUST wrap the ABI calls, not GlobalAlloc — GlobalAlloc counts all process allocations
- std::thread::scope borrows &Registry directly — no Arc needed for concurrency tests
- OnceLock registry accessor must return graceful null (never panic) per AGENTS.md Rule 4

### Metis Review
**Identified Gaps** (addressed in plan):
- Criterion version pinned to 0.8
- Benchmark crate pattern: [[bench]] in polyplug-runtime not a separate crate (consistent with [[test]] pattern)
- TrackingAllocator abstraction level clarified
- OnceLock graceful-degradation requirement noted
- AbiError.message string lifetime: message.ptr must remain valid across the read window — stress test must verify this
- "cross-plugin chain: B errors, A propagates" requires dispatcher impl first — plan orders these correctly

---

## Work Objectives

### Core Objective
Verify that every memory ownership contract and error propagation path in the existing ABI actually works correctly under stress, with zero leaks proven by a tracking allocator, and establish criterion performance baselines before adding more languages.

### Concrete Deliverables
- Smoke test binary that runs all existing integration tests end-to-end
- `memory_plugin` cdylib with 4 functions covering all Buffer and StringView ownership paths
- `error_plugin` cdylib with 3 functions: return-error-with-message, panic, error-chain
- `TrackingAllocator` struct in `allocator/tracking/mod.rs`
- 11 stress test functions across 2 new test binaries
- Dispatcher: `host_find_plugin` and `host_call_plugin` fully wired to Registry
- 4 criterion benchmarks populated with real numbers in BENCHMARKS.md

### Definition of Done
- [ ] `cargo test --workspace` passes with zero failures
- [ ] All stress tests pass with `assert_no_leaks()` showing zero net allocations
- [ ] `cargo bench -p polyplug-runtime` completes and produces output
- [ ] `BENCHMARKS.md` populated with real numbers
- [ ] `cargo clippy --workspace -- -D warnings` → zero warnings
- [ ] `cargo fmt --check` → clean

### Must Have
- Phase 1 is a hard gate — nothing from Phase 2 or 3 begins until Phase 1 is green
- TrackingAllocator must wrap `polyplug_host_alloc`/`polyplug_host_free`, not GlobalAlloc
- AbiError.message allocation and free must be explicitly tested (currently untested)
- Both Buffer ownership paths tested: (A) host pre-allocates + plugin fills, (B) plugin allocates + host frees
- Dispatcher implementation must not break any existing tests
- OnceLock registry accessor must degrade gracefully (return null) when runtime not set

### Must NOT Have (Guardrails)
- No new ABI struct fields (§7 ABI stability — HostVTable is frozen)
- No .unwrap() in any new production code (existing test infrastructure may use .expect() per AGENTS.md)
- No GlobalAlloc implementation in TrackingAllocator (wrong abstraction level — counts unrelated allocs)
- No bare `filename.rs` module roots — all new modules must use `dirname/mod.rs`
- No `use` statements inside functions or impl blocks
- No new features beyond what hardening requires
- No valgrind/ASAN in the verification checklist (tracking allocator is sufficient)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (7 existing test binaries)
- **Automated tests**: Tests-after (no TDD for stress tests)
- **Framework**: Rust built-in `#[test]` + criterion for benchmarks
- **No GlobalAlloc**: TrackingAllocator counts only ABI boundary calls

### QA Policy
Every task MUST include agent-executed QA scenarios.
- **CLI**: Use Bash — `cargo test`, `cargo clippy`, `cargo bench`
- **API/Library**: Use Bash — import, call, compare output
- Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.txt`

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 — Phase 1: SMOKE GATE (sequential, must fully pass before Wave 2)
└── Task 1: Run cargo test --workspace and fix all failures (Phase 1 smoke gate)
    Task 2: Write tests/smoke/mod.rs with E2E codegen roundtrips

Wave 2 — Foundation (after Phase 1 green, run in PARALLEL):
├── Task 3: Add criterion 0.8 to workspace Cargo.toml + [[bench]] to polyplug-runtime
├── Task 4: Implement TrackingAllocator in allocator/tracking/mod.rs
├── Task 5: Implement dispatcher (host_find_plugin + host_call_plugin via OnceLock)
├── Task 6: Write tests/fixtures/memory_plugin/ cdylib
└── Task 7: Write tests/fixtures/error_plugin/ cdylib

Wave 3 — Stress Tests (after Wave 2, run in PARALLEL):
├── Task 8: Write tests/stress_memory/mod.rs (6 test functions)
└── Task 9: Write tests/stress_error/mod.rs (4 test functions)

Wave 4 — Benchmarks (after Waves 1-3):
├── Task 10: Write crates/polyplug-runtime/benches/vtable_dispatch.rs (4 benchmarks)
└── Task 11: Run benchmarks, write BENCHMARKS.md with real numbers

Wave FINAL — Verification (after ALL tasks):
├── Task F1: cargo test --workspace passes (oracle)
├── Task F2: cargo clippy --workspace -- -D warnings (unspecified-high)
├── Task F3: All stress tests show zero leaks (deep)
└── Task F4: BENCHMARKS.md populated with real numbers (unspecified-high)
```

### Dependency Matrix
- Task 1 → ALL (Phase 1 gate)
- Task 2 → depends: 1
- Tasks 3,4,5,6,7 → depends: 1,2 (Wave 2, parallel)
- Task 8 → depends: 4,5,6
- Task 9 → depends: 4,5,7
- Task 10 → depends: 3,5
- Task 11 → depends: 10

### Agent Dispatch Summary
- Wave 1: T1 → `deep`, T2 → `unspecified-high`
- Wave 2: T3 → `quick`, T4 → `unspecified-high`, T5 → `deep`, T6 → `unspecified-high`, T7 → `unspecified-high`
- Wave 3: T8 → `deep`, T9 → `unspecified-high`
- Wave 4: T10 → `deep`, T11 → `unspecified-high`
- Wave FINAL: F1 → `oracle`, F2-F4 → `unspecified-high`

---

## TODOs
- [x] 11. Run benchmarks and write `BENCHMARKS.md` with real numbers

  **What to do**:
  - Run: `cargo bench -p polyplug-runtime --bench vtable_dispatch 2>&1 | tee .sisyphus/evidence/task-11-bench-output.txt`
  - Extract numbers from criterion output (lines like `dispatch/noop  time: [X.XX ns X.XX ns X.XX ns]`)
  - Create `BENCHMARKS.md` at workspace root with this EXACT template:
    ```markdown
    # polyplug Benchmark Results

    ## Methodology
    - Tool: criterion 0.8 (https://docs.rs/criterion)
    - Platform: [runner fills in: OS, CPU, RAM]
    - Rust toolchain: [runner fills in: rustc --version]
    - Optimization: --release (criterion default)
    - Iterations: criterion auto-selects based on measurement time

    ## Results

    | Benchmark | Mean (ns) | Std Dev (ns) | Notes | Epic 6 Baseline |
    |-----------|-----------|--------------|-------|-----------------|
    | dispatch/noop | N.NN | N.NN | Pure vtable dispatch, no args | YES |
    | dispatch/buffer_arg | N.NN | N.NN | 4096-byte buffer fill + dispatch | YES |
    | dispatch/struct_arg_and_return | N.NN | N.NN | AddArgs struct in, u32 out | YES |
    | dispatch/cross_plugin | N.NN | N.NN | Full dispatcher chain: OnceLock + Registry.find + dispatch | YES |

    ## Interpretation
    - `dispatch/noop` baseline establishes pure ABI overhead
    - `dispatch/cross_plugin` minus `dispatch/noop` = cross-plugin indirection cost
    - Future epics: add new rows as new paths are introduced; compare against Epic 6 Baseline

    ## Epic History
    | Epic | Date | Notes |
    |------|------|-------|
    | Epic 6 | [date] | Initial baseline |
    ```
  - Fill in every `N.NN` with the real number from criterion output
  - Fill in platform, toolchain, date
  - Do NOT leave any N.NN unfilled

  **Must NOT do**:
  - Do NOT fabricate numbers — run the actual benchmark and copy the mean value
  - Do NOT leave the Std Dev column empty

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Shell out to cargo bench, parse output, write markdown
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Task 10)
  - **Parallel Group**: Wave 4 (sequential after Task 10)
  - **Blocks**: F4
  - **Blocked By**: Task 10

  **References**:
  - Task 10 output: `crates/polyplug-runtime/benches/vtable_dispatch.rs`
  - `.sisyphus/evidence/task-11-bench-output.txt` — criterion output to parse

  **Acceptance Criteria**:
  - [ ] `BENCHMARKS.md` exists at workspace root
  - [ ] All 4 benchmark rows populated with real numeric values
  - [ ] Epic 6 Baseline column says YES for all 4
  - [ ] No N.NN placeholder remains

  **QA Scenarios**:
  ```
  Scenario: BENCHMARKS.md has 4 data rows with real numbers
    Tool: Bash
    Steps:
      1. Run: grep -c '| dispatch/' BENCHMARKS.md
    Expected Result: 4
    Failure Indicators: fewer than 4 rows, or any row contains 'N.NN'
    Evidence: .sisyphus/evidence/task-11-benchmarks-md.txt

  Scenario: No placeholder values remain
    Tool: Bash
    Steps:
      1. grep 'N\.NN' BENCHMARKS.md
    Expected Result: empty output (no N.NN found)
    Evidence: .sisyphus/evidence/task-11-no-placeholders.txt
  ```

  **Commit**: YES
  - Message: `bench: add vtable dispatch criterion benchmarks; populate BENCHMARKS.md baseline`
  - Files: `crates/polyplug-runtime/benches/vtable_dispatch.rs`, `BENCHMARKS.md`
  - Pre-commit: `cargo bench -p polyplug-runtime --bench vtable_dispatch -- --test` (dry run)


- [x] 10. Write `crates/polyplug-runtime/benches/vtable_dispatch.rs` — 4 criterion benchmarks

  **What to do**:
  - Create `crates/polyplug-runtime/benches/vtable_dispatch.rs`
  - File header: `// THIS IS A BENCHMARK FILE — do not add #[test] functions here`
  - Setup:
    ```rust
    use criterion::BenchmarkId;
    use criterion::Criterion;
    use criterion::criterion_group;
    use criterion::criterion_main;
    ```
  - Set up once before benchmarks: load test_plugin .so (use `env!("TEST_PLUGIN_SO")`), call polyplug_init, capture vtable, register in a Registry.
  - Use `criterion_group!` and `criterion_main!` macros at bottom of file

  **Benchmark 1 — `dispatch/noop`**:
  ```
  // Name: "dispatch/noop"
  // Measure: pure vtable dispatch overhead for a no-arg, no-output function
  // Setup: need a noop function in the vtable. PROBLEM: test_plugin only has 'add'.
  // SOLUTION: Use add(0, 0) which has trivial computation close to noop.
  // Arguments: AddArgs { a: 0, b: 0 }, out: u32 = 0
  // The benchmark body: single dispatch_fn(...) call per iteration
  // group.throughput(Throughput::Elements(1)) — measuring latency per call
  ```

  **Benchmark 2 — `dispatch/buffer_arg`**:
  ```
  // Name: "dispatch/buffer_arg"
  // Measure: dispatch latency when passing a pre-allocated Buffer argument
  // Setup: Load memory_plugin (env!("MEMORY_PLUGIN_SO")), fn 0 (fill_preallocated_buffer)
  // Allocate a 4096-byte buffer once BEFORE the benchmark loop (in setup closure)
  // Benchmark body: call fill fn (1 iteration = 1 dispatch + 4096 byte fill)
  // NOTE: buffer fill includes ~4KB write, so this measures dispatch + memory write combined
  // That's intentional — represents a real Buffer-passing call
  ```

  **Benchmark 3 — `dispatch/struct_arg_and_return`**:
  ```
  // Name: "dispatch/struct_arg_and_return"
  // Measure: dispatch latency when passing a struct arg and receiving a u32 return
  // Setup: test_plugin fn 0 (add) with AddArgs { a: 42, b: 57 }
  // Benchmark body: single dispatch call, out: u32
  // This measures the dominant real-world path: struct in, primitive out
  ```

  **Benchmark 4 — `dispatch/cross_plugin`**:
  ```
  // Name: "dispatch/cross_plugin"
  // Measure: cross-plugin call through HostVTable.call_plugin dispatcher
  // Setup:
  //   1. Load memory_plugin
  //   2. Register in Registry via set_global_registry (Task 5)
  //   3. Call error_plugin fn 2 (chain_propagate) which internally calls memory_plugin via dispatcher
  // This measures: fn call → HostVTable.call_plugin → OnceLock lookup → Registry.find → Registry.resolve → dispatch
  // Represents the overhead added by cross-plugin indirection vs direct vtable call
  ```

  **Must NOT do**:
  - Do NOT use `criterion::black_box` incorrectly — wrap inputs AND outputs in `black_box`
  - Do NOT put business logic in benchmarks
  - Do NOT use .unwrap() — use .expect() (benchmarks are in benches/, not #[cfg(test)]... actually benches ARE dev code, .expect() is acceptable here per AGENTS.md)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires combining libloading, unsafe vtable dispatch, criterion API, and correct benchmark design (avoid measurement artifacts)
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 11 only if bench file exists first)
  - **Parallel Group**: Wave 4
  - **Blocks**: Task 11, F4
  - **Blocked By**: Tasks 3, 5

  **References**:
  - `tests/integration_dispatch/mod.rs` lines 87-162 — pattern for loading plugin + vtable dispatch (copy dispatch setup)
  - `crates/polyplug-runtime/Cargo.toml` (after Task 3) — criterion dev-dep and [[bench]] entry already added
  - criterion docs: https://docs.rs/criterion/0.5/criterion/ — `criterion_group!`, `Criterion::bench_function`, `black_box`
  - Task 5 output: `set_global_registry()` function for cross-plugin benchmark setup

  **Acceptance Criteria**:
  - [ ] `cargo bench -p polyplug-runtime --bench vtable_dispatch -- --test` passes (dry run mode)
  - [ ] File has exactly 4 benchmark functions under a `dispatch/` group
  - [ ] All benchmark inputs wrapped in `criterion::black_box`

  **QA Scenarios**:
  ```
  Scenario: Benchmarks compile and run in test mode
    Tool: Bash
    Steps:
      1. Run: cargo bench -p polyplug-runtime --bench vtable_dispatch -- --test 2>&1 | tee .sisyphus/evidence/task-10-bench-test.txt
      2. Check: grep 'test result:' .sisyphus/evidence/task-10-bench-test.txt | grep '0 failed'
    Expected Result: 4 benchmarks pass in test mode
    Evidence: .sisyphus/evidence/task-10-bench-test.txt
  ```

  **Commit**: NO (groups with Task 11)


- [x] 9. Write `tests/stress_error/mod.rs` — 4 error model stress test functions

  **What to do**:
  - Create directory `tests/stress_error/` and file `tests/stress_error/mod.rs`
  - Add `[[test]] name = "stress_error" path = "../../tests/stress_error/mod.rs"` to `crates/polyplug-runtime/Cargo.toml`
  - Load error_plugin via `env!("ERROR_PLUGIN_SO")`

  - **Test 1: `stress_error_code_and_message_received_correctly()`**
    - Load error_plugin
    - Call fn 0 (`return_with_message`) via vtable
    - Assert `result.code == 99`
    - Assert `result.message.len == 21`
    - Read the message bytes: `slice::from_raw_parts(result.message.ptr, result.message.len)` == b`"test error from plugin"`
    - FREE the message: `polyplug_host_free(result.message.ptr as *mut u8, result.message.len, 1)`
    - Create TrackingAllocator, thread-reset it, call the function again, assert_no_leaks() after free

  - **Test 2: `stress_panic_returns_abi_error_panic_process_continues()`**
    - Load error_plugin
    - Call fn 1 (`error_panic`) via vtable
    - Assert `result.code == ABI_ERROR_PANIC` (== 3)
    - Assert `result.message.ptr == b"plugin panicked".as_ptr()` OR message bytes == b`"plugin panicked"`
    - Assert process continues (the fact that the test reaches this assert IS the proof)
    - Note: `ABI_ERROR_PANIC`'s message uses `StringView::from_static` — it is NOT host_alloc'd, do NOT free it

  - **Test 3: `stress_error_chain_b_errors_a_propagates()`**
    - This test requires the dispatcher to be wired (Task 5)
    - Load memory_plugin AND error_plugin
    - Set up global registry: call `set_global_registry(Arc::clone(&registry))` (Task 5 export)
    - Register memory_plugin's error-returning function as a callable target:
      - OR: simply call fn 0 of error_plugin (chain propagate) with a target that IS error_plugin fn 0 itself (self-chain: A calls A.fn0)
      - Simpler: register both plugins in registry; error_plugin fn 2 calls error_plugin fn 0 via dispatcher
    - Call error_plugin fn 2 with HostVTable pointing to real dispatcher
    - Assert: result.code == 99 (the error from the inner call was propagated)
    - Free the message if it was host_alloc'd (tracker.assert_no_leaks())

  - **Test 4: `stress_error_message_lifetime_valid_during_read()`**
    - Load error_plugin
    - Call fn 0, receive AbiError with message StringView
    - Immediately read message.ptr in a loop 1000 times (verifying pointer is stable)
    - Assert all 1000 reads return same bytes
    - Free message AFTER reads complete (not before)
    - assert_no_leaks()
    - Purpose: validates message.ptr remains valid across the read window (lifetime contract)

  **Must NOT do**:
  - Do NOT free ABI_ERROR_PANIC messages (they are from_static, not host_alloc'd)
  - Do NOT .unwrap() in production paths

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Error model ownership rules, unsafe pointer reads, dispatcher interaction
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 8)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1, F2, F3
  - **Blocked By**: Tasks 4, 5, 7

  **References**:
  - `tests/integration_panic/mod.rs` lines 252-273 — pattern for calling panic fn + asserting ABI_ERROR_PANIC
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 7, 10 — `ABI_OK`, `ABI_ERROR_PANIC` constants
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 72-84 — AbiError struct (message ownership doc comment)
  - `crates/polyplug-runtime/src/allocator/mod.rs` — polyplug_host_free for freeing error messages
  - `crates/polyplug-runtime/src/runtime/mod.rs` (Task 5 output) — `set_global_registry()` function for Test 3

  **Acceptance Criteria**:
  - [ ] `cargo test --test stress_error` passes (4 tests, 0 failed)
  - [ ] Test 1 and 4: message ptr is freed and assert_no_leaks passes
  - [ ] Test 2: ABI_ERROR_PANIC code == 3 asserted
  - [ ] Test 3: cross-plugin error propagation produces code == 99 end-to-end

  **QA Scenarios**:
  ```
  Scenario: All 4 error stress tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --test stress_error -- --nocapture 2>&1 | tee .sisyphus/evidence/task-9-stress-error.txt
      2. Check: grep 'test result:' .sisyphus/evidence/task-9-stress-error.txt | grep '0 failed'
    Expected Result: test result: 4 passed, 0 failed
    Evidence: .sisyphus/evidence/task-9-stress-error.txt

  Scenario: Message is freed after read (no leak)
    Tool: Bash
    Steps:
      1. grep -n 'assert_no_leaks\|polyplug_host_free' tests/stress_error/mod.rs | head -20
    Expected Result: both appear in tests 1 and 4
    Evidence: .sisyphus/evidence/task-9-leak-check.txt
  ```

  **Commit**: YES (Wave 3 commit with Tasks 8-9)
  - Message: `test(stress): add memory + error model stress tests with zero-leak assertions`
  - Files: `tests/stress_memory/mod.rs`, `tests/stress_error/mod.rs`, `crates/polyplug-runtime/Cargo.toml`
  - Pre-commit: `cargo test --workspace`


- [x] 8. Write `tests/stress_memory/mod.rs` — 6 memory model stress test functions

  **What to do**:
  - Create directory `tests/stress_memory/` and file `tests/stress_memory/mod.rs`
  - Add `[[test]] name = "stress_memory" path = "../../tests/stress_memory/mod.rs"` to `crates/polyplug-runtime/Cargo.toml`
  - File header: `#![allow(clippy::expect_used)]` (integration test convention)
  - Set up helpers at top of file:
    - `fn workspace_root() -> PathBuf` (same pattern as integration_codegen_rust)
    - Load memory_plugin with libloading using `env!("MEMORY_PLUGIN_SO")`
    - Build a real `RegistryWithHostVTable` helper: creates a Registry, a TrackingAllocator, and a HostVTable wired to both the tracking allocator and the dispatcher

  - **Test 1: `stress_large_buffer_fill_and_read()`**
    - Load memory_plugin
    - Allocate a 1 MiB buffer via `polyplug_host_alloc(1024*1024, 1)` (host pre-allocates)
    - Call fn 0 (`fill_preallocated_buffer`) with fill_byte = 0xAB
    - Assert all 1,048,576 bytes == 0xAB
    - Call `polyplug_host_free(ptr, 1024*1024, 1)`
    - Call `tracker.assert_no_leaks()`

  - **Test 2: `stress_string_view_non_ascii_utf8()`**
    - Load memory_plugin
    - Create a byte slice of non-ASCII UTF-8: `"Héllo Wörld — 日本語テスト"` as a `&[u8]`
    - Build a `StringView { ptr: bytes.as_ptr(), len: bytes.len() }`
    - Call fn 2 (`echo_string_view`) via vtable
    - Assert returned StringView has same ptr and len
    - Validate: `std::str::from_utf8(slice::from_raw_parts(out_sv.ptr, out_sv.len))` returns Ok

  - **Test 3: `stress_zero_length_buffer_and_string_view()`**
    - Load memory_plugin
    - Construct zero-length Buffer: `Buffer { ptr: std::ptr::null_mut(), len: 0, cap: 0 }`
    - Construct zero-length StringView: `StringView::null()`
    - Call fn 3 (`zero_length_roundtrip`)
    - Assert out.buf_len == 0 and out.sv_len == 0
    - Assert no panic occurred

  - **Test 4: `stress_concurrent_8_threads_no_shared_memory()`**
    - Load memory_plugin (library stays alive for scope)
    - Create a Registry, register memory_plugin vtable
    - Use `std::thread::scope(|s| { for _ in 0..8 { s.spawn(|| { /* call fn 0 */ }) } })`
    - Each thread: allocate its own 4096-byte buffer via `polyplug_host_alloc`, call fill (fn 0), verify bytes, free buffer
    - No shared mutable state between threads (each has own alloc/free pair)
    - After scope joins, assert no races occurred (no panic from any thread)
    - Use a per-thread TrackingAllocator OR a shared AtomicUsize counter to verify 8 allocs + 8 frees

  - **Test 5: `stress_plugin_allocates_returns_to_host_then_host_frees()`**
    - Load memory_plugin
    - Build a HostVTable with TrackingAllocator's alloc/free function pointers
    - Call fn 1 (`alloc_buffer_via_host`) with a real HostVTable pointer
    - Receive the returned Buffer in out
    - Assert out.ptr != null, out.len > 0
    - Call `polyplug_host_free(out.ptr, out.cap, 1)` to free
    - Call `tracker.assert_no_leaks()` — alloc_count == 1, free_count == 1

  - **Test 6: `stress_caller_alloc_plugin_fills_freed_after_use()`**
    - Load memory_plugin
    - Host allocates output buffer of 64 bytes via `polyplug_host_alloc(64, 1)` and tracks it
    - Calls fn 0 to have plugin fill it
    - Reads result (verify fill_byte pattern)
    - Frees the buffer via `polyplug_host_free`
    - Calls `tracker.assert_no_leaks()`
    - (This specifically reproduces the 'Caller allocates output buffer, plugin fills, freed after use' epic scenario)

  **Must NOT do**:
  - Do NOT use `Arc<Mutex<>>` shared state in concurrent test — each thread has its own allocations
  - Do NOT .unwrap() on anything that can fail in production paths

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires understanding ABI ownership rules, unsafe pointer patterns, std::thread::scope borrowing, and correct use of TrackingAllocator
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 9)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1, F2, F3
  - **Blocked By**: Tasks 4, 5, 6

  **References**:
  - `tests/integration_dispatch/mod.rs` — FULL — pattern for loading plugin + building registrar + dispatching through vtable
  - `crates/polyplug-runtime/src/allocator/tracking/mod.rs` (Task 4 output) — TrackingAllocator API
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 54-66 — Buffer struct fields
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 170-182 — HostVTable for fn 1 (plugin allocates via host)
  - `crates/polyplug-runtime/src/allocator/mod.rs` lines 19-31 — `polyplug_host_alloc` signature
  - `crates/polyplug-runtime/src/allocator/mod.rs` lines 44-59 — `polyplug_host_free` signature

  **Acceptance Criteria**:
  - [ ] `cargo test --test stress_memory` passes (6 tests, 0 failed)
  - [ ] All 6 tests include `assert_no_leaks()` call
  - [ ] Concurrent test uses 8 threads verified by log output or atomic counter

  **QA Scenarios**:
  ```
  Scenario: All 6 stress_memory tests pass with zero leaks
    Tool: Bash
    Steps:
      1. Run: cargo test --test stress_memory -- --nocapture 2>&1 | tee .sisyphus/evidence/task-8-stress-memory.txt
      2. Check: grep 'test result:' .sisyphus/evidence/task-8-stress-memory.txt | grep '0 failed'
    Expected Result: test result: 6 passed, 0 failed
    Failure Indicators: any 'FAILED' or 'leaked' in output
    Evidence: .sisyphus/evidence/task-8-stress-memory.txt

  Scenario: assert_no_leaks is called in every test
    Tool: Bash
    Steps:
      1. grep -c 'assert_no_leaks' tests/stress_memory/mod.rs
    Expected Result: 6 or more (one per test)
    Evidence: .sisyphus/evidence/task-8-leak-checks.txt
  ```

  **Commit**: NO (groups with Task 9)


- [x] 7. Write `tests/fixtures/error_plugin/` hand-written cdylib

  **What to do**:
  - Create `tests/fixtures/error_plugin/Cargo.toml` (same pattern as `tests/fixtures/test_plugin/Cargo.toml` — NO `[workspace]` table):
    ```toml
    [package]
    name             = "error_plugin"
    version          = "0.1.0"
    edition.workspace      = true
    license.workspace      = true
    rust-version.workspace = true
    publish          = false

    [lib]
    name       = "error_plugin"
    crate-type = ["cdylib"]

    [lints]
    workspace = true
    ```
  - Add `"tests/fixtures/error_plugin"` to workspace `members` list in root `Cargo.toml`
  - Create `tests/fixtures/error_plugin/src/lib.rs`
  - Mirror ABI types from test_plugin. Add the `polyplug_host_alloc` extern declaration:
    ```rust
    // Imported from the host (via the polyplug_host_alloc C symbol exported by the runtime)
    extern "C" { fn polyplug_host_alloc(size: usize, align: usize) -> *mut u8; }
    ```
  - Implement THREE functions:

    **fn 0 — `error_return_with_message`**:
    ```
    // args: *const () (ignored)
    // out: *mut () (ignored)
    // Behavior:
    //   1. Allocate a UTF-8 message via polyplug_host_alloc(msg.len(), 1)
    //   2. Write the bytes of b"test error from plugin" into the allocation
    //   3. Return AbiError { code: 99, message: StringView { ptr: alloc_ptr, len: 21 } }
    // OWNERSHIP: caller (host) must free message.ptr via polyplug_host_free(ptr, len, 1)
    ```

    **fn 1 — `error_panic`**:
    ```
    // args: *const () (ignored)
    // out: *mut () (ignored)
    // Behavior:
    //   1. catch_unwind wrapper (hand-written):
    //      - Inner closure: panic!("intentional error_plugin panic")
    //      - On unwind: return AbiError { code: ABI_ERROR_PANIC, message: StringView::from_static(b"plugin panicked") }
    //   2. The catch_unwind is hand-written (not generated) to test the ABI layer directly
    // The generated pattern from polyplugc uses std::panic::catch_unwind wrapping the inner call
    ```

    **fn 2 — `error_chain_propagate`**:
    ```
    // args: *const ChainArgs where ChainArgs = { host: *const HostVTable, target_contract_id: u64, target_fn_id: u32 }
    // out: *mut AbiError
    // Behavior:
    //   1. Call host.call_plugin(host.find_plugin(target_contract_id, 0), target_fn_id, null, null)
    //   2. If the inner call returns non-ABI_OK, propagate that AbiError to out
    //   3. Write the AbiError (success or propagated) to *out
    // This tests Plugin A calling Plugin B through the dispatcher and propagating B's error
    ```

  - Static vtable: 3 functions. Contract: `error.test@1`. FNV-1a of `"error.test@1"`.
  - Add to workspace members and add build.rs step (same pattern as memory_plugin)

  **Must NOT do**:
  - Do NOT suppress the catch_unwind — it must actually catch the panic
  - Do NOT use .unwrap()
  - Do NOT link against polyplug-runtime directly (cdylib circular dep)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Low-level unsafe Rust, catch_unwind pattern at ABI boundary, cross-plugin call through HostVTable
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 9
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `tests/fixtures/test_plugin/src/lib.rs` — FULL — ABI type mirrors, static vtable, polyplug_init pattern
  - `tests/integration_panic/mod.rs` lines 120-161 — How to write src/lib.rs for a plugin that uses catch_unwind
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 95-101 — `AbiError::panic_caught()` constant (same code to emit in catch_unwind handler)
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 170-182 — HostVTable definition (for fn 2 chain call)

  **Acceptance Criteria**:
  - [ ] `cargo build -p error_plugin --release` succeeds
  - [ ] ERROR_PLUGIN_SO env var set by build.rs
  - [ ] vtable has 3 functions

  **QA Scenarios**:
  ```
  Scenario: error_plugin compiles and links
    Tool: Bash
    Steps:
      1. Run: cargo build -p error_plugin --release 2>&1 | tee .sisyphus/evidence/task-7-build.txt
      2. Check exit code: echo $?
    Expected Result: exit code 0
    Evidence: .sisyphus/evidence/task-7-build.txt
  ```

  **Commit**: YES (Wave 2 commit with Tasks 3-7)
  - Message: `feat(hardening): add memory_plugin + error_plugin fixtures; tracking allocator; dispatcher; criterion`
  - Files: all new files in tasks 3-7
  - Pre-commit: `cargo test --workspace && cargo build -p memory_plugin --release && cargo build -p error_plugin --release`


- [x] 6. Write `tests/fixtures/memory_plugin/` hand-written cdylib

  **What to do**:
  - Create `tests/fixtures/memory_plugin/Cargo.toml` (follow `tests/fixtures/test_plugin/Cargo.toml` EXACTLY — NO `[workspace]` table):
    ```toml
    [package]
    name             = "memory_plugin"
    version          = "0.1.0"
    edition.workspace      = true
    license.workspace      = true
    rust-version.workspace = true
    publish          = false

    [lib]
    name       = "memory_plugin"
    crate-type = ["cdylib"]

    [lints]
    workspace = true
    ```
  - Add `"tests/fixtures/memory_plugin"` to the workspace `members` list in the root `Cargo.toml` (line 3, alongside `"tests/fixtures/test_plugin"`)
  - Create `tests/fixtures/memory_plugin/src/lib.rs` (NOTE: lib.rs is a crate root — exempt from dirname/mod.rs per AGENTS.md)
  - Copy the ABI type mirrors from `tests/fixtures/test_plugin/src/lib.rs` (StringView, AbiError, Buffer, PluginVTable, PluginDescriptor, HostVTable, PluginRegistrar, FnPtr). Do NOT add a dep on polyplug-runtime.
  - Add `Buffer` type (mirrors `crates/polyplug-runtime/src/abi/mod.rs` lines 60-66):
    ```rust
    #[repr(C)]
    #[derive(Debug)]
    pub struct Buffer { pub ptr: *mut u8, pub len: usize, pub cap: usize }
    ```
  - Implement FOUR exported functions:

    **fn 0 — `memory_fill_preallocated_buffer`**:
    ```
    // args: *const FillArgs where FillArgs = { buf: Buffer, fill_byte: u8 }
    // out: *mut u32 (bytes written)
    // Behavior: fills buf.ptr[0..buf.cap] with fill_byte, sets buf.len = buf.cap, writes buf.cap as u32 to out
    // Does NOT allocate. Host owns the buffer. Plugin only writes.
    ```

    **fn 1 — `memory_alloc_buffer_via_host`**:
    ```
    // args: *const AllocArgs where AllocArgs = { host: *const HostVTable, size: u64, fill_byte: u8 }
    // out: *mut Buffer
    // Behavior: calls host.alloc(size, 1) to allocate; fills with fill_byte; writes Buffer to out
    // Host must call host.free(buf.ptr, buf.cap, 1) after reading
    ```

    **fn 2 — `memory_echo_string_view`**:
    ```
    // args: *const StringView (input UTF-8 bytes including non-ASCII)
    // out: *mut StringView (output — same ptr/len as input, no copy; just echo)
    // Behavior: reads input StringView, writes same ptr+len to output. Validates UTF-8 first.
    // NOTE: this tests that non-ASCII bytes survive the boundary unchanged
    ```

    **fn 3 — `memory_zero_length_roundtrip`**:
    ```
    // args: *const ZeroArgs where ZeroArgs = { buf: Buffer, sv: StringView }
    // out: *mut ZeroResult where ZeroResult = { buf_len: u64, sv_len: u64 }
    // Behavior: reads zero-length Buffer and StringView, writes their .len fields to output
    // Tests that zero-length values are handled without panic or UB
    ```

  - Export `polyplug_abi_version() -> u32` and `polyplug_init(registrar) -> AbiError`
  - Static vtable with 4 functions. Contract: `memory.test@1`. `contract_id` = FNV-1a(`"memory.test@1"`) computed at compile time as a const.
  - Add workspace member: add `"tests/fixtures/memory_plugin"` to `Cargo.toml` workspace members list
  - Add build step: `tests/fixtures/memory_plugin` must be compiled. Build via `build.rs` in polyplug-runtime OR use a pre-built .so approach like test_plugin. PREFERRED: add a build.rs step that runs `cargo build --release --manifest-path tests/fixtures/memory_plugin/Cargo.toml` and sets `MEMORY_PLUGIN_SO` env var. Follow `crates/polyplug-runtime/build.rs` pattern exactly.

  **Must NOT do**:
  - Do NOT add a dep on polyplug-runtime (cdylib circular dep issue)
  - Do NOT use the Buffer protocol where the host vtable alloc pointer is null (test_plugin passes null for host in registrar — memory_plugin fn 1 needs a real HostVTable)
  - Do NOT call .unwrap() anywhere

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Writing a safe but low-level Rust cdylib with correct unsafe ABI wrappers, mirroring existing test_plugin patterns
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2
  - **Blocks**: Task 8
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `tests/fixtures/test_plugin/src/lib.rs` — FULL CONTENTS — exact pattern to mirror for ABI type mirroring, static vtable, polyplug_init, polyplug_abi_version, FnPtr wrapper
  - `crates/polyplug-runtime/build.rs` — FULL CONTENTS — exact pattern for building fixture .so and setting env vars
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 54-66 — Buffer definition with ownership doc
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 170-182 — HostVTable layout (needed for fn 1 to call host.alloc)
  - `Cargo.toml` line 3 — workspace members list format

  **Acceptance Criteria**:
  - [ ] `cargo build --manifest-path tests/fixtures/memory_plugin/Cargo.toml` succeeds
  - [ ] MEMORY_PLUGIN_SO env var is set by build.rs
  - [ ] memory_plugin exports: `polyplug_abi_version`, `polyplug_init`, and vtable has 4 functions

  **QA Scenarios**:
  ```
  Scenario: memory_plugin compiles as a cdylib
    Tool: Bash
    Steps:
      1. Run: cargo build -p memory_plugin --release 2>&1 | tee .sisyphus/evidence/task-6-build.txt
      2. Check: echo $?  (must be 0)
    Expected Result: exit code 0, .so produced
    Evidence: .sisyphus/evidence/task-6-build.txt

  Scenario: vtable has 4 functions (ABI version check)
    Tool: Bash (via cargo test)
    Steps: Verified indirectly via Task 8 stress tests loading the plugin
    Evidence: .sisyphus/evidence/task-8-memory-plugin-loaded.txt (deferred to Task 8)
  ```

  **Commit**: NO (groups with Wave 2 commit)


- [x] 5. Implement real dispatcher: wire `host_find_plugin` + `host_call_plugin` via `OnceLock<Arc<Registry>>`

  **What to do**:
  - In `crates/polyplug-runtime/src/runtime/mod.rs`:
    1. Add a module-level static: `static GLOBAL_REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();`
    2. Add a public function `pub fn set_global_registry(registry: Arc<Registry>)` that calls `GLOBAL_REGISTRY.set(registry)`. If already set, this is a no-op (or returns an error — document the choice).
    3. Add a pub(crate) function `fn global_registry() -> Option<Arc<Registry>>` that returns `GLOBAL_REGISTRY.get().cloned()`
    4. Replace `host_find_plugin` stub body:
       ```rust
       unsafe extern "C" fn host_find_plugin(contract_id: u64, min_version: u32) -> PluginHandle {
           match global_registry() {
               Some(reg) => reg.find(contract_id, min_version).unwrap_or(PluginHandle::null()),
               None => PluginHandle::null(), // graceful degradation, no panic
           }
       }
       ```
    5. Replace `host_call_plugin` stub body:
       ```rust
       unsafe extern "C" fn host_call_plugin(
           plugin: PluginHandle, fn_id: u32, args: *const (), out: *mut ()
       ) -> AbiError {
           match global_registry() {
               Some(reg) => {
                   let vtable_ptr: *const PluginVTable = match reg.resolve(plugin) {
                       Ok(p) => p,
                       Err(e) => return registry_error_to_abi_error(e),
                   };
                   // SAFETY: vtable_ptr is 'static (never-drop invariant). fn_id < function_count
                   // is validated below. args and out are caller-provided per ABI contract.
                   let vtable: &PluginVTable = unsafe { &*vtable_ptr };
                   if fn_id >= vtable.function_count {
                       return AbiError { code: ABI_FUNCTION_NOT_AVAIL, message: StringView::null() };
                   }
                   let fn_ptr: *const () = unsafe { *vtable.functions.add(fn_id as usize) };
                   let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
                       unsafe { core::mem::transmute(fn_ptr) };
                   unsafe { dispatch_fn(args, out) }
               }
               None => AbiError { code: ABI_ERROR_NOT_FOUND, message: StringView::null() },
           }
       }
       ```
    6. In `RuntimeBuilder::build()`, after creating the `Arc<Registry>`, call `set_global_registry(Arc::clone(&registry))` BEFORE leaking the HostVTable.
  - The existing `Runtime::call_plugin()` method stays as-is (host Rust API)

  **Must NOT do**:
  - Do NOT change HostVTable struct definition (frozen ABI §7)
  - Do NOT use .unwrap() on the OnceLock — use pattern matching
  - Do NOT break any existing test (existing tests use null host pointer — fine, OnceLock gracefully returns None)

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires understanding the ABI safety invariants, the Registry ownership model, and the call dispatch path. All unsafe blocks need correct SAFETY comments.
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2
  - **Blocks**: Tasks 8, 9, 10
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug-runtime/src/runtime/mod.rs` lines 172-216 — FULL existing stub implementations to replace
  - `crates/polyplug-runtime/src/runtime/mod.rs` lines 219-231 — `registry_error_to_abi_error()` helper (already exists, reuse)
  - `crates/polyplug-runtime/src/runtime/mod.rs` lines 108-164 — `Runtime::call_plugin()` — copy the dispatch pattern from here
  - `crates/polyplug-runtime/src/registry/mod.rs` lines 65-139 — Registry API: `find()`, `resolve()`, `register()`
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 6-13 — `ABI_ERROR_NOT_FOUND`, `ABI_FUNCTION_NOT_AVAIL` constants

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace` still passes (no regressions)
  - [ ] `host_find_plugin` with no runtime set returns `PluginHandle::null()` (not panic)
  - [ ] `host_call_plugin` with no runtime set returns `AbiError { code: ABI_ERROR_NOT_FOUND }`
  - [ ] Unit tests in `runtime/mod.rs`: add `dispatcher_graceful_degradation_when_no_registry()` test that calls `host_find_plugin(0, 0)` with OnceLock empty and verifies null handle returned

  **QA Scenarios**:
  ```
  Scenario: Existing tests still pass after dispatcher impl
    Tool: Bash
    Steps:
      1. Run: cargo test --workspace 2>&1 | tail -5
    Expected Result: test result: N passed, 0 failed
    Evidence: .sisyphus/evidence/task-5-no-regressions.txt

  Scenario: Graceful degradation when OnceLock not set
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug-runtime runtime::tests::dispatcher_graceful 2>&1
    Expected Result: test passed
    Evidence: .sisyphus/evidence/task-5-graceful-degradation.txt
  ```

  **Commit**: NO (groups with Wave 2 commit)


- [x] 4. Implement `TrackingAllocator` in `crates/polyplug-runtime/src/allocator/tracking/mod.rs`

  **What to do**:
  - Create directory `crates/polyplug-runtime/src/allocator/tracking/` and file `mod.rs`
  - In `crates/polyplug-runtime/src/allocator/mod.rs`, add `pub mod tracking;` at the top
  - Implement:
    ```rust
    // Exact public API the executer must produce:
    pub struct TrackingAllocator {
        alloc_count: Arc<AtomicUsize>,
        free_count: Arc<AtomicUsize>,
    }

    impl TrackingAllocator {
        pub fn new() -> TrackingAllocator { ... }

        // Returns function pointers suitable for use as HostVTable.alloc/free
        pub fn alloc_fn(&self) -> unsafe extern "C" fn(usize, usize) -> *mut u8 { ... }
        pub fn free_fn(&self) -> unsafe extern "C" fn(*mut u8, usize, usize) { ... }

        pub fn alloc_count(&self) -> usize { self.alloc_count.load(Ordering::SeqCst) }
        pub fn free_count(&self) -> usize { self.free_count.load(Ordering::SeqCst) }

        // Panics with details if alloc_count != free_count
        pub fn assert_no_leaks(&self) { ... }
    }
    ```
  - IMPLEMENTATION NOTE: `alloc_fn()` and `free_fn()` CANNOT close over self directly — extern "C" fn pointers cannot capture. Use a thread_local or a static AtomicUsize pair that the TrackingAllocator instance resets/reads. The safest pattern:
    - `TrackingAllocator` stores a unique `usize` slot_id
    - A static `[AtomicUsize; MAX_SLOTS]` stores counts per slot
    - `alloc_fn()` returns a pointer to a slot-specific extern "C" fn (or use thread_local per-test)
    - **Simpler alternative**: use `thread_local! { static TRACK_ALLOC: AtomicUsize; static TRACK_FREE: AtomicUsize; }` and have `TrackingAllocator::new()` reset them. `assert_no_leaks()` reads them. Tests are single-threaded per binary by default.
  - The simpler thread_local approach is PREFERRED for correctness. Implement that.
  - The wrapper calls `polyplug_host_alloc`/`polyplug_host_free` internally and increments counters
  - `#[cfg(test)]` on the module or at minimum doc it as test-only

  **Must NOT do**:
  - Do NOT implement `GlobalAlloc` or `std::alloc::Allocator` trait — wrong abstraction
  - Do NOT track allocations from non-ABI code paths (test setup, String allocations, etc.)
  - Do NOT use .unwrap() — use if/match for counter reads

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Requires careful unsafe code, SAFETY comments, and understanding the ABI boundary
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 3, 5, 6, 7)
  - **Blocks**: Tasks 8, 9
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug-runtime/src/allocator/mod.rs` — FULL CONTENTS — wraps System allocator. TrackingAllocator wraps these same functions
  - `crates/polyplug-runtime/src/abi/mod.rs` lines 172-182 — HostVTable.alloc/free signature: `unsafe extern "C" fn(size: usize, align: usize) -> *mut u8` and `unsafe extern "C" fn(ptr: *mut u8, size: usize, align: usize)`
  - AGENTS.md Rule 6: all unsafe blocks need // SAFETY: comment
  - AGENTS.md Rule 1: module root must be dirname/mod.rs

  **Acceptance Criteria**:
  - [ ] `tests/stress_memory/mod.rs` (Task 8) compiles and uses `TrackingAllocator::new()` without errors
  - [ ] `assert_no_leaks()` panics when alloc_count > free_count (unit test in the module)
  - [ ] `assert_no_leaks()` passes when alloc_count == free_count
  - [ ] No GlobalAlloc implementation exists in tracking/mod.rs

  **QA Scenarios**:
  ```
  Scenario: TrackingAllocator unit tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test -p polyplug-runtime allocator::tracking 2>&1 | tee .sisyphus/evidence/task-4-tracking-tests.txt
    Expected Result: test result: N passed, 0 failed
    Evidence: .sisyphus/evidence/task-4-tracking-tests.txt

  Scenario: No GlobalAlloc trait implementation
    Tool: Bash
    Steps:
      1. Run: grep -n 'GlobalAlloc\|impl.*Allocator' crates/polyplug-runtime/src/allocator/tracking/mod.rs
    Expected Result: Empty (no GlobalAlloc)
    Evidence: .sisyphus/evidence/task-4-no-global-alloc.txt
  ```

  **Commit**: NO (groups with Wave 2 commit)


- [x] 3. Add `criterion 0.8` to workspace and `[[bench]]` entry to `polyplug-runtime/Cargo.toml`

  **What to do**:
  - Add to `Cargo.toml` (workspace) under `[workspace.dependencies]`:
    ```toml
    criterion = { version = "0.8", features = ["html_reports"] }
    ```
  - Add to `crates/polyplug-runtime/Cargo.toml` under `[dev-dependencies]`:
    ```toml
    criterion = { workspace = true }
    ```
  - Add to `crates/polyplug-runtime/Cargo.toml`:
    ```toml
    [[bench]]
    name = "vtable_dispatch"
    harness = false
    ```
  - Create the directory `crates/polyplug-runtime/benches/` (empty, Task 10 fills the file)
  - Run `cargo check -p polyplug-runtime` to verify Cargo.toml parses

  **Must NOT do**:
  - Do not create a separate `crates/polyplug-bench/` crate — benches go in polyplug-runtime
  - Do not write the bench file yet (Task 10)

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Trivial Cargo.toml edits + create empty directory
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 4, 5, 6, 7)
  - **Blocks**: Task 10
  - **Blocked By**: Task 1, 2

  **References**:
  - `Cargo.toml` lines 10-29 — workspace.dependencies format
  - `crates/polyplug-runtime/Cargo.toml` lines 12-15 — [dependencies] section
  - `crates/polyplug-runtime/Cargo.toml` lines 20-46 — [[test]] entry format (bench entries follow the same pattern)

  **Acceptance Criteria**:
  - [ ] `cargo check -p polyplug-runtime` passes
  - [ ] `grep 'criterion' Cargo.toml` returns a line
  - [ ] `crates/polyplug-runtime/benches/` directory exists

  **QA Scenarios**:
  ```
  Scenario: Cargo parses cleanly after edits
    Tool: Bash
    Steps:
      1. Run: cargo check -p polyplug-runtime 2>&1 | tee .sisyphus/evidence/task-3-cargo-check.txt
      2. Check: echo $? (must be 0)
    Expected Result: Exit code 0, no errors
    Evidence: .sisyphus/evidence/task-3-cargo-check.txt
  ```

  **Commit**: NO (groups with Wave 2 commit)


- [x] 2. Write `tests/smoke/mod.rs` — smoke tests for Rust and C++ codegen round-trips

  **What to do**:
  - Create directory `tests/smoke/` and file `tests/smoke/mod.rs`
  - Add `[[test]] name = "smoke" path = "../../tests/smoke/mod.rs"` to `crates/polyplug-runtime/Cargo.toml`
  - Write two test functions:
    1. `smoke_rust_codegen_dispatch()` — runs polyplugc generate (Rust), compiles the plugin, loads it, calls add(3,5), asserts result == 8 and ABI_OK. (Pattern: mirror integration_codegen_rust/mod.rs steps 1-10 with minimal boilerplate)
    2. `smoke_cpp_codegen_dispatch()` — runs polyplugc generate (C++), compiles with g++ if available, loads pre-built .so, calls add(10,20), asserts result == 30. Skip gracefully if g++ absent.
  - File header: `//! Smoke tests — Phase 1 gate. Must pass before any hardening work begins.`
  - `#![allow(clippy::expect_used)]` at top (integration test convention)

  **Must NOT do**:
  - Do not call anything from Phase 2 or 3
  - Do not add test infrastructure beyond what's needed for the two roundtrip tests
  - Do not change integration_codegen_rust or integration_codegen_cpp

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Writing Rust integration test that shells out to cargo and libloading, following existing patterns
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (must follow Task 1)
  - **Parallel Group**: Wave 1
  - **Blocks**: Tasks 3-11
  - **Blocked By**: Task 1

  **References**:
  - `tests/integration_codegen_rust/mod.rs` — EXACT pattern to follow (lines 1-323)
  - `tests/integration_codegen_cpp/mod.rs` — EXACT pattern to follow (lines 1-299)
  - `crates/polyplug-runtime/Cargo.toml` lines 20-46 — [[test]] entry format

  **Acceptance Criteria**:
  - [ ] `cargo test --test smoke` passes, both test functions run (or gracefully skip for C++)
  - [ ] `tests/smoke/mod.rs` exists
  - [ ] `crates/polyplug-runtime/Cargo.toml` has `[[test]] name = "smoke"` entry

  **QA Scenarios**:
  ```
  Scenario: Smoke tests pass (happy path)
    Tool: Bash
    Preconditions: cargo test --workspace already passes (Task 1 done)
    Steps:
      1. Run: cargo test --test smoke 2>&1 | tee .sisyphus/evidence/task-2-smoke.txt
      2. Check: grep 'smoke_rust_codegen_dispatch ... ok' .sisyphus/evidence/task-2-smoke.txt
      3. Check: grep 'test result: .* 0 failed' .sisyphus/evidence/task-2-smoke.txt
    Expected Result: Both lines found
    Failure Indicators: 'FAILED' or 'error' in output
    Evidence: .sisyphus/evidence/task-2-smoke.txt

  Scenario: Module root uses dirname/mod.rs pattern (AGENTS.md Rule 1)
    Tool: Bash
    Preconditions: tests/smoke/ directory created
    Steps:
      1. Run: test -f tests/smoke/mod.rs && echo 'OK' || echo 'FAIL: should be mod.rs'
      2. Run: test ! -f tests/smoke.rs && echo 'OK' || echo 'FAIL: bare .rs exists'
    Expected Result: Both commands print OK
    Evidence: .sisyphus/evidence/task-2-module-structure.txt
  ```

  **Commit**: YES (groups with Task 1 if no regressions found, separate if fixes needed)
  - Message: `test(smoke): add Phase 1 smoke gate for Rust and C++ codegen`
  - Files: `tests/smoke/mod.rs`, `crates/polyplug-runtime/Cargo.toml`
  - Pre-commit: `cargo test --test smoke`



- [x] 1. Run `cargo test --workspace`, diagnose failures, fix all Epic 5 regressions

  **What to do**:
  - Run `cargo test --workspace 2>&1 | tee .sisyphus/evidence/task-1-initial-test-run.txt`
  - Read every failure. Categorize: codegen bug, runtime bug, test infrastructure issue
  - Fix failures in source files (polyplugc generators, runtime code, test fixtures)
  - Re-run until all tests pass
  - This is a GATE. Nothing else starts until `cargo test --workspace` is clean

  **Must NOT do**:
  - Do not skip flaky tests with `#[ignore]` without explicit approval
  - Do not add .unwrap() to fix panics — fix the actual bug
  - Do not modify test assertions to match wrong behavior — fix the implementation

  **Recommended Agent Profile**:
  > Debugging and fixing a potentially broken Epic 5 implementation. Needs to trace failures, understand generator output, and patch Rust/C++ codegen.
  - **Category**: `deep`
    - Reason: Requires reading generator code, tracing failures, understanding multi-file codegen bugs
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (sequential gate)
  - **Blocks**: ALL other tasks
  - **Blocked By**: None (starts immediately)

  **References**:

  **Pattern References** (existing code to follow):
  - `crates/polyplugc/src/generators/rust/mod.rs` — Rust generator to fix if broken
  - `crates/polyplugc/src/generators/cpp/mod.rs` — C++ generator to fix if broken
  - `tests/integration_codegen_rust/mod.rs` — How Rust codegen test works end-to-end
  - `tests/integration_codegen_cpp/mod.rs` — How C++ codegen test works
  - `tests/integration_panic/mod.rs` — Panic test pattern
  - `tests/fixtures/test_api.toml` — The API contract schema used by codegen tests

  **WHY**: The executer starts by reading every failure, understanding the codegen pipeline, and fixing root causes.

  **Acceptance Criteria**:
  - [ ] `cargo test --workspace 2>&1 | tail -5` shows `test result: N passed, 0 failed`
  - [ ] No test binary exits with non-zero status

  **QA Scenarios**:
  ```
  Scenario: All tests pass after fixes
    Tool: Bash
    Preconditions: Fresh build, no stale artifacts
    Steps:
      1. Run: cargo test --workspace 2>&1 | tee .sisyphus/evidence/task-1-initial-test-run.txt
      2. Check exit code: echo $? (must be 0)
      3. Run: grep 'FAILED\|error\[' .sisyphus/evidence/task-1-initial-test-run.txt
    Expected Result: grep returns empty (no FAILED, no compile errors)
    Failure Indicators: Any line containing 'FAILED' or 'error[E' in output
    Evidence: .sisyphus/evidence/task-1-all-tests-green.txt

  Scenario: No .unwrap() introduced in production code
    Tool: Bash
    Preconditions: All fixes applied
    Steps:
      1. Run: grep -rn '\.unwrap()' crates/ --include='*.rs' | grep -v '#\[cfg(test)\]' | grep -v '/tests' | grep -v '// allow'
    Expected Result: Empty output (no .unwrap() in production code)
    Failure Indicators: Any matching line
    Evidence: .sisyphus/evidence/task-1-no-unwrap.txt
  ```

  **Commit**: YES
  - Message: `test(smoke): fix Epic 5 regressions; all workspace tests green`
  - Files: `crates/polyplugc/src/generators/rust/mod.rs`, `crates/polyplugc/src/generators/cpp/mod.rs` (and any other files fixed)
  - Pre-commit: `cargo test --workspace`


---

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [x] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists. For each "Must NOT Have": search codebase for forbidden patterns. Verify no .unwrap() in new production code. Verify TrackingAllocator does NOT implement GlobalAlloc. Check evidence files exist.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [x] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy --workspace -- -D warnings`, `cargo fmt --check`, `cargo test --workspace`.
  Review all new files for: bare `filename.rs` module roots, `use` inside functions, missing `// SAFETY:` comments, .unwrap() in production code, GlobalAlloc implementation in TrackingAllocator.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | VERDICT`

- [x] F3. **Leak Verification** — `deep`
  For each stress test that calls `assert_no_leaks()`: confirm the assertion passed, confirm alloc_count == free_count. Verify the tracking allocator counted the correct operations (Buffer pre-alloc path AND plugin-alloc path). Verify AbiError.message allocation was freed.
  Output: `Leak tests [N/N pass] | Alloc paths verified [A+B] | Message free verified [YES/NO] | VERDICT`

- [x] F4. **BENCHMARKS.md Completeness** — `unspecified-high`
  Open BENCHMARKS.md. Verify: all 4 benchmarks present (noop, buffer_arg, struct_arg_and_return, cross_plugin), columns populated with real numbers (not 0 or N/A), Epic 6 Baseline column present, units are nanoseconds.
  Output: `Benchmarks [N/4] | Numbers present [YES/NO] | VERDICT`

---

## Commit Strategy

- T1-T2 (Phase 1 gate): `test(smoke): add smoke gate tests; fix Epic 5 regressions`
- T3-T7 (foundations): `feat(hardening): add tracking allocator, memory/error plugins, criterion, dispatcher`
- T8-T9 (stress tests): `test(stress): add memory and error model stress tests with leak detection`
- T10-T11 (benchmarks): `bench: add vtable dispatch criterion benchmarks; populate BENCHMARKS.md`

---

## Success Criteria

### Verification Commands
```bash
cargo test --workspace              # Expected: test result: N passed, 0 failed
cargo clippy --workspace -- -D warnings  # Expected: (no output, zero warnings)
cargo fmt --check                   # Expected: (no output)
cargo bench -p polyplug-runtime     # Expected: benchmark output printed
grep -c "^|" BENCHMARKS.md          # Expected: ≥5 (header + 4 data rows)
```

### Final Checklist
- [ ] All "Must Have" present
- [ ] All "Must NOT Have" absent
- [ ] All tests pass including smoke, stress_memory, stress_error
- [ ] All leak assertions show alloc_count == free_count
- [ ] BENCHMARKS.md has 4 rows with real numbers
- [ ] clippy: zero warnings
- [ ] fmt: clean
