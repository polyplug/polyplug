# Test Implementation Work Plan for Polyplug

## TL;DR

**Goal**: Add ~180 REAL and NEEDED tests across 8 crates to achieve comprehensive coverage of safety-critical paths, error handling, concurrency, and FFI boundaries.

**Current State**: 61 unit tests + 19 integration tests = ~80 tests. Strong happy path coverage, critical gaps in safety and error paths.

**Target State**: ~260 tests with emphasis on:
1. **Trust boundary enforcement** (Phase 1→2 transition) - 15 tests
2. **Concurrency stress and races** - 20 tests  
3. **FFI robustness against malicious guests** - 15 tests
4. **Parser/CLI error handling** - 50 tests
5. **Language binding coverage** - 25 tests
6. **Unit tests for pure functions** - 55 tests

**Execution Strategy**: 6 waves of parallel work, organized by crate and criticality.

---

## Context

### Original Request
Add REAL and NEEDED tests for the polyplug universal plugin runtime, not basic tests but comprehensive coverage of edge cases, error conditions, security boundaries, and concurrency scenarios.

### Research Findings
- 92 Rust source files analyzed
- 8 crates: polyplug (runtime), polyplugc (CLI), polyplug_codegen, polyplug_dotnet, polyplug_js, polyplug_js_deno, polyplug_lua, polyplug_python
- Current tests: 61 unit tests (polyplug) + 49 unit tests (codegen) + 19 integration tests
- Critical gaps identified by Metis review:
  - Trust boundary enforcement (TLS-based dependency declaration) completely untested
  - Quiescence race window (use-after-free vector) undertested
  - Registrar callback security (raw pointer dereference through TLS) undertested
  - Global state contamination between tests (OnceLock limitation)
  - 8 test items already covered by existing tests (duplicates)
  - 3 impractical test scenarios (u32::MAX exhaustion, 1000+ bundles)

### Key Constraints from AGENTS.md
- NO `.unwrap()` or `.expect()` in production code - all error paths must be tested
- All `unsafe` blocks need `// SAFETY:` comments - test the safety invariants
- ABI stability is frozen - test version compatibility thoroughly
- Memory crossing plugin boundaries must use host allocator - test allocation paths
- All strings at ABI boundary are UTF-8 `StringView` - test encoding edge cases

---

## Work Objectives

### Core Objective
Implement ~180 new tests across all 8 crates, focusing on safety-critical paths, error handling, concurrency stress, and FFI robustness. Ensure tests are deterministic, fast (<1s per test for unit tests), and don't require external toolchains for core runtime tests.

### Concrete Deliverables
1. **15 integration tests** for polyplug runtime (trust boundary, quiescence races, FFI robustness)
2. **50 CLI/parser tests** for polyplugc (error handling, malformed input, argument validation)
3. **25 language binding tests** for dotnet/js/lua/python (loader initialization, dispatch, error handling)
4. **55 unit tests** for pure functions (hash stability, version parsing, type resolution, error formatting)
5. **35 integration tests** for codegen output verification (cross-language ABI alignment)

### Definition of Done
- [ ] All tests pass: `cargo test --workspace 2>&1 | grep "test result: ok"`
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings 2>&1 | grep -c "^error"` returns 0
- [ ] No duplicate test coverage (verified against existing tests)
- [ ] All safety-critical paths have tests (trust boundary, quiescence, registrar callback)
- [ ] Documentation for test isolation constraints (OnceLock global state)

### Must Have
- Trust boundary transition tests (Phase 1→2 init guard)
- Arc::strong_count quiescence race test
- Registrar callback security tests
- CLI argument validation
- Parser error handling for malformed TOML
- Cross-language type mapping verification
- Thread-local error isolation tests

### Must NOT Have
- Tests requiring u32::MAX iterations (impractical)
- Tests requiring 1000+ bundle chains (tests petgraph, not polyplug)
- Tests requiring external toolchains in core suite (separate CI job only)
- Flaky concurrency tests without thread barriers
- Tests duplicating existing coverage (8 items already covered)

---

## Verification Strategy

### Test Categories
- **Unit tests** (`#[cfg(test)]` in source files): Fast, isolated, no I/O
- **Integration tests** (`tests/*.rs`): Cross-module, may use filesystem
- **Stress tests** (`tests/stress_*.rs`): Concurrent, may be slow - mark `#[ignore]`

### QA Policy
Every test includes Agent-Executed QA Scenarios:
- **Unit tests**: Run with `cargo test --lib`, verify PASS
- **Integration tests**: Run with `cargo test --test <name>`, verify PASS
- **Stress tests**: Run with `cargo test --test <name> -- --ignored`, verify PASS

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (CRITICAL Safety Tests - Start Immediately):
├── Task 1: Trust boundary transition tests (15 tests)
├── Task 2: Quiescence race window test (8 tests)
├── Task 3: Registrar callback security tests (10 tests)
└── Task 4: pack_handle/unpack_handle roundtrip unit tests (5 tests)

Wave 2 (CLI & Parser - Can Parallelize):
├── Task 5: CLI argument validation tests (20 tests)
├── Task 6: Parser error handling tests (30 tests)
└── Task 7: TOML malformed input tests (20 tests)

Wave 3 (Concurrency & FFI - Depends on Wave 1):
├── Task 8: Concurrent registry stress tests (12 tests)
├── Task 9: FFI robustness tests (15 tests)
└── Task 10: LAST_ERROR thread isolation tests (8 tests)

Wave 4 (Language Bindings - Independent):
├── Task 11: .NET loader tests (8 tests)
├── Task 12: QuickJS loader tests (8 tests)
├── Task 13: Deno loader tests (5 tests)
├── Task 14: Lua loader tests (8 tests)
└── Task 15: Python loader tests (6 tests)

