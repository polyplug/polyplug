---
phase: 01-abi-types
plan: 08
subsystem: abi
tags: [abi, sdk, rust, guest, imports, error-codes]

# Dependency graph
requires:
  - phase: 01-05
    provides: AbiErrorCode enum and FFI helper exports from polyplug_abi
provides:
  - Clean SDK guest library without deprecated ABI_* constant imports
  - AbiErrorCode enum export for error code access
  - Correct submodule path imports
affects: [sdk-guest, rust-sdk, abi-types]

# Tech tracking
tech-stack:
  added: []
  patterns: [submodule-path-imports, enum-error-codes]

key-files:
  created: []
  modified: [sdks/rust/guest/src/lib.rs]

key-decisions:
  - "Use AbiErrorCode enum instead of deprecated ABI_* constants"
  - "Import from submodule paths (types::StringView) instead of root"

patterns-established:
  - "Error codes: Use AbiErrorCode::Ok, AbiErrorCode::Generic, etc. instead of ABI_OK, ABI_ERROR_GENERIC"

requirements-completed: [ABI-12]

# Metrics
duration: 343s
completed: 2026-04-03
---
# Phase 01 Plan 08: SDK Guest Library ABI_* Import Removal Summary

**Removed deprecated ABI_* constant imports from Rust SDK guest library, replaced with AbiErrorCode enum export and fixed blocking import path issues**

## Performance

- **Duration:** 5.7 min (343s)
- **Started:** 2026-04-03T21:32:17Z
- **Completed:** 2026-04-03T21:38:00Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Removed all deprecated ABI_* constant imports (ABI_OK, ABI_ERROR_GENERIC, etc.)
- Added AbiErrorCode enum export for proper error code access
- Fixed import paths to use submodule paths (types::StringView, etc.)
- Removed non-existent host contract type imports that caused compilation errors
- Updated doc comment and alloc_string function to use new error code pattern

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove ABI_* constant imports from SDK guest library** - `9335616` (fix)

**Plan metadata:** (not yet committed)

## Files Created/Modified
- `sdks/rust/guest/src/lib.rs` - Removed deprecated imports, added AbiErrorCode export, fixed import paths

## Decisions Made
- Use `AbiErrorCode` enum instead of deprecated `ABI_OK`, `ABI_ERROR_*` constants
- Import types from submodule paths (`polyplug_abi::types::StringView`) instead of root level
- Remove non-existent host contract type imports that referenced old architecture
- Replace private utility function imports with public ID type exports

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Fixed import paths for types not exported at polyplug_abi root**
- **Found during:** Task 1 (cargo check after removing ABI_* imports)
- **Issue:** SDK imported StringView, Buffer, AbiError, HostContext, DispatchType, etc. from polyplug_abi root but these types are not exported there
- **Fix:** Updated imports to use submodule paths: `polyplug_abi::types::StringView`, `polyplug_abi::host::host_context::HostContext`, `polyplug_abi::dispatch::dispatch_type::DispatchType`, etc.
- **Files modified:** sdks/rust/guest/src/lib.rs
- **Verification:** cargo check passes with 0 errors
- **Committed in:** 9335616 (Task 1 commit)

**2. [Rule 3 - Blocking] Removed non-existent host contract type imports**
- **Found during:** Task 1 (cargo check)
- **Issue:** SDK imported HostContractVTableHeader, NativeHostContractDispatch, VmHostContractDispatch, HostContractDispatch, HostContractVTable which no longer exist (replaced by HostContractInterface in architecture refactor)
- **Fix:** Removed all these imports entirely - they were old types from before the architecture refactor
- **Files modified:** sdks/rust/guest/src/lib.rs
- **Verification:** cargo check passes with 0 errors
- **Committed in:** 9335616 (Task 1 commit)

**3. [Rule 3 - Blocking] Replaced private utility imports with public ID types**
- **Found during:** Task 1 (cargo check)
- **Issue:** SDK imported `bundle_id` module and `contract_id` function from polyplug_utils but these are private
- **Fix:** Replaced with public ID type exports: `BundleId`, `GuestContractId`, `HostContractId`
- **Files modified:** sdks/rust/guest/src/lib.rs
- **Verification:** cargo check passes with 0 errors
- **Committed in:** 9335616 (Task 1 commit)

**4. [Rule 1 - Bug] Fixed PluginDispatch import to correct type name**
- **Found during:** Task 1 (cargo check)
- **Issue:** SDK imported `PluginDispatch` but the actual type is `DispatchMechanisms`
- **Fix:** Changed import to `DispatchMechanisms` from correct submodule path
- **Files modified:** sdks/rust/guest/src/lib.rs
- **Verification:** cargo check passes with 0 errors
- **Committed in:** 9335616 (Task 1 commit)

**5. [Rule 2 - Missing Critical] Updated code usage to use new error code pattern**
- **Found during:** Task 1 (plan execution)
- **Issue:** alloc_string function used `ABI_ERROR_GENERIC` constant which was removed; doc comment example also used it
- **Fix:** Updated to use `AbiErrorCode::Generic as u32` for the PluginError.code field (u32), updated doc comment to show `AbiErrorCode::Generic`
- **Files modified:** sdks/rust/guest/src/lib.rs
- **Verification:** cargo check passes with 0 errors
- **Committed in:** 9335616 (Task 1 commit)

---

**Total deviations:** 5 auto-fixed (4 blocking, 1 bug, 1 missing critical)
**Impact on plan:** All auto-fixes necessary for SDK to compile. The plan's "truths" assumption that SDK would compile was violated due to pre-existing import issues unrelated to ABI_* constants. Fixed to deliver working SDK.

## Issues Encountered
- Pre-existing import path issues in SDK guest library - types imported from polyplug_abi root but not exported there
- Non-existent host contract type imports referencing old architecture before rename
- Private utility function imports that shouldn't have been exposed

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- SDK guest library now compiles without deprecated ABI_* imports
- AbiErrorCode enum pattern established for error codes
- Import path convention established (submodule paths for types not at root)

---
*Phase: 01-abi-types*
*Completed: 2026-04-03*

## Self-Check: PASSED
- SUMMARY.md exists at .planning/phases/01-abi-types/01-08-SUMMARY.md
- Task commit 9335616 exists in git log