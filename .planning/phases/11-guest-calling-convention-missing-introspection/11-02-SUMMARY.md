---
phase: 11-guest-calling-convention-missing-introspection
plan: 02
subsystem: abi
tags: [ffi, runtime, self-passing, tls]

requires: [11-01]
provides:
  - Removed RuntimeContext/HostContext indirection layer
  - Self-passing pattern for all interfaces
  - TLS-based bundle_id tracking for dependency enforcement
affects: [loaders, sdks, test-fixtures]

tech-stack:
  added: []
  patterns:
    - "Self-passing pattern: this: *const Interface as first parameter"
    - "TLS for init-phase bundle_id: INIT_BUNDLE_ID Cell<u64>"

key-files:
  deleted:
    - crates/polyplug_abi/src/host/runtime_context.rs
    - crates/polyplug_abi/src/host/host_context.rs
  modified:
    - crates/polyplug_abi/src/host/host_interface.rs
    - crates/polyplug_abi/src/guest/guest_contract_interface.rs
    - crates/polyplug_abi/src/host/host_contract_interface.rs
    - crates/polyplug/src/runtime.rs
    - crates/polyplug_native/src/loader.rs

key-decisions:
  - "Use TLS for bundle_id during init phase instead of passing through HostContext"
  - "HostContractInterface now has runtime: *mut c_void field"
  - "GuestContractInterface callbacks receive opaque host pointer"

patterns-established:
  - "Host callbacks extract runtime from (*this).runtime"
  - "set_init_bundle_id() before polyplug_init, clear after"

requirements-completed: [D-03, D-12, D-13]

duration: 20min
completed: 2026-04-07
---

# Phase 11: Plan 02 Summary

**Deleted RuntimeContext and HostContext wrapper types, implementing self-passing pattern across all interfaces with TLS-based bundle_id tracking.**

## Performance

- **Duration:** 20 min
- **Started:** 2026-04-07T15:50:00Z
- **Completed:** 2026-04-07T16:10:00Z
- **Tasks:** 3
- **Files modified:** 18

## Accomplishments
- Deleted runtime_context.rs and host_context.rs (no more indirection)
- Updated all host callbacks to use `this: *const HostInterface` parameter
- Added TLS for init-phase bundle_id tracking (INIT_BUNDLE_ID)
- Updated GuestContractInterface with opaque host pointer parameter
- Updated HostContractInterface with embedded runtime field
- Updated native loader and test fixtures for new polyplug_init signature

## Task Commits

1. **Wave 2 combined commit** - `9cba273` (feat)

## Files Created/Modified
- `crates/polyplug_abi/src/host/host_context.rs` - DELETED
- `crates/polyplug_abi/src/host/runtime_context.rs` - DELETED
- `crates/polyplug_abi/src/host/host_interface.rs` - Self-passing pattern
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` - Opaque host pointer
- `crates/polyplug_abi/src/host/host_contract_interface.rs` - Runtime field + self-passing
- `crates/polyplug/src/runtime.rs` - TLS, updated callbacks
- `crates/polyplug_native/src/loader.rs` - New init signature
- Test fixtures - Updated polyplug_init signatures

## Decisions Made
- Use TLS instead of passing bundle_id through HostContext
- GuestContractInterface uses `*const c_void` (opaque) for ABI stability
- HostContractInterface has its own runtime field (per D-13)

## Deviations from Plan
None - plan executed as specified.

## Next Phase Readiness
- Self-passing pattern implemented
- Remaining: Wave 3 (Array<T>, DependencyInfo), Wave 4 (interface updates), Wave 5 (introspection), Wave 6 (docs)

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Plan: 02*
*Completed: 2026-04-07*