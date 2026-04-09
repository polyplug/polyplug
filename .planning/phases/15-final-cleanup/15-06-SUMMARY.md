---
phase: 15-final-cleanup
plan: 06
subsystem: test-fixtures
tags: [terminology, fixtures, interface, cleanup]
dependency_graph:
  requires: [15-02]
  provides: [interface-terminology-in-fixtures]
  affects: [test-fixtures]
tech_stack:
  added: []
  patterns: [interface-terminology]
key_files:
  created: []
  modified:
    - tests/fixtures/test_plugin/src/lib.rs
    - tests/fixtures/memory_plugin/src/lib.rs
    - tests/fixtures/error_plugin/src/lib.rs
    - tests/fixtures/depender_plugin/src/lib.rs
    - tests/fixtures/reload_plugin_v1/src/lib.rs
    - tests/fixtures/reload_plugin_v2/src/lib.rs
    - tests/fixtures/test_plugin_python/test_plugin.py
    - tests/fixtures/test_plugin_js/bundle.js
    - tests/fixtures/deno_host_test.ts
decisions:
  - Preserve ABI type names (HostVTable) in comments where they reference FFI types
  - Preserve FFI parameter names (host_vtable) as they are ABI contract names
  - Rename SDK method calls are kept as-is (guard.vtable()) since they reference SDK API
metrics:
  duration: 6m
  completed_date: 2026-04-09T06:18:51Z
  files_changed: 9
  lines_changed: 36
---

# Phase 15 Plan 06: Test Fixture Interface Terminology Summary

## One-liner

Updated test fixture source files to use interface terminology, replacing vtable references in comments and local variables while preserving ABI type names and FFI parameter names.

## Changes Made

### Task 1: Rust Test Fixture Source Files

Updated 6 Rust fixture files with interface terminology:

- `tests/fixtures/test_plugin/src/lib.rs`: Section headers and comments updated
- `tests/fixtures/memory_plugin/src/lib.rs`: Section headers and comments updated
- `tests/fixtures/error_plugin/src/lib.rs`: Section headers and comments updated
- `tests/fixtures/depender_plugin/src/lib.rs`: FnPtr wrapper comment updated
- `tests/fixtures/reload_plugin_v1/src/lib.rs`: Static variable names (VTABLE → INTERFACE, VTABLE_FNS → INTERFACE_FNS)
- `tests/fixtures/reload_plugin_v2/src/lib.rs`: Static variable names (VTABLE → INTERFACE, VTABLE_FNS → INTERFACE_FNS)

Key changes:
- "Static VTable" → "Static Interface" section headers
- "static vtable array" → "static interface array" in FnPtr wrapper comments
- Contract-specific vtable comments → interface comments
- "NOT registered in the vtable" → "NOT registered in the interface"

### Task 2: Python and JS Test Fixtures

Updated 3 non-Rust fixture files:

- `tests/fixtures/test_plugin_python/test_plugin.py`: "Plugin interface (vtable)" → "Plugin interface", docstring fix
- `tests/fixtures/test_plugin_js/bundle.js`: Local variable `vtable` → `interface`
- `tests/fixtures/deno_host_test.ts`: Test name and variable names updated

Key changes:
- Python: "Plugin interface (vtable)" section header simplified
- Python: "# vtable" type hint comment → "# interface"
- Python: HostVTable docstring clarified as ABI type reference
- JS: Local variable renamed from vtable to interface
- Deno: Test name "guard_vtable_nonnull" → "guard_interface_nonnull"
- Deno: Variable `vt` → `iface`, error message "vtable is null" → "interface is null"

## Verification

### Rust Fixtures

```bash
grep -rn "vtable" tests/fixtures/*/src/lib.rs | grep -v "vtable_version" | wc -l
# Output: 0 (all vtable references removed)
```

### Python/JS Fixtures

```bash
grep -n "vtable" tests/fixtures/test_plugin_python/test_plugin.py tests/fixtures/test_plugin_js/bundle.js tests/fixtures/deno_host_test.ts | grep -v "HostVTable" | grep -v "host_vtable" | wc -l
# Output: 1 (only SDK method call guard.vtable() remains - this is correct)
```

Remaining references preserved:
- `guard.vtable()` - SDK API method (correct - SDK uses this method name)
- `host_vtable` parameter name - FFI contract name (preserved per plan)
- `HostVTable` type name - ABI type name (preserved per plan)

### Build Verification

```bash
cargo build -p test_plugin -p memory_plugin -p error_plugin -p depender_plugin -p reload_plugin_v1 -p reload_plugin_v2
# Output: Finished successfully
```

## Deviations from Plan

### Auto-fixed Issues

None - plan executed exactly as written.

## Threat Flags

None - no new security surface introduced. Changes are purely terminology updates in test fixtures.

## Known Stubs

None - all fixtures are fully functional test code.

## Self-Check: PASSED

All files and commits verified:
- 9 fixture files exist and modified
- Commit 4c6088e exists in git history
- Commit 6d2611b exists in git history

## Commits

| Commit | Description |
|--------|-------------|
| 4c6088e | refactor(15-06): update Rust test fixtures to interface terminology |
| 6d2611b | refactor(15-06): update Python/JS test fixtures to interface terminology |