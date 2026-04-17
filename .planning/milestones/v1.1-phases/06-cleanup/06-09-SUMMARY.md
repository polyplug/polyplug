---
phase: 06-cleanup
plan: 09
completed: 2026-04-05T18:00:00Z
status: partial
---

# Plan 06-09: Regenerate Example Code and Final Verification

## Summary

Regenerated all example code with updated generators. Most tasks completed but some test failures remain.

## Changes Made

### Regenerated Rust Guest Examples
- decoder, encoder, reporter, transformer, validator
- Now use `interfaces.rs` instead of `vtables.rs`
- Uses `Version` struct, `register_contract`, `AbiErrorCode` enum

### Regenerated Python Host Bindings
- `interface_factories.py` generated
- Removed old `vtable_factories.py`

### Generator Fixes Applied
- Fixed host contract callers to use `HostContractInstance` instead of raw vtable pointer
- Updated method bodies to cast `instance.data` to interface pointer

## Verification Status

### Passing
- `cargo build -p polyplugc` succeeds
- Generated files use correct naming (`interfaces.rs`, `interface_factories.py`)
- No `version_major/minor/patch` in generated code
- No `register_plugin` in generated code

### Remaining Issues
- Some test files still have `AbiErrorCode` vs `u32` mismatches
- Some test files have `function_count` missing from `NativeDispatch`
- Some test files have `BundleId` vs `u64` mismatches

## Commits

- `b23cdf7` - feat(examples): regenerate example code with updated generators
- `ad9ea83` - fix(polyplugc): update host contract callers to use HostContractInstance

## Next Steps

Run `cargo test --workspace` and fix remaining test compilation issues:
1. Fix `AbiErrorCode` type mismatches (cast to u32 where needed)
2. Add missing `function_count` fields to `NativeDispatch` initializers
3. Fix `BundleId` vs `u64` type mismatches