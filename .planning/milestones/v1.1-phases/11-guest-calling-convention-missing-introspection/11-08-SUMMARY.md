---
phase: 11-guest-calling-convention-missing-introspection
plan: 08
subsystem: polyplugc
tags: [codegen, calling-convention, host-interface]
dependency_graph:
  requires: [11-07]
  provides: [updated-codegen]
  affects: [examples]
tech_stack:
  added: []
  patterns: [self-passing-pattern, host-interface]
key_files:
  created: []
  modified:
    - crates/polyplugc/src/generators/python.rs
    - crates/polyplugc/src/generators/lua.rs
    - crates/polyplugc/src/generators/cpp.rs
    - crates/polyplugc/src/generators/rust.rs
decisions:
  - Use host parameter instead of rt_ctx in create/destroy instance stubs
  - Use host self-passing pattern in register_contract calls
metrics:
  duration: "45 minutes"
  completed_date: "2026-04-07"
  tasks_completed: 5
  files_modified: 4 generators + 34 example files
---

# Phase 11 Plan 08: Update Codegen Calling Convention Summary

## One-liner
Updated all polyplugc generators to emit new HostInterface self-passing calling convention, removing rt_ctx parameters.

## Changes Made

### Task 1: Python Generator
- Updated `create_instance_stub` and `destroy_instance_stub` signatures
- Changed `rt_ctx: ctypes.c_void_p` to `host: ctypes.c_void_p`
- 4 locations updated in both `generate_guest_contract_vtable` and `generate_guest_plugin_vtable`

### Task 2: Lua Generator
- Updated stub function signatures
- Changed `rt_ctx` to `host` parameter
- 2 locations updated in vtable stub functions

### Task 3: C++ Generator
- Updated `register_contract` calls
- Changed `host->register_contract(rt_ctx, ...)` to `host->register_contract(host, ...)`
- 2 locations updated in init.hpp generation

### Task 4: Rust Generator
- Updated `register_contract` call in else branch
- Changed `(host.register_contract)(rt_ctx, ...)` to `(host.register_contract)(host, ...)`
- 1 location updated (bundle branch was already correct from previous edits)

### Task 5: Example Regeneration
- Regenerated all Rust guest plugins (decoder, encoder, transformer, reporter, validator)
- Regenerated Rust host
- Regenerated Python, Lua, C++ guest plugins
- All generated files now use correct calling convention

## Deviations from Plan

None - plan executed exactly as written.

## Verification

- `grep -r "rt_ctx" crates/polyplugc/src/generators/` returns no matches
- All examples compile with new calling convention
- polyplugc compiles successfully

## Commits

1. `0f5b9bf` - feat(11-08): update Python generator to use host parameter instead of rt_ctx
2. `529bcd1` - feat(11-08): update Lua generator to use host parameter instead of rt_ctx
3. `2e43b6e` - feat(11-08): update C++ generator to use host self-passing pattern
4. `e72d59f` - feat(11-08): update Rust generator to use host self-passing pattern
5. Regenerated examples (multiple commits)

## Self-Check: PASSED

- [x] All generators updated (python.rs, lua.rs, cpp.rs, rust.rs)
- [x] No rt_ctx references in generator code
- [x] Examples regenerated with correct output
- [x] polyplugc compiles