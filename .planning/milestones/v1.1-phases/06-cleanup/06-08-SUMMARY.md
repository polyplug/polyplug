---
phase: 06-cleanup
plan: 08
completed: 2026-04-05T16:00:00Z
status: completed
---

# Plan 06-08: Rename Test File and Update Test Imports

## Summary

Renamed test file and updated all test imports to use correct type names (GuestContractInterface, RuntimeAbi) instead of removed aliases.

## Changes Made

### Test File Rename
- `vtable_factories_tests.rs` -> `interface_factories_tests.rs`

### polyplugc Tests
- `smoke.rs`: Updated imports, PluginDescriptor, GuestContractInterface, GuestContractHandle, renamed CAPTURED_VTABLE -> CAPTURED_INTERFACE
- `integration_codegen_rust.rs`: Same updates as smoke.rs
- `generator_correctness.rs`: Changed contract_id function reference
- `integration_host_contracts.rs`: Updated type references

### polyplug Tests (26 files)
All test files updated with:
- Import updates: `GuestContractInterface` -> `GuestContractInterface`, `HostInterface` -> `RuntimeAbi`
- PluginDescriptor: `version: Version { major, minor, patch }`
- GuestContractInterface: Removed `rt_ctx`, added `create_instance`/`destroy_instance`
- GuestContractHandle: Removed `generation` field
- PluginContext: Removed `host_abi_version` field
- NativeDispatch: Added `function_count` field
- Error codes: `AbiErrorCode::*` enum instead of `ABI_*` constants
- Module paths: `plugin_registry` -> `registry::plugin_registry`

### Generator Test Fixtures
Added missing `singleton: false` field to all `ResolvedHostContract` initializers in:
- cpp.rs, csharp.rs, js_quickjs.rs, lua.rs, rust.rs

## Verification

- `cargo build -p polyplugc -p polyplug --tests` succeeds
- No `GuestContractInterface` or `HostInterface` imports remain (except in historical comments)
- Test file successfully renamed

## Commit

`64b74e6` - test: update all test files to use new ABI types