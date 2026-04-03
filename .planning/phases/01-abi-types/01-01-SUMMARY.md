---
phase: 01-abi-types
plan: 01
subsystem: abi
tags: [guest-contract, rename, breaking-change, hash-prefix]

# Dependency graph
requires: []
provides:
  - GuestContractId type with "guest_contract:" hash prefix
  - Deprecation alias PluginContractId for migration
affects: [phase-02-registry, phase-05-sdk-updates]

# Tech tracking
tech-stack:
  added: []
  patterns: [Guest/Host naming convention, deprecation aliases]

key-files:
  created: [crates/polyplug_utils/src/guest_contract_id.rs]
  modified: [crates/polyplug_utils/src/lib.rs]

key-decisions:
  - "GuestContractId hash prefix: guest_contract: (consistent with Guest/Host terminology)"
  - "Deprecation alias PluginContractId = GuestContractId for migration"

patterns-established:
  - "Guest/Host naming: guest contracts use guest_contract: prefix, host contracts use host_contract: prefix"

requirements-completed: [ABI-11]

# Metrics
duration: 2min
completed: 2026-04-03
---

# Phase 01 Plan 01: Rename PluginContractId to GuestContractId Summary

**Renamed PluginContractId to GuestContractId with "guest_contract:" hash prefix for consistent Guest/Host terminology**

## Performance

- **Duration:** 2 min
- **Started:** 2026-04-03T16:44:57Z
- **Completed:** 2026-04-03T16:46:50Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- GuestContractId type established with consistent naming
- Hash prefix changed from "plugin_contract:" to "guest_contract:"
- Deprecation alias added for migration compatibility
- All tests pass with new naming

## Task Commits

Each task was committed atomically:

1. **Task 1: Rename file and struct** - `6b79a4a` (feat)
2. **Task 2: Update lib.rs exports** - `51bea47` (feat)

**Plan metadata:** (pending final commit)

## Files Created/Modified
- `crates/polyplug_utils/src/guest_contract_id.rs` - New file with GuestContractId type and "guest_contract:" hash prefix
- `crates/polyplug_utils/src/lib.rs` - Updated exports with deprecation alias

## Decisions Made
- Hash prefix "guest_contract:" for consistent Guest/Host terminology (breaking change)
- Deprecation alias PluginContractId = GuestContractId for smooth migration

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- GuestContractId type ready for use in subsequent plans
- Deprecation alias allows gradual migration in dependent code
- SDK files will need updates in phase 05

---
*Phase: 01-abi-types*
*Completed: 2026-04-03*