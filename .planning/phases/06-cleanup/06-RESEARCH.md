# Phase 6: Cleanup - Research

**Researched:** 2026-04-04
**Domain:** Terminology cleanup, naming consistency, documentation updates
**Confidence:** HIGH

## Summary

Phase 6 focuses on removing legacy terminology ("vtable", "VTable", "VTABLE") and ensuring consistent Guest/Host naming throughout the codebase. This is a cleanup phase following the major refactoring in Phases 1-5.

**Primary recommendation:** Systematic search-and-replace across all crates, SDKs, documentation, and tests with verification via grep patterns. Phase 5's plan 05-08 already completed the RuntimeConfigC rename, reducing scope for CLN-02.

<user_constraints>
## User Constraints (from STATE.md)

### Locked Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-03 | Remove "vtable" naming | Confusing terminology; use GuestContractInterface |
| 2026-04-03 | Rename Plugin Contract -> Guest Contract | Clear Host/Guest separation |
| 2026-04-03 | RuntimeAbi naming | Clearer than HostVTable (host != runtime) |
| 2026-04-03 | All public ABI structs repr(C) | Single source of truth, no *C types |
| 2026-04-03 | Legacy aliases PluginInterface/HostVTable | Smooth transition for dependent code |

### Deferred Ideas (OUT OF SCOPE)

- WASM runtime support
- Plugin sandboxing
- New loader implementations

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLN-01 | Remove all "vtable" naming from codebase | 223 occurrences in source; detailed file breakdown below |
| CLN-02 | Remove `*C` suffix types from FFI | RuntimeConfigC already renamed in Phase 5 (05-08); verification needed |
| CLN-03 | Update documentation to use Guest/Host terminology | 8 docs files with old terminology; Guest/Host patterns established |
| CLN-04 | Update tests to use new instance model | 586 occurrences in tests/benchmarks; PluginGuard in examples |

</phase_requirements>

## Scope Discovery

### VTable Naming (CLN-01)

**Total occurrences in source code (crates/ + sdks/): 223**

By location category:

