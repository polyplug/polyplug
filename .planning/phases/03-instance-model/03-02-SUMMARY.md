---
phase: 03-instance-model
plan: 02
subsystem: codegen
tags: [instance-model, codegen, guest-vtables, lifecycle]
duration: 45
completed: "2026-04-04T12:56:00Z"
key_decisions:
  - "Guest vtables include create_instance/destroy_instance stub functions"
  - "Instance parameter passed as first argument to dispatch wrappers"
  - "Stub functions return null instance / no-op for stateless plugins"
  - "Generated code matches GuestContractInterface ABI structure"
files_created: []
files_modified:
  - crates/polyplugc/src/generators/rust.rs
  - crates/polyplugc/src/generators/csharp.rs
  - crates/polyplugc/src/generators/python.rs
  - crates/polyplugc/src/generators/lua.rs
  - crates/polyplugc/src/generators/cpp.rs
  - crates/polyplugc/src/generators/js_quickjs.rs
  - sdks/rust/guest/src/lib.rs
commits:
  - hash: 8f1704c
    message: "feat(03-02): add instance lifecycle to Rust guest vtable generation"
  - hash: 9ad2d6e
    message: "feat(03-02): add instance lifecycle to all guest vtable generators"
---

# Phase 03 Plan 02: Guest VTable Instance Lifecycle Summary

## One-liner

Updated all 6 code generators to produce guest vtables with create_instance/destroy_instance stubs and dispatch signatures that include instance parameter.

## What Changed

### Rust Generator (`rust.rs`)
- Added `GuestContractInstance` import to generated code
- Added `create_instance` stub function returning `GuestContractInstance::null()`
- Added `destroy_instance` stub function as no-op
- Updated vtable structure to use `GuestContractInterface` fields (removed `rt_ctx`, `function_count`; added `create_instance`, `destroy_instance`)
- Updated dispatch wrapper signature: `fn(instance: GuestContractInstance, args: *const (), out: *mut ()) -> AbiError`

### C# Generator (`csharp.rs`)
- Added `GuestContractInstance` parameter to ABI wrapper methods
- Added `CreateInstanceStub` and `DestroyInstanceStub` functions with `[UnmanagedCallersOnly]` attribute
- Updated vtable construction to include `CreateInstance` and `DestroyInstance` function pointers
- Updated function pointer delegate signatures to include `GuestContractInstance`

### Python Generator (`python.rs`)
- Added `_GuestContractInstance` parameter to ABI wrapper functions
- Added `create_instance_stub` and `destroy_instance_stub` functions
- Added `_CREATE_INSTANCE_FN_CTYPE` and `_DESTROY_INSTANCE_FN_CTYPE` callback types
- Updated `PluginInterface` construction to use `dispatch.native.function_count` and `dispatch.native.functions`

### Lua Generator (`lua.rs`)
- Added `create_instance_stub` and `destroy_instance_stub` functions
- Updated vtable fields to include `create_instance` and `destroy_instance`
- Updated `set_X_impl` function to set `dispatch.native.function_count` and `dispatch.native.functions`

### C++ Generator (`cpp.rs`)
- Added `GuestContractInstance` parameter to ABI wrapper functions
- Added `create_instance_stub` and `destroy_instance_stub` static functions
- Updated `PluginInterface` structure construction
- Updated `NativeDispatch` to include `function_count` field

### JS/QuickJS Generator (`js_quickjs.rs`)
- Added `instanceDataLo`, `instanceDataHi` parameters to ABI wrapper functions
- Added `createInstance` and `destroyInstance` stub functions in vtable object
- Updated vtable structure with instance lifecycle methods

### polyplug_guest SDK (`sdks/rust/guest/src/lib.rs`)
- Exported `GuestContractInstance` from `polyplug_abi::guest`

## Verification Results

| Check | Expected | Actual | Status |
|-------|----------|--------|--------|
| `create_instance` in rust.rs | >= 2 | 10 | PASS |
| `destroy_instance` in rust.rs | >= 2 | 8 | PASS |
| `GuestContractInstance` in generators | >= 6 | 40 | PASS |
| `cargo check -p polyplugc -p polyplug_guest` | exit 0 | exit 0 | PASS |

## Deviations from Plan

None - plan executed exactly as written.

## Next Steps

The following plans will build on this work:
- **03-03**: Update host contract implementations for singleton support
- **03-04**: Implement `get_host_contract` in runtime
- **03-05**: Implement `call_method` for cross-dispatch calls