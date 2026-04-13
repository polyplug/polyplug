---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
plan: 06
subsystem: abi-codegen
tags: [build-script, codegen, helpers, sdk, ffi]

# Dependency graph
requires:
  - phase: 19-04
    provides: "Deleted hand-written FFI structs from SDK host files"
provides:
  - "Inline helper method const strings in generate.rs for 4 languages"
  - "get_inline_helpers() replacing file-based helper pipeline"
  - "Fixed C++ handle.hpp with index-only GuestContractHandle comparison"
affects: [polyplug_abi build script, all SDK abi.* files]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Inline const helper strings for build script codegen reliability"]

key-files:
  created: []
  modified:
    - crates/polyplug_abi/build/generate.rs
    - crates/polyplug_codegen/src/languages/csharp.rs
    - sdks/cpp/host/polyplug/handle.hpp
    - sdks/csharp/abi/Abi.cs
    - sdks/cpp/abi/polyplug/abi.hpp
    - sdks/lua/abi/abi.lua
    - sdks/js/abi/abi.ts

key-decisions:
  - "Embed helper methods as const strings in generate.rs instead of relying on external helper files"
  - "Add using System.Text to C# codegen header for StringHelpers Encoding.UTF8 usage"

patterns-established:
  - "Inline const pattern for build script helper content: avoids file I/O and deletion race conditions"

requirements-completed: [D-12, D-23]

# Metrics
duration: 8min
completed: 2026-04-13
---

# Phase 19 Plan 06: Gap Closure Summary

**Embed helper methods as inline const strings in generate.rs to survive consecutive rebuilds; fix C++ handle.hpp index-only comparison**

## Performance

- **Duration:** 8 min
- **Started:** 2026-04-13T00:51:55Z
- **Completed:** 2026-04-13T00:59:27Z
- **Tasks:** 2
- **Files modified:** 7

## Accomplishments
- Helper methods now embedded as 5 const strings (HELPER_CSHARP_STRING_VIEW, HELPER_CSHARP_STRING_HELPERS, HELPER_LUA, HELPER_JS, HELPER_CPP) in generate.rs
- All 4 SDK abi.* files contain helper methods after clean rebuild AND after second consecutive build
- Removed file-based helper pipeline (read_helper_files, delete_merged_helper_files, helper_files method)
- Fixed C++ handle.hpp to use only index field for GuestContractHandle comparison (no generation field)
- Added using System.Text to C# codegen header for StringHelpers compilation

## Task Commits

Each task was committed atomically:

1. **Task 1: Embed helper method content as const strings in generate.rs** - `f4e77e0` (feat)
2. **Task 2: Fix C++ handle.hpp to use only index field for GuestContractHandle** - `5d31704` (fix)

## Files Created/Modified
- `crates/polyplug_abi/build/generate.rs` - Added 5 inline const helper strings, get_inline_helpers(), removed file-based pipeline
- `crates/polyplug_codegen/src/languages/csharp.rs` - Added using System.Text to C# header
- `sdks/cpp/host/polyplug/handle.hpp` - Fixed index-only comparison, removed generation references
- `sdks/csharp/abi/Abi.cs` - Regenerated with helper methods (StringViewHelper, StringHelpers)
- `sdks/cpp/abi/polyplug/abi.hpp` - Regenerated with helper methods (to_string_view, split, etc.)
- `sdks/lua/abi/abi.lua` - Regenerated with helper methods (M.to_str, M.starts_with, etc.)
- `sdks/js/abi/abi.ts` - Regenerated with helper methods (stringViewToString, stripPrefix, etc.)

## Decisions Made
- **Inline const pattern over file-based pipeline:** The original helper file merge pipeline had a fundamental flaw -- helper files were deleted after first merge, causing subsequent builds to produce abi.* files without helpers. Embedding as const strings makes the pipeline self-contained and deterministic.
- **Add using System.Text to C# header:** The StringHelpers class uses Encoding.UTF8 which requires System.Text namespace. Added to the codegen header since it's a common namespace needed by the generated file.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added using System.Text to C# codegen header**
- **Found during:** Task 1 (Embed helper methods)
- **Issue:** The C# StringHelpers class uses Encoding.UTF8.GetString() which requires `using System.Text;`, but the generated Abi.cs only had `using System.Runtime.InteropServices;`
- **Fix:** Added `using System.Text;` to the C# generator header in csharp.rs
- **Files modified:** crates/polyplug_codegen/src/languages/csharp.rs
- **Verification:** Build passes, Abi.cs contains both using statements
- **Committed in:** f4e77e0 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical functionality)
**Impact on plan:** Essential fix -- without it, generated Abi.cs would fail to compile when StringHelpers references Encoding.UTF8. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 19 gap closure complete -- helper methods persist across rebuilds, handle.hpp compiles correctly
- VERIFICATION.md Truth 7 (helper methods preserved) now passes
- All polyplug_abi tests pass (58/58)

## Self-Check: PASSED

All 7 modified files verified present on disk. Both task commits (f4e77e0, 5d31704) verified in git log.

---
*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Completed: 2026-04-13*
