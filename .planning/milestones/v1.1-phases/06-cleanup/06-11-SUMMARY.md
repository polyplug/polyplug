---
phase: 06-cleanup
plan: 11
status: completed
created: 2026-04-05T12:30:00Z
completed: 2026-04-05T13:25:00Z
---

# Summary: Export VmDispatch from polyplug_abi and Regenerate

## What Was Done

1. **Exported `VmDispatch`** from `polyplug_abi` crate root
2. **Exported `HostContractId`** from `polyplug_abi` (re-export from `polyplug_utils`)
3. **Fixed closure capture issue** in generated interface factories by using `AtomicPtr` instead of `OnceLock` for thread-safe static storage
4. **Added `Version` import** to generated types.rs

## Files Modified

- `crates/polyplug_abi/src/lib.rs`
- `crates/polyplugc/src/generators/rust.rs`

## Key Technical Decision

Used `AtomicPtr<c_void>` with `Ordering::SeqCst` for storing implementation pointers in generated factories. This allows nested `fn` items to access the static without closure capture issues, and is thread-safe.

## Verification

- `cargo build -p polyplug_abi` passes
- Generated code now compiles without "VmDispatch not found" or "HostContractId not found" errors

## Commit

`63084c8` - feat(polyplug_abi): export VmDispatch and HostContractId from crate root