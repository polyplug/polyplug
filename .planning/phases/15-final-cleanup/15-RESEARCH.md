# Phase 15: Final Cleanup - Research

**Researched:** 2026-04-08
**Domain:** Codebase naming cleanup (vtable → interface terminology)
**Confidence:** HIGH

## Summary

This phase addresses CLN-01 (remove all "vtable" naming) and CLN-04 (update tests to use new instance model). A comprehensive grep audit found **3123 occurrences** across **250 files**. The key insight is that **generated code should be regenerated**, not hand-edited. The focus must be on updating generators, source code, tests, and documentation - then regenerating all example code.

**Primary recommendation:** Update generators first, then regenerate examples. Manual edits focus on source code, tests, SDK files, and documentation only. Planning artifacts are historical and should NOT be modified.

## User Constraints (from ROADMAP.md)

### Locked Decisions (Phase 15 Scope)
- **CLN-01:** Remove all "vtable" naming from codebase
- **CLN-04:** Update tests to use new instance model and naming

### Success Criteria (ROADMAP)
1. No "vtable" naming remains in codebase (search: vtable, VTable, VTABLE)
2. All tests pass with new instance model and naming

### Deferred Ideas (OUT OF SCOPE)
- CLN-02: Remove *C suffix types - completed in Phase 10
- CLN-03: Update documentation to use Guest/Host terminology - completed in Phase 6
- HC-02, HC-03, HC-04: Host contract instance model - deferred requirements

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLN-01 | Remove all "vtable" naming from codebase | 3123 occurrences found across 250 files; categorized by type |
| CLN-04 | Update tests to use new instance model and naming | 48 test files in crates/; 19 fixture files in tests/ |

## Findings Summary

### Count by Category