| Category | Count | Action Required |
|----------|-------|-----------------|
| Source files (crates/) | ~150 | Rename variables, comments, struct fields |
| Test files (crates/*/tests/) | ~50 | Update test fixture code |
| Benchmark files (benches/) | ~20 | Update benchmark comments and fixtures |
| SDK files (sdks/) | ~3 | Update generated code patterns |

**Key files with vtable terminology:**

| File | Occurrences | Context |
|------|-------------|---------|
| `crates/polyplug/benches/vtable_dispatch.rs` | 40+ | Benchmark file - rename to `contract_dispatch.rs` |
| `crates/polyplug/src/registry/plugin_registry.rs` | 5 | Comments: "vtable" -> "interface" |
| `crates/polyplug_abi/src/host/runtime_abi.rs` | 3 | Doc comment about rename history |
| `crates/polyplug_native/src/loader.rs` | 8 | Comments and variable names |
| `sdks/cpp/guest/polyplug/contract.hpp` | 10 | Method name `vtable()` -> `interface()` |
| `sdks/csharp/guest/HostVTableStorage.cs` | 5 | Class name -> `RuntimeAbiStorage` |

### *C Suffix Types (CLN-02)

**Status: PARTIALLY COMPLETE (Phase 5 Plan 05-08)**

Phase 5 already renamed `RuntimeConfigC` to `RuntimeConfig` in all SDKs:

| SDK | Status | Verified |
|-----|--------|----------|
| Python | Renamed in 05-08 | Yes - `grep -r RuntimeConfigC sdks/python` = 0 |
| C# | Renamed in 05-08 | Yes - `grep -r RuntimeConfigC sdks/csharp` = 0 |
| Lua | Renamed in 05-08 | Yes - `grep -r RuntimeConfigC sdks/lua` = 0 |
| C++ | Already correct from 05-07 | Yes |

**Remaining *C suffix types:**

| Type | Location | Action |
|------|----------|--------|
| `ReloadPhaseC` | `crates/polyplug/src/ffi.rs:50` | Rename to `ReloadPhaseFfi` (FFI-safe variant) |
| `RuntimeCreateOptionsC` | Not found - may be named differently | Verify FFI types |
| `PluginContextC` | Not found - PluginContext is canonical | None needed |

**Note:** `ReloadPhaseC` is intentionally an FFI-safe variant per ABI-06. Consider renaming to `ReloadPhaseFfi` for clarity, not removing entirely.

### Legacy Aliases in polyplug_abi (CLN-01)

**Location:** `crates/polyplug_abi/src/lib.rs` lines 46-58

```rust
// ─── Legacy aliases for transition (Phase 6 removes these) ────────────────────

/// Legacy alias for GuestContractInterface during transition.
/// Will be removed in Phase 6.
pub type PluginInterface = GuestContractInterface;

/// Legacy alias for RuntimeAbi during transition.
/// Will be removed in Phase 6.
pub type HostVTable = RuntimeAbi;

/// Legacy alias for DispatchMechanisms during transition.
/// Will be removed in Phase 6.
pub type PluginDispatch = DispatchMechanisms;
```

**Impact:** Removing these requires updating all imports across workspace:
- `PluginInterface` used in 1800+ locations
- `HostVTable` used in 163+ locations
- `PluginDispatch` used in 43+ locations

### PluginGuard (CLN-04)

**Total occurrences: 15 in examples/generated code**

| Location | Count | Action |
|----------|-------|--------|
| `examples/hosts/csharp/generated/host/Callers.cs` | 10 | Replace with instance model |
| `examples/hosts/python/generated/host/callers.py` | 4 | Replace with instance model |
| `examples/hosts/rust/generated/host/host_callers.rs` | 1 | Replace with instance model |

**Note:** PluginGuard was removed from SDK core files in Phase 5. Remaining occurrences are in generated example code.

### Documentation Updates (CLN-03)

**Files requiring terminology updates:**

| File | Old Terminology | New Terminology |
|------|-----------------|-----------------|
| `docs/ABI_ARCHITECTURE.md` | vtable, PluginInterface | contract interface, GuestContractInterface |
| `docs/HOT_RELOAD_DESIGN.md` | VTableSlot, vtable swap | interface swap |
| `docs/HOST_CONTRACTS.md` | host vtable | host contract interface |
| `docs/HOST_CONTRACTS_API.md` | HostVTable | RuntimeAbi |
| `docs/PERFORMANCE.md` | vtable dispatch | contract dispatch |
| `docs/PLUGIN_INTERFACE_DESIGN.md` | PluginInterface | GuestContractInterface |
| `docs/abi_types.md` | vtable terminology | Guest/Host terminology |
| `docs/ARCHITECTURE_CLARIFICATIONS.md` | PluginInterface, HostVTable | GuestContractInterface, RuntimeAbi |

## Standard Stack

### Current Naming Conventions

| Old Name | New Name | Source Location |
|----------|----------|-----------------|
| `PluginInterface` | `GuestContractInterface` | polyplug_abi/src/guest/ |
| `HostVTable` | `RuntimeAbi` | polyplug_abi/src/host/ |
| `PluginDispatch` | `DispatchMechanisms` | polyplug_abi/src/dispatch/ |
| `PluginContractId` | `GuestContractId` | polyplug_utils/src/ids.rs |
| `PluginGuard` | (removed) | Instance model replaces |
| `VTableSlot` | (removed) | Direct Arc<Interface> storage |
| `vtable` (variable) | `interface` | Throughout codebase |

### Reference Patterns from Completed Phases

From Phase 1 (ABI Types):
- `GuestContractInterface` defined in `crates/polyplug_abi/src/guest/guest_contract_interface.rs`
- `RuntimeAbi` defined in `crates/polyplug_abi/src/host/runtime_abi.rs`
- `HostContractInterface` for host-provided contracts

From Phase 5 (SDK Updates):
- SDK files updated to import from `polyplug_abi` directly
- Generated code patterns established

## Dependencies

### Module Dependency Graph

```
polyplug_abi (canonical types)
    |
    ├── polyplug (imports polyplug_abi types)
    │       └── polyplug_native, polyplug_python, etc. (loaders)
    |
    ├── polyplugc (generators use type names)
    │       └── generated code in examples/ and sdks/
    |
    └── sdks/ (import from abi modules)
            ├── python/polyplug_abi/
            ├── csharp/Abi namespace
            ├── lua/polyplug_abi
            ├── js/abi/
            └── cpp/abi/
```

### Files That Depend on Legacy Aliases

| Module | Uses PluginInterface | Uses HostVTable | Uses PluginDispatch |
|--------|---------------------|-----------------|--------------------|
| polyplug/tests | Yes (fixtures) | Yes (mocks) | Yes (dispatch) |
| polyplug/benches | Yes (fixtures) | Yes (mocks) | Yes (dispatch) |
| polyplug_native | Yes | Yes | No |
| polyplug_python | Yes | No | Yes |
| polyplug_lua | Yes | No | Yes |
| polyplug_js | Yes | No | Yes |
| polyplug_dotnet | Yes | Yes | No |
| polyplugc/generators | Yes | No | Yes |

## Risks

### Breaking Changes

| Change | Risk Level | Mitigation |
|--------|------------|------------|
| Remove legacy aliases | HIGH | Workspace-wide import update; staged approach |
| Rename benchmark file | LOW | git mv preserves history |
| Update generated code | MEDIUM | Regenerate with updated codegen |
| Documentation changes | LOW | No runtime impact |

### FFI Boundary Considerations

| Concern | Status | Action |
|---------|--------|--------|
| ABI stability | Not published yet | Breaking changes acceptable |
| Binary compatibility | Internal use only | No external consumers |
| Struct layouts unchanged | Verified | Rename only, layout preserved |

### Test Rework Estimate

| Test Category | Files | Effort |
|---------------|-------|--------|
| Integration tests | ~20 files | Import updates + fixture changes |
| Benchmarks | 4 files | Rename imports + variable names |
| Stress tests | 4 files | Import updates |
| Loader tests | 5 files | Import updates |

## Verification Strategy

### Grep Patterns for Completion

```bash
# CLN-01: No vtable terminology
grep -ri "vtable\|VTable\|VTABLE" crates/ sdks/ --include="*.rs" --include="*.cs" --include="*.py" --include="*.lua" --include="*.js" --include="*.hpp"
# Expected: 0 matches (except documentation references explaining rename history)

# CLN-02: No *C suffix types
grep -r "RuntimeConfigC\|PluginContextC\|HostVTableC" crates/ sdks/
# Expected: 0 matches (ReloadPhaseC renamed to ReloadPhaseFfi)

# CLN-03: Documentation uses Guest/Host
grep -r "PluginInterface\|HostVTable" docs/
# Expected: Only in "renamed from" explanatory notes

# CLN-04: No PluginGuard in source
grep -r "PluginGuard" crates/ sdks/ --include="*.rs" --include="*.cs" --include="*.py"
# Expected: 0 matches (generated code regenerated)
```

### Acceptance Criteria

| Criterion | Verification |
|-----------|--------------|
| Legacy aliases removed | `grep -r "pub type PluginInterface" crates/polyplug_abi/` returns 0 |
| All imports updated | `cargo build --workspace` succeeds |
| Tests pass | `cargo test --workspace` succeeds |
| Benchmarks renamed | `crates/polyplug/benches/vtable_dispatch.rs` renamed to `contract_dispatch.rs` |
| SDKs use new naming | All 5 SDKs import GuestContractInterface, RuntimeAbi |
| Generated code updated | Examples regenerated with polyplugc |

## Common Pitfalls

### Pitfall 1: Incomplete Import Updates
**What goes wrong:** Legacy alias removed but imports not updated, causing compilation failures.
**How to avoid:** Run `cargo build --workspace` after each staged removal to catch missing imports.

### Pitfall 2: Generated Code Not Regenerated
**What goes wrong:** Old generated code in examples/ still uses PluginInterface.
**How to avoid:** Run `polyplugc generate` on all example bundles after codegen updates.

### Pitfall 3: Comment/Documentation Inconsistency
**What goes wrong:** Code uses new names but comments still reference "vtable".
**How to avoid:** Include comment review in grep patterns; update simultaneously with code.

### Pitfall 4: Test Fixture Mock Interfaces
**What goes wrong:** Test mock functions still return `PluginInterface` type.
**How to avoid:** Update all test fixture code to use `GuestContractInterface`.

## Code Examples

### Current Legacy Pattern (to remove)
```rust
// crates/polyplug_abi/src/lib.rs
pub type PluginInterface = GuestContractInterface;  // REMOVE
pub type HostVTable = RuntimeAbi;                   // REMOVE
pub type PluginDispatch = DispatchMechanisms;       // REMOVE
```

### After Cleanup Pattern
```rust
// crates/polyplug_abi/src/lib.rs
// Direct exports only, no aliases:
pub use guest::{GuestContractInterface, GuestContractInstance};
pub use host::{HostContractInterface, HostContractInstance, RuntimeAbi};
pub use dispatch::{DispatchType, DispatchMechanisms, NativeDispatch};
```

### Import Update Pattern
```rust
// Before:
use polyplug_abi::{PluginInterface, HostVTable, PluginDispatch};

// After:
use polyplug_abi::{GuestContractInterface, RuntimeAbi, DispatchMechanisms};
```

### Benchmark Rename Pattern
```bash
# Before: crates/polyplug/benches/vtable_dispatch.rs
# After:  crates/polyplug/benches/contract_dispatch.rs

git mv crates/polyplug/benches/vtable_dispatch.rs crates/polyplug/benches/contract_dispatch.rs
```

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All crates | Yes | 1.85 | - |
| cargo | Build/test | Yes | - | - |
| polyplugc | Regenerate examples | Yes | local | - |

**Step 2.6: SKIPPED** (no external dependencies - purely code/config changes)

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | RuntimeConfigC renamed in Phase 5 | *C Suffix | CLN-02 scope larger than expected |
| A2 | PluginGuard removed from SDK core | PluginGuard | CLN-04 scope larger than expected |
| A3 | Examples use generated code | Generated code | Manual update needed if not regenerated |

All claims verified via grep searches in this session.

## Open Questions (RESOLVED)

1. **ReloadPhaseC naming** — RESOLVED: Rename to `ReloadPhaseFfi` for clarity (addressed in Plan 06-02 Task 4)
   - What we know: Intentional FFI-safe variant per ABI-06
   - What's unclear: Should it be renamed to `ReloadPhaseFfi` for consistency?
   - Recommendation: Rename to `ReloadPhaseFfi` to clarify purpose, not "C suffix"

2. **Generated code regeneration scope** — RESOLVED: Run polyplugc on all example bundles after codegen updates (addressed in Plan 06-04 Task 4)
   - What we know: Examples have generated code with old naming
   - What's unclear: Which bundles need regeneration?
   - Recommendation: Run polyplugc on all example bundles after codegen updates

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_abi/src/lib.rs` - Legacy alias definitions verified
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` - Canonical GuestContractInterface
- `crates/polyplug_abi/src/host/runtime_abi.rs` - Canonical RuntimeAbi
- `.planning/phases/05-sdk-updates/05-08-SUMMARY.md` - RuntimeConfigC rename verified complete

### Secondary (MEDIUM confidence)
- `.planning/ROADMAP.md` - Phase 6 requirements
- `.planning/STATE.md` - Locked decisions
- `.planning/REQUIREMENTS.md` - CLN-01 to CLN-04 definitions

### Tertiary (grep verification)
- `grep -r "vtable"` results - 223 occurrences in source
- `grep -r "PluginInterface"` results - 1800 occurrences
- `grep -r "HostVTable"` results - 163 occurrences
- `grep -r "PluginGuard"` results - 15 occurrences

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Types verified in polyplug_abi
- Architecture: HIGH - Patterns established in Phase 1
- Pitfalls: HIGH - Common cleanup issues known
- Scope: HIGH - grep counts verified

**Research date:** 2026-04-04
**Valid until:** 30 days (stable codebase, no external API changes)

---

*Phase: 06-cleanup*
*Research gathered: 2026-04-04*