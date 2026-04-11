---
phase: 17-refactor-contractregistry-to-unified-runtimestore
fixed_at: 2026-04-11T00:00:00Z
review_path: .planning/phases/17-refactor-contractregistry-to-unified-runtimestore/17-REVIEW.md
iteration: 1
findings_in_scope: 4
fixed: 3
skipped: 1
status: partial
---

# Phase 17: Code Review Fix Report

**Fixed at:** 2026-04-11T00:00:00Z
**Source review:** .planning/phases/17-refactor-contractregistry-to-unified-runtimestore/17-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 4
- Fixed: 3
- Skipped: 1

## Fixed Issues

### WR-01: register_bundle_metadata pushes duplicate BundleIds into bundle_name_index on re-registration

**Files modified:** `crates/polyplug/src/registry/runtime_store.rs`
**Commit:** d920dfa
**Applied fix:** Added a duplicate check before pushing `bundle_id` into `bundle_name_index`. The method now iterates the existing Vec and only pushes if the `bundle_id` is not already present, preventing accumulation of duplicate entries during hot-reload cycles.

### WR-02: bundle_data and bundle_declared_deps are never cleaned up -- no unload/remove methods

**Files modified:** `crates/polyplug/src/registry/runtime_store.rs`
**Commit:** 648f4f9
**Applied fix:** Added `remove_bundle_metadata(bundle_id)` method to RuntimeStore. This method: (1) collects all plugin slot indices and the bundle name from `bundle_data`, (2) for each slot, reads the contract_id, removes the slot index from `guest_contract_index` (cleaning up empty Vec entries), then clears the slot's entry and interface, (3) removes the bundle from `bundle_data`, (4) removes the bundle_id from `bundle_name_index` (cleaning up empty Vec entries), (5) removes the bundle from `bundle_declared_deps`. Returns the count of unloaded slots.

### WR-03: register_guest_contract accumulates stale indices in guest_contract_index for vacant slots

**Files modified:** `crates/polyplug/src/registry/runtime_store.rs`
**Commit:** 648f4f9
**Applied fix:** Addressed by the same `remove_bundle_metadata` method from WR-02. When a bundle is removed, each slot's index is removed from `guest_contract_index` via `Vec::retain`, preventing stale index accumulation. Empty index Vecs are removed from the HashMap entirely.

## Skipped Issues

### CR-01: Hot-reload interface swap is a no-op -- find_guest_contract returns old slot, not new

**File:** `crates/polyplug/src/reload.rs:160-171`
**Reason:** Pre-existing issue not caused by Phase 17. The fix requires significant architectural changes to the reload swap logic (tracking new vs old slot registrations during reload), which is out of scope for this phase and carries risk of breaking existing hot-reload behavior. Per explicit instructions: "If fixing it is risky or out of scope for Phase 17, SKIP it."
**Original issue:** After `loader.reload()` completes, `find_guest_contract` returns the OLD slot (first in insertion order), making the `swap_guest_contract_interface` call a no-op. Newly registered interfaces remain in orphaned slots.

---

_Fixed: 2026-04-11T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