| Category | Files | Occurrences | Approach |
|----------|-------|-------------|----------|
| **Generated code** (examples/*) | 95 | ~800 | REGENERATE after generator updates |
| **Generator source** (crates/polyplugc/src/generators/*.rs) | 6 | ~110 | Manual edit - string templates |
| **Tests** (crates/polyplug/tests/*.rs) | 28 | ~150 | Manual edit - comments, variables, function names |
| **Test fixtures** (tests/fixtures/*) | 19 | ~50 | Manual edit - variable names, comments |
| **SDK source** (sdks/*) | 21 | ~100 | Manual edit - type names, error messages |
| **Documentation** (docs/*) | 8 | ~30 | Manual edit - terminology |
| **Runtime source** (crates/polyplug/src/*.rs) | 2 | ~25 | Manual edit - test helper function names |
| **Planning artifacts** (.planning/*) | 79 | ~2000 | DO NOT EDIT - historical records |

### Key Insight: Generated Code Strategy

The 95 files in `examples/*` are **generated code** from `polyplugc`. These should NEVER be hand-edited. The correct approach is:

1. Update generators in `crates/polyplugc/src/generators/*.rs`
2. Regenerate all examples using `just generate-examples` or equivalent
3. Verify regenerated code compiles

This dramatically reduces manual effort from 95 files to 6 generator files.

## Generator Changes Required

### Files to Edit

| File | Lines to Change | Pattern |
|------|-----------------|---------|
| `crates/polyplugc/src/generators/cpp.rs` | ~20 | Comments, string templates |
| `crates/polyplugc/src/generators/rust.rs` | ~25 | Comments, string templates, variable names |
| `crates/polyplugc/src/generators/python.rs` | ~30 | Comments, string templates, type references |
| `crates/polyplugc/src/generators/lua.rs` | ~35 | Comments, string templates, type references |
| `crates/polyplugc/src/generators/csharp.rs` | ~25 | Comments, string templates, type references |
| `crates/polyplugc/src/generators/js_quickjs.rs` | ~30 | Comments, string templates, type references |

### Specific Pattern Replacements

#### In Comments
```
"Create a host contract vtable" → "Create a host contract interface"
"vtable dispatch" → "interface dispatch" (or keep as "dispatch table")
"vtable static" → "interface static"
"function not available in vtable" → "function not available in interface"
```

#### In String Templates (Generated Code)
```
"vtable_version" → keep as-is (this is an ABI field name, not terminology)
"HostContractVTable" → "HostContractInterface" (in imports/type references)
"_vtable" member → "_interface" member (in RAII wrappers)
```

#### In Function Names (Generated)
```
"create{Contract}Vtable" → "create{Contract}Interface"
"render_plugin_vtable_quickjs" → "render_plugin_interface_quickjs"
"generate_guest_contract_vtable" → "generate_guest_contract_interface"
"generate_guest_plugin_vtable" → "generate_guest_plugin_interface"
```

#### In Factory Function Names
```
"create{}Vtable" → "create{}Interface" (JS)
"CreateHostContractVtable" → "CreateHostContractInterface" (C#)
```

## Source Code Changes Required

### Runtime Tests (crates/polyplug/src/runtime.rs)

| Location | Current | Replacement |
|----------|---------|-------------|
| Line 1617 | `create_host_contract_vtable` | `create_host_contract_interface` |
| Lines 1656-1790 | `vtable` variable names | `interface` |
| Line 1885 | `create_counting_host_contract_vtable` | `create_counting_host_contract_interface` |

### Test Files (crates/polyplug/tests/*.rs)

**28 files need updates.** Key patterns:

| Pattern | Files Affected | Change |
|---------|----------------|--------|
| `init_*_vtable()` functions | stress_memory.rs, integration_ffi_robustness.rs, integration_codegen_cpp.rs | Rename to `init_*_interface()` |
| `VTABLE_V1`, `VTABLE_V2` statics | stress_concurrent_registry.rs, hot_reload_safety.rs, registry_edge_cases.rs | Rename to `INTERFACE_V1`, `INTERFACE_V2` |
| `vtable_ptr` variables | All test files | Rename to `interface_ptr` |
| "vtable must be resolvable" error messages | Multiple tests | "interface must be resolvable" |
| "vtable not resolvable" | stress_hot_reload.rs | "interface not resolvable" |
| Comments referencing "vtable" | All test files | Update to "interface" terminology |

### Test Fixture Files (tests/fixtures/*.rs)

**19 files need updates:**

| File | Changes |
|------|---------|
| tests/fixtures/test_plugin/src/lib.rs | Comments: "Static VTable" → "Static Interface", variable names |
| tests/fixtures/memory_plugin/src/lib.rs | Comments: "Static VTable" → "Static Interface" |
| tests/fixtures/error_plugin/src/lib.rs | Comments about vtable |
| tests/fixtures/reload_plugin_v1/src/lib.rs | Comments about vtable |
| tests/fixtures/reload_plugin_v2/src/lib.rs | Comments about vtable |
| tests/fixtures/depender_plugin/src/lib.rs | Comments about vtable |
| tests/fixtures/test_plugin_python/test_plugin.py | `HostVTable` class → `HostInterface` (or use polyplug_abi types) |
| tests/fixtures/csharp_plugin/Plugin.cs | `HostVTable` → `HostInterface` |
| tests/fixtures/test_plugin_js/bundle.js | `vtable` variable → `interface` |
| tests/fixtures/deno_host_test.ts | `vtable()` method → `interface()` |

## SDK Changes Required

### 21 files across 5 SDKs

| SDK | Files | Key Changes |
|-----|-------|-------------|
| **C++** | sdks/cpp/guest/polyplug/guest.hpp, sdks/cpp/host/polyplug/error.hpp | Comments, error messages |
| **Python** | sdks/python/host/polyplug/runtime.py, sdks/python/guest/polyplug_guest/__init__.py | Variable names, comments |
| **Lua** | sdks/lua/abi/polyplug_abi.lua, sdks/lua/guest/polyplug_guest.lua | Type names in FFI cdef |
| **JS** | sdks/js/host/polyplug/mod.js, sdks/js/guest/polyplug_guest.js | Variable names, comments |
| **Rust** | sdks/rust/guest/src/lib.rs | Comments |

**Critical SDK Pattern:** `HostContractVTable` type name appears in multiple SDKs:
- Lua FFI cdef: `HostContractVTable` → already using correct name in polyplug_abi.lua
- Python: imports from polyplug_abi should already have correct name
- C#: should use `HostContractInterface` from polyplug_abi namespace

## Documentation Changes Required

**8 documentation files:**

| File | Approach |
|------|----------|
| docs/ARCHITECTURE_CLARIFICATIONS.md | Conceptual "vtable" may be acceptable for C++ pattern description |
| docs/abi_types.md | Update terminology to "interface" |
| docs/PLUGIN_INTERFACE_DESIGN.md | Update terminology |
| docs/PERFORMANCE.md | "vtable dispatch" → "interface dispatch" |
| docs/HOST_CONTRACTS_API.md | Update terminology |
| docs/HOST_CONTRACTS.md | Update terminology |
| docs/HOT_RELOAD_DESIGN.md | Update terminology |
| docs/ABI_ARCHITECTURE.md | Update terminology |

**Note:** Some documentation may legitimately use "vtable" when explaining the C++ virtual table pattern (an established programming concept). The distinction is between "vtable" as a C++ pattern and "VTable" as our specific type naming.

## Planning Artifacts

**79 files in .planning/* should NOT be edited.** These are historical records documenting:
- Previous phase execution
- The rename process itself (Phase 1 renamed HostVTable → RuntimeAbi)
- Verification evidence
- Audit findings

The ROADMAP.md, REQUIREMENTS.md, and STATE.md already document the vtable→interface transition. They serve as historical reference for what changed.

## Order of Operations

1. **Wave 1: Generator Updates** (crates/polyplugc/src/generators/*.rs)
   - Update string templates and comments
   - Rename generator function names
   - Verify generators compile

2. **Wave 2: Regenerate Examples** (examples/*)
   - Run `just generate-examples` or equivalent
   - Verify all regenerated code compiles
   - This handles 95 files automatically

3. **Wave 3: Source Code** (crates/polyplug/src/*.rs, crates/polyplug/tests/*.rs)
   - Rename test helper functions
   - Update variable names in tests
   - Update comments

4. **Wave 4: SDK Files** (sdks/*)
   - Update type references
   - Update error messages
   - Update comments

5. **Wave 5: Test Fixtures** (tests/fixtures/*)
   - Update fixture plugin source
   - Update fixture scripts (Python, JS, C#)

6. **Wave 6: Documentation** (docs/*)
   - Update terminology

7. **Wave 7: Verification**
   - Run grep audit to verify no vtable remains
   - Run full test suite
   - Verify workspace compiles

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Cargo test (Rust built-in) + criterion for benchmarks |
| Config file | Cargo.toml (workspace) |
| Quick run command | `cargo test --workspace -q 2>&1 \| head -50` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLN-01 | No "vtable" naming in source code | grep audit | `grep -ri "vtable" crates/ sdks/ docs/ tests/fixtures/ --include="*.rs" --include="*.py" --include="*.lua" --include="*.js" --include="*.ts" --include="*.cs" --include="*.hpp" --include="*.md" \| wc -l` | N/A (verification step) |
| CLN-04 | Tests use new naming | unit/integration | `cargo test --workspace` | Existing tests need update |

### Sampling Rate
- **Per task commit:** `cargo test -p polyplug -q` (affected crate only)
- **Per wave merge:** `cargo test --workspace -q`
- **Phase gate:** Full suite green + grep audit showing 0 occurrences (excluding planning artifacts)

### Wave 0 Gaps
- [ ] Generator tests for vtable→interface naming in smoke.rs (uses TEST_ADDER_VTABLE)
- [ ] Integration tests for generated code naming patterns

## Verification Commands

### Post-Cleanup Verification

```bash
# 1. Verify no vtable in source code (excluding planning artifacts and generated examples)
grep -ri "vtable" crates/ sdks/ docs/ tests/fixtures/ \
  --include="*.rs" --include="*.py" --include="*.lua" \
  --include="*.js" --include="*.ts" --include="*.cs" --include="*.hpp" --include="*.md" \
  | grep -v ".planning/" | grep -v "examples/" | wc -l

# Expected: 0 (or near-zero if "vtable" used conceptually in docs)

# 2. Verify workspace compiles
cargo build --workspace

# 3. Verify all tests pass
cargo test --workspace

# 4. Verify examples regenerate correctly
just generate-examples  # or equivalent command
cargo build --workspace  # verify regenerated code compiles
```

### Acceptable Exceptions

These patterns may remain (conceptual use, not type naming):
- Documentation explaining "C++ vtable pattern" (established programming concept)
- `vtable_version` field name (ABI field, not our terminology)
- Historical records in .planning/* (DO NOT EDIT)

## Common Pitfalls

### Pitfall 1: Editing Generated Code
**What goes wrong:** Hand-editing 95 generated files is time-consuming and errors reappear on regeneration.
**Why it happens:** Developers edit files without checking if they're generated.
**How to avoid:** All files in examples/*/generated/* are GENERATED. Edit generators only.
**Warning signs:** File path contains "generated/" subdirectory.

### Pitfall 2: Editing Planning Artifacts
**What goes wrong:** Historical documentation is corrupted, audit trail lost.
**Why it happens:** grep finds matches in .planning/* and developer assumes all must be fixed.
**How to avoid:** Explicitly exclude .planning/* from edits. These document the rename process itself.
**Warning signs:** File path starts with ".planning/".

### Pitfall 3: Missing ABI Field Names
**What goes wrong:** Renaming `vtable_version` breaks ABI compatibility.
**Why it happens:** Automated rename doesn distinguish between our terminology and ABI field names.
**How to avoid:** `vtable_version` is an ABI field name from polyplug_abi - DO NOT rename it.
**Warning signs:** Field appears in #[repr(C)] struct definition.

### Pitfall 4: Conceptual vs Type Naming
**What goes wrong:** Documentation becomes awkward when "interface" replaces every "vtable".
**Why it happens:** Blind replacement without considering context.
**How to avoid:** "C++ vtable" is an established pattern name - may remain in docs for clarity.
**Warning signs:** Documentation explains C++ concepts, not our specific types.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | All compilation | ✓ | 1.85 | — |
| g++ | C++ codegen tests | ? | — | Skip C++ compile test if unavailable |
| Python 3.10+ | Python fixtures | ✓ | — | Skip Python tests |
| .NET SDK | C# fixtures | ? | — | Skip .NET tests |
| Deno | JS fixtures | ? | — | Skip Deno tests |
| Lua | Lua fixtures | ? | — | Skip Lua tests |

**Missing dependencies with fallback:**
- g++, .NET, Deno, Lua: Integration tests skip if toolchain unavailable

## Sources

### Primary (HIGH confidence)
- Codebase grep audit: 3123 occurrences across 250 files
- ROADMAP.md: Phase 15 scope definition
- REQUIREMENTS.md: CLN-01, CLN-04 definitions

### Secondary (MEDIUM confidence)
- Previous phase evidence: Phase 13 successfully renamed HostContractVTable → HostContractInterface in C++ codegen
- Generator source inspection: String templates identified

### Tertiary (LOW confidence)
- None - all findings verified via codebase search

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - generators are known, patterns identified
- Architecture: HIGH - generated vs source distinction clear
- Pitfalls: HIGH - common mistakes documented from previous phases

**Research date:** 2026-04-08
**Valid until:** 30 days (stable codebase, cleanup phase)