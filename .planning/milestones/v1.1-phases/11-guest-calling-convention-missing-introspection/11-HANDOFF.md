---
phase: 11-guest-calling-convention-missing-introspection
status: paused
paused_at: "2026-04-07T19:15:00Z"
context_used: 92%
completed_waves: [11-01, 11-02, 11-03, 11-04, 11-05-partial]
remaining_waves: [11-05-tests, 11-06]
---

# Phase 11 Handoff: Guest Calling Convention & Missing Introspection

## Resume Command
```
/gsd-execute-phase 11
```

## Completed Waves

### ✅ Wave 1 (11-01) — Interface Structs
**Commit:** `8e9693f`, `f12b0ae`
- Renamed `RuntimeAbi` → `HostInterface` (72 bytes → 88 bytes with new functions)
- Created `RuntimeInterface` struct (80 bytes → 96 bytes with new functions)
- Both have `runtime: *mut c_void` field at offset 0

### ✅ Wave 2 (11-02) — Self-Passing Pattern
**Commit:** `9cba273`
- Deleted `RuntimeContext` and `HostContext` files
- All host callbacks use `this: *const HostInterface` parameter
- Added TLS for bundle_id tracking: `INIT_BUNDLE_ID`

### ✅ Wave 3 (11-03) — ABI Types
**Commit:** `98d10df`
- `Array<T>`: items, len, align fields (24 bytes)
- `GuestContractInstance`: added contract_id field (16 bytes)
- `DependencyInfo`: new struct (24 bytes)

### ✅ Wave 4 (11-04) — Interface Callback Updates
**Commit:** `3717702`
- GuestContractInterface callbacks use `*const HostInterface`
- HostContractInterface already had self-passing pattern

### ⏳ Wave 5 (11-05) — Introspection ABIs (ABI changes done, tests pending)
**Commit:** `1ac696c`
- HostInterface: `list_bundles`, `get_dependencies`, `find_all_by_contract` returns Array
- RuntimeInterface: matching functions
- `Array::new()` constructor added
- `host_list_bundles` and `host_get_dependencies` implemented
- `count_by_contract` and `find_all_by_contract_into` added to PluginRegistry

**Remaining:** Integration tests need updates for removed `RuntimeAbi`/`RuntimeContext` types

### ⏳ Wave 6 (11-06) — Documentation
- Add first-class documentation to all interface types per D-14

## Known Issues to Fix

Integration tests reference removed types:
- `RuntimeAbi` → removed, use `HostInterface`
- `RuntimeContext` → removed, use `HostInterface`

Run `cargo test -p polyplug` to identify all failing tests.

## Build Status

```
cargo build -p polyplug: SUCCESS (warnings only)
cargo build -p polyplug_abi: SUCCESS
```

## Files Modified This Session

### Phase 11.04
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs`
- `crates/polyplug/src/runtime.rs`
- `crates/polyplug/src/registry/plugin_registry.rs`
- `crates/polyplug_abi/src/dispatch/vm_dispatch.rs`

### Phase 11.05
- `crates/polyplug_abi/src/host/host_interface.rs`
- `crates/polyplug_abi/src/host/runtime_interface.rs`
- `crates/polyplug_abi/src/types/array.rs`
- `crates/polyplug/src/runtime.rs`
- `crates/polyplug/src/runtime_builder.rs`
- `crates/polyplug/src/registry/plugin_registry.rs`