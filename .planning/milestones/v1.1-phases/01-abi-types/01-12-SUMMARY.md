---
phase: 01-abi-types
plan: 12
subsystem: abi
tags: [serde, toml, manifest, deserialization, id-types]

# Dependency graph
requires: []
provides:
  - serde::Deserialize trait on GuestContractId for TOML manifest parsing
  - serde::Deserialize and Default traits on BundleId for manifest parsing with #[serde(default)]
affects: [manifest-parsing, bundle-loading]

# Tech tracking
tech-stack:
  added: [serde dependency in polyplug_utils]
  patterns: [derive-based serde traits for FFI ID types]

key-files:
  created: []
  modified:
    - crates/polyplug_utils/Cargo.toml
    - crates/polyplug_utils/src/guest_contract_id.rs
    - crates/polyplug_utils/src/bundle_id.rs

key-decisions:
  - "Use serde::Deserialize derive for transparent u64 wrapper types - enables direct deserialization from u64 values in TOML"
  - "Add Default to BundleId (not GuestContractId) - only BundleId needs #[serde(default)] for optional manifest fields"
  - "GuestContractId does not need Default - must be explicitly constructed via new() or from_u64()"

patterns-established:
  - "ID types derive serde::Deserialize for manifest parsing integration"
  - "Default trait only on types representing optional/unset states"

requirements-completed: []

# Metrics
duration: 2min
completed: 2026-04-04
---
# Phase 01 Plan 12: Serde Traits for ID Types Summary

Added serde traits to polyplug_utils ID types (GuestContractId, BundleId) enabling TOML manifest parsing support with #[serde(default)] attributes.

## Performance

- **Duration:** 2 min
- **Started:** 2026-04-04T00:00:00Z
- **Completed:** 2026-04-04T00:02:00Z
- **Tasks:** 3
- **Files modified:** 3

## Accomplishments
- Added serde workspace dependency to polyplug_utils crate
- GuestContractId now derives serde::Deserialize for TOML parsing
- BundleId now derives serde::Deserialize and Default for manifest parsing with optional fields

## Task Commits

Each task was committed atomically:

1. **Task 1: Add serde dependency to polyplug_utils** - `84ee9ca` (feat)
2. **Task 2: Add serde::Deserialize to GuestContractId** - `c383043` (feat)
3. **Task 3: Add serde::Deserialize and Default to BundleId** - `11e93f6` (feat)

## Files Created/Modified
- `crates/polyplug_utils/Cargo.toml` - Added serde workspace dependency
- `crates/polyplug_utils/src/guest_contract_id.rs` - Added serde::Deserialize derive
- `crates/polyplug_utils/src/bundle_id.rs` - Added serde::Deserialize and Default derives

## Decisions Made
- Used derive-based serde traits for transparent u64 wrapper types - simpler than custom Deserialize implementations
- Default only on BundleId (not GuestContractId) because:
  - BundleId(0) represents unset/missing bundle ID field in manifest parsing
  - GuestContractId must always be explicit - cannot have a "default" contract
- Workspace serde dependency provides derive feature automatically

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- polyplug_utils ID types now support serde deserialization
- Ready for manifest parsing integration in core runtime
- BundleId default value (0) represents "no bundle" state

---
*Phase: 01-abi-types*
*Completed: 2026-04-04*