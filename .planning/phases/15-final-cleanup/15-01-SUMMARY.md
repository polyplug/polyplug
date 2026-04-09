---
phase: 15-final-cleanup
plan: 01
subsystem: polyplugc
tags: [terminology, cleanup, generators]
dependency_graph:
  requires: []
  provides: [interface-terminology-in-generators]
  affects: [generated-code-output]
tech_stack:
  added: []
  patterns: [string-template-replacement]
key_files:
  created: []
  modified:
    - crates/polyplugc/src/generators/cpp.rs
    - crates/polyplugc/src/generators/rust.rs
    - crates/polyplugc/src/generators/python.rs
    - crates/polyplugc/src/generators/lua.rs
    - crates/polyplugc/src/generators/csharp.rs
    - crates/polyplugc/src/generators/js_quickjs.rs
decisions:
  - Preserve ABI field names (vtable_version, vtableVersion)
  - Preserve SDK function names (store_host_vtable, get_host_vtable)
  - Preserve Rust fat pointer vtable terminology (Rust language concept)
  - Update all generated code variable/parameter/member names to use "interface"
metrics:
  duration: 15m
  tasks_completed: 3
  files_modified: 6
  commits: 6
  lines_changed: 294
  completed_date: 2026-04-09
---

# Phase 15 Plan 01: Generator Terminology Update Summary

## One-liner

Updated all 6 polyplugc code generators to use "interface" terminology instead of "vtable" in comments, variable names, and string templates, eliminating ~800 occurrences in future generated code.

## What Changed

### Generators Updated

| Generator | File | Key Changes |
|-----------|------|-------------|
| C++ | cpp.rs | Module doc, error messages, comments |
| Rust | rust.rs | Function names, doc comments, variable names, SAFETY comments |
| Python | python.rs | Function names, doc comments, variable/member names |
| Lua | lua.rs | Comments, parameter names, doc comments, variable names |
| C# | csharp.rs | Field names, parameter names, doc comments, variable names |
| JavaScript | js_quickjs.rs | Field names, parameter names, doc comments, SAFETY comments |

### Terminology Mapping

| Old Term | New Term | Context |
|----------|----------|---------|
| vtable dispatch | interface dispatch | Comments |
| vtable statics | interface statics | Comments |
| _vtable | _interface | Private member names |
| vtable | interface | Variable/parameter names |
| vtablePtr | interfacePtr | Pointer variable names |
| "function not available in vtable" | "function not available in interface" | Error messages |
| "Register all plugin vtables" | "Register all plugin interfaces" | Doc comments |
| "Create a host contract vtable" | "Create a host contract interface" | Factory doc comments |

### Preserved Terms

The following were intentionally NOT changed:

1. **ABI Field Names**: `vtable_version`, `vtableVersion` - These are FFI field names in the ABI structs
2. **SDK Function Names**: `store_host_vtable`, `get_host_vtable` - These are FFI function names in SDKs
3. **ABI Struct Names**: `HostContractVTable`, `GuestContractVTable` - These are ABI struct names
4. **Rust Fat Pointer Terminology**: Comment about Rust trait object vtables (line 2174 rust.rs) - This is Rust language terminology

## Verification

- All generators compile successfully
- No unexpected vtable terminology remains (only preserved ABI terms)

## Commits

1. `9fd7fd3` - cpp.rs: module doc, comment, error messages
2. `f22da11` - rust.rs: function names, doc comments, variable names, SAFETY comments
3. `09902cf` - python.rs: function names, doc comments, variable/member names
4. `ab0cceb` - lua.rs: comments, parameter names, doc comments, variable names
5. `a61c98c` - csharp.rs: field names, parameter names, doc comments, variable names
6. `cd6a38e` - js_quickjs.rs: field names, parameter names, doc comments, SAFETY comments

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- All 6 generator files exist and compile: VERIFIED
- All 6 commits exist in git history: VERIFIED
- No unexpected vtable terminology remains: VERIFIED