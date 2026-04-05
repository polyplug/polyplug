---
status: testing
phase: 04-hot-reload
source: 04-01-SUMMARY.md, 04-02-SUMMARY.md, 04-03-SUMMARY.md
started: 2026-04-04T14:30:00Z
updated: 2026-04-04T14:30:00Z
---

## Current Test

number: 1
name: Callback-Based Hot-Reload Flow
expected: |
  When calling reload_bundle() on a loaded plugin:
  1. Preparing callback fires first (before any reload work)
  2. Reloaded callback fires after successful interface swap
  3. Failed callback fires if init fails (no interface swap)
awaiting: user response

## Tests

### 1. Callback-Based Hot-Reload Flow
expected: When calling reload_bundle() on a loaded plugin: Preparing callback fires first, then Reloaded callback fires after successful interface swap. Failed callback fires if init fails (no interface swap).
result: [pending]

### 2. Warning Emission for Remaining Arc Refs
expected: If host fails to destroy all instances before Preparing callback returns, runtime emits warning message containing "Potential UB" and "Arc refs still exist" but proceeds with reload anyway (not blocking).
result: [pending]

### 3. Interface Swap After Reload Success
expected: After reload_bundle() succeeds, calling find_by_contract() returns handle pointing to NEW interface (updated vtable). Old instances destroyed in Preparing callback, new instances use swapped interface.
result: [pending]

### 4. No Interface Swap on Reload Failure
expected: If loader.reload() fails (e.g., init error), Failed callback fires and interface swap does NOT happen. Existing interfaces remain unchanged.
result: [pending]

### 5. Callback Phase Data Accuracy
expected: Preparing callback includes bundle_id, bundle_name, retry_count. Reloaded callback includes bundle_id, bundle_name. Failed callback includes bundle_id, bundle_name, reason string describing error.
result: [pending]

### 6. Documentation Accuracy
expected: reload.rs module docs accurately describe callback-based flow (5 steps: Preparing, warning check, loader.reload, interface swap, Reloaded). Safety contract documented: "Host MUST destroy all instances in Preparing callback."
result: [pending]

## Summary

total: 6
passed: 0
issues: 0
pending: 6
skipped: 0

## Gaps

[none yet]