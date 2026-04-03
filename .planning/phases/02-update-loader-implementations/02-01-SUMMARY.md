---
phase: 02-update-loader-implementations
plan: 01
subsystem: loader
tags: [native-loader, error-handling, refactoring]

# Dependency graph
requires:
  - phase: 01-define-loader-local-error-types
    provides: Loader-local error types defined, core LoaderError stripped
provides:
  - NativeLoader using core LoaderError::InitFailed directly at all error sites
  - Removed NativeLoaderError type (was never tracked in git)
  - Inlined load_internal() logic into load() and reload()
affects: [02-02, 02-03, 02-04, 02-05]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - InitFailed pattern: All loader-specific errors converted to LoaderError::InitFailed { bundle, error } with descriptive message
    - Inline error handling: No intermediate load_internal() method, all logic in BundleLoader trait methods

key-files:
  created: []
  modified:
    - crates/polyplug_native/src/loader.rs (inlined load_internal, removed NativeLoaderError import)
    - crates/polyplug_native/src/lib.rs (removed pub mod error and NativeLoaderError export)
  deleted:
    - crates/polyplug_native/src/error.rs (NativeLoaderError type removed)

key-decisions:
  - "NativeLoaderError removed: No longer needed with unified InitFailed pattern (per D-04)"
  - "load_internal() inlined: Per D-02, removed intermediate method for direct error construction"
  - "BundleTampered and ManifestMissingFile use core LoaderError variants: These are generic errors used across loaders"

patterns-established:
  - "InitFailed pattern: LoaderError::InitFailed { bundle: manifest.name.clone(), error: format!(...) } at all error sites"
  - "Hot-reload disabled check: RuntimeError::HotReloadDisabled returned when runtime.config().hot_reload_enabled is false"

requirements-completed: [ERR-06]

# Metrics
duration: 5min
completed: 2026-04-03
---
# Phase 02 Plan 01: NativeLoader Error Update Summary

**NativeLoader refactored to use LoaderError::InitFailed directly at all error sites, removing NativeLoaderError type and inlining load_internal() logic**

## Performance

- **Duration:** 5 min (304 seconds)
- **Started:** 2026-04-03T08:02:23Z
- **Completed:** 2026-04-03T08:07:27Z
- **Tasks:** 3 (combined into single coherent commit)
- **Files modified:** 2 (+ 1 deleted)

## Accomplishments
- Removed NativeLoaderError type entirely (file deleted, exports removed)
- Inlined load_internal() into load() and reload() methods per D-02
- All 9 error sites now use LoaderError::InitFailed with descriptive messages
- Hot-reload disabled check returns RuntimeError::HotReloadDisabled (1 instance)
- BundleTampered and ManifestMissingFile use core LoaderError variants directly

## Task Commits

All three tasks formed one coherent refactoring, committed atomically:

1. **Task 1-3: Remove NativeLoaderError, inline load_internal, update reload** - `987e832` (feat)

## Files Created/Modified
- `crates/polyplug_native/src/loader.rs` - Removed NativeLoaderError import, removed load_internal() method, inlined all logic into load() and reload() with direct LoaderError::InitFailed construction
- `crates/polyplug_native/src/lib.rs` - Removed `pub mod error;` and `pub use error::NativeLoaderError;`

Files deleted:
- `crates/polyplug_native/src/error.rs` - NativeLoaderError type removed (was never tracked in git)

## Decisions Made
- NativeLoaderError removed per D-04 (no longer needed with unified InitFailed pattern)
- load_internal() inlined per D-02 (direct error construction without intermediate method)
- Core generic variants (BundleTampered, ManifestMissingFile, InitSymbolMissing) used directly from LoaderError enum

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed inverted manifest.file check in reload()**
- **Found during:** Task 3 (reload() implementation)
- **Issue:** Condition `if !manifest.file.is_empty()` returned error when file WAS present (inverted logic)
- **Fix:** Changed to `if manifest.file.is_empty()` to correctly return ManifestMissingFile when file is missing
- **Files modified:** crates/polyplug_native/src/loader.rs
- **Verification:** Logic now matches load() implementation pattern
- **Committed in:** 987e832 (combined commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Bug fix necessary for correctness. No scope creep.

## Issues Encountered
- Pre-existing compilation errors in core polyplug crate (unrelated to this plan's changes)
- error.rs file was never tracked in git, so deletion didn't need staging

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- NativeLoader now fully uses core error types, ready for other loader updates
- Plans 02-02 through 02-05 will follow same pattern for Python, Lua, JS, .NET loaders

---
*Phase: 02-update-loader-implementations*
*Completed: 2026-04-03*