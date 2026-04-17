---
phase: 03-instance-model
plan: 01
subsystem: codegen
tags: [parser, ir, singleton, host-contracts, api-toml]

# Dependency graph
requires:
  - phase: 01-abi-types
    provides: HostContractInterface with singleton field
provides:
  - singleton field parsing in api.toml
  - singleton field in ResolvedHostContract IR
  - public helper functions for ID computation in polyplug_utils
affects:
  - 03-02 (codegen updates for singleton)
  - 03-03 (runtime get_host_contract implementation)
  - 05-sdk-updates (SDK host contract factories)

# Tech tracking
tech-stack:
  added: []
  patterns: [singleton field with #[serde(default)], public ID helper functions]

key-files:
  created: []
  modified:
    - crates/polyplugc/src/parser.rs
    - crates/polyplugc/src/ir.rs
    - crates/polyplug_utils/src/lib.rs

key-decisions:
  - "singleton defaults to false via #[serde(default)] - explicit opt-in for singleton mode"
  - "singleton not part of host_contract_id hash - mode doesn't affect contract identity"
  - "polyplug_utils visibility fixed: modules public, helper functions added"

patterns-established:
  - "Optional contract fields use #[serde(default)] for backward compatibility"
  - "ID computation via helper functions that wrap type constructors"

requirements-completed:
  - HC-01
  - CG-06
  - CG-01

# Metrics
duration: 15min
completed: 2026-04-04
---
# Phase 03 Plan 01: Singleton Field Parser Support Summary

**Parser and IR support for singleton: bool field on host contracts, enabling efficient shared services like logging, configuration, or asset managers without per-call instance creation.**

## Performance

- **Duration:** ~15 min
- **Started:** 2026-04-04T12:15:00Z
- **Completed:** 2026-04-04T12:30:00Z
- **Tasks:** 2 (1 modification, 1 verification)
- **Files modified:** 3

## Accomplishments
- singleton: bool field added to RawHostContract with #[serde(default)]
- singleton: bool field added to ResolvedHostContract IR struct
- singleton propagation in lower_api() function
- polyplug_utils visibility fixed: modules made public, helper functions added
- All generators verified to use GuestContractInterface (alias for GuestContractInterface)

## Task Commits

Each task was committed atomically:

1. **Task 1: Add singleton field to parser and IR** - `57cc1c0` (feat)

**Plan metadata:** (pending final commit)

## Files Created/Modified
- `crates/polyplugc/src/parser.rs` - Added singleton: bool to RawHostContract with #[serde(default)], propagated in lower_api()
- `crates/polyplugc/src/ir.rs` - Added singleton: bool to ResolvedHostContract, fixed re-export to use guest_contract_id function
- `crates/polyplug_utils/src/lib.rs` - Made modules public, added bundle_id/guest_contract_id/host_contract_id helper functions

## Decisions Made
- singleton defaults to false via #[serde(default)] - explicit opt-in for singleton mode
- singleton not part of host_contract_id hash - mode doesn't affect contract identity
- polyplug_utils helper functions wrap type constructors for convenient ID computation

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed polyplug_utils visibility issues**
- **Found during:** Task 1 (cargo check failed after singleton field addition)
- **Issue:** polyplug_utils modules (guest_contract_id, host_contract_id) were private, contract_id function was private, preventing re-exports in ir.rs
- **Fix:** Made modules public, added public helper functions (bundle_id, guest_contract_id, host_contract_id) that wrap type constructors
- **Files modified:** crates/polyplug_utils/src/lib.rs, crates/polyplugc/src/ir.rs
- **Verification:** cargo check -p polyplugc passes with no errors
- **Committed in:** 57cc1c0 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (blocking issue)
**Impact on plan:** Necessary fix for pre-existing visibility problem that blocked compilation. No scope creep.

## Issues Encountered
- Pre-existing visibility issue in polyplug_utils (documented in STATE.md blocker) resolved as part of this plan

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Parser and IR ready for singleton field usage
- Codegen generators verified to use correct naming (GuestContractInterface alias)
- Next: 03-02 will update generators to produce singleton-aware host contract factories

---
*Phase: 03-instance-model*
*Completed: 2026-04-04*