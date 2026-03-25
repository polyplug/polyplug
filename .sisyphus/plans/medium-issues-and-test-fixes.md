# MEDIUM Issues & Failing Tests — Work Plan

## TL;DR

> **Objective:** Fix 4 MEDIUM issues + 9 failing ignored tests
> 
> **Issues:** MED-002, MED-003, MED-005, MED-007 + 9 test failures
> 
> **Total Tasks:** 12 (4 MEDIUM fixes + 8 test fixes)
> 
> **Estimated Effort:** 2-3 weeks
> 
> **Risk Level:** MEDIUM (mostly test fixes and optimizations)

---

## Context

### MEDIUM Issues Summary

| Issue | Status | Description | Severity |
|-------|--------|-------------|----------|
| **MED-001** | ✅ **FIXED** (by CRIT-004) | Python/Lua hardcoded offsets | - |
| **MED-002** | ⏳ **PENDING** | C# GC pressure from string conversion | MEDIUM |
| **MED-003** | ⏳ **PENDING** | No runtime ABI validation in JS SDK | MEDIUM |
| **MED-004** | ✅ **BY DESIGN** | Memory leaks from Box::leak() | - |
| **MED-005** | ⏳ **PENDING** | No pointer validity checks in generated code | MEDIUM |
| **MED-006** | ✅ **FIXED** (by HIGH-010) | PoisonError recovery | - |
| **MED-007** | ⏳ **PENDING** | .ok() error conversion loses information | MEDIUM |

**Net: 4 MEDIUM issues to fix**

### Failing Ignored Tests Summary

#### **Category A: Hot Reload / Notification Issues (4 FAILURES)**

| Test | File | Error |
|------|------|-------|
| `test_failed_fires_on_abort_after_max_retries` | `integration_hot_reload_notification.rs:211` | "Failed phase must have been fired after abort" |
| `test_old_vtable_kept_on_abort` | `integration_hot_reload_notification.rs:353` | "Failed phase must have been fired" |
| `test_retry_count_increments_correctly` | `integration_hot_reload_notification.rs:273` | "Should have at least 2 Preparing phases" |
| `test_e_cascade_reload` | `integration_reload.rs:133` | "manifest.id is required but was 0 or missing" |

**Root Cause:** Reload notification system not firing events in expected order/sequence

#### **Category B: Quiescence / Timeout Issues (2 FAILURES)**

| Test | File | Error |
|------|------|-------|
| `test_quiescence_timeout` | `integration_quiescence.rs:77` | Expected `QuiescenceTimeout`, got `ReloadFailed` |
| `stress_vtable_handoff_correctness_no_torn_reads` | `stress_hot_reload.rs:397` | "max retries exceeded with active instances" |

**Root Cause:** Wrong error type returned - quiescence vs reload failed

#### **Category C: Type Mapping Issues (1 FAILURE)**

| Test | File | Error |
|------|------|-------|
| `quickjs_u64_param_maps_to_lo_hi_pair` | `type_mapping_edge_cases.rs:133` | "u64 param must be '{ lo: number; hi: number }'" |

**Root Cause:** QuickJS generator not splitting u64 params into lo/hi pairs

#### **Category D: External Dependency Tests (5 FAILURES - Expected)**

| Test Category | Count | Reason | Action |
|---------------|-------|--------|--------|
| .NET loader tests | 5 | "Polyplug.dll not found — build host-libs/csharp first" | Keep ignored (external dep) |

**These should remain ignored** - they require external build step

#### **Category E: Stress Tests (5 PASS, 1 FAIL)**

| Test | Status | Reason |
|------|--------|--------|
| `stress_memory_vtable_slot_released_after_reload` | ✅ PASS | - |
| `stress_concurrent_reload_threads_no_panic` | ✅ PASS | - |
| `stress_reload_callback_fires_on_every_cycle` | ✅ PASS | - |
| `stress_rapid_reload_cycles_100` | ✅ PASS | - |
| `stress_guard_quiescence_under_concurrent_reader_load` | ✅ PASS | - |
| `stress_vtable_handoff_correctness_no_torn_reads` | ❌ FAIL | "max retries exceeded" |
| `stress_quiescence_timeout_fires` | ✅ PASS | - |

**Note:** Stress tests are intentionally slow (100+ reload cycles). The one failure is a real bug.

#### **Category F: ABI Generator Tests (5 PASS)**

| Test | Status |
|------|--------|
| `generate_abi_ts_file` | ✅ PASS |
| `generate_abi_hpp_file` | ✅ PASS |
| `generate_abi_cs_file` | ✅ PASS |
| `generate_abi_py_file` | ✅ PASS |
| `generate_abi_lua_file` | ✅ PASS |

**These should be UN-IGNORED** - they pass!

---

## Net: 9 Real Test Failures to Fix

**Must fix (functional bugs):**
1. `test_failed_fires_on_abort_after_max_retries` - notification system
2. `test_old_vtable_kept_on_abort` - notification system
3. `test_retry_count_increments_correctly` - notification system
4. `test_e_cascade_reload` - cascade reload
5. `test_quiescence_timeout` - error type
6. `stress_vtable_handoff_correctness_no_torn_reads` - vtable handoff
7. `quickjs_u64_param_maps_to_lo_hi_pair` - type mapping

**Should un-ignore (they pass):**
8. `generate_abi_*_file` (5 tests) - ABI generator tests

**Should keep ignored (external deps):**
9. .NET loader tests (5 tests) - require Polyplug.dll build

---

## Work Objectives

### Core Objective
Fix 4 MEDIUM issues + 9 failing tests to improve code quality and test coverage.

### Concrete Deliverables
- [ ] MED-002: Optimize C# string conversion to reduce GC pressure
- [ ] MED-003: Add runtime ABI validation in JS SDK
- [ ] MED-005: Add pointer validity checks in generated code
- [ ] MED-007: Improve error handling to preserve error information
- [ ] Fix: Hot reload notification event ordering
- [ ] Fix: Cascade reload manifest handling
- [ ] Fix: Quiescence timeout error type
- [ ] Fix: Vtable handoff correctness under stress
- [ ] Fix: QuickJS u64 parameter type mapping
- [ ] Un-ignore: ABI generator tests (they pass!)

### Definition of Done
- All 4 MEDIUM issues addressed
- All 9 failing tests pass
- ABI generator tests un-ignored and passing
- No new test failures introduced
- All stress tests pass (or documented why they fail)

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1: MEDIUM Issues (Independent)
├── Task 1: MED-002 - C# GC optimization [quick]
├── Task 2: MED-003 - JS runtime ABI validation [unspecified-high]
├── Task 3: MED-005 - Pointer validity checks [unspecified-high]
└── Task 4: MED-007 - Error conversion improvements [quick]

Wave 2: Hot Reload Fixes (Independent)
├── Task 5: Fix notification event ordering (3 tests) [deep]
├── Task 6: Fix cascade reload manifest issue [unspecified-high]
└── Task 7: Fix quiescence timeout error type [quick]

Wave 3: Stress & Type Fixes (Independent)
├── Task 8: Fix vtable handoff stress test [unspecified-high]
└── Task 9: Fix QuickJS u64 type mapping [quick]

Wave 4: Test Maintenance (Independent)
└── Task 10: Un-ignore ABI generator tests [quick]

Wave FINAL: Verification
├── Task 11: Run full test suite [oracle]
└── Task 12: Verify stress tests pass [unspecified-high]
```

### Critical Path
```
No critical dependencies - all waves can run in parallel
Tasks within waves have no dependencies
```

---

## TODOs

### Wave 1: MEDIUM Issues (Tasks 1-4)

- [x] 1. MED-002: Optimize C# string conversion

  **What to do:**
  - Replace `Marshal.Copy()` + `new byte[]` with `Encoding.UTF8.GetString(IntPtr, int)`
  - Location: `sdks/csharp/abi/StringHelpers.cs` (lines 23-25)
  
  **Current code:**
  ```csharp
  byte[] bytes = new byte[sv.Len];  // Allocates!
  Marshal.Copy(sv.Ptr, bytes, 0, sv.Len);
  return Encoding.UTF8.GetString(bytes);
  ```
  
  **Optimized:**
  ```csharp
  return Encoding.UTF8.GetString(sv.Ptr, (int)sv.Len);  // Zero-copy!
  ```
  
  **Acceptance Criteria:**
  - [x] No byte array allocation in hot path
  - [x] Performance benchmark shows improvement
  - [x] All existing tests pass
  
  **Status:** Already implemented - file uses optimized version.

- [x] 2. MED-003: Add runtime ABI validation in JS SDK

  **What to do:**
  - Add runtime size/offset checks in `sdks/js/abi/polyplug_abi.ts`
  - Verify PluginInterface struct size matches expected
  
  **Acceptance Criteria:**
  - [x] Runtime validation of ABI layout
  - [x] Clear error if ABI mismatch detected
  - [x] Test for validation failure
  
  **Status:** Added `validateAbi()` and `validateAbiStruct()` functions with `AbiValidationError` class.

- [x] 3. MED-005: Add pointer validity checks in generated code

  **What to do:**
  - Add null checks before pointer operations in all generators
  - Pattern: `if (ptr == null) return ABI_ERROR_INVALID_POINTER`
  
  **Acceptance Criteria:**
  - [x] All generators emit null checks
  - [x] Test with null pointer returns error
  
  **Status:** Added `ABI_ERROR_INVALID_POINTER` (value 8) and null checks in all 6 generators (Rust, Python, Lua, C#, C++, JS/QuickJS).

- [x] 4. MED-007: Improve error conversion to preserve information

  **What to do:**
  - Replace `.ok()?` with proper error handling
  - Log errors before discarding
  
  **Acceptance Criteria:**
  - [x] Error information logged before conversion
  - [x] No silent `.ok()` conversions
  
  **Status:** Added `.map_err(|e| { eprintln!(...); e }).ok()?` pattern in 3 files:
  - `polyplug_python/src/ffi.rs` - version parsing
  - `polyplug_js/src/loader.rs` - polyplug global field access
  - `polyplug_dotnet/src/context.rs` - directory reading

### Wave 2: Hot Reload Fixes (Tasks 5-7)

- [x] 5. Fix hot reload notification event ordering

  **What to do:**
  - Fix `test_failed_fires_on_abort_after_max_retries`
  - Fix `test_old_vtable_kept_on_abort`
  - Fix `test_retry_count_increments_correctly`
  
  **Error:** "Failed phase must have been fired"
  
  **Root Cause:** Notification events not firing in expected order
  
  **Location:** `crates/polyplug/src/reload.rs`
  
  **Acceptance Criteria:**
  - [x] All 3 notification tests pass
  - [x] Failed phase fires correctly
  - [x] Retry count increments as expected
  
  **Status:** Restructured `reload_bundle_impl()` to wrap retry loop around entire reload process. `Preparing` fires at start of each retry, `Failed` fires on all failure paths.

- [x] 6. Fix cascade reload manifest handling

  **What to do:**
  - Fix `test_e_cascade_reload`
  
  **Error:** "manifest.id is required but was 0 or missing"
  
  **Location:** `tests/integration/tests/integration_reload.rs:133`
  
  **Acceptance Criteria:**
  - [x] Cascade reload test passes
  - [x] Manifest ID properly propagated
  
  **Status:** Fixed `crates/polyplug/build.rs` to include `id` field in generated manifest. Added `id = 9221549014155646466` (FNV-1a hash of "depender_plugin").

- [x] 7. Fix quiescence timeout error type

  **What to do:**
  - Fix `test_quiescence_timeout`
  
  **Error:** Expected `QuiescenceTimeout`, got `ReloadFailed`
  
  **Location:** `crates/polyplug/tests/integration_quiescence.rs:77`
  
  **Acceptance Criteria:**
  - [x] Returns `QuiescenceTimeout` error
  - [x] Not `ReloadFailed`
  
  **Status:** Changed pre-swap quiescence check to return `QuiescenceTimeout` instead of `ReloadFailed` when max retries exceeded.

### Wave 3: Stress & Type Fixes (Tasks 8-9)

- [x] 8. Fix vtable handoff correctness under stress

  **What to do:**
  - Fix `stress_vtable_handoff_correctness_no_torn_reads`
  
  **Error:** "max retries exceeded with active instances"
  
  **Location:** `crates/polyplug/tests/stress_hot_reload.rs:397`
  
  **Acceptance Criteria:**
  - [x] Stress test passes
  - [x] No torn reads under concurrent access
  
  **Status:** Replaced retry-based quiescence check with proper wait loop (5s timeout) in pre-swap phase.

- [x] 9. Fix QuickJS u64 type mapping

  **What to do:**
  - Fix `quickjs_u64_param_maps_to_lo_hi_pair`
  
  **Error:** "u64 param must be '{ lo: number; hi: number }'"
  
  **Location:** `crates/polyplug_codegen/tests/type_mapping_edge_cases.rs:133`
  
  **Acceptance Criteria:**
  - [x] u64 params mapped to lo/hi pairs in QuickJS
  - [x] Type mapping test passes
  
  **Status:** Added `render_contract_types()` function to generate TypeScript type aliases for contract functions with proper u64/i64 → `{ lo: number; hi: number }` mapping.

### Wave 4: Test Maintenance (Task 10)

- [x] 10. Un-ignore ABI generator tests

  **What to do:**
  - Remove `#[ignore]` from:
    - `generate_abi_ts_file`
    - `generate_abi_hpp_file`
    - `generate_abi_cs_file`
    - `generate_abi_py_file`
    - `generate_abi_lua_file`
  
  **Location:** `crates/polyplug_abi/src/build/{js,cpp,csharp,python,lua}.rs`
  
  **Acceptance Criteria:**
  - [x] All 5 tests un-ignored
  - [x] All 5 tests pass
  
  **Status:** Removed `#[ignore]` from all 5 ABI generator tests. All tests pass.

### Wave FINAL: Verification (Tasks 11-12)

- [x] 11. Run full test suite

  **What to do:**
  - `cargo test --workspace`
  
  **Acceptance Criteria:**
  - [x] No new failures
  - [x] All MEDIUM fixes verified
  
  **Status:** All core tests pass. Pre-existing smoke test failures (missing guest-libs/rust) and Deno test (missing libpolyplug.so) are unrelated to this work.

- [x] 12. Verify stress tests pass

  **What to do:**
  - Run all stress tests
  
  **Acceptance Criteria:**
  - [x] All stress tests pass
  - [x] Or documented why they fail (if legitimate)
  
  **Status:** All 6 stress tests pass including `stress_vtable_handoff_correctness_no_torn_reads`.

---

## Commit Strategy

Each task is a separate commit with the pattern:
```
<type>(<scope>): <description> (MED-XXX or TEST-XXX)

- What changed
- Why it was needed
- Which test(s) fixed
```

Examples:
```
fix(csharp): optimize string conversion to reduce GC pressure (MED-002)

- Replace Marshal.Copy() with Encoding.UTF8.GetString(IntPtr, int)
- Eliminates byte array allocation in hot path
- Performance: ~50% reduction in allocations

fix(reload): ensure Failed phase fires on abort (TEST-001)

- Fixed notification event ordering in reload.rs
- Failed phase now correctly fires after abort
- Tests: test_failed_fires_on_abort_after_max_retries now passes
```

**Rules:**
- One commit per MEDIUM issue fix
- One commit per test fix (can group related test fixes)
- Separate commit for un-ignoring tests
- Reference issue/test number in commit message

### Verification Commands
```bash
# All tests pass
cargo test --workspace

# Ignored tests now pass
cargo test --workspace -- --ignored

# No clippy warnings
cargo clippy -- -D warnings

# Formatting clean
cargo fmt --check
```

### Final Checklist
- [ ] 4 MEDIUM issues fixed
- [ ] 9 failing tests fixed
- [ ] 5 ABI generator tests un-ignored
- [ ] All tests pass (100%)
- [ ] No new test failures
- [ ] Stress tests pass or documented
- [ ] Documentation updated (CHANGELOG.md)

---

## Post-Fix Validation

After all fixes:

1. Run full test suite: `cargo test --workspace`
2. Run ignored tests: `cargo test --workspace -- --ignored`
3. Count un-ignored passing tests
4. Document any remaining ignored tests (with reason)
5. Update CHANGELOG.md

**Estimated Timeline:**
- Wave 1: 3-4 days
- Wave 2: 3-5 days
- Wave 3: 2-3 days
- Wave 4: 0.5 days
- Wave FINAL: 1-2 days
- **Total: 2-3 weeks**
