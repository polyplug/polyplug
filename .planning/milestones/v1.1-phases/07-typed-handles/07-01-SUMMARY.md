---
plan: 07-01
phase: 07-typed-handles
status: completed
commit: 8eefe46
tasks_completed: 2
tasks_total: 2
---

# Plan 07-01: Create RuntimeContext and VmLoaderData Opaque Handles

## Summary

Created two new opaque handle structs following the established `GuestContractInstance`/`HostContractInstance` pattern:

1. **RuntimeContext** — Opaque handle wrapping `*mut HostContext`, passed to plugins during `polyplug_init`
2. **VmLoaderData** — Opaque handle wrapping VM-specific loader state (Python, Lua, JS)

Both structs use `#[repr(C)]` with a single `data: *mut c_void` field, providing type safety at the FFI boundary without changing runtime behavior.

## Files Created

| File | Purpose |
|------|---------|
| `crates/polyplug_abi/src/host/runtime_context.rs` | RuntimeContext opaque handle |
| `crates/polyplug_abi/src/dispatch/vm_loader_data.rs` | VmLoaderData opaque handle |

## Files Modified

| File | Change |
|------|--------|
| `crates/polyplug_abi/src/host/mod.rs` | Added `pub mod runtime_context` and export |
| `crates/polyplug_abi/src/dispatch/mod.rs` | Added `pub mod vm_loader_data` and export |
| `crates/polyplug_abi/src/lib.rs` | Added RuntimeContext and VmLoaderData to root exports |

## Tests

- `layout_runtime_context` — Passes (size=8, align=8)
- `layout_vm_loader_data` — Passes (size=8, align=8)
- `null_context` — Passes
- `null_loader_data` — Passes

## Verification

- [x] RuntimeContext struct with `#[repr(C)]` and single `data` field
- [x] VmLoaderData struct with `#[repr(C)]` and single `data` field
- [x] Both structs have `null()`/`is_null()` methods
- [x] Both structs have `unsafe impl Send + Sync`
- [x] Both exported from polyplug_abi root
- [x] All 43 polyplug_abi tests pass

## Deviations

None.

## Next Phase Readiness

Ready for Plan 07-02 (Update RuntimeAbi to use RuntimeContext) and Plan 07-03 (Update GuestContractInterface, HostContractInterface, VmDispatch).