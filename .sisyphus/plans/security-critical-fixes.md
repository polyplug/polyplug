# Security Fixes: 12 CRITICAL, HIGH & ARCH Issues — Work Plan

## TL;DR

> **Objective:** Fix 5 CRITICAL + 5 HIGH + 2 ARCH confirmed security/architecture issues in polyplug
> 
> **Issues:** CRIT-001, CRIT-002, CRIT-004, CRIT-005, CRIT-006, HIGH-005, HIGH-006, HIGH-008, HIGH-009, HIGH-010, PY-ARCH-001, ARCH-001
> 
> **Total Tasks:** 24 (14 implementation + 4 verification + 6 architecture sub-tasks)
> 
> **Estimated Effort:** 7-8 weeks
> 
> **Risk Level:** HIGH (ABI changes, architectural changes, cross-language impact)

---

## Context

### Validated Issues Summary

#### CRITICAL Issues (5)

| Issue | Location | Problem | Severity |
|-------|----------|---------|----------|
| **CRIT-001** | `crates/polyplug_js/src/loader.rs` | Thread-local `REGISTRATION_DATA` violates AGENTS.md Rule 12 | CRITICAL |
| **CRIT-002** | `sdks/js/guest/polyplug_guest.js` | `toString()` returns empty strings (placeholder code) | CRITICAL |
| **CRIT-004** | `polyplug_codegen/src/generators/{python,lua,csharp}.rs` | Wrong ABI offsets (12,16 instead of 20,32) | CRITICAL |
| **CRIT-005** | `polyplug_codegen/src/generators/{csharp,cpp,js_quickjs}.rs` | Missing bounds checks on fn_id | CRITICAL |
| **CRIT-006** | `polyplug_codegen/src/ir.rs` | No validation before version encoding | MEDIUM |

#### HIGH Issues (5 confirmed)

| Issue | Location | Problem | Severity |
|-------|----------|---------|----------|
| **HIGH-005** | `sdks/js/guest/polyplug_guest.js:218-224` | Byte-by-byte memory access (O(n) FFI calls) | HIGH |
| **HIGH-006** | All generators | Error messages discarded: `message: StringView::null()` | HIGH |
| **HIGH-008** | `crates/polyplug_js/src/loader.rs:431,442` | Panic on array creation failure | HIGH |
| **HIGH-009** | `polyplug_codegen/src/generators/rust.rs:1206` | Transmute without SAFETY comments | HIGH |
| **HIGH-010** | `reload.rs`, `registry.rs`, `runtime.rs` | PoisonError recovery masks panics (30 instances) | HIGH |

#### Architecture Issues (2 confirmed)

| Issue | Location | Problem | Severity |
|-------|----------|---------|----------|
| **PY-ARCH-001** | `sdks/python/host/polyplug/helpers.py` | Python SDK package organization - duplicates ABI | MEDIUM |
| **ARCH-001** | All languages | Cross-language helper duplication | MEDIUM |

#### FALSE_POSITIVE Issues (not in plan)

| Issue | Verdict |
|-------|---------|
| CRIT-003 | FALSE_POSITIVE - SAFETY comments already exist |
| CRIT-007 | FALSE_POSITIVE - TOCTOU gap handled correctly |
| HIGH-001 | FALSE_POSITIVE - C++ global operator replacement is correct |
| HIGH-002 | FALSE_POSITIVE - C# GCHandle freed correctly |
| HIGH-003 | FALSE_POSITIVE - Mutable pointer pattern is safe FFI |
| HIGH-004 | FALSE_POSITIVE - Generators have null checks |
| HIGH-007 | FALSE_POSITIVE - OnceLock IS thread-safe |

---

## Work Objectives

### Core Objective
Fix all 5 CRITICAL + 5 HIGH + 2 ARCH issues to make polyplug production-ready for v1.0.

