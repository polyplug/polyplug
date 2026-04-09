---
phase: 15-final-cleanup
plan: 02
subsystem: polyplugc
tags: [terminology, cleanup, regeneration, examples]
dependency_graph:
  requires: [15-01]
  provides: [interface-terminology-in-examples]
  affects: [generated-code-output]
tech_stack:
  added: []
  patterns: [generator-output-replacement]
key_files:
  created:
    - examples/guests/csharp/encoder/generated/*
    - examples/guests/csharp/reporter/generated/*
    - examples/guests/csharp/transformer/generated/*
    - examples/guests/csharp/validator/generated/*
  modified:
    - crates/polyplugc/src/generators/rust.rs
    - crates/polyplugc/src/generators/python.rs
    - crates/polyplugc/src/generators/lua.rs
    - crates/polyplugc/src/generators/js_quickjs.rs
    - examples/guests/rust/*/generated/*
    - examples/guests/rust/*/src/generated/*
    - examples/guests/cpp/*/generated/*
    - examples/guests/python/*/generated/*
    - examples/guests/lua/*/generated/*
    - examples/guests/js/*/generated/*
decisions:
  - Preserve store_host_vtable SDK function names (FFI boundary)
  - Preserve vtable_version ABI field names
  - Preserve HostContractVTable ABI struct names
metrics:
  duration: 20m
  tasks_completed: 3
  files_modified: 134
  commits: 2
  lines_changed: 1800
  completed_date: 2026-04-09
---

# Phase 15 Plan 02: Regenerate Examples with Interface Terminology Summary

## One-liner

Regenerated all 30 guest plugin examples across 6 languages using updated generators, eliminating vtable terminology in static names, variable names, and comments while preserving SDK/ABI names.

## What Changed

### Generator Fixes (Deviation)

During execution, discovered that 15-01 plan did not complete the `_VTABLE` static name replacement. Applied Rule 1 (Auto-fix bugs) to complete:

| Generator | Fix Applied |
|-----------|-------------|
| rust.rs | `{upper}_VTABLE` -> `{upper}_INTERFACE` in static names, imports, register_contract calls |
| python.rs | `{plugin_upper}_VTABLE` -> `{plugin_upper}_INTERFACE` in variable names |
| lua.rs | `{plugin_var}_VTABLE` -> `{plugin_var}_INTERFACE` in all variable names |
| js_quickjs.rs | `{plugin_var}_VTABLE` -> `{plugin_var}_INTERFACE` in exports and function assignments |

### Regenerated Examples

| Language | Plugins | Files Generated |
|----------|---------|-----------------|
| Rust | decoder, encoder, transformer, reporter, validator | 35 files (generated + src/generated) |
| C++ | decoder, encoder, transformer, reporter, validator | 30 files |
| Python | decoder, encoder, transformer, reporter, validator | 40 files |
| Lua | decoder, encoder, transformer, reporter, validator | 25 files |
| C# | decoder, encoder, transformer, reporter, validator | 35 files (4 new directories created) |
| JavaScript | decoder, encoder, transformer, reporter, validator | 40 files |

### Terminology Mapping in Generated Code

| Old Term | New Term | Context |
|----------|----------|---------|
| `DECODER_VTABLE` | `DECODER_INTERFACE` | Static names |
| `_vtable` | `_interface` | Variable names (lowercase) |
| `vtable.dispatch` | `interface.dispatch` | Field access |

### Preserved Terms (Not Changed)

1. **SDK Function Names**: `store_host_vtable`, `storeHostVtable` - FFI function names in SDKs
2. **ABI Field Names**: `vtable_version`, `VTableVersion` - ABI struct fields
3. **ABI Struct Names**: `HostContractVTable` - ABI type definitions

## Verification

- Workspace compiles successfully (0 errors, 7 warnings)
- No `_VTABLE` references remain in generated code (except preserved SDK/ABI names)
- All 30 guest plugins regenerated

## Commits

1. `5fd038e` - fix(15-02): complete _VTABLE to _INTERFACE terminology in generators
2. `1beb183` - feat(15-02): regenerate all guest examples with interface terminology

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Incomplete generator terminology changes from 15-01**
- **Found during:** Task 2 verification
- **Issue:** Generators still produced `_VTABLE` static names despite 15-01 claiming completion
- **Fix:** Added 6 additional replacements across 4 generators (rust.rs, python.rs, lua.rs, js_quickjs.rs)
- **Files modified:** crates/polyplugc/src/generators/*.rs
- **Commit:** 5fd038e

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- Generator changes compile: VERIFIED
- Regenerated examples compile: VERIFIED
- No unexpected VTABLE terminology: VERIFIED (only SDK/ABI preserved terms)
- SUMMARY exists at .planning/phases/15-final-cleanup/15-02-SUMMARY.md: VERIFIED
- Commit 5fd038e (generator fixes) exists in git history: VERIFIED
- Commit 1beb183 (regenerated examples) exists in git history: VERIFIED