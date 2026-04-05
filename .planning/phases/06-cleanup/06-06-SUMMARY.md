---
phase: 06-cleanup
plan: 06
completed: 2026-04-05T12:00:00Z
status: completed
---

# Plan 06-06: Fix Generator ABI Structure Templates

## Summary

Updated all 6 generator templates to produce code matching the current ABI structure, fixing compilation errors in generated code.

## Changes Made

### PluginDescriptor Version Field
All generators now output `version: Version { major, minor, patch }` instead of separate `version_major/minor/patch` fields.

### Error Code Handling
All generators now use `AbiErrorCode` enum values instead of `ABI_*` u32 constants:
- `AbiErrorCode::Ok` instead of `ABI_OK`
- `AbiErrorCode::Generic` instead of `ABI_ERROR_GENERIC`
- `AbiErrorCode::Panic` instead of `ABI_ERROR_PANIC`

### Registration Function
All generators now use `register_contract` instead of `register_plugin`.

### GuestContractInterface Fields
All generators now correctly output:
- `contract_id: GuestContractId` (or u64 with proper casting)
- `contract_version: Version { major, minor, patch }`
- `dispatch_type: DispatchType`
- `create_instance` and `destroy_instance` function pointers
- `dispatch: DispatchMechanisms`

Removed incorrect fields:
- `rt_ctx` field removed (doesn't exist on the struct)
- `function_count` moved to `dispatch.native.function_count`

### Generator-Specific Changes

**Rust Generator:**
- Added `Version`, `GuestContractId`, `AbiErrorCode` imports to generated code
- Uses `GuestContractId::from_u64()` for contract_id initialization
- Added `Version` export to `sdks/rust/guest/src/lib.rs`

**C# Generator:**
- Uses `Version` struct with `Major/Minor/Patch` properties
- Uses `RegisterContract` method on `RuntimeAbi`
- Fixed `FunctionCount` to use `Dispatch.Native.FunctionCount`

**C++ Generator:**
- Uses `Version` struct with `static_cast<uint32_t>(AbiErrorCode::*)`
- Fixed `contract_version` to use `Version` struct instead of packed u32

**Python Generator:**
- Uses `Version(major=X, minor=Y, patch=Z)` constructor
- Uses `AbiErrorCode.Ok` / `AbiErrorCode.Generic`

**Lua Generator:**
- Uses `version.major/minor/patch` nested field assignment
- Fixed `contract_version` to use Version struct fields

**JS Generator:**
- Uses `version: { major: X, minor: Y, patch: Z }` object
- Uses `register_contract` function

## Verification

All acceptance criteria passed:
- Generated init files use `version: Version { major, minor, patch }`
- Generated code uses `register_contract`
- Generated interfaces have no `rt_ctx` field
- Generated code uses `AbiErrorCode::*`
- `cargo build -p polyplugc` exits with code 0

## Commit

`4cdfcf6` - feat(polyplugc): rename generators to interface terminology, fix ABI templates