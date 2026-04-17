---
phase: 04-hot-reload
plan: 02
status: completed
commit: e8b5a60
requirements: [HR-03, HR-05, HR-06]
completed_at: 2026-04-04
---

# SUMMARY: Implement Callback-Based Hot-Reload with Interface Swap

## What Was Done

Modified `Runtime.reload_bundle()` to implement the callback-based model with explicit interface swap after loader.reload() succeeds.

## Changes Made

### crates/polyplug/src/reload.rs

**Warning Check (HR-06):**
- Added `Arc::strong_count` check after Preparing callback returns
- Emits warning via `emit_warning()` if refs > 1 (informational only)
- Warning message: "Potential UB: Arc refs still exist..."
- Only emits once per bundle (break after first detection)

**Interface Swap (HR-05):**
- Store `slot_indices` before loader.reload() call
- After successful loader.reload(), swap each slot's interface:
  1. Get contract_id from old slot (stable across reload)
  2. Find NEW interface handle via `find_by_contract()`
  3. Get Arc to NEW interface via `get_interface_arc()`
  4. Atomic swap via `swap_interface()`
- On failure: Fire Failed callback WITHOUT interface swap

**Documentation Updates:**
- Updated module doc to describe callback-based flow
- Updated `ReloadPhase::Preparing` doc to emphasize host responsibility
- Updated `ReloadPhase::Reloaded` doc to reflect interface swap completion

## Verification

All grep checks passed:
- `Arc::strong_count` present in warning check
- `emit_warning` call present
- `swap_interface` call present in Ok branch
- `find_by_contract` call present for locating new interfaces
- "Potential UB" warning message present

Build: `cargo build -p polyplug` succeeds with 0 errors

## Next Steps

Plan 04-03 will update test documentation to reflect callback-based model.

## Issues

None encountered.