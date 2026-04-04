---
phase: 03-instance-model
plan: 05
subsystem: codegen
tags: [codegen, host-contract, singleton, instance-lifecycle]
dependency_graph:
  requires: [03-01, 03-04]
  provides: [host-contract-singleton-generation]
  affects: [polyplugc-generators]
tech_stack:
  added: [singleton-field-generation, create-instance-stub, destroy-instance-stub]
  patterns: [host-contract-factory-with-singleton]
key_files:
  created: []
  modified:
    - path: crates/polyplugc/src/generators/rust.rs
      change: "Updated to use HostContractInterface with singleton and lifecycle stubs"
    - path: crates/polyplugc/src/generators/csharp.rs
      change: "Added singleton field to HostContractVTableHeader"
    - path: crates/polyplugc/src/generators/python.rs
      change: "Added singleton field to HostContractVTableHeader"
    - path: crates/polyplugc/src/generators/lua.rs
      change: "Added singleton field to HostContractVTable header"
    - path: crates/polyplugc/src/generators/cpp.rs
      change: "Added singleton field to HostContractVTableHeader"
    - path: crates/polyplugc/src/generators/js_quickjs.rs
      change: "Added singleton property to host contract vtable"
decisions:
  - "Rust generator uses HostContractInterface directly (not legacy aliases)"
  - "All generators emit singleton field from ResolvedHostContract.singleton"
  - "Instance lifecycle stubs generated for both NATIVE and VM dispatch"
metrics:
  duration: 180s
  tasks: 2
  files: 9
  completed_date: "2026-04-04"
---

# Phase 03 Plan 05: Host Contract Singleton Generation Summary

## One-Liner

Added singleton field support and create/destroy_instance stub generation to all 6 language generators for host contract vtable factories.

## Changes Made

### Task 1: Update Rust host contract factory to include singleton

**Files:** crates/polyplugc/src/generators/rust.rs

**Changes:**
- Changed imports from legacy `HostContractVTable`/`HostContractVTableHeader` to `HostContractInterface`/`HostContractInstance`
- Added `DispatchMechanisms`, `NativeDispatch`, `VmDispatch`, `Version` imports
- Added singleton field extraction: `let singleton: bool = contract.singleton;`
- Added create_instance stub generation for NATIVE dispatch
- Added destroy_instance stub generation for NATIVE dispatch
- Added create_instance stub generation for VM dispatch
- Added destroy_instance stub generation for VM dispatch
- Updated vtable structure to match HostContractInterface:
  - `contract_id: u64`
  - `contract_version: Version { major, minor, patch }`
  - `singleton: bool`
  - `dispatch_type: DispatchType`
  - `create_instance: fn`
  - `destroy_instance: fn`
  - `dispatch: DispatchMechanisms`

### Task 2: Update other generators for host contract singleton

**Files:** crates/polyplugc/src/generators/{csharp,python,lua,cpp,js_quickjs}.rs

**Changes:**
- C# generator: Added `singleton` variable extraction and `Singleton = {singleton}` to header
- Python generator: Added `singleton` variable extraction and `singleton={singleton}` to header
- Lua generator: Added `singleton` variable extraction and `vtable.header.singleton = {singleton}` to header
- C++ generator: Added `singleton` variable extraction and `{},  // singleton` to header
- JS generator: Added `singleton` variable extraction and `singleton: {singleton}` to header

All generators now include singleton field in both NATIVE and VM dispatch factory functions.

## Deviations from Plan

### Architectural Improvement

The Rust generator was updated to use the actual `HostContractInterface` type from polyplug_abi instead of the legacy `HostContractVTable`/`HostContractVTableHeader` types that don't exist in polyplug_abi. This ensures the generated code uses the correct ABI structure (64 bytes, flat layout with singleton and instance lifecycle functions).

The other generators (C#, Python, Lua, C++, JS) continue to use their SDK-defined types (`HostContractVTable`, `HostContractVTableHeader`) which are defined separately in each SDK's FFI layer.

## Verification Results

All success criteria passed:

| Check | Expected | Actual |
|-------|----------|--------|
| `singleton` in rust.rs | >= 2 | 6 |
| `singleton` in csharp.rs | >= 1 | 3 |
| `singleton` in python.rs | >= 1 | 3 |
| `singleton` in lua.rs | >= 1 | 3 |
| `singleton` in cpp.rs | >= 1 | 3 |
| `singleton` in js_quickjs.rs | >= 1 | 3 |
| `cargo check -p polyplugc` | exits 0 | PASSED |

## Generated Code Pattern (Rust)

The generated host contract factory now follows this pattern:

```rust
pub fn create_host_logger_vtable(implementation: Box<dyn HostLogger>) -> &'static HostContractInterface {
    // ... fat pointer handling ...

    // Instance lifecycle stubs
    unsafe extern "C" fn host_logger_create_instance_stub(
        _rt_ctx: *mut c_void,
        _args: *const (),
    ) -> HostContractInstance {
        HostContractInstance { data: impl_ptr as *mut c_void }
    }

    unsafe extern "C" fn host_logger_destroy_instance_stub(
        _rt_ctx: *mut c_void,
        _instance: HostContractInstance,
    ) {
        // Singleton: no cleanup needed
    }

    let vtable: HostContractInterface = HostContractInterface {
        contract_id: 0xF53EB5F2845853BB_u64,
        contract_version: Version { major: 1, minor: 0, patch: 0 },
        singleton: false,
        dispatch_type: DispatchType::Native,
        create_instance: host_logger_create_instance_stub,
        destroy_instance: host_logger_destroy_instance_stub,
        dispatch: DispatchMechanisms {
            native: NativeDispatch { functions: FUNCTIONS.as_ptr() as *const *const () },
        },
    };

    Box::leak(Box::new(vtable))
}
```

## Key Decisions

1. **HostContractInterface vs HostContractVTable**: Rust generator uses the actual `HostContractInterface` from polyplug_abi for correctness
2. **Singleton from IR**: All generators read `contract.singleton` from `ResolvedHostContract`
3. **Instance lifecycle stubs**: Rust generates stubs that return impl_ptr as instance for host contracts
4. **VM dispatch stubs**: Separate create/destroy stubs for VM dispatch that use bridge_data

## Self-Check: PASSED

- Created files exist: N/A (no new files)
- Commit exists: 7ca0c03 verified in git log