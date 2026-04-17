---
phase: 06-cleanup
plan: 12
status: completed
created: 2026-04-05T12:30:00Z
completed: 2026-04-05T13:35:00Z
---

# Summary: Update Host Example to Use New Interface Names

## What Was Done

1. **Updated imports**: Added `HostContractInterface`, `RuntimeConfig` to imports
2. **Updated factory call**: Changed `create_host_logger_vtable` to `create_host_logger_interface`
3. **Updated type**: Changed `HostContractVTable` to `HostContractInterface`
4. **Fixed scanner call**: Changed `scan_dir` to `scan_dirs` with correct argument format
5. **Fixed `RuntimeConfig` construction**: Used struct literal instead of removed builder methods
6. **Added `UpperHex`/`LowerHex` impl** to `BundleId` for `{:016X}` formatting

## Files Modified

- `examples/hosts/rust/src/main.rs`
- `crates/polyplug_utils/src/bundle_id.rs`

## Remaining Issues

3 type mismatches remain in generated `host_callers.rs`:
- `polyplug_runtime_resolve_plugin` expects `*const OpaqueRuntime`, generated code passes `*mut c_void`
- Two `AbiErrorCode` vs `u32` mismatches

These are issues in the generator that need to be fixed.

## Commit

`c1236ee` - fix(examples): update host example to use new interface names