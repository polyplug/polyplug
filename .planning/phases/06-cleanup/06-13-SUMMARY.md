---
phase: 06-cleanup
plan: 13
status: partial
created: 2026-04-05T12:30:00Z
completed: null
---

# Summary: Update Test Files to Use New Type Names (Partial)

## What Was Done

**Generator fixes committed:**
- Changed `ContractError.code` and `HostContractError.code` from `u32` to `AbiErrorCode`
- Removed unnecessary `as u32` casts for AbiErrorCode values
- Fixed `polyplug_runtime_resolve_plugin` call to cast `rt_ctx` to `*const _`
- Added `UpperHex`/`LowerHex` impl to `BundleId` for formatting

## Remaining Issues

**In generated code:**
1. `VmDispatch.call` signature mismatch - function now requires `GuestContractInstance` parameter
2. `NativeLoader::new()` now takes 2 arguments (needs `RuntimeAbi`)
3. `AbiErrorCode` doesn't implement `Display` for error formatting

**In test files (many files across crates):**
- `polyplug_abi::GuestContractInterface` → `polyplug_abi::GuestContractInterface`
- `polyplug_abi::HostInterface` → `polyplug_abi::RuntimeAbi`
- `polyplug_abi::HostContractVTable` → `polyplug_abi::HostContractInterface`
- `polyplug_abi::ABI_OK` → removed (use `abi_error_ok()`)
- `polyplug_abi::bundle_id()` → `polyplug_utils::bundle_id()`
- `polyplug_abi::contract_id()` → `polyplug_utils::guest_contract_id()`
- `NativeDispatch` now requires `function_count` field
- `PluginDescriptor` uses `version: Version` instead of separate fields
- `GuestContractHandle` no longer has `generation` field

## Commits

`ad9b9a8` - fix(polyplugc): fix remaining ABI type issues in generated code

## Next Steps

1. Fix `VmDispatch.call` signature in generator
2. Update test files to use new type names
3. Add `Display` impl to `AbiErrorCode` or use `.code as u32` in format strings