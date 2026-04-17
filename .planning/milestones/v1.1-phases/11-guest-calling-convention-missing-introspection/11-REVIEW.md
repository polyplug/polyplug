---
phase: 11-guest-calling-convention-missing-introspection
reviewed: 2026-04-07T10:30:00Z
depth: standard
files_reviewed: 13
files_reviewed_list:
  - crates/polyplug_abi/src/host/runtime_interface.rs
  - crates/polyplug_abi/src/host/host_interface.rs
  - crates/polyplug_abi/src/host/mod.rs
  - crates/polyplug_abi/src/lib.rs
  - crates/polyplug/src/runtime.rs
  - crates/polyplug/src/runtime_builder.rs
  - crates/polyplug_abi/src/guest/guest_contract_interface.rs
  - crates/polyplug_abi/src/host/host_contract_interface.rs
  - crates/polyplug_native/src/loader.rs
  - crates/polyplug/src/registry/plugin_registry.rs
  - crates/polyplug_abi/src/types/array.rs
  - crates/polyplug_abi/src/guest/guest_contract_instance.rs
  - crates/polyplug_abi/src/types/dependency_info.rs
findings:
  critical: 2
  warning: 4
  info: 3
  total: 9
status: issues_found
---

# Phase 11: Code Review Report

**Reviewed:** 2026-04-07T10:30:00Z
**Depth:** standard
**Files Reviewed:** 13
**Status:** issues_found

## Summary

Reviewed 13 source files focusing on FFI safety, struct layout correctness, thread safety, documentation completeness, and API consistency. Found 2 critical issues, 4 warnings, and 3 info-level observations.

The codebase demonstrates strong FFI design patterns with comprehensive `#[repr(C)]` annotations, detailed safety documentation, and proper thread synchronization. However, there are significant issues with runtime pointer initialization, memory leaks in context pointer creation, and broken test imports.

## Critical Issues

### CR-01: Null Runtime Pointer in HostInterface Passed to Plugins

**File:** `crates/polyplug/src/runtime_builder.rs:106`
**Issue:** The `HostInterface` created in `RuntimeBuilder::build()` has `runtime: std::ptr::null_mut()`, but the FFI callbacks in `runtime.rs` dereference this pointer without checking for null. This would cause a null pointer dereference crash when a plugin calls any HostInterface callback.

**Evidence:**
- `runtime_builder.rs:106`: `runtime: std::ptr::null_mut()` is set on the leaked HostInterface
- `runtime.rs:632`: `let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };` dereferences without null check
- The comment at line 104 says "For now, it's null - the callbacks extract runtime from RuntimeContext" but RuntimeContext is not defined anywhere in the codebase

**Fix:**
```rust
// Option A: Set runtime pointer after Runtime is created
// This requires using interior mutability or a two-phase init

// Option B: Pass Runtime pointer directly to loaders instead of HostInterface
// Change loader.rs:126-127 to use as_context_ptr() which creates a HostInterface with runtime set

let host_ctx: *const HostInterface = runtime.as_context_ptr();
let init_result: AbiError = unsafe { init_fn_ptr(host_ctx, &ctx) };
// Note: as_context_ptr() has its own leak issue (see CR-02)
```

### CR-02: Memory Leak in as_context_ptr Method

**File:** `crates/polyplug/src/runtime.rs:249-272`
**Issue:** Every call to `as_context_ptr()` creates a new `HostInterface` via `Box::new()` and leaks it with `Box::into_raw()`. The comment acknowledges "This is a small leak (72 bytes per call)" but this accumulates on repeated calls. The method should either cache the pointer or use a different approach.

**Fix:**
```rust
// Option A: Cache the HostInterface in Runtime struct
pub struct Runtime {
    // Add cached HostInterface with runtime pointer set
    host_abi_with_runtime: OnceLock<Box<HostInterface>>,
    // ... other fields
}

pub fn as_context_ptr(&self) -> *const HostInterface {
    self.host_abi_with_runtime.get_or_init(|| {
        Box::new(HostInterface {
            runtime: self as *const Runtime as *mut core::ffi::c_void,
            // ... other fields
        })
    }).as_ref() as *const HostInterface
}

// Option B: Return the existing host_abi if runtime pointer is already set
// This requires fixing RuntimeBuilder to set runtime pointer properly
```

## Warnings

### WR-01: Missing catch_unwind Around Host Callbacks

**File:** `crates/polyplug/src/runtime.rs:609-1030`
**Issue:** The host callback functions (`host_find_by_contract`, `host_alloc`, `host_free`, `host_get_host_contract`, etc.) call registry and runtime methods without `catch_unwind` protection. The doc comment at line 612 states "All FFI entry points wrapped in catch_unwind" but this is not implemented for these callbacks. A panic in registry code would cross the FFI boundary and cause undefined behavior.

**Fix:**
```rust
pub(crate) unsafe extern "C" fn host_find_by_contract(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> GuestContractHandle {
    std::panic::catch_unwind(|| {
        // existing implementation
    }).unwrap_or_else(|_| GuestContractHandle::null())
}
```

### WR-02: Missing Null Check Before Interface Pointer Dereference

**File:** `crates/polyplug/src/registry/plugin_registry.rs:160`
**Issue:** The unsafe `register` function dereferences `(*interface_ptr)` without first validating that the pointer is non-null. While the function signature requires a valid pointer, explicit null checking prevents crashes from accidentally passed null pointers.

**Fix:**
```rust
pub unsafe fn register(
    &self,
    descriptor: PluginDescriptor,
    interface_ptr: *const GuestContractInterface,
    contract_name: String,
    bundle_id: BundleId,
) -> Result<GuestContractHandle, RegistryError> {
    if interface_ptr.is_null() {
        return Err(RegistryError::InvalidHandle { index: 0 }); // or specific error
    }
    let contract_id: GuestContractId = unsafe { (*interface_ptr).contract_id };
    // ... rest of implementation
}
```

### WR-03: Version Comparison Encoding Mismatch

**File:** `crates/polyplug/src/runtime.rs:872` vs `crates/polyplug_abi/src/host/host_contract_interface.rs:66`
**Issue:** The version comparison in `host_get_host_contract` callback uses `(interface.contract_version.major << 16) | interface.contract_version.minor` (encoding minor version), but `HostContractInterface` documentation and other places use only `contract_version.major`. This encoding inconsistency could cause incorrect version matching.

**Fix:**
```rust
// Standardize on one encoding approach. If min_version is (major << 16) | minor:
// Update HostContractInterface documentation to clarify encoding

// Or if min_version is just major:
// Fix runtime.rs:872 to use just major:
if iface.contract_id.id() == contract_id &&
    iface.contract_version.major >= min_version  // not: (min_version >> 16)
```

### WR-04: Potential Use-After-Free in NativeLoader::reload

**File:** `crates/polyplug_native/src/loader.rs:258-263`
**Issue:** The old library is dropped (dlclose) immediately after hot-reload without waiting for hosts to destroy cached function pointers. The comment states "SAFETY CONTRACT: Host must not have cached raw function pointers!" but there's no enforcement mechanism. A host that hasn't properly destroyed instances would get SIGSEGV.

**Fix:**
```rust
// Add a quiescence check before dropping old library
// This is mentioned in documentation but not implemented in the loader

// Consider adding:
if let Some(old_library) = self.libraries.lock().unwrap().remove(&bundle_id) {
    // SAFETY: on_reload_cb has already fired, giving host chance to clean up
    // In production, consider a synchronization mechanism to ensure cleanup
    drop(old_library);
}
```

## Info

### IN-01: Duplicate Type Alias ContractHandle

**File:** `crates/polyplug_abi/src/host/runtime_interface.rs:39` and `crates/polyplug_abi/src/host/host_interface.rs:39`
**Issue:** Both files define `pub type ContractHandle = GuestContractHandle` with documentation saying "Will be replaced with ContractHandle in Phase 2." This is confusing - it's already named ContractHandle but says it will be replaced with itself. The documentation should clarify the transition plan or remove the alias.

**Fix:**
```rust
// Update documentation:
/// Type alias for backward compatibility during transition.
/// GuestContractHandle will be renamed to ContractHandle in Phase 2.
/// Currently GuestContractHandle == ContractHandle.
pub type ContractHandle = GuestContractHandle;
```

### IN-02: Missing Debug Implementations on FFI Structs

**File:** `crates/polyplug_abi/src/host/host_interface.rs`, `runtime_interface.rs`, etc.
**Issue:** HostInterface, RuntimeInterface, HostContractInterface don't implement Debug. While these are FFI structs, implementing Debug (at least for non-pointer fields) would aid debugging and testing.

**Fix:**
```rust
#[repr(C)]
#[derive(Debug)] // Add Debug where feasible
pub struct HostInterface {
    // Note: function pointers don't implement Debug, so manual impl needed
}
```

### IN-03: Missing Documentation for min_version Encoding

**File:** Multiple files
**Issue:** The `min_version` parameter in callbacks is a `u32` but the encoding is inconsistent across the codebase. Some code compares against `major`, others use `(major << 16) | minor`. This should be documented in the ABI specification.

**Fix:**
```rust
// Add documentation block in abi.toml or lib.rs:
/// # min_version Encoding
/// The min_version u32 parameter encodes version requirements:
/// - Bits 16-31: major version (min_version >> 16)
/// - Bits 0-15: minor version (min_version & 0xFFFF)
/// Example: min_version = 0x00020003 means major >= 2, minor >= 3
```

---

_Reviewed: 2026-04-07T10:30:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_