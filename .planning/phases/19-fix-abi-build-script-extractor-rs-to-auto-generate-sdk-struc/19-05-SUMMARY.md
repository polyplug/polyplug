---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
plan: 05
subsystem: abi
tags: [host-interface, guest-sdk, ffi, naming-cleanup]

# Dependency graph
requires:
  - phase: 19
    provides: "Prior plans fixed ABI build script and deleted hand-written structs"
provides:
  - "Complete removal of PluginRegistrar alias from entire codebase"
  - "All guest SDKs and documentation use HostInterface consistently"
affects: [sdk-validation, documentation, guest-sdks]

# Tech tracking
tech-stack:
  added: []
  patterns: ["HostInterface as sole init-time type across all SDKs"]

key-files:
  created: []
  modified:
    - sdks/cpp/guest/polyplug/guest.hpp
    - sdks/js/guest/polyplug_guest.js
    - sdks/rust/guest/src/lib.rs
    - sdks/rust/guest/README.md
    - sdks/cpp/README.md
    - sdks/js/README.md
    - sdks/lua/README.md
    - sdks/python/README.md
    - sdks/csharp/README.md
    - docs/ABI_ARCHITECTURE.md
    - docs/abi_types.md
    - PRD.md
    - AGENTS.md
    - tests/fixtures/test_plugin.py
    - tests/fixtures/test_plugin.lua
    - tests/fixtures/test_plugin_lua/test_plugin.lua

key-decisions:
  - "Replaced PluginRegistrar with HostInterface everywhere; no type alias retained"
  - "Updated parameter names from registrar to host across all SDKs and docs"
  - "Updated function pointer names from register_plugin to register_contract in docs/examples"

patterns-established:
  - "HostInterface is the sole type used in polyplug_init() signatures across all SDKs"

requirements-completed: [D-29, D-30]

# Metrics
duration: 8min
completed: 2026-04-12
---

# Phase 19 Plan 05: Remove PluginRegistrar Summary

**Complete removal of PluginRegistrar alias -- replaced with HostInterface across 16 files in all 5 SDKs, docs, PRD, and test fixtures**

## Performance

- **Duration:** 8 min
- **Started:** 2026-04-12T02:12:42Z
- **Completed:** 2026-04-12T02:20:23Z
- **Tasks:** 2
- **Files modified:** 16

## Accomplishments
- Removed PluginRegistrar from all guest SDK files (C++ macro, JS typedef, Rust doc comment)
- Updated all 6 SDK READMEs to reference HostInterface instead of PluginRegistrar
- Replaced all 6 PluginRegistrar references in PRD.md with HostInterface
- Updated both docs files (ABI_ARCHITECTURE.md, abi_types.md)
- Updated Python test fixture import, docstring, and variable names
- Updated both Lua test fixture comments
- Verified zero PluginRegistrar references remain in any source file across entire codebase

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove PluginRegistrar from guest SDK files and SDK READMEs** - `9ba9ccf` (feat)
2. **Task 2: Remove PluginRegistrar from documentation, PRD, and test fixtures** - `099054e` (feat)

## Files Created/Modified
- `sdks/cpp/guest/polyplug/guest.hpp` - POLYPLUG_GUEST_MAIN macro uses HostInterface* host
- `sdks/js/guest/polyplug_guest.js` - JSDoc typedef and param updated to HostInterface
- `sdks/rust/guest/src/lib.rs` - Doc comment updated from PluginRegistrar::host to HostInterface
- `sdks/rust/guest/README.md` - All code examples use HostInterface, parameter names host
- `sdks/cpp/README.md` - Feature table updated
- `sdks/js/README.md` - Feature table updated
- `sdks/lua/README.md` - Feature table updated
- `sdks/python/README.md` - Import example and feature table updated
- `sdks/csharp/README.md` - Init example and feature table updated
- `docs/ABI_ARCHITECTURE.md` - polyplug_init signature and ABI stability section updated
- `docs/abi_types.md` - RuntimeAbi note updated
- `PRD.md` - 6 references replaced (struct definition, examples in C#/Lua/Rust, signature)
- `AGENTS.md` - Prose updated to remove stale PluginRegistrar mention
- `tests/fixtures/test_plugin.py` - Import, docstring, variable names updated
- `tests/fixtures/test_plugin.lua` - Comment updated
- `tests/fixtures/test_plugin_lua/test_plugin.lua` - Comment updated

## Decisions Made
- Replaced PluginRegistrar with HostInterface in all contexts (no alias retained)
- Updated parameter names from `registrar` to `host` where the type changed
- Updated function pointer field names from `register_plugin` to `register_contract` in doc examples for consistency with the actual HostInterface struct

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Fixed stale PluginRegistrar reference in AGENTS.md**
- **Found during:** Task 2 (codebase-wide grep verification)
- **Issue:** AGENTS.md line 289 contained prose contrasting PluginRegistrar with HostInterface as an example of divergent registration -- but PluginRegistrar no longer exists, making the prose misleading
- **Fix:** Rewrote the prose to reference divergent HostInterface field layouts instead
- **Files modified:** AGENTS.md
- **Verification:** Full grep shows zero PluginRegistrar references in source files
- **Committed in:** 099054e (Task 2 commit)

---

**Total deviations:** 1 auto-fixed (1 missing critical -- stale reference in non-plan-listed file)
**Impact on plan:** Minimal -- AGENTS.md was not in plan file list but needed updating for success criteria compliance.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All PluginRegistrar references removed from codebase
- D-29 and D-30 requirements satisfied
- Build passes, all polyplug_abi tests pass (58/58)

## Self-Check: PASSED
- All 17 key files verified present
- Both task commits verified in git log (9ba9ccf, 099054e)
- Zero PluginRegistrar references in source files (grep verified)
- polyplug_abi build passes, 58/58 tests pass

---
*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Completed: 2026-04-12*
