---
phase: 11-guest-calling-convention-missing-introspection
plan: 01
subsystem: abi
tags: [ffi, interface, host, runtime, self-passing]

requires: []
provides:
  - HostInterface struct (renamed from RuntimeAbi)
  - RuntimeInterface struct (new)
  - Symmetric interface architecture foundation
affects: [runtime, loaders, sdks]

tech-stack:
  added: []
  patterns:
    - "Self-passing pattern: functions take interface pointer as first parameter"
    - "Symmetric interfaces: HostInterface (guest calls) / RuntimeInterface (host calls)"

key-files:
  created:
    - crates/polyplug_abi/src/host/runtime_interface.rs
  modified:
    - crates/polyplug_abi/src/host/host_interface.rs
    - crates/polyplug_abi/src/host/mod.rs
    - crates/polyplug_abi/src/lib.rs
    - crates/polyplug/src/runtime.rs
    - crates/polyplug/src/runtime_builder.rs

key-decisions:
  - "Added runtime: *mut c_void field at offset 0 in both interfaces"
  - "Renamed call_method to call_guest_method for clarity"
  - "Function signatures still use RuntimeContext (Wave 2 will update)"

patterns-established:
  - "Interface documentation with Who provides/Who calls/Ownership/Lifetime sections"

requirements-completed: [D-01, D-02]

duration: 15min
completed: 2026-04-07
---

# Phase 11: Plan 01 Summary

**Renamed RuntimeAbi to HostInterface and created RuntimeInterface for symmetric interface architecture with self-passing pattern.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-07T15:30:00Z
- **Completed:** 2026-04-07T15:45:00Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- HostInterface struct with runtime: *mut c_void field at offset 0 (72 bytes)
- RuntimeInterface struct for host-to-runtime API (80 bytes)
- Renamed call_method to call_guest_method for clarity
- Updated all exports and references in polyplug crate

## Task Commits

1. **Task 1: Rename RuntimeAbi to HostInterface** - `8e9693f` (feat)
2. **Task 2: Create RuntimeInterface struct** - `f12b0ae` (feat)

## Files Created/Modified
- `crates/polyplug_abi/src/host/host_interface.rs` - Renamed from runtime_abi.rs, added runtime field
- `crates/polyplug_abi/src/host/runtime_interface.rs` - NEW: RuntimeInterface struct
- `crates/polyplug_abi/src/host/mod.rs` - Updated exports
- `crates/polyplug_abi/src/lib.rs` - Updated crate-level exports
- `crates/polyplug/src/runtime.rs` - Updated to use HostInterface
- `crates/polyplug/src/runtime_builder.rs` - Updated to construct HostInterface

## Decisions Made
- Kept RuntimeContext in function signatures for Wave 1 (Wave 2 deletes RuntimeContext)
- Set runtime field to null in HostInterface construction (callbacks extract from RuntimeContext)
- RuntimeInterface has 9 function pointers (80 bytes total, not 88 as plan estimated)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Corrected RuntimeInterface size calculation**
- **Found during:** Task 2 (layout test)
- **Issue:** Plan specified 88 bytes (10 functions) but CONTEXT.md lists only 9 functions
- **Fix:** Corrected test to expect 80 bytes (8 runtime + 9*8 functions)
- **Files modified:** crates/polyplug_abi/src/host/runtime_interface.rs
- **Verification:** Layout test passes
- **Committed in:** f12b0ae (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking - test correction)
**Impact on plan:** Minor - test expectation corrected to match actual function count

## Issues Encountered
None - both tasks completed smoothly.

## Next Phase Readiness
- HostInterface and RuntimeInterface structs ready
- Wave 2 will delete RuntimeContext/HostContext and update function signatures

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Plan: 01*
*Completed: 2026-04-07*