### Concrete Deliverables
- [ ] CRIT-006: Version overflow validation in `Version::parse()`
- [ ] CRIT-004: Python/Lua/C# generators use ABI types (not hardcoded offsets)
- [ ] CRIT-005: Bounds checks added to all generators
- [ ] CRIT-002: Working `StringView.toString()` in JS SDK with FFI memory reading
- [ ] CRIT-001: Thread-local replaced with context-embedded data in JS loader
- [ ] HIGH-005: JS bulk memory read implementation
- [ ] HIGH-006: Error message preservation across ABI
- [ ] HIGH-008: JS loader panic replaced with exceptions
- [ ] HIGH-009: Transmute SAFETY comments added
- [ ] HIGH-010: PoisonError recovery logging implemented
- [ ] PY-ARCH-001: Python SDK restructured (remove host/helpers.py duplication)
- [ ] ARCH-001: Cross-language helper duplication fixed (C++, Python, JS, C#)

### Definition of Done
- All fixes pass `cargo test` and `cargo clippy`
- Each fix has dedicated security test that would fail pre-fix
- Cross-language integration tests pass (Rust, Python, Lua, C#, C++, JS)
- ABI layout tests in `polyplug_abi` pass
- No new clippy warnings or unsafe blocks without SAFETY comments

### Must Have
- [ ] Each fix is a **separate PR** (no mixing)
- [ ] Each PR includes **regression test**
- [ ] **ABI compatibility** maintained (no breaking changes to frozen ABI)
- [ ] **AGENTS.md compliance** verified for all changes

### Must NOT Have (Guardrails)
- [ ] No refactoring or style changes mixed with security fixes
- [ ] No new features added during security fixes
- [ ] No breaking changes to frozen ABI structs
- [ ] No changes without explicit SAFETY comments for unsafe code

---

## Verification Strategy

### Test Infrastructure Assessment
**Infrastructure exists:** YES (cargo test, integration tests)
**Framework:** Built-in Rust test framework
**Test Strategy:** TDD for each fix + integration tests

### Agent-Executed QA (MANDATORY for each task)
Each task must include:
- Unit test that fails before fix, passes after
- Integration test across affected languages
- Evidence captured in `.sisyphus/evidence/`

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1: Foundation (Independent - can all start immediately)
├── Task 1: CRIT-006 - Version overflow validation [quick]
├── Task 2: CRIT-002 - JS SDK toString implementation [unspecified-high]
├── Task 3: Research - JS loader thread-local alternatives [unspecified-high]
├── Task 4: HIGH-009 - Transmute SAFETY comments [quick]
└── Task 5: HIGH-010 - PoisonError recovery logging [unspecified-high]

Wave 2: Generator Fixes (After Wave 1 - CRIT-006 unblocks)
├── Task 6: CRIT-004 - Python generator uses ABI types [unspecified-high]
├── Task 7: CRIT-004 - Lua generator uses ABI types [unspecified-high]
├── Task 8: CRIT-004 - C# generator uses ABI types [unspecified-high]
└── Task 9: CRIT-005 - Bounds checks in all generators [quick]

Wave 3: JS & Error Handling (After Wave 2)
├── Task 10: HIGH-005 - JS bulk memory read [unspecified-high]
├── Task 11: HIGH-008 - JS loader panic fix [quick]
└── Task 12: HIGH-006 - Error message preservation [unspecified-high]

Wave 4: Architecture Fixes (Can run parallel with Waves 2-3)
├── Task 13: PY-ARCH-001 - Python SDK restructure [unspecified-high]
├── Task 14: ARCH-001 - C++ helper deduplication [quick]
├── Task 15: ARCH-001 - Python helper deduplication [quick]
├── Task 16: ARCH-001 - JavaScript helper move [quick]
└── Task 17: ARCH-001 - C# helper unification [quick]

Wave 5: Major Architectural (After Waves 1-4)
├── Task 18: CRIT-001 - Replace thread-local in JS loader [deep]
└── Task 19: Integration testing across all languages [unspecified-high]

Wave FINAL: Verification (After ALL tasks)
├── Task 20: Security audit - all issues fixed [oracle]
├── Task 21: ABI compatibility verification [deep]
├── Task 22: Cross-language integration test [unspecified-high]
└── Task 23: AGENTS.md compliance check [quick]
```

### Critical Path
```
Task 1 (CRIT-006) → Task 6/7/8 (CRIT-004) → Task 9 (CRIT-005) → Task 10 (HIGH-005) → Task 18 (CRIT-001) → Task 19 → Task 20-23 → user okay
```

### Dependency Matrix

| Task | Dependencies | Blocks |
|------|--------------|--------|
| 1 | None | 6, 7, 8, 9 |
| 2 | None | 19 |
| 3 | None | 18 |
| 4 | None | None |
| 5 | None | None |
| 6 | 1 | 9 |
| 7 | 1 | 9 |
| 8 | 1 | 9 |
| 9 | 6, 7, 8 | 10, 11, 12 |
| 10 | 9 | 18 |
| 11 | 9 | 18 |
| 12 | 9 | 19 |
| 13 | None | 15 |
| 14 | None | None |
| 15 | 13 | None |
| 16 | None | None |
| 17 | None | None |
| 18 | 3, 10, 11 | 19 |
| 19 | 2, 12, 18 | 20-23 |
| 20 | 19 | 21 |
| 21 | 20 | 22 |
| 22 | 21 | 23 |
| 23 | 22 | None |

---

## TODOs

### Wave 1: Foundation (Tasks 1-5)

- [x] 1. CRIT-006: Add version overflow validation in `Version::parse()`

  **What to do:**
  - Add validation in `polyplug_codegen/src/ir.rs` before encoding
  - Check `minor > 65535 || patch > 65535`
  - Return error if overflow would occur
  
  **Must NOT do:**
  - Don't change the encoding formula
  - Don't modify `minor_patch_encoded()` signature
  
  **Acceptance Criteria:**
  - [ ] Test: `Version::parse("1.65536.0")` returns error
  - [ ] Test: `Version::parse("1.0.65536")` returns error
  - [ ] Test: `Version::parse("1.65535.65535")` succeeds
  
  **Commit:** YES
  - Message: `fix(codegen): add version overflow validation (CRIT-006)`

- [x] 2. CRIT-002: Implement `StringView.toString()` in JS SDK

  **What to do:**
  - Replace placeholder in `sdks/js/guest/polyplug_guest.js:169-174`
  - Implement FFI memory reading
  
  **Acceptance Criteria:**
  - [ ] `StringViewHelper.toString({ptr: valid_ptr, len: 5})` returns correct string
  - [ ] Integration test: round-trip with QuickJS loader
  
  **Commit:** YES
  - Message: `fix(js-sdk): implement StringView.toString() (CRIT-002)`

- [x] 3. Research: Design thread-local replacement for JS loader

  **What to do:**
  - Analyze current thread-local usage in `crates/polyplug_js/src/loader.rs`
  - Design alternative: context-embedded data in QuickJS Context
  
  **Acceptance Criteria:**
  - [ ] Document current thread-local usage pattern
  - [ ] Propose 2-3 alternative designs
  - [ ] Recommend best approach with rationale
  
  **Commit:** NO

- [x] 4. HIGH-009: Add SAFETY comment to transmute in rust.rs

  **What to do:**
  - Add SAFETY comment before line 1206 in `crates/polyplug_codegen/src/generators/rust.rs`
  
  **Acceptance Criteria:**
  - [ ] SAFETY comment added explaining transmute soundness
  
  **Commit:** YES
  - Message: `docs(codegen): add SAFETY comment to transmute (HIGH-009)`

- [x] 5. HIGH-010: Fix PoisonError recovery (30 instances)

  **What to do:**
  - Replace `.lock().unwrap_or_else(|e| e.into_inner())` pattern
  - Log error before recovery
  
  **Acceptance Criteria:**
  - [ ] All 30 instances updated with logging
  - [ ] No silent PoisonError recovery
  
  **Commit:** YES
  - Message: `fix(core): log PoisonError recovery instead of silent masking (HIGH-010)`

### Wave 2: Generator Fixes (Tasks 6-9)

- [x] 6. CRIT-004: Python generator uses ABI types

  **What to do:**
  - Modify `polyplug_codegen/src/generators/python.rs`
  - Replace hardcoded offsets (12, 16) with ABI type field access
  
  **Acceptance Criteria:**
  - [ ] Generated code uses `PluginInterface.from_address(vtable_ptr)`
  - [ ] No hardcoded offset literals in generated callers.py
  
  **Commit:** YES
  - Message: `fix(codegen): Python generator uses ABI types (CRIT-004)`

- [x] 7. CRIT-004: Lua generator uses ABI types

  **What to do:**
  - Modify `polyplug_codegen/src/generators/lua.rs`
  - Replace hardcoded offsets with ABI struct field access
  
  **Acceptance Criteria:**
  - [ ] Generated code uses FFI struct for PluginInterface
  - [ ] No offset arithmetic in generated callers.lua
  
  **Commit:** YES
  - Message: `fix(codegen): Lua generator uses ABI types (CRIT-004)`

- [x] 8. CRIT-004: C# generator uses ABI types

  **What to do:**
  - Modify `polyplug_codegen/src/generators/csharp.rs`
  - Replace hardcoded offset (32) with struct field access
  
  **Acceptance Criteria:**
  - [ ] Generated code uses `*(PluginInterface*)vtablePtr`
  - [ ] Uses proper struct fields, not pointer arithmetic
  
  **Commit:** YES
  - Message: `fix(codegen): C# generator uses ABI types (CRIT-004)`

- [x] 9. CRIT-005: Add bounds checks to all generators

  **What to do:**
  - Add bounds check before array access in C#, JS QuickJS, C++ (void returns)
  - Pattern: `if (fn_id >= function_count) return ABI_FUNCTION_NOT_AVAIL`
  
  **Acceptance Criteria:**
  - [ ] C# generator checks bounds before array access
  - [ ] JS QuickJS generator checks bounds
  - [ ] C++ generator checks bounds for void-return functions
  
  **Commit:** YES
  - Message: `fix(codegen): add bounds checks to C#, JS, C++ generators (CRIT-005)`

### Wave 3: JS & Error Handling (Tasks 10-12)

- [x] 10. HIGH-005: Add bulk memory read to QuickJS loader

  **What to do:**
  - Add `readMemory(ptr, len)` function to QuickJS loader
  - Update `sdks/js/guest/polyplug_guest.js:218-224` to use bulk read
  
  **Acceptance Criteria:**
  - [ ] `readMemory(ptr, len)` function exists
  - [ ] `readBytes()` uses bulk read (single FFI call)
  - [ ] Performance: 1KB read takes <10ms
  
  **Commit:** YES
  - Message: `perf(js): add bulk memory read to QuickJS loader (HIGH-005)`

- [x] 11. HIGH-008: Replace panic with JS exception

  **What to do:**
  - Change `crates/polyplug_js/src/loader.rs:431,442` to throw JS exceptions
  - Use `Exception::throw_internal()` instead of `panic!()`
  
  **Acceptance Criteria:**
  - [ ] No `panic!()` calls in JS loader
  - [ ] Array creation failure throws JS exception
  
  **Commit:** YES
  - Message: `fix(js-loader): replace panic with JS exceptions (HIGH-008)`

- [x] 12. HIGH-006: Preserve error messages across ABI

  **What to do:**
  - Update all generators to allocate error messages via `host_alloc`
  - Update error conversion from `PluginError` to `AbiError`
  
  **Acceptance Criteria:**
  - [ ] All generators preserve `PluginError.message` in `AbiError`
  - [ ] Host frees message after reading
  
  **Commit:** YES
  - Message: `fix(abi): preserve error messages across ABI boundary (HIGH-006)`

### Wave 4: Architecture Fixes (Tasks 13-17)

- [x] 13. PY-ARCH-001: Python SDK restructure

  **What to do:**
  - Audit all imports from `host.polyplug.helpers`
  - Move `_PluginInterface`, `_AbiError` to `polyplug_abi`
  - Remove duplicate `to_str()`, `to_string()` from host/helpers.py
  
  **Acceptance Criteria:**
  - [ ] No duplicate functions between host and ABI
  - [ ] All callers import from `polyplug_abi`
  
  **Commit:** YES
  - Message: `refactor(python): remove host/helpers.py duplication (PY-ARCH-001)`

- [x] 14. ARCH-001: C++ helper deduplication

  **What to do:**
  - Remove `to_string()` from host/guest helpers.hpp
  - Use ABI version from `abi/polyplug/helpers.hpp`
  
  **Acceptance Criteria:**
  - [ ] Only one `to_string()` implementation (in ABI)
  - [ ] Host/guest include from ABI
  
  **Commit:** YES
  - Message: `refactor(cpp): deduplicate to_string() helper (ARCH-001)`

- [x] 15. ARCH-001: Python helper deduplication

  **What to do:**
  - Make ABI `_to_str()` public (remove underscore)
  - Remove duplicate from host/helpers.py
  
  **Acceptance Criteria:**
  - [ ] Single `to_str()` implementation in ABI
  
  **Commit:** YES (part of Task 13)

- [x] 16. ARCH-001: JavaScript helper move

  **What to do:**
  - Move `StringViewHelper` class from guest to ABI
  - Update `sdks/js/abi/polyplug_abi.ts`
  
  **Acceptance Criteria:**
  - [ ] `StringViewHelper` in ABI package
  - [ ] Guest imports from ABI
  
  **Commit:** YES
  - Message: `refactor(js): move StringViewHelper to ABI package (ARCH-001)`

- [x] 17. ARCH-001: C# helper unification

  **What to do:**
  - Create shared `abi/StringViewHelper.cs`
  - Unify host and guest implementations
  
  **Acceptance Criteria:**
  - [ ] Single `StringViewHelper` in ABI
  - [ ] Host and guest use shared implementation
  
  **Commit:** YES
  - Message: `refactor(csharp): unify StringViewHelper in ABI (ARCH-001)`

### Wave 5: Major Architectural (Tasks 18-19)

- [x] 18. CRIT-001: Replace thread-local in JS loader

  **What to do:**
  - Implement design from Task 3
  - Replace `thread_local!` REGISTRATION_DATA with context-embedded approach
  
  **Acceptance Criteria:**
  - [ ] No `thread_local!` usage in JS loader
  - [ ] Multi-threaded plugin load test passes
  
  **Commit:** YES
  - Message: `fix(js-loader): replace thread-local with context data (CRIT-001)`

- [x] 19. Integration testing across all languages

  **What to do:**
  - Run full cross-language test matrix
  - Verify all generator outputs work correctly
  
  **Acceptance Criteria:**
  - [ ] All integration tests pass
  - [ ] All examples build and run
  
  **Commit:** NO

### Wave FINAL: Verification (Tasks 20-23)

- [x] 20. Security audit - all issues fixed

  **What to do:**
  - Verify each issue has been addressed
  - Check for any remaining instances of the same patterns
  
  **Acceptance Criteria:**
  - [ ] All 12 issues verified fixed
  - [ ] No new security issues introduced
  
  **Commit:** NO

- [x] 21. ABI compatibility verification

  **What to do:**
  - Run ABI layout tests
  - Verify no frozen ABI structs changed
  
  **Acceptance Criteria:**
  - [ ] `cargo test -p polyplug_abi` passes
  - [ ] No changes to `#[repr(C)]` struct layouts
  
  **Commit:** NO

- [x] 22. Cross-language integration test

  **What to do:**
  - Re-run full integration test suite
  - Verify all language combinations work
  
  **Acceptance Criteria:**
  - [ ] All integration tests pass
  - [ ] No regressions in any language
  
  **Commit:** NO

- [x] 23. AGENTS.md compliance check

  **What to do:**
  - Check all changes comply with AGENTS.md rules
  - Verify no new violations introduced
  
  **Acceptance Criteria:**
  - [ ] No new thread-locals (Rule 12)
  - [ ] All unsafe blocks have SAFETY comments (Rule 6)
  - [ ] No unwrap/expect in production code (Rule 4)
  
  **Commit:** NO

---

## Commit Strategy

Each task is a separate commit with the pattern:
```
<type>(<scope>): <description> (ISSUE-XXX)

- What changed
- Why it was needed
- Testing performed
```

---

## Success Criteria

### Verification Commands
```bash
# All tests pass
cargo test --workspace

# No clippy warnings
cargo clippy -- -D warnings

# ABI tests pass
cargo test -p polyplug_abi

# Integration tests pass
cargo test -p polyplug_integration

# Formatting clean
cargo fmt --check
```

### Final Checklist
- [ ] All 5 CRITICAL issues fixed
- [ ] All 5 HIGH issues fixed
- [ ] All 2 ARCH issues fixed
- [ ] Each fix has dedicated test
- [ ] All tests pass (100%)
- [ ] No new AGENTS.md violations
- [ ] ABI frozen - no breaking changes
- [ ] Documentation updated (CHANGELOG.md)
- [ ] Security audit passed

---

## Post-Fix Validation

After all fixes are complete:

1. Run full test suite: `cargo test --workspace`
2. Run examples: `./examples/build_all.sh && ./examples/run_all.sh`
3. Security review: Re-run security scan
4. Performance check: Ensure no regressions
5. Documentation: Update CHANGELOG.md with security fixes

**Estimated Timeline:**
- Wave 1: 3-4 days
- Wave 2: 7-10 days
- Wave 3: 5-7 days
- Wave 4: 5-7 days
- Wave 5: 5-7 days
- Wave FINAL: 2-3 days
- **Total: 7-8 weeks**