Wave 5 (Codegen & Integration - Depends on Wave 2):
├── Task 16: Cross-language type mapping tests (20 tests)
├── Task 17: Generator output correctness tests (15 tests)
└── Task 18: Pack command tests (10 tests)

Wave 6 (Unit Tests & Polish - Independent):
├── Task 19: Version parsing unit tests (15 tests)
├── Task 20: Error formatting unit tests (10 tests)
├── Task 21: Hash function stability tests (8 tests)
├── Task 22: Loader/manifest unit tests (12 tests)
└── Task 23: Graph edge case unit tests (10 tests)

Wave FINAL (Verification & Documentation - After ALL):
├── Task F1: Plan compliance audit (verify all critical paths tested)
├── Task F2: Test isolation documentation (OnceLock constraints)
├── Task F3: CI configuration for stress tests
└── Task F4: Coverage report generation
```

### Dependency Matrix
- **1-4**: — — 5-7, 8-10, 11-15, 16-18, 19-23, 1
- **5-7**: — — 16-18, 2
- **8-10**: 1-4 — 11-15, 19-23, 3
- **11-15**: — — F1-F4, 4
- **16-18**: 5-7 — F1-F4, 5
- **19-23**: 8-10 — F1-F4, 6

---

## TODOs

### Wave 1: Critical Safety Tests

- [x] **Task 1: Trust Boundary Transition Tests**

  **What to do**: Implement tests for Phase 1→Phase 2 trust boundary enforcement
  
  **Must NOT do**: Test implementation details of TLS storage, only observable behavior

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Reasoning**: Requires deep understanding of thread-local storage and init guards

  **Parallelization**: Wave 1, Task 1-4
  **Blocks**: Tasks 8-10
  **Blocked By**: None

  **References**:
  - `crates/polyplug/src/loader/mod.rs:BundleInitGuard` - RAII guard (search for struct)
  - `crates/polyplug/src/loader/mod.rs:343-351` - TLS variable initialization
  - `crates/polyplug/src/runtime.rs:78-85` - INIT_BUNDLE_ID usage
  - `TRUST_MODEL.md` - Trust boundary documentation

  **Acceptance Criteria**:
  - [ ] Test: bundle_id=0 cannot escape enforcement (returns RuntimeError::UndeclaredDependency)
  - [ ] Test: TLS state cleared after init completes (guard drop fires)
  - [ ] Test: Panic during init still triggers guard drop
  - [ ] Test: Reentrant load on same thread doesn't leak bundle_id
  - [ ] Test: Lazy load during init doesn't corrupt TLS

  **QA Scenarios**:
  ```
  Scenario: bundle_id=0 bypass attempt
    Tool: cargo test --test integration_trust_boundary
    Steps:
      1. Create plugin that attempts to register with bundle_id=0
      2. Load plugin and verify registration is rejected
    Expected: RuntimeError::UndeclaredDependency error
    Evidence: .sisyphus/evidence/task-1-trust-boundary.txt
  ```

  **Commit**: YES
  - Message: `test: add trust boundary transition tests`
  - Files: `crates/polyplug/tests/integration_trust_boundary.rs`

- [x] **Task 2: Quiescence Race Window Tests**

  **What to do**: Test the Arc::strong_count check vs actual drop window

  **Must NOT do**: Use sleep-based timing for synchronization (code uses sleep internally)

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
  - **Reasoning**: Requires precise timing control and understanding of Arc internals

  **Parallelization**: Wave 1, Task 1-4
  **Blocks**: Tasks 8-10
  **Blocked By**: None

  **References**:
  - `crates/polyplug/src/reload.rs:212-226` - Quiescence loop with sleep
  - `crates/polyplug/src/reload.rs:228` - drop(old_arcs)

  **Acceptance Criteria**:
  - [ ] Test: Guard acquired, reload started, verify Arc::strong_count reaches 1 before drop
  - [ ] Test: Old vtable inaccessible after drop (new resolves get new vtable)
  - [ ] Test: No new guards can reference old Arc during quiescence window
  - [ ] Test: Stress test with rapid reload cycles (10-20 cycles, not 1000)

  **QA Scenarios**:
  ```
  Scenario: Quiescence race
    Tool: cargo test --test stress_quiescence_race -- --ignored
    Steps:
      1. Hold resolve guard on old vtable
      2. Trigger reload
      3. Verify strong_count reaches 1 before old_arcs dropped
    Expected: Clean handoff, no use-after-free
    Evidence: .sisyphus/evidence/task-2-quiescence-race.txt
  ```

  **Commit**: YES
  - Message: `test: add quiescence race window tests`
  - Files: `crates/polyplug/tests/stress_quiescence_race.rs`

- [x] **Task 3: Registrar Callback Security Tests**

  **What to do**: Test registrar_callback resilience against malformed inputs

  **Must NOT do**: Cause actual undefined behavior (use validation before raw operations)

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Reasoning**: Tests raw pointer dereference paths

  **Parallelization**: Wave 1, Task 1-4
  **Blocks**: Tasks 8-10
  **Blocked By**: None

  **References**:
  - `crates/polyplug/src/loader/mod.rs:426-461` - registrar_callback
  - `crates/polyplug/src/loader/mod.rs:400-425` - TLS registry pointer

  **Acceptance Criteria**:
  - [ ] Test: Null registry TLS pointer returns error code 1
  - [ ] Test: Vtable with function_count > actual array length detected by manifest validation
  - [ ] Test: Malformed descriptor with garbage pointer handled gracefully (validation layer)
  - [ ] Test: Contract name from descriptor vs computed hash mismatch

  **QA Scenarios**:
  ```
  Scenario: Null registry pointer
    Tool: cargo test --test integration_registrar_security
    Steps:
      1. Clear REGISTRAR_REGISTRY_PTR before callback
      2. Attempt plugin registration
    Expected: Error code 1 returned (non-zero)
    Evidence: .sisyphus/evidence/task-3-registrar-security.txt
  ```

  **Commit**: YES
  - Message: `test: add registrar callback security tests`
  - Files: `crates/polyplug/tests/integration_registrar_security.rs`

- [x] **Task 4: pack_handle/unpack_handle Unit Tests**

  **What to do**: Unit tests for FFI handle packing/unpacking

  **Must NOT do**: Test private functions directly (test through public FFI or inline)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Straightforward boundary value tests

  **Parallelization**: Wave 1, Task 1-4
  **Blocks**: Tasks 8-10
  **Blocked By**: None

  **References**:
  - `crates/polyplug/src/ffi.rs:19-36` - pack_handle/unpack_handle

  **Acceptance Criteria**:
  - [ ] Unit test: pack_handle(index=u32::MAX-1, gen=u32::MAX) roundtrips
  - [ ] Unit test: pack_handle(index=0, gen=0) roundtrips
  - [ ] Unit test: pack_handle(unpack_handle(x)) == x for boundary values
  - [ ] Unit test: Sentinel values (u64::MAX) handled correctly

  **QA Scenarios**:
  ```
  Scenario: Handle roundtrip
    Tool: cargo test --lib ffi::tests::handle_roundtrip
    Steps:
      1. Create handle with boundary values
      2. Pack to u64
      3. Unpack back to handle
    Expected: Original values restored
    Evidence: .sisyphus/evidence/task-4-handle-roundtrip.txt
  ```

  **Commit**: YES
  - Message: `test: add pack_handle/unpack_handle unit tests`
  - Files: `crates/polyplug/src/ffi.rs` (inline tests)

### Wave 2: CLI & Parser

- [x] **Task 5: CLI Argument Validation Tests**

  **What to do**: Test polyplugc CLI for all error paths

  **Must NOT do**: Test actual file system operations (use tempdir from tempfile crate)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Command-line testing is straightforward

  **Parallelization**: Wave 2, Task 5-7
  **Blocks**: Tasks 16-18
  **Blocked By**: None

  **References**:
  - `crates/polyplugc/src/main.rs` - CLI entry points
  - `crates/polyplugc/src/error.rs` - Error types

  **Acceptance Criteria**:
  - [ ] Test: Missing --api flag returns error
  - [ ] Test: Invalid language string returns error
  - [ ] Test: Language aliases work (c#, c++, py, js)
  - [ ] Test: Conflicting flags detected
  - [ ] Test: Non-existent path returns FileNotFound
  - [ ] Test: Directory instead of file returns error
  - [ ] Test: Read-only output directory returns PermissionDenied
  - [ ] Test: Parent directory creation or clear error

  **QA Scenarios**:
  ```
  Scenario: Missing required argument
    Tool: cargo test --test cli_validation
    Steps:
      1. Run polyplugc generate without --api
    Expected: Helpful error message mentioning required --api
    Evidence: .sisyphus/evidence/task-5-cli-validation.txt
  ```

  **Commit**: YES
  - Message: `test: add CLI argument validation tests`
  - Files: `crates/polyplugc/tests/cli_validation.rs`

- [x] **Task 6: Parser Error Handling Tests**

  **What to do**: Test parser.rs for all error conditions

  **Must NOT do**: Test toml crate itself (only polyplug-specific validation)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Complex validation logic needs thorough testing

  **Parallelization**: Wave 2, Task 5-7
  **Blocks**: Tasks 16-18
  **Blocked By**: None

  **References**:
  - `crates/polyplug_codegen/src/parser.rs` - Parser implementation
  - `crates/polyplug_codegen/src/ir.rs` - IR types

  **Acceptance Criteria**:
  - [ ] Test: Malformed TOML syntax errors (error kind, not line-specific)
  - [ ] Test: Missing required fields (version, name)
  - [ ] Test: Duplicate contract names detected
  - [ ] Test: Duplicate type names detected
  - [ ] Test: Invalid type references (non-existent type)
  - [ ] Test: Circular type definitions
  - [ ] Test: Enum expression validation (deep nesting, invalid ops)
  - [ ] Test: Hex/binary literal parsing
  - [ ] Test: Overflow detection (U8 with value 256)
  - [ ] Test: Version parsing edge cases

  **QA Scenarios**:
  ```
  Scenario: Enum with overflow value
    Tool: cargo test --test parser_errors
    Steps:
      1. Parse enum with repr=u8 and variant value 256
    Expected: PolyplugcError::ValidationFailed with specific message
    Evidence: .sisyphus/evidence/task-6-parser-errors.txt
  ```

  **Commit**: YES
  - Message: `test: add parser error handling tests`
  - Files: `crates/polyplug_codegen/tests/parser_errors.rs`

- [x] **Task 7: TOML Malformed Input Tests**

  **What to do**: Test TOML parsing edge cases

  **Must NOT do**: Test valid TOML (covered by existing tests)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Input validation testing

  **Parallelization**: Wave 2, Task 5-7
  **Blocks**: Tasks 16-18
  **Blocked By**: None

  **References**:
  - `crates/polyplug_codegen/src/parser.rs` - parse_api_str, parse_bundle_str

  **Acceptance Criteria**:
  - [ ] Test: Missing closing bracket in TOML
  - [ ] Test: Invalid escape sequences in strings
  - [ ] Test: Mixed table/array syntax
  - [ ] Test: Empty TOML content
  - [ ] Test: TOML with only comments
  - [ ] Test: Invalid Unicode in strings
  - [ ] Test: Extremely long lines
  - [ ] Test: Deeply nested tables

  **QA Scenarios**:
  ```
  Scenario: Malformed TOML
    Tool: cargo test --test toml_malformed
    Steps:
      1. Parse TOML with missing closing bracket
    Expected: TOML parse error
    Evidence: .sisyphus/evidence/task-7-toml-malformed.txt
  ```

  **Commit**: YES
  - Message: `test: add TOML malformed input tests`
  - Files: `crates/polyplug_codegen/tests/toml_malformed.rs`

### Wave 3: Concurrency & FFI

- [x] **Task 8: Concurrent Registry Stress Tests**

  **What to do**: Stress test registry under concurrent load

  **Must NOT do**: Use sleep-based timing (use barriers/atomics)

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
  - **Reasoning**: Requires careful thread synchronization

  **Parallelization**: Wave 3, Task 8-10
  **Blocks**: Tasks 19-23
  **Blocked By**: Tasks 1-4

  **References**:
  - `crates/polyplug/src/registry.rs` - Registry implementation
  - `crates/polyplug/src/abi.rs` - resolve_plugin

  **Acceptance Criteria**:
  - [ ] Test: 100 threads resolving same plugin simultaneously
  - [ ] Test: Concurrent registration and resolution
  - [ ] Test: Swap vtable during heavy resolution
  - [ ] Test: Concurrent bundle loading from multiple threads
  - [ ] Test: Registry capacity boundary (not u32::MAX, but high count)
  - [ ] Test: Thread-safety of find_all_by_contract

  **QA Scenarios**:
  ```
  Scenario: Thundering herd
    Tool: cargo test --test stress_concurrent_registry -- --ignored
    Steps:
      1. Spawn 100 threads
      2. All threads call resolve_plugin simultaneously
      3. Verify no deadlocks, all succeed
    Expected: All threads get valid handles
    Evidence: .sisyphus/evidence/task-8-concurrent-registry.txt
  ```

  **Commit**: YES
  - Message: `test: add concurrent registry stress tests`
  - Files: `crates/polyplug/tests/stress_concurrent_registry.rs`

- [ ] **Task 9: FFI Robustness Tests (CORRECTED)**

  **What to do**: Test host resilience against malformed ABI calls

  **Must NOT do**: Cause undefined behavior (use safe test doubles)

  **IMPORTANT NOTE**: 3 of these test cases already exist - ONLY implement the NEW ones:
  - ~~NULL StringView with non-zero length~~ - ALREADY COVERED by integration_ffi_null.rs
  - ~~Invalid UTF-8 in StringView~~ - ALREADY COVERED by integration_invalid_utf8.rs  
  - ~~StringView with embedded NULLs~~ - ALREADY COVERED by integration_stringview_nulls.rs

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Reasoning**: Tests memory safety boundaries

  **Parallelization**: Wave 3, Task 8-10
  **Blocks**: Tasks 19-23
  **Blocked By**: Tasks 1-4

  **References**:
  - `crates/polyplug/src/ffi.rs` - FFI entry points
  - `crates/polyplug/src/abi.rs` - ABI types

  **Acceptance Criteria** (ONLY these 3 - the others already exist):
  - [ ] Test: Misaligned Buffer pointer (returns non-zero error code)
  - [ ] Test: Cross-thread StringView/Buffer usage (thread-safe)
  - [ ] Test: Buffer cap smaller than len (validation error)

  **QA Scenarios**:
  ```
  Scenario: Misaligned Buffer
    Tool: cargo test --test integration_ffi_robustness
    Steps:
      1. Request allocation with alignment 16
      2. Provide pointer not 16-byte aligned
    Expected: Returns non-zero error code (not crash)
    Evidence: .sisyphus/evidence/task-9-ffi-robustness.txt
  ```

  **Commit**: YES
  - Message: `test: add FFI robustness tests for uncovered edge cases`
  - Files: `crates/polyplug/tests/integration_ffi_robustness.rs`

- [ ] **Task 10: LAST_ERROR Thread Isolation Tests**

  **What to do**: Test thread-local error handling at `crates/polyplug/src/ffi.rs:361`

  **Must NOT do**: Test implementation details (test observable behavior)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Thread-local storage testing

  **Parallelization**: Wave 3, Task 8-10
  **Blocks**: Tasks 19-23
  **Blocked By**: Tasks 1-4

  **References**:
  - `crates/polyplug/src/ffi.rs:361-380` - polyplug_last_error
  - `crates/polyplug/src/ffi.rs:11-17` - LAST_ERROR thread-local

  **Acceptance Criteria**:
  - [ ] Test: Error cleared after read (second call returns 0)
  - [ ] Test: Cross-thread isolation (error on A not visible on B)
  - [ ] Test: Error message truncation (buffer smaller than message)
  - [ ] Test: Error message null termination (not relying on it)
  - [ ] Test: Large error message handling

  **QA Scenarios**:
  ```
  Scenario: Thread isolation
    Tool: cargo test --test integration_last_error
    Steps:
      1. Thread A sets error via failed operation
      2. Thread B reads last error
    Expected: Thread B gets no error (or different error)
    Evidence: .sisyphus/evidence/task-10-last-error.txt
  ```

  **Commit**: YES
  - Message: `test: add LAST_ERROR thread isolation tests`
  - Files: `crates/polyplug/tests/integration_last_error.rs`

### Wave 4: Language Bindings

- [ ] **Task 11: .NET Loader Tests**

  **What to do**: Test polyplug_dotnet loader

  **Must NOT do**: Require full .NET runtime in basic tests (feature-gate or separate)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Requires understanding of both .NET and Rust FFI

  **Parallelization**: Wave 4, Task 11-15
  **Blocks**: Tasks F1-F4
  **Blocked By**: None

  **References**:
  - `crates/polyplug_dotnet/src/lib.rs` - DotnetLoader
  - `crates/polyplug_dotnet/src/version.rs` - TFM reading
  - `crates/polyplug_dotnet/src/context.rs` - CLR context

  **Acceptance Criteria**:
  - [ ] Test: TFM reading from assembly
  - [ ] Test: Version compatibility checking (net6.0 vs net7.0)
  - [ ] Test: Framework version mismatch detection
  - [ ] Test: Assembly loading (valid/missing/corrupted)
  - [ ] Test: CLR initialization (first load creates, subsequent reuse)
  - [ ] Test: Hostfxr location (auto-detect vs explicit)
  - [ ] Test: check_version_compatibility edge cases

  **QA Scenarios**:
  ```
  Scenario: Version compatibility
    Tool: cargo test --test dotnet_loader --features dotnet-tests
    Steps:
      1. Test net6.0 meets net6.0 requirement
      2. Test net7.0 meets net6.0 requirement
      3. Test net5.0 fails net6.0 requirement
    Expected: Appropriate success/failure
    Evidence: .sisyphus/evidence/task-11-dotnet-loader.txt
  ```

  **Commit**: YES
  - Message: `test: add .NET loader tests`
  - Files: `crates/polyplug_dotnet/tests/dotnet_loader.rs`

- [ ] **Task 12: QuickJS Loader Tests**

  **What to do**: Test polyplug_js loader

  **Must NOT do**: Test QuickJS internals (test polyplug integration only)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: JavaScript VM integration

  **Parallelization**: Wave 4, Task 11-15
  **Blocks**: Tasks F1-F4
  **Blocked By**: None

  **References**:
  - `crates/polyplug_js/src/loader.rs` - JsLoader

  **Acceptance Criteria**:
  - [ ] Test: QuickJS runtime initialization
  - [ ] Test: Bundle evaluation (valid/syntax error/runtime error)
  - [ ] Test: VTable registration (registerVtable callback)
  - [ ] Test: Trampoline dispatch
  - [ ] Test: Memory management (no JS value leaks)
  - [ ] Test: Thread safety

  **QA Scenarios**:
  ```
  Scenario: JS bundle load
    Tool: cargo test --test js_quickjs_loader
    Steps:
      1. Load valid JS bundle
      2. Verify registration callback fires
    Expected: Plugin registered successfully
    Evidence: .sisyphus/evidence/task-12-js-loader.txt
  ```

  **Commit**: YES
  - Message: `test: add QuickJS loader tests`
  - Files: `crates/polyplug_js/tests/js_quickjs_loader.rs`

- [ ] **Task 13: Deno Loader Tests**

  **What to do**: Test polyplug_js_deno loader

  **Must NOT do**: Test Deno internals

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Deno runtime integration

  **Parallelization**: Wave 4, Task 11-15
  **Blocks**: Tasks F1-F4
  **Blocked By**: None

  **References**:
  - `crates/polyplug_js_deno/src/lib.rs` - DenoLoader

  **Acceptance Criteria**:
  - [ ] Test: Deno runtime initialization
  - [ ] Test: Module loading from file
  - [ ] Test: Permission handling (--allow-* flags)
  - [ ] Test: TypeScript support

  **QA Scenarios**:
  ```
  Scenario: Deno TS plugin
    Tool: cargo test --test js_deno_loader
    Steps:
      1. Load TypeScript plugin
      2. Verify it compiles and runs
    Expected: Plugin loads and functions
    Evidence: .sisyphus/evidence/task-13-deno-loader.txt
  ```

  **Commit**: YES
  - Message: `test: add Deno loader tests`
  - Files: `crates/polyplug_js_deno/tests/js_deno_loader.rs`

- [ ] **Task 14: Lua Loader Tests**

  **What to do**: Test polyplug_lua loader

  **Must NOT do**: Test LuaJIT internals

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Lua VM integration

  **Parallelization**: Wave 4, Task 11-15
  **Blocks**: Tasks F1-F4
  **Blocked By**: None

  **References**:
  - `crates/polyplug_lua/src/loader.rs` - LuaLoader

  **Acceptance Criteria**:
  - [ ] Test: LuaJIT VM initialization
  - [ ] Test: Lua bundle loading (valid/syntax error)
  - [ ] Test: Function registry (slot assignment, cleanup)
  - [ ] Test: Trampoline dispatch (pointer as i64)
  - [ ] Test: Guest library loading (GUEST_LUA_DIR)
  - [ ] Test: Thread safety (Mutex protection)

  **QA Scenarios**:
  ```
  Scenario: Lua plugin dispatch
    Tool: cargo test --test lua_loader
    Steps:
      1. Load Lua plugin
      2. Call function through trampoline
    Expected: Function executes, returns result
    Evidence: .sisyphus/evidence/task-14-lua-loader.txt
  ```

  **Commit**: YES
  - Message: `test: add Lua loader tests`
  - Files: `crates/polyplug_lua/tests/lua_loader.rs`

- [ ] **Task 15: Python Loader Tests**

  **What to do**: Test polyplug_python loader

  **Must NOT do**: Test CPython internals

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Python C API integration

  **Parallelization**: Wave 4, Task 11-15
  **Blocks**: Tasks F1-F4
  **Blocked By**: None

  **References**:
  - `crates/polyplug_python/src/lib.rs` - PythonLoader

  **Acceptance Criteria**:
  - [ ] Test: Python interpreter initialization
  - [ ] Test: GIL handling
  - [ ] Test: Module import (valid/missing)
  - [ ] Test: Function dispatch
  - [ ] Test: Exception handling
  - [ ] Test: Return value marshalling

  **QA Scenarios**:
  ```
  Scenario: Python plugin
    Tool: cargo test --test python_loader
    Steps:
      1. Load Python plugin
      2. Call function and verify return
    Expected: Correct result returned
    Evidence: .sisyphus/evidence/task-15-python-loader.txt
  ```

  **Commit**: YES
  - Message: `test: add Python loader tests`
  - Files: `crates/polyplug_python/tests/python_loader.rs`

### Wave 5: Codegen & Integration

- [ ] **Task 16: Cross-Language Type Mapping Tests (FOCUSED)**

  **What to do**: Verify type mappings for UNCOVERED edge cases (NOT general verification)

  **Must NOT do**: Test all type mappings (already covered in integration_codegen_*.rs)

  **IMPORTANT**: Existing `integration_codegen_*.rs` already covers basic type mapping. ONLY test:
  - BigInt handling in JS for U64/I64 (NOT covered)
  - Alignment requirements in C++ (NOT covered)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Cross-language ABI edge cases

  **Parallelization**: Wave 5, Task 16-18
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 5-7

  **References**:
  - `crates/polyplug_codegen/src/generators/*.rs` - All generators
  - `crates/polyplug_codegen/tests/integration_codegen_*.rs` - Existing tests

  **Acceptance Criteria**:
  - [ ] Test: U64/I64 maps to BigInt in JavaScript (Deno/QuickJS)
  - [ ] Test: Alignment specifiers in C++ for SIMD types
  - [ ] Test: C# StructLayout attributes for explicit alignment

  **QA Scenarios**:
  ```
  Scenario: JavaScript BigInt
    Tool: cargo test --test type_mapping_edge_cases
    Steps:
      1. Generate JS code for contract with u64 parameter
      2. Verify output contains BigInt usage
    Expected: BigInt present in generated code
    Evidence: .sisyphus/evidence/task-16-type-mapping.txt
  ```

  **Commit**: YES
  - Message: `test: add cross-language type mapping edge case tests`
  - Files: `crates/polyplug_codegen/tests/type_mapping_edge_cases.rs`

- [ ] **Task 17: Generator Output Correctness Tests (FOCUSED)**

  **What to do**: Verify specific UNCOVERED correctness aspects

  **Must NOT do**: Test general output structure (already in integration_codegen_*.rs)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Reasoning**: Code generation verification

  **Parallelization**: Wave 5, Task 16-18
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 5-7

  **References**:
  - `crates/polyplug_codegen/src/generators/*.rs` - All generators

  **Acceptance Criteria**:
  - [ ] Test: VTable slot indices are sequential and correct
  - [ ] Test: Function signatures exactly match contract (types, order)
  - [ ] Test: Missing function detection (contract has fn not in vtable)

  **QA Scenarios**:
  ```
  Scenario: Slot indices
    Tool: cargo test --test generator_correctness
    Steps:
      1. Generate vtable for contract with 5 functions
      2. Verify slot indices are 0,1,2,3,4
    Expected: Sequential indices
    Evidence: .sisyphus/evidence/task-17-generator-correctness.txt
  ```

  **Commit**: YES
  - Message: `test: add generator correctness edge case tests`
  - Files: `crates/polyplug_codegen/tests/generator_correctness.rs`

- [ ] **Task 18: Pack Command Tests**

  **What to do**: Test pack command scaffold generation

  **Must NOT do**: Test actual build (test output content only)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: File generation testing

  **Parallelization**: Wave 5, Task 16-18
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 5-7

  **References**:
  - `crates/polyplug_codegen/src/pack.rs` - Pack implementation

  **Acceptance Criteria**:
  - [ ] Test: Cargo.toml generation (valid TOML, correct deps)
  - [ ] Test: CMakeLists.txt generation (valid CMake)
  - [ ] Test: package.json generation (valid JSON)
  - [ ] Test: Naming conversions (my-bundle -> MyBundle/MyBundle)
  - [ ] Test: Missing metadata handling (defaults)

  **QA Scenarios**:
  ```
  Scenario: Pack Rust scaffold
    Tool: cargo test --test pack_command
    Steps:
      1. Run pack command for Rust
      2. Verify Cargo.toml is valid TOML
    Expected: TOML parses, dependencies listed
    Evidence: .sisyphus/evidence/task-18-pack-command.txt
  ```

  **Commit**: YES
  - Message: `test: add pack command tests`
  - Files: `crates/polyplugc/tests/pack_command.rs`

### Wave 6: Unit Tests

- [ ] **Task 19: Version Parsing Unit Tests (FOCUSED)**

  **What to do**: Test UNCOVERED edge cases (basic parsing covered in integration_version.rs)

  **Must NOT do**: Test basic version parsing (already covered)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Input validation

  **Parallelization**: Wave 6, Task 19-23
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 8-10

  **References**:
  - `crates/polyplug/src/version.rs` - Version implementation

  **Acceptance Criteria**:
  - [ ] Test: "1.2.3.4" overflow handling
  - [ ] Test: Pre-release versions ("1.0.0-alpha", "1.0.0-rc.1")
  - [ ] Test: Wildcard requirements ("^1.2.0", "~1.2.0", ">=1.0")

  **QA Scenarios**:
  ```
  Scenario: Version parse edge cases
    Tool: cargo test --lib version::tests::parse_edge_cases
    Steps:
      1. Parse "1.2.3.4" (should error or truncate)
      2. Parse "1.0.0-alpha"
      3. Match "^1.2.0" against "1.3.0"
    Expected: Correct parsing or errors
    Evidence: .sisyphus/evidence/task-19-version-parsing.txt
  ```

  **Commit**: YES
  - Message: `test: add version parsing edge case unit tests`
  - Files: `crates/polyplug/src/version.rs` (inline tests)

- [ ] **Task 20: Error Formatting Unit Tests**

  **What to do**: Unit tests for error Display implementations

  **Must NOT do**: Test exact message strings (brittle)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: User-facing messages

  **Parallelization**: Wave 6, Task 19-23
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 8-10

  **References**:
  - `crates/polyplug/src/error.rs` - Error types
  - `crates/polyplug_codegen/src/error.rs` - Codegen errors

  **Acceptance Criteria**:
  - [ ] Test: PolyplugError Display contains expected context (IDs, names)
  - [ ] Test: LoaderError Display contains path/context
  - [ ] Test: RegistryError Display contains contract/bundle info
  - [ ] Test: Key substrings present (not exact match)

  **QA Scenarios**:
  ```
  Scenario: Error display
    Tool: cargo test --lib error::tests::display_formatting
    Steps:
      1. Create each error variant
      2. Format to string
      3. Check for expected substrings
    Expected: Contains expected context
    Evidence: .sisyphus/evidence/task-20-error-formatting.txt
  ```

  **Commit**: YES
  - Message: `test: add error formatting unit tests`
  - Files: `crates/polyplug/src/error.rs`, `crates/polyplug_codegen/src/error.rs`

- [ ] **Task 21: Hash Function Stability Tests (CORRECTED)**

  **What to do**: Unit tests for FNV-1a hash stability

  **Must NOT do**: Test "hash collision resistance" (theoretically impossible)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: ABI stability requirement

  **Parallelization**: Wave 6, Task 19-23
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 8-10

  **References**:
  - `crates/polyplug/src/abi.rs` - contract_id, bundle_id
  - `crates/polyplug_codegen/src/ir.rs` - compute_contract_id, compute_bundle_id

  **Acceptance Criteria**:
  - [ ] Test: contract_id produces same output for same input
  - [ ] Test: bundle_id produces same output for same input
  - [ ] Test: Codegen hashes match runtime hashes
  - [ ] Test: Known hash values (golden tests)
  - ~~Test: Hash collision resistance~~ REMOVED (untestable)

  **QA Scenarios**:
  ```
  Scenario: Hash stability
    Tool: cargo test --lib abi::tests::hash_stability
    Steps:
      1. Compute contract_id for "test_contract" v1
      2. Verify against known value
    Expected: Matches expected hash
    Evidence: .sisyphus/evidence/task-21-hash-stability.txt
  ```

  **Commit**: YES
  - Message: `test: add hash function stability tests`
  - Files: `crates/polyplug/src/abi.rs`, `crates/polyplug_codegen/src/ir.rs`

- [ ] **Task 22: Loader/Manifest Unit Tests**

  **What to do**: Unit tests for loader module

  **Must NOT do**: Test file I/O in unit tests (integration tests for that)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Validation logic

  **Parallelization**: Wave 6, Task 19-23
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 8-10

  **References**:
  - `crates/polyplug/src/loader/manifest.rs` - Manifest validation
  - `crates/polyplug/src/loader/scanner.rs` - Directory scanning

  **Acceptance Criteria**:
  - [ ] Test: ManifestData::validate_file edge cases
  - [ ] Test: RawManifestDependency::resolve with various kinds
  - [ ] Test: Path normalization across OSes
  - [ ] Test: Scanner permission error handling
  - [ ] Test: Scanner does NOT follow symlinks (security)

  **QA Scenarios**:
  ```
  Scenario: Manifest validation
    Tool: cargo test --lib loader::manifest::tests::validation
    Steps:
      1. Create manifest with edge case paths
      2. Validate
    Expected: Appropriate success/error
    Evidence: .sisyphus/evidence/task-22-loader-manifest.txt
  ```

  **Commit**: YES
  - Message: `test: add loader/manifest unit tests`
  - Files: `crates/polyplug/src/loader/manifest.rs`, `crates/polyplug/src/loader/scanner.rs`

- [ ] **Task 23: Graph Edge Case Unit Tests**

  **What to do**: Unit tests for graph edge cases

  **Must NOT do**: Test basic toposort (covered)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Graph algorithm edge cases

  **Parallelization**: Wave 6, Task 19-23
  **Blocks**: Tasks F1-F4
  **Blocked By**: Tasks 8-10

  **References**:
  - `crates/polyplug/src/graph.rs` - Graph implementation

  **Acceptance Criteria**:
  - [ ] Test: Diamond dependency detection (A->B, A->C, B->D, C->D)
  - [ ] Test: Self-dependency detection
  - [ ] Test: Multi-bundle cycle error message (A->B->C->A)
  - [ ] Test: Version-aware graph resolution
  - [ ] Test: Disconnected graph handling
  - ~~Test: Deep chain 20 bundles~~ CHECK if already covered

  **QA Scenarios**:
  ```
  Scenario: Diamond dependency
    Tool: cargo test --lib graph::tests::diamond_dependency
    Steps:
      1. Create A->B, A->C, B->D, C->D
      2. Toposort
    Expected: D before B and C, valid order
    Evidence: .sisyphus/evidence/task-23-graph-edge-cases.txt
  ```

  **Commit**: YES
  - Message: `test: add graph edge case unit tests`
  - Files: `crates/polyplug/src/graph.rs`

### Wave FINAL: Verification & Documentation

- [ ] **Task F1: Plan Compliance Audit**

  **What to do**: Verify all critical paths from plan are tested

  **Recommended Agent Profile**:
  - **Category**: `oracle`
  - **Reasoning**: Comprehensive verification

  **Parallelization**: Wave FINAL, Tasks F1-F4
  **Blocks**: None
  **Blocked By**: All tasks 1-23

  **Acceptance Criteria**:
  - [ ] Verify: Trust boundary tests exist and pass
  - [ ] Verify: Quiescence race tests exist and pass
  - [ ] Verify: Registrar callback security tests exist and pass
  - [ ] Verify: FFI robustness tests exist and pass (only uncovered cases)
  - [ ] Verify: CLI/parser tests exist and pass
  - [ ] Verify: No duplicate test coverage
  - [ ] Verify: All tests are deterministic
  - [ ] Verify: Stress tests marked #[ignore]

  **QA Scenarios**:
  ```
  Scenario: Compliance check
    Tool: Manual review + cargo test --workspace
    Steps:
      1. Review test files against plan
      2. Run all tests
    Expected: All critical paths covered, all tests pass
    Evidence: .sisyphus/evidence/task-F1-compliance.txt
  ```

  **Commit**: NO

- [ ] **Task F2: Test Isolation Documentation**

  **What to do**: Document OnceLock global state constraints

  **Recommended Agent Profile**:
  - **Category**: `writing`
  - **Reasoning**: Documentation

  **Parallelization**: Wave FINAL, Tasks F1-F4
  **Blocks**: None
  **Blocked By**: All tasks 1-23

  **Acceptance Criteria**:
  - [ ] Document: OnceLock global state behavior (set-once-per-process)
  - [ ] Document: Test isolation requirements (separate binaries for fresh Runtime)
  - [ ] Document: Runtime::builder() limitation (OnceLock ignores subsequent builds)
  - [ ] Document: How to write isolated tests (use FFI facade or separate test binaries)

  **QA Scenarios**:
  ```
  Scenario: Documentation review
    Tool: Manual review
    Steps:
      1. Read documentation
      2. Verify clarity
    Expected: Clear guidance for test authors
    Evidence: .sisyphus/evidence/task-F2-documentation.txt
  ```

  **Commit**: YES
  - Message: `docs: add test isolation guidelines`
  - Files: `crates/polyplug/TESTING.md`

- [ ] **Task F3: CI Configuration for Stress Tests**

  **What to do**: Configure CI to run stress tests separately

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: CI configuration

  **Parallelization**: Wave FINAL, Tasks F1-F4
  **Blocks**: None
  **Blocked By**: All tasks 1-23

  **Acceptance Criteria**:
  - [ ] Configure: Fast tests on PR (< 30 seconds)
  - [ ] Configure: Stress tests on merge to main
  - [ ] Configure: Coverage reports generated
  - [ ] Configure: External toolchain tests as separate job

  **QA Scenarios**:
  ```
  Scenario: CI validation
    Tool: GitHub Actions
    Steps:
      1. Push PR
      2. Verify fast tests run
    Expected: CI passes
    Evidence: .sisyphus/evidence/task-F3-ci-config.txt
  ```

  **Commit**: YES
  - Message: `ci: configure stress test jobs`
  - Files: `.github/workflows/test.yml`

- [ ] **Task F4: Coverage Report Generation**

  **What to do**: Generate and analyze coverage report

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Reasoning**: Metrics

  **Parallelization**: Wave FINAL, Tasks F1-F4
  **Blocks**: None
  **Blocked By**: All tasks 1-23

  **Acceptance Criteria**:
  - [ ] Generate: Coverage report (tarpaulin or llvm-cov)
  - [ ] Analyze: Coverage increase from ~80 to ~260 tests
  - [ ] Document: Coverage gaps (if any)
  - [ ] Verify: Critical paths have high coverage

  **QA Scenarios**:
  ```
  Scenario: Coverage check
    Tool: cargo tarpaulin or llvm-cov
    Steps:
      1. Run coverage
      2. Generate report
    Expected: Report shows increased coverage
    Evidence: .sisyphus/evidence/task-F4-coverage.txt
  ```

  **Commit**: NO

---

## Commit Strategy

- **Wave 1**: `test: add trust boundary and safety tests`
- **Wave 2**: `test: add CLI and parser error handling tests`
- **Wave 3**: `test: add concurrency and FFI robustness tests`
- **Wave 4**: `test: add language binding loader tests`
- **Wave 5**: `test: add codegen and pack command tests`
- **Wave 6**: `test: add unit tests for core modules`
- **Wave FINAL**: `docs: add testing guidelines and CI configuration`

---

## Success Criteria

### Verification Commands
```bash
# All tests pass
cargo test --workspace 2>&1 | grep "test result: ok"
# Expected: test result: ok for all crates

# No clippy warnings
cargo clippy --workspace -- -D warnings 2>&1 | grep -c "^error"
# Expected: 0

# Fast test suite (< 30 seconds)
time cargo test --workspace --lib
# Expected: < 30s

# Stress tests available (marked #[ignore])
grep -r "#\[ignore" crates/*/tests/*.rs | wc -l
# Expected: >= 10

# Coverage report shows increase
cargo tarpaulin --workspace --out Stdout 2>&1 | grep "Coverage:"
# Expected: Significant increase from baseline
```

### Final Checklist
- [ ] All 23 tasks completed
- [ ] All F1-F4 tasks completed
- [ ] No duplicate test coverage (8 items from original plan removed)
- [ ] Trust boundary, quiescence race, registrar security all tested
- [ ] CLI/parser error handling comprehensive
- [ ] Language bindings have basic coverage
- [ ] Unit tests for pure functions
- [ ] Documentation for test isolation
- [ ] CI configured for fast/stress test separation
- [ ] Coverage report generated

---

## CORRECTIONS APPLIED (from Self-Review and Momus Review)

1. **Task 2**: Adjusted criteria to match actual code behavior (sleep exists in code, test verifies strong_count reaches 1)
2. **Task 9**: Removed 3 duplicate test cases (NULL StringView, invalid UTF-8, embedded NULLs already covered)
3. **Task 16-17**: Narrowed scope to specific gaps (BigInt, alignment) instead of general verification
4. **Task 19**: Removed basic version parsing (covered), kept edge cases only
5. **Task 21**: Removed "hash collision resistance" as untestable
6. **Task 22**: Changed symlink test to verify symlinks are NOT followed (security)
7. **All file references**: Changed from `src/...` to `crates/polyplug/src/...`
8. **All line numbers**: Updated to match actual code (verified against codebase)
9. **Error types**: Fixed references to use actual error variants (RuntimeError::UndeclaredDependency, not AccessDenied)
