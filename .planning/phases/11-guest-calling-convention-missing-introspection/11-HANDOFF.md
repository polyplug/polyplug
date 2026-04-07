---
phase: 11-guest-calling-convention-missing-introspection
status: paused
paused_at: "2026-04-07T16:30:00Z"
completed_waves: [11-01, 11-02, 11-03]
remaining_waves: [11-04, 11-05, 11-06]
---

# Phase 11 Handoff: Guest Calling Convention & Missing Introspection

## Resume Command
```
/gsd-execute-phase 11
```

## Completed Waves

### ✅ Wave 1 (11-01) — Interface Structs
**Commit:** `8e9693f`, `f12b0ae`
- Renamed `RuntimeAbi` → `HostInterface` (72 bytes)
- Created `RuntimeInterface` struct (80 bytes)
- Both have `runtime: *mut c_void` field at offset 0

### ✅ Wave 2 (11-02) — Self-Passing Pattern
**Commit:** `9cba273`
- Deleted `RuntimeContext` and `HostContext` files
- All host callbacks use `this: *const HostInterface` parameter
- Added TLS for bundle_id tracking: `INIT_BUNDLE_ID`
- Updated native loader and test fixtures

### ✅ Wave 3 (11-03) — ABI Types
**Commit:** `98d10df`
- `Array<T>`: items, len, align fields (24 bytes)
- `GuestContractInstance`: added contract_id field (16 bytes)
- `DependencyInfo`: new struct (24 bytes)

## Remaining Waves

### ⏳ Wave 4 (11-04) — Interface Callback Updates
- Update GuestContractInterface callbacks to use `*const HostInterface`
- Update HostContractInterface callbacks to use self-passing pattern
- Update plugin_registry.rs test callbacks

### ⏳ Wave 5 (11-05) — Introspection ABIs
- Add `list_bundles` to HostInterface
- Add `get_dependencies` to HostInterface
- Change `find_all_by_contract` to return `Array<ContractHandle>`

### ⏳ Wave 6 (11-06) — Documentation
- Add first-class documentation to all interface types per D-14

## Known Issues to Fix

The following files still reference removed types and need updates:
- `crates/polyplug_python/src/lib.rs` — RuntimeContext, HostContext
- `crates/polyplug_lua/src/loader.rs` — RuntimeContext, HostContext
- `crates/polyplug_js/src/loader.rs` — RuntimeContext, HostContext
- `crates/polyplug_dotnet/src/lib.rs` — RuntimeContext, HostContext
- Generated code in `examples/guests/*/generated/` — needs regeneration

## Key Patterns Established

### Self-Passing Pattern
```rust
pub(crate) unsafe extern "C" fn host_find_by_contract(
    this: *const HostInterface,
    contract_id: u64,
    min_version: u32,
) -> PluginHandle {
    let runtime: &Runtime = unsafe { &*((*this).runtime as *const Runtime) };
    // ...
}
```

### TLS Bundle ID (for init phase)
```rust
std::thread_local! {
    static INIT_BUNDLE_ID: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}
set_init_bundle_id(bundle_id);
// call polyplug_init
clear_init_bundle_id();
```

## Files Modified This Session
- `crates/polyplug_abi/src/host/host_interface.rs`
- `crates/polyplug_abi/src/host/runtime_interface.rs` (NEW)
- `crates/polyplug_abi/src/host/mod.rs`
- `crates/polyplug_abi/src/host/runtime_context.rs` (DELETED)
- `crates/polyplug_abi/src/host/host_context.rs` (DELETED)
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs`
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs`
- `crates/polyplug_abi/src/host/host_contract_interface.rs`
- `crates/polyplug_abi/src/types/array.rs`
- `crates/polyplug_abi/src/types/dependency_info.rs` (NEW)
- `crates/polyplug_abi/src/types/mod.rs`
- `crates/polyplug_abi/src/lib.rs`
- `crates/polyplug/src/runtime.rs`
- `crates/polyplug/src/runtime_builder.rs`
- `crates/polyplug/src/lib.rs`
- `crates/polyplug/src/registry/plugin_registry.rs`
- `crates/polyplug_native/src/loader.rs`
- `sdks/rust/guest/src/lib.rs`
- `tests/fixtures/*/src/lib.rs` (all 6 fixtures)