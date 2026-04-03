---
phase: 02-update-loader-implementations
plan: 05
subsystem: loader-errors
tags: [dotnet, error-handling, initfailed, hotreload]

# Dependency graph
requires:
  - phase: 01-define-loader-local-error-types
    provides: Core LoaderError stripped of loader-specific variants
provides:
  - DotnetLoader using unified InitFailed pattern at all error sites
  - RuntimeError::HotReloadDisabled for hot-reload disabled
affects: [phase-03-verification]

# Tech tracking
tech-stack:
  added: []
  patterns: [InitFailed with descriptive error messages, HotReloadDisabled for unsupported reload]

key-files:
  created: []
  modified:
    - crates/polyplug_dotnet/src/lib.rs
  deleted:
    - crates/polyplug_dotnet/src/error.rs

key-decisions:
  - "D-01: Use LoaderError::InitFailed for all loader-specific errors with descriptive messages"
  - "D-03: Use RuntimeError::HotReloadDisabled for hot-reload disabled (not InitFailed)"
  - "D-04: Remove unused DotnetLoaderError type entirely"

patterns-established:
  - "InitFailed pattern: bundle field identifies context, error field provides descriptive message"
  - "Hot-reload disabled: return RuntimeError::HotReloadDisabled directly"

requirements-completed: [ERR-06]

# Metrics
duration: 7min
completed: 2026-04-03
---
# Phase 02 Plan 05: DotnetLoader Error Unification Summary

**DotnetLoader updated to use unified InitFailed pattern at all error sites, DotnetLoaderError type removed, hot-reload returns RuntimeError::HotReloadDisabled**

## Performance

- **Duration:** 7 min
- **Started:** 2026-04-03T08:02:09Z
- **Completed:** 2026-04-03T08:08:53Z
- **Tasks:** 3
- **Files modified:** 2 (lib.rs modified, error.rs deleted)

## Accomplishments
- Deleted DotnetLoaderError type and error.rs module entirely
- Replaced 6 .NET-specific error variants with unified LoaderError::InitFailed
- Fixed reload() method to return RuntimeError::HotReloadDisabled per D-03
- Updated all tests to expect InitFailed instead of removed variants

## Task Commits

Each task was committed atomically as one combined commit (all changes in same file):

1. **Task 1-3: DotnetLoader error unification** - `6d523da` (feat)

## Files Created/Modified
- `crates/polyplug_dotnet/src/lib.rs` - DotnetLoader implementation with unified InitFailed pattern
- `crates/polyplug_dotnet/src/error.rs` - DELETED (DotnetLoaderError type removed)

## Decisions Made
- Per D-01: All loader-specific errors now use InitFailed with descriptive messages
- Per D-03: Hot-reload disabled returns RuntimeError::HotReloadDisabled directly
- Bundle field in InitFailed uses min_framework/tfm for version compatibility errors
- Bundle field uses manifest.name for assembly loading errors

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Pre-existing compilation errors in core polyplug crate (out of scope for this plan)
- These errors are unrelated to DotnetLoader changes and do not block this plan's verification

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- DotnetLoader now uses unified error pattern consistent with other loaders
- All .NET-specific error variants replaced with InitFailed
- Ready for Phase 03 verification once core crate compilation issues resolved

---
*Phase: 02-update-loader-implementations*
*Completed: 2026-04-03*

## Self-Check: PASSED
- lib.rs exists and modified correctly
- error.rs deleted
- Commit 6d523da exists
- SUMMARY.md created