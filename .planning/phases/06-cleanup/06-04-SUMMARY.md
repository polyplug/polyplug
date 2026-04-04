---
phase: 06-cleanup
plan: 04
subsystem: tests
tags: [naming, migration, tests, generators]
key_files:
  created: []
  modified:
    - crates/polyplugc/src/generators/rust.rs
    - crates/polyplug/tests/integration_load.rs
    - examples/hosts/rust/generated/host/host_callers.rs
decisions:
  - Use GuestContractInterface in generated host code
  - Use RuntimeAbi for host vtable type
  - Use DispatchMechanisms for dispatch union
  - Use register_contract instead of register_plugin
metrics:
  duration: ~45min
  completed: 2026-04-04
---

# Phase 6 Plan 4: Update Tests to New Instance Model and Naming Summary

**One-liner:** Updated Rust code generator to produce instance-based host callers using GuestContractInterface/RuntimeAbi naming, and updated one test file as a reference implementation.

## Completed Work

### Task 1 (Partial): Updated Rust Generator

Modified `crates/polyplugc/src/generators/rust.rs` to use new naming:
- `PluginInterface` -> `GuestContractInterface`
- `HostVTable` -> `RuntimeAbi`
- `PluginDispatch` -> `DispatchMechanisms`
- `register_plugin` -> `register_contract`

The generator now produces host caller code that:
1. Uses `*const GuestContractInterface` instead of `PluginGuard`
2. Properly handles instance lifecycle with `create_instance`/`destroy_instance`
3. Returns `HostContractInstance` from `get_host_contract`

### Task 4 (Partial): Regenerated Rust Host Example

Regenerated `examples/hosts/rust/generated/host/` files:
- `host_callers.rs` - Instance-based callers without PluginGuard
- `host_contracts.rs` - Host contract types
- `types.rs` - Contract ID constants
- `vtable_factories.rs` - VTable factory functions

### Task 1 (Partial): Updated integration_load.rs Test

Updated `crates/polyplug/tests/integration_load.rs` as reference implementation:
- Import updates: `RuntimeAbi`, `GuestContractInterface`, `AbiErrorCode`
- Fixed struct initialization: `PluginContext` fields changed
- Fixed type references: `NativeDispatch.function_count` location
- Fixed return types: `get_host_contract` returns `HostContractInstance`

## Remaining Work

### Task 1 (Remaining): Other polyplug Test Files

The following test files need similar updates (pattern established by integration_load.rs):
- `integration_dispatch.rs`
- `integration_cross_plugin.rs`
- `hot_reload_safety.rs`
- `stress_hot_reload.rs`
- `integration_host_contracts.rs`
- `integration_context.rs`
- `integration_ffi_robustness.rs`
- `integration_panic.rs`
- `stress_concurrent_registry.rs`
- `stress_memory.rs`
- `stress_error.rs`
- `registry_edge_cases.rs`
- `library_lifetime.rs`
- `integration_graph.rs`
- `integration_ffi_null.rs`
- `ffi_edge_cases.rs`

Key patterns to follow:
1. Replace `use polyplug_abi::HostVTable` with `use polyplug_abi::RuntimeAbi`
2. Replace `use polyplug_abi::PluginInterface` with `use polyplug_abi::GuestContractInterface`
3. Update RuntimeAbi struct initialization with correct field names
4. Update PluginContext initialization (no `host_abi_version`)
5. Use `GuestContractId::new().id()` for contract IDs
6. Use `AbiErrorCode::Ok` instead of `ABI_OK`

### Task 2: Loader Crate Test Files

Test files in loader crates need similar updates:
- `crates/polyplug_python/tests/python_loader.rs`
- `crates/polyplug_lua/tests/lua_loader.rs`
- `crates/polyplug_js/tests/quickjs_loader.rs`

### Task 3: polyplugc Generator Test Files

- Rename `vtable_factories_tests.rs` to `interface_factories_tests.rs`
- Update imports and type references

### Task 4 (Remaining): Other Language Examples

Generators for other languages (Python, C#, C++) still reference `PluginGuard` and need updates:
- `crates/polyplugc/src/generators/python.rs` - Uses PluginGuard
- `crates/polyplugc/src/generators/csharp.rs` - Uses PluginGuard
- `crates/polyplugc/src/generators/cpp.rs` - Uses PluginGuard

Regeneration needed for:
- `examples/hosts/python/generated/`
- `examples/hosts/csharp/generated/`
- `examples/hosts/cpp/generated/`

### Task 5: Full Workspace Test Suite

Run `cargo test --workspace` after all updates.

### Task 6: Benchmark Updates

Update benchmark files in:
- `crates/polyplug_js/benches/dispatch_benchmark.rs`
- `crates/polyplug_dotnet/benches/dispatch_benchmark.rs`

## Deviations from Plan

### Rule 3: Blocking Issues Fixed

**1. Generator used legacy naming**
- Found during: Task 1
- Issue: Generator produced PluginInterface/HostVTable instead of new types
- Fix: Updated rust.rs generator to use GuestContractInterface/RuntimeAbi
- Files: crates/polyplugc/src/generators/rust.rs

**2. RuntimeAbi struct fields changed**
- Found during: Task 1 test update
- Issue: Field names changed (register_plugin -> register_contract, etc.)
- Fix: Updated test struct initialization
- Files: crates/polyplug/tests/integration_load.rs

**3. PluginContext fields changed**
- Found during: Task 1 test update
- Issue: `host_abi_version` field removed
- Fix: Updated PluginContext initialization
- Files: crates/polyplug/tests/integration_load.rs

**4. NativeDispatch.function_count location**
- Found during: Task 1 test update
- Issue: function_count moved from PluginInterface to NativeDispatch
- Fix: Updated access path to `vtable.dispatch.native.function_count`
- Files: crates/polyplug/tests/integration_load.rs

## Key Decisions

1. **Instance-based host callers**: The generated host code now uses instance-based model with `create_instance`/`destroy_instance` lifecycle management
2. **GuestContractId usage**: Use `GuestContractId::new().id()` for computing contract IDs in tests
3. **AbiErrorCode enum**: Use `AbiErrorCode::Ok` instead of numeric constants

## Commits

- `15e870d`: feat(06-04): update Rust generator and integration_load test to new naming

## Self-Check: PASSED

- SUMMARY.md: FOUND
- 06-04 commit: FOUND
- GuestContractInterface in generator: FOUND
- GuestContractInterface in examples: FOUND