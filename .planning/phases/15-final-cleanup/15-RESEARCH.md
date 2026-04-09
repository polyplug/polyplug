# Phase 15: Final Cleanup - Research

**Researched:** 2026-04-08
**Domain:** Codebase naming cleanup (vtable -> interface terminology)
**Confidence:** HIGH

## Summary

This phase addresses CLN-01 (remove all "vtable" naming) and CLN-04 (update tests to use new instance model). A comprehensive grep audit found vtable references across the codebase. The critical insight is that **generated code should be regenerated, not hand-edited**. The focus must be on updating generators, source code, tests, and SDKs - then regenerating all example code.

**Primary recommendation:** Update generators first, then regenerate examples. Manual edits focus on source code, tests, SDK files, and documentation only. Planning artifacts are historical and should NOT be modified.

<user_constraints>
## User Constraints (from CONTEXT.md / ROADMAP.md)

### Locked Decisions (Phase 15 Scope)
- **CLN-01:** Remove all "vtable" naming from codebase (excluding ABI fields and planning artifacts)
- **CLN-04:** Update tests to use new instance model and naming

### Success Criteria
1. No "vtable" naming remains in codebase (excluding ABI fields and planning artifacts)
2. All tests use new instance model and naming

### Deferred Ideas (OUT OF SCOPE)
- CLN-02: Remove *C suffix types - completed in Phase 10
- CLN-03: Update documentation to use Guest/Host terminology - completed in Phase 6
- HC-02, HC-03, HC-04: Host contract instance model - deferred requirements
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLN-01 | Remove all "vtable" naming from codebase | Categorized findings: generated code regenerates, source/tests/SDKs need manual updates |
| CLN-04 | Update tests to use new instance model and naming | Test files identified with specific variable/function name changes |
</phase_requirements>

## Findings Summary

### Count by Category (Excluding Planning Artifacts)

| Category | Files | Occurrences | Approach |
|----------|-------|-------------|----------|
| **Generated code** (examples/*/generated/) | ~95 | ~800 | REGENERATE after generator updates |
| **Generator source** (crates/polyplugc/src/generators/*.rs) | 6 | ~110 | Manual edit - string templates |
| **Tests** (crates/polyplug/tests/*.rs) | 28 | ~150 | Manual edit - comments, variables, function names |
| **Integration tests** (tests/integration/tests/*.rs) | 8 | ~250 | Manual edit - variable names, comments |
| **Test fixtures** (tests/fixtures/*) | 19 | ~50 | Manual edit - variable names, comments |
| **SDK source** (sdks/*) | 21 | ~100 | Manual edit - type names, error messages |
| **Documentation** (*.md in root) | 5 | ~30 | Manual edit - terminology |
| **C++ SDK/examples** (*.hpp, *.cpp) | 15 | ~100 | Mix of SDK + generated |
| **Planning artifacts** (.planning/*) | 79 | ~2000 | DO NOT EDIT - historical records |

### Critical Exclusions (Must Remain Unchanged)

1. **`vtable_version` ABI field** - This is an FFI field name in `HostContractVTableHeader`, not our terminology. DO NOT rename.

2. **Planning artifacts (.planning/*)** - Historical records documenting the rename process itself. DO NOT EDIT.

3. **FFI function names** - Functions like `store_host_vtable`, `get_host_vtable`, `host_vtable_storage` are FFI boundary names that may need to remain for API compatibility or be renamed consistently across all SDKs.

## Generator Changes Required

### Files to Edit (Wave 1)

| File | Key Changes |
|------|-------------|
| `crates/polyplugc/src/generators/cpp.rs` | Comments: "vtable dispatch" -> "interface dispatch"; string templates |
| `crates/polyplugc/src/generators/rust.rs` | Variable names, string templates |
| `crates/polyplugc/src/generators/python.rs` | Type references, string templates |
| `crates/polyplugc/src/generators/lua.rs` | FFI cdef comments, string templates |
| `crates/polyplugc/src/generators/csharp.rs` | Type references, string templates |
| `crates/polyplugc/src/generators/js_quickjs.ts` | Variable names, string templates |

### Specific Pattern Replacements in Generators

**Comments (change):**
- "Create a host contract vtable" -> "Create a host contract interface"
- "function not available in vtable" -> "function not available in interface"
- "Store host vtable" -> "Store host interface"

**String templates (generated code):**
- `vtable_version` -> KEEP AS-IS (ABI field name)
- `HostContractVTable` type -> May need to stay (check polyplug_abi definitions)
- `_vtable` member -> `_interface` member

**Function names in generators:**
- `render_plugin_vtable_quickjs` -> `render_plugin_interface_quickjs`
- Variables named `vtable` -> `interface`

## Source Code Changes Required

### Integration Tests (tests/integration/tests/*.rs)

**Key file: cross_language.rs (~250 occurrences)**

| Pattern | Lines | Change |
|---------|-------|--------|
| `host_vtable: HostInterface` | Multiple | Rename to `host_interface` |
| `capture_vtable_cb` | ~109 | Rename to `capture_interface_cb` |
| `CAPTURED_VT` thread-local | ~114 | Rename to `CAPTURED_INTERFACE` |
| `vtable_ptr` variables | Throughout | Rename to `interface_ptr` |
| `get_vtable_from_runtime()` | ~195 | Rename to `get_interface_from_runtime()` |
| `dispatch_add_and_verify(vtable_ptr)` | ~207 | Update parameter name |
| Comments about "vtable" | Throughout | Update terminology |

**Other integration test files:**
- `integration_reload.rs`: `vtable` variables -> `interface`
- `integration_hot_reload_notification.rs`: Test names reference "vtable_swap"
- `integration_dotnet.rs`: `get_vtable()` helper -> `get_interface()`
- `integration_lua.rs`: `get_vtable()` helper -> `get_interface()`
- `integration_ffi_native.rs`: Comments about vtable

### Unit Tests (crates/polyplug/tests/*.rs)

**Files needing updates:**
- `stress_error.rs`: `init_*_vtable()` functions
- `stress_memory.rs`: `init_memory_plugin_vtable()` -> `init_memory_plugin_interface()`
- `stress_hot_reload.rs`: `VTABLE_MEM_A`, `VTABLE_MEM_B` statics -> `INTERFACE_*`
- `stress_concurrent_registry.rs`: `VTABLES_V1` array -> `INTERFACES_V1`
- `registry_edge_cases.rs`: `VTABLE_A`, `VTABLE_B`, `VTABLE_C` -> `INTERFACE_*`
- `integration_panic.rs`: `CAPTURED_VTABLE_PTR` -> `CAPTURED_INTERFACE_PTR`
- `integration_load.rs`: `test_init_registers_vtable()` -> `test_init_registers_interface()`

### Test Fixtures (tests/fixtures/*)

| File | Changes |
|------|---------|
| `test_plugin/src/lib.rs` | Comments about vtable |
| `memory_plugin/src/lib.rs` | Comments about vtable |
| `error_plugin/src/lib.rs` | Comments about vtable |
| `test_plugin_python/test_plugin.py` | `HostInterface` class, `_VTABLE` variable |
| `csharp_plugin/Plugin.cs` | `HostInterface` type reference |
| `deno_host_test.ts` | `vtable()` method call |

## SDK Changes Required

### C++ SDK (sdks/cpp/)

| File | Changes |
|------|---------|
| `guest/polyplug/guest.hpp` | `store_host_vtable()` -> `store_host_interface()`, `get_host_vtable()` -> `get_host_interface()` |
| `host/polyplug/error.hpp` | Comment about "vtable dispatch" |

**Note:** `using HostInterface = RuntimeAbi;` is an alias for backwards compatibility - may keep or remove.

### Python SDK (sdks/python/)

| File | Changes |
|------|---------|
| `polyplug_abi/polyplug_abi/abi.py` | `HostInterface` class name (check if this is from polyplug_abi crate) |
| `guest/polyplug_guest/__init__.py` | `store_host_vtable()`, `get_host_vtable()` functions |
| `host/polyplug/runtime.py` | Comments about "vtable swap" |

### Lua SDK (sdks/lua/)

| File | Changes |
|------|---------|
| `abi/polyplug_abi.lua` | `HostInterface` in FFI cdef |
| `guest/polyplug_guest.lua` | `store_host_vtable()`, `get_host_vtable()`, `cast_host_vtable()` |
| `host/polyplug/reload_phase.lua` | Comments about "Before vtable swap" |

### JS SDK (sdks/js/)

| File | Changes |
|------|---------|
| Generated code uses `vtable` variables | Will be regenerated |

## Documentation Changes Required

| File | Changes |
|------|---------|
| `CLAUDE.md` | Multiple references to vtable in code examples and architecture docs |
| `TRUST_MODEL.md` | References to "vtable" in security model |
| `BENCHMARKS.md` | "vtable dispatch" -> "interface dispatch" |
| `AGENTS.md` | References to HostInterface |
| `SUMMARY.md` | Historical notes about VTableSlot |

**Note:** Some documentation may legitimately use "vtable" when explaining the C++ virtual table pattern (an established programming concept).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Cargo test (Rust built-in) |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test --workspace -q 2>&1 | head -50` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLN-01 | No "vtable" naming in source code | grep audit | See verification commands below | N/A |
| CLN-04 | Tests use new naming | unit/integration | `cargo test --workspace` | Existing tests |

### Sampling Rate
- **Per task commit:** `cargo test -p <crate> -q` (affected crate only)
- **Per wave merge:** `cargo test --workspace -q`
- **Phase gate:** Full suite green + grep audit clean

### Wave 0 Gaps
- [ ] Generator test `smoke.rs` uses `TEST_ADDER_VTABLE` - needs update
- [ ] Integration test `integration_codegen_cpp.rs` has assertions about `_VTABLE` suffix
- [ ] Generator test `interface_factories_tests.rs` uses "vtable" terminology

## Verification Commands

### Post-Cleanup Verification

```bash
# 1. Verify no vtable in source code (excluding planning artifacts and generated examples)
grep -ri "vtable" crates/ sdks/ docs/ tests/fixtures/ tests/integration/ \
  --include="*.rs" --include="*.py" --include="*.lua" \
  --include="*.js" --include="*.ts" --include="*.cs" --include="*.hpp" --include="*.md" \
  | grep -v ".planning/" | grep -v "examples/" | grep -v "vtable_version" | wc -l
# Expected: 0

# 2. Verify workspace compiles
cargo build --workspace

# 3. Verify all tests pass
cargo test --workspace

# 4. Regenerate examples and verify
cd examples && ./build.sh
```

### Acceptable Exceptions

These patterns may remain:
- `vtable_version` field name (ABI field in HostContractVTableHeader)
- Historical records in .planning/* (DO NOT EDIT)
- Documentation explaining "C++ vtable pattern" (established concept)

## Common Pitfalls

### Pitfall 1: Editing Generated Code
**What goes wrong:** Hand-editing 95 generated files is time-consuming and errors reappear on regeneration.
**How to avoid:** All files in `examples/*/generated/` are GENERATED. Edit generators only.

### Pitfall 2: Editing Planning Artifacts
**What goes wrong:** Historical documentation is corrupted, audit trail lost.
**How to avoid:** Explicitly exclude `.planning/*` from edits.

### Pitfall 3: Renaming ABI Field Names
**What goes wrong:** Renaming `vtable_version` breaks ABI compatibility.
**How to avoid:** `vtable_version` is an ABI field name - DO NOT rename.

### Pitfall 4: Inconsistent FFI Function Renames
**What goes wrong:** Renaming `store_host_vtable` in one SDK but not others causes API inconsistency.
**How to avoid:** Rename FFI functions consistently across ALL SDKs or keep as-is.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All compilation | Yes | 1.85 | - |
| g++ | C++ codegen tests | Check | - | Skip C++ compile test |
| Python 3.10+ | Python fixtures | Yes | - | Skip Python tests |
| .NET SDK | C# fixtures | Check | - | Skip .NET tests |
| Deno | JS fixtures | Check | - | Skip Deno tests |
| Lua | Lua fixtures | Check | - | Skip Lua tests |

## Sources

### Primary (HIGH confidence)
- Codebase grep audit: Comprehensive search across all source files
- ROADMAP.md: Phase 15 scope definition
- REQUIREMENTS.md: CLN-01, CLN-04 definitions
- examples/build.sh: Regeneration strategy confirmed

### Secondary (MEDIUM confidence)
- Previous phase evidence: Phase 13 renamed HostContractVTable -> HostContractInterface in C++ codegen
- Generator source inspection: String templates identified

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - generators known, patterns identified
- Architecture: HIGH - generated vs source distinction clear
- Pitfalls: HIGH - documented from previous phases

**Research date:** 2026-04-08
**Valid until:** 30 days (stable codebase, cleanup phase)