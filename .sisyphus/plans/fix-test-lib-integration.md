# Fix Test Fixture Library Integration

## TL;DR

> **Problem:** Test fixtures duplicate ABI definitions instead of using shared SDK libraries, creating maintenance nightmares and silent failure risks.
>
> **Solution:** Refactor C# and JavaScript test fixtures to import from SDKs, remove duplicated code, add CI prevention.
>
> **Deliverables:** 
> - C# fixture uses `sdks/csharp/abi/` (removes 200+ lines of duplication)
> - JavaScript fixture uses `sdks/js/guest/` (if duplication found)
> - CI check prevents future duplication
>
> **Estimated Effort:** Medium (2-3 hours)
> **Parallel Execution:** YES - C# and JS tasks independent
> **Critical Path:** C# fixture → JS fixture → CI check

---

## Context

### Original Request
User identified that integration test fixtures don't properly rely on host/guest/ABI libraries. Specifically, C# test fixture duplicates all ABI types instead of using the shared SDK.

### Interview Summary - Verification Complete

**Verified Findings:**

| Fixture | Language | Uses SDK? | Status |
|---------|----------|-----------|--------|
| `csharp_plugin` | C# | ✗ **NO** - Duplicates 200+ lines | **CRITICAL** |
| `test_plugin` | Rust | ✓ **YES** - Uses `polyplug_abi` | ✓ Good (no change needed) |
| `test_plugin_python` | Python | ✓ **YES** | ✓ Good |
| `test_plugin_lua` | Lua | ✓ **YES** | ✓ Good |
| `test_plugin_js` | JS | ? **NEEDS INVESTIGATION** | Check for duplication |

**C# Problem (Critical):**
- File: `tests/fixtures/csharp_plugin/Plugin.cs` (lines 1-210)
- Duplicates: `StringView`, `AbiError`, `PluginDescriptor`, `HostVTable`, constants
- All exist in: `sdks/csharp/abi/Abi.cs` with namespace `Polyplug.Abi`
- Risk: ABI changes cause silent test failures

**Rust - No Action Needed:**
- User confirmed: `polyplug_guest` just re-exports from `polyplug_abi`
- Current fixture uses `polyplug_abi` directly - this is correct
- Location: `crates/polyplug_guest/` (re-exports `polyplug_abi::*`)

**JavaScript - No Action Needed:**
- File: `tests/fixtures/test_plugin_js/bundle.js` uses runtime-injected `polyplug` object
- SDK `sdks/js/guest/polyplug_guest.js` provides constants and helpers
- JS fixtures use runtime injection pattern (not duplication) - this is correct

### CI Integration Explained
"CI integration" means adding the check script to `.github/workflows/test.yml` so it runs automatically on every PR, preventing future duplication.

---

## Work Objectives

### Core Objective
Refactor C# and JavaScript test fixtures to use shared SDK/guest libraries, eliminating ABI duplication and ensuring fixtures serve as proper usage examples.

### Concrete Deliverables
1. C# fixture imports from `sdks/csharp/abi/` (remove all duplicated types)
2. CI check prevents future ABI duplication in fixtures
3. All integration tests pass after C# fixes

### Definition of Done
- [ ] C# fixture `.csproj` references `Polyplug.Abi.csproj`
- [ ] C# fixture `Plugin.cs` has no duplicated ABI types (uses `Polyplug.Abi` namespace)
- [ ] All integration tests pass after C# fixes
- [ ] CI check script exists and runs on PR

### Must Have
- C# fixture uses SDK ABI types
- Tests pass after C# fixes
- CI prevention in place

### Must NOT Have (Guardrails)
- No changes to actual SDK libraries (only fixture changes)
- No ABI breakage (maintain compatibility)
- No changes to test logic (only imports/dependencies)
- No changes to Rust fixture (already correct)

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES - `cargo test` works
- **Automated tests**: YES - Tests-after each wave
- **Framework**: Rust built-in test runner + integration tests

### QA Policy
Every task includes agent-executed QA scenarios:
- **Build**: Verify compilation succeeds
- **Test**: Run integration tests after each wave
- **Evidence**: Build output, test results, file diffs

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately - foundation):
├── Task 1: C# fixture project reference [quick]
└── Task 2: CI check script creation [quick]

Wave 2 (After Wave 1 - C# implementation):
├── Task 3: C# fixture remove duplicated types [unspecified-high]
└── Task 4: Run C# integration tests [unspecified-high]

Wave 3 (After Wave 2 - verification):
└── Task 5: Verify CI check works [quick]

Wave FINAL (After ALL tasks - 2 parallel reviews):
├── Task F1: Code quality review (unspecified-high)
└── Task F2: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: Task 1 → Task 3 → Task 4 → Task 5 → F1-F2 → user okay
Parallel Speedup: ~30% faster than sequential
```

---

## TODOs

