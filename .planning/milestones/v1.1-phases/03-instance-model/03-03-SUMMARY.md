---
phase: 03-instance-model
plan: 03
subsystem: runtime
tags: [host-contracts, singleton-cache, cross-dispatch, ffi]
dependencies:
  requires: [03-01, 03-02]
  provides: [get_host_contract-implementation, singleton-cache, call_method-placeholder]
  affects: [Runtime, RuntimeBuilder]
tech_stack:
  added: [singleton_instances RwLock<HashMap>]
  patterns: [singleton-cache, double-check-locking]
key_files:
  created: []
  modified:
    - path: crates/polyplug/src/runtime.rs
      changes: [singleton_instances field, host_get_host_contract implementation, host_call_method documentation]
    - path: crates/polyplug/src/runtime_builder.rs
      changes: [singleton_instances initialization]
decisions:
  - id: D03-03-01
    summary: "Use double-check locking pattern for singleton instance caching"
    rationale: "Prevents race conditions where multiple threads could create singleton instances simultaneously"
  - id: D03-03-02
    summary: "call_method placeholder with documented implementation path"
    rationale: "Full implementation requires instance-to-contract mapping; placeholder documents options for future implementation"
metrics:
  duration: "180s"
  tasks: 3
  files: 2
---

# Phase 03 Plan 03: Host Contract Instance Retrieval Summary

## One-liner

Implemented get_host_contract with singleton instance caching and documented call_method placeholder requiring instance-contract mapping for full implementation.

## Changes Summary

### Task 1: Singleton Instance Cache

Added `singleton_instances: RwLock<HashMap<u64, HostContractInstance>>` field to Runtime struct for caching singleton host contract instances. Updated RuntimeBuilder to initialize the empty HashMap on construction.

### Task 2: host_get_host_contract FFI Callback

Implemented full `host_get_host_contract` callback that:
- Finds matching HostContractInterface by contract_id and version
- For singleton contracts: checks cache, creates if missing, caches with double-check locking
- For multi-instance contracts: calls create_instance each time
- Properly handles error cases with null returns and error messages

### Task 3: host_call_method FFI Callback

Updated `host_call_method` with:
- Null check for both rt_ctx and instance
- Detailed documentation of implementation requirements
- Placeholder error indicating need for instance-contract mapping
- Documented two options for full implementation (instance.data struct or separate mapping)

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

| Stub | File | Line | Reason |
|------|------|------|--------|
| call_method placeholder | runtime.rs | 741-780 | Requires instance-to-contract mapping not yet implemented; documented options for future work |

## Self-Check: PASSED

- [x] singleton_instances field exists in Runtime struct (line 81)
- [x] RuntimeBuilder initializes singleton_instances (line 161)
- [x] host_get_host_contract calls create_instance (lines 836, 844)
- [x] host_call_method exists (line 741)
- [x] Commit 312004b verified in git log

## Next Steps

For full call_method implementation:
- Decide on instance-to-contract mapping approach (Option A or B from documentation)
- Implement GuestContractInstance.data wrapper struct if using Option A
- Add instance tracking HashMap to Runtime if using Option B
- Implement dispatch routing based on interface.dispatch_type