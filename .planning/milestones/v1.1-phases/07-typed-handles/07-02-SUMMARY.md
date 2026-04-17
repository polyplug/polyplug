---
plan: 07-02
phase: 07-typed-handles
status: completed
commit: 982435f
tasks_completed: 2
tasks_total: 2
---

# Plan 07-02: Update RuntimeAbi to use RuntimeContext

## Summary

Updated all 8 RuntimeAbi function signatures to use `RuntimeContext` instead of `*mut c_void` for the rt_ctx parameter. Updated host callback implementations in runtime.rs to accept RuntimeContext and extract HostContext via `rt_ctx.data`.

## Files Modified

| File | Change |
|------|--------|
| `crates/polyplug_abi/src/host/runtime_abi.rs` | All 8 functions use RuntimeContext |
| `crates/polyplug/src/runtime.rs` | Host callbacks use RuntimeContext |

## Verification

- `cargo build -p polyplug_abi` ✓
- `cargo build -p polyplug` ✓

## Deviations

None.