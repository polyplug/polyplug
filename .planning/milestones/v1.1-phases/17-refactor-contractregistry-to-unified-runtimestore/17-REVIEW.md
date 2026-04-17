---
phase: 17-refactor-contractregistry-to-unified-runtimestore
reviewed: 2026-04-11T00:00:00Z
depth: standard
files_reviewed: 6
files_reviewed_list:
  - crates/polyplug/src/registry/runtime_store.rs
  - crates/polyplug/src/registry/mod.rs
  - crates/polyplug/src/runtime.rs
  - crates/polyplug/src/loader/manifest.rs
  - crates/polyplug/src/reload.rs
  - crates/polyplug/src/ffi.rs
findings:
  critical: 1
  warning: 3
  info: 4
  total: 8
status: issues_found
---

# Phase 17: Code Review Report

**Reviewed:** 2026-04-11T00:00:00Z
**Depth:** standard
**Files Reviewed:** 6
**Status:** issues_found

## Summary

Reviewed the Phase 17 refactoring that renamed ContractRegistry to RuntimeStore and added BundleData/BundleDescriptor/BundleDependency structs with O(1) bundle lookups via `bundle_data` HashMap and `bundle_name_index`. The new data structures are well-designed and the thread-safety model (single RwLock protecting all mutable state) is sound.

One critical bug was found in the hot-reload swap logic: `find_guest_contract` returns the first matching slot, which after reload is the OLD slot, not the newly registered one. This makes the interface swap a no-op. Several warnings around index consistency and missing cleanup methods were also identified.

## Critical Issues

### CR-01: Hot-reload interface swap is a no-op -- find_guest_contract returns old slot, not new

**File:** `crates/polyplug/src/reload.rs:160-171`
**Issue:** After `loader.reload()` completes, new interfaces are registered into fresh slots (appended to `guest_contract_index`). The swap loop then calls `find_guest_contract(contract_id, 0)` to locate the "new" interface. However, `find_guest_contract` iterates `guest_contract_index[contract_id]` in insertion order and returns the first match. The OLD slot (at a lower index) still has `entry.is_some()` and `interface.is_some()`, so it matches first. The result:

1. `find_guest_contract` returns the OLD slot handle
2. `get_guest_contract_interface_arc` gets the OLD interface Arc
3. `swap_guest_contract_interface` swaps the OLD slot with itself (Arc clone of same)

The newly registered interfaces remain in orphaned slots and are never used. The hot-reload produces no functional effect.

**Fix:** After the new interfaces are registered during `loader.reload()`, the swap loop should locate the NEW registrations by finding the last entry in `guest_contract_index[contract_id]` that was not in the original `slot_indices` set, or by looking up slots by bundle_id that were registered after the reload. One approach:

```rust
// In reload.rs, after loader.reload() succeeds:

// Collect the NEW slot indices for this bundle
let new_slot_indices: Vec<u32> = self.registry.get_bundle_plugin_slots(bundle_id);
let new_slots: Vec<u32> = new_slot_indices
    .into_iter()
    .filter(|idx| !slot_indices.contains(idx))
    .collect();

// Build a map from contract_id -> new slot index
for new_slot_idx in &new_slots {
    let contract_id = self.registry
        .get_slot_guest_contract_id(*new_slot_idx)
        .ok_or_else(|| ...)?;
    // ... use new_slot_idx as the source, old slot as the target for swap
}
```

Alternatively, add a method to RuntimeStore that finds the most recently registered slot for a given contract_id, or that returns all slots except a given exclusion set.

## Warnings

### WR-01: register_bundle_metadata pushes duplicate BundleIds into bundle_name_index on re-registration

**File:** `crates/polyplug/src/registry/runtime_store.rs:607-610`
**Issue:** The method unconditionally pushes `bundle_id` into `bundle_name_index[bundle_name]` on every call. If `register_bundle_metadata` is called multiple times for the same bundle (e.g., during a reload cycle that re-parses and re-registers metadata), the same `BundleId` is appended multiple times, creating duplicates. `get_bundles_by_name` would then return the same ID twice.

**Fix:** Check if the bundle_id already exists before pushing:

```rust
data.bundle_name_index
    .entry(bundle_name)
    .or_default()
    .iter()
    .find(|id| **id == bundle_id)
    .is_none()
    .then(|| {
        data.bundle_name_index
            .get_mut(&bundle_name)
            .unwrap()
            .push(bundle_id);
    });
```

Or use a `HashSet<BundleId>` instead of `Vec<BundleId>` for the name index values if order doesn't matter.

### WR-02: bundle_data and bundle_declared_deps are never cleaned up -- no unload/remove methods

**File:** `crates/polyplug/src/registry/runtime_store.rs` (entire file)
**Issue:** While `clear_for_test` exists for test cleanup, there are no production methods to remove a bundle's data from `bundle_data`, `bundle_name_index`, or `bundle_declared_deps`. During hot-reload, old bundle metadata persists alongside new data. Over many reload cycles, this accumulates stale entries. The `guest_contract_index` grows with stale slot references, and `find_guest_contract` must skip over vacant slots (entry.is_none()) that were never cleaned from the index.

**Fix:** Add a `remove_bundle_metadata` or `unload_bundle` method that:
1. Removes the bundle_id from `bundle_data`
2. Removes the bundle_id from `bundle_name_index` Vec entries (and removes the Vec entry if empty)
3. Removes the bundle_id from `bundle_declared_deps`
4. Cleans up stale entries from `guest_contract_index`

### WR-03: register_guest_contract accumulates stale indices in guest_contract_index for vacant slots

**File:** `crates/polyplug/src/registry/runtime_store.rs:209-213`
**Issue:** When a plugin is unloaded (slot.entry set to None, slot.interface set to None), the slot index remains in `guest_contract_index`. The `find_guest_contract` method handles this by checking `slot.entry.is_some()`, but the index Vec grows without bound. With many load/unload cycles, the index becomes dominated by stale entries, making lookups O(stale + live) instead of O(live).

**Fix:** When clearing a slot (during unload), also remove its index from `guest_contract_index`. This requires either an O(n) scan of the Vec or a secondary index mapping slot_idx back to contract_id. Alternatively, compact the index periodically.

## Info

### IN-01: BundleDescriptor manual clone implementation could derive Clone

**File:** `crates/polyplug/src/registry/runtime_store.rs:634-648`
**Issue:** `get_bundle_descriptor` manually clones each field of `BundleDescriptor` with a comment explaining "BundleDescriptor doesn't derive Clone". Adding `#[derive(Clone)]` to `BundleDescriptor`, `BundleDependency` (both only contain Clone-able types) would eliminate the manual clone code and reduce maintenance burden when fields are added.

**Fix:** Add `#[derive(Clone)]` to `BundleDescriptor` and `BundleDependency`:
```rust
#[derive(Clone)]
pub struct BundleDescriptor { ... }

#[derive(Clone)]
pub struct BundleDependency { ... }
```

### IN-02: TODO comment in manifest.rs about moving TOML parsing to host Rust SDK

**File:** `crates/polyplug/src/loader/manifest.rs:7`
**Issue:** `// TODO: Move toml parse to host rust SDK` -- left as a tracking note.

**Fix:** Track in issue tracker if not already tracked. No code change needed.

### IN-03: Trailing blank line at end of get_guest_contract_interface_arc method

**File:** `crates/polyplug/src/registry/runtime_store.rs:686-688`
**Issue:** Double blank line between `get_guest_contract_interface_arc` and `clear_for_test` methods.

**Fix:** Remove the extra blank line for consistency with the rest of the file.

### IN-04: runtime_language_from_str defaults unknown runtimes to Rust

**File:** `crates/polyplug/src/runtime.rs:631-641`
**Issue:** The match arm `_ => RuntimeLanguage::Rust` silently treats unrecognized runtime strings as Rust. If a manifest has a typo (e.g., `"pyton"` instead of `"python"`), the bundle will be loaded with the wrong runtime type. This is a pre-existing pattern, not new to Phase 17.

**Fix:** Consider returning an error or using `Option<RuntimeLanguage>` so the caller can reject unknown runtime strings.

---

_Reviewed: 2026-04-11T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
