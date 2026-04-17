---
phase: 03-instance-model
plan: 04
subsystem: codegen
tags: [codegen, rust-generator, instance-wrapper, raii]
dependency_graph:
  requires: [03-02]
  provides: [host-callers-with-instance-wrappers]
  affects: [polyplugc-rust-generator]
tech_stack:
  added: [GuestContractInstance, GuestContractHandle.pack()]
  patterns: [RAII-instance-wrapper, create/destroy-factory]
key_files:
  created: []
  modified:
    - path: crates/polyplugc/src/generators/rust.rs
      change: "Generated host callers now use instance-based RAII pattern"
    - path: crates/polyplug_abi/src/plugin/plugin_handle.rs
      change: "Added pack() method for FFI calls"
decisions:
  - "Use rt_ctx: *mut c_void in wrapper for create/destroy calls"
  - "Import polyplug_runtime_resolve_plugin from polyplug::ffi"
  - "Pass instance as first arg to native dispatch, second arg to VM dispatch"
metrics:
  duration: 261s
  tasks: 4
  files: 2
  completed_date: "2026-04-04"
---

# Phase 03 Plan 04: Instance Wrapper Codegen Summary

## One-Liner

Generated RAII instance wrappers for host-side contract callers with create_instance/destroy_instance lifecycle management, replacing the PluginGuard-based pattern.

## Changes Made

### Task 1: Update host caller struct to use instance wrapper

**Files:** crates/polyplugc/src/generators/rust.rs

**Changes:**
- Removed `use polyplug::registry::PluginGuard;` import
- Removed `use polyplug::runtime::Runtime;` import
- Added `use polyplug_abi::GuestContractInstance;` import
- Added `use polyplug::ffi::polyplug_runtime_resolve_plugin;` import
- Changed struct fields from `guard: PluginGuard` to:
  - `interface: *const GuestContractInterface`
  - `instance: GuestContractInstance`
  - `rt_ctx: *mut core::ffi::c_void`
- Updated struct doc comment to reflect instance model

### Task 2: Update new() method to call create_instance

**Files:** crates/polyplugc/src/generators/rust.rs

**Changes:**
- Updated new() signature: `pub fn new(handle: GuestContractHandle, rt_ctx: *mut core::ffi::c_void) -> Option<Self>`
- Calls `polyplug_runtime_resolve_plugin(rt_ctx, handle.pack())` to get interface
- Calls `((*interface).create_instance)(rt_ctx, core::ptr::null())` to create instance
- Returns None on null interface or null instance
- Added GuestContractHandle.pack() method to polyplug_abi for FFI compatibility

### Task 3: Add Drop impl to call destroy_instance

**Files:** crates/polyplugc/src/generators/rust.rs, crates/polyplug_abi/src/plugin/plugin_handle.rs

**Changes:**
- Added Drop impl generation after struct impl block
- Drop impl calls `((*self.interface).destroy_instance)(self.rt_ctx, self.instance)` if instance is non-null
- Updated is_valid() to check `!self.instance.data.is_null()`
- Updated reset() to destroy current instance and create new one

### Task 4: Update dispatch to pass instance parameter

**Files:** crates/polyplugc/src/generators/rust.rs

**Changes:**
- Changed vtable access from `self.guard.vtable()` to `unsafe { &*self.interface }`
- Native dispatch signature updated to `fn(GuestContractInstance, *const (), *mut ()) -> AbiError`
- Native dispatch now passes `self.instance` as first argument
- VM dispatch now passes instance as second argument (after loader_data)
- Changed function_count check from `vtable.function_count` to `vtable.dispatch.native.function_count`

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

All success criteria passed:

| Check | Expected | Actual |
|-------|----------|--------|
| `instance: GuestContractInstance` count | >= 2 | 5 |
| `impl Drop for` count | >= 1 | 1 |
| `create_instance` count | >= 3 | 17 |
| `destroy_instance` count | >= 2 | 12 |
| `self.instance` count | >= 4 | 8 |
| `cargo check -p polyplugc` | exits 0 | PASSED |

## Generated Code Pattern

The generated host caller now follows this pattern:

```rust
pub struct TestAddContract {
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
    rt_ctx: *mut core::ffi::c_void,
}

impl TestAddContract {
    pub fn new(handle: GuestContractHandle, rt_ctx: *mut core::ffi::c_void) -> Option<Self> {
        let interface = unsafe { polyplug_runtime_resolve_plugin(rt_ctx, handle.pack()) };
        if interface.is_null() { return None; }
        let instance = unsafe { ((*interface).create_instance)(rt_ctx, core::ptr::null()) };
        if instance.data.is_null() { return None; }
        Some(Self { interface, instance, rt_ctx })
    }

    pub fn is_valid(&self) -> bool { !self.instance.data.is_null() }

    pub fn reset(&mut self) {
        if !self.instance.data.is_null() {
            unsafe { ((*self.interface).destroy_instance)(self.rt_ctx, self.instance); }
        }
        self.instance = unsafe { ((*self.interface).create_instance)(self.rt_ctx, core::ptr::null()) };
    }

    pub fn add(&self, a: i32, b: i32) -> Result<i32, ContractError> {
        // dispatch with instance as first arg (native) or second arg (VM)
    }
}

impl Drop for TestAddContract {
    fn drop(&mut self) {
        if !self.instance.data.is_null() {
            unsafe { ((*self.interface).destroy_instance)(self.rt_ctx, self.instance); }
        }
    }
}
```

## Key Decisions

1. **rt_ctx storage**: Wrapper stores `rt_ctx: *mut c_void` to pass to create/destroy_instance calls
2. **FFI import**: Import `polyplug_runtime_resolve_plugin` from `polyplug::ffi` (not polyplug_abi)
3. **Instance dispatch**: Native dispatch passes instance as first arg, VM dispatch as second (after loader_data)

## Self-Check: PASSED

- Created files exist: N/A (no new files)
- Commit exists: 911b14b verified in git log