---
phase: 05-sdk-updates
plan: 04
subsystem: sdk
tags: [lua, luajit, ffi, runtime-config, instance-model]

# Dependency graph
requires:
  - phase: 01-abi-types
    provides: RuntimeConfig layout (24 bytes) with compatibility field
provides:
  - Lua SDK with correct RuntimeConfigC FFI matching polyplug_abi
  - Removed duplicate runtime_config.lua
  - Removed Guard class for instance-based model
affects: [lua-sdk, hot-reload, instance-model]

# Tech tracking
tech-stack:
  added: []
  patterns: [ffi-cdef-with-padding, instance-based-plugin-access]

key-files:
  created: []
  modified:
    - sdks/lua/host/polyplug/runtime.lua
    - sdks/lua/host/polyplug.lua

key-decisions:
  - "RuntimeConfigC FFI struct uses explicit padding arrays for alignment"
  - "Compatibility defaults to Strict (0) when not specified"
  - "resolve_plugin returns raw cdata handle, not Guard wrapper"

patterns-established:
  - "FFI padding: uint8_t _padN[3] between bool and u32 fields"
  - "Instance model: host accesses GuestContractInterface directly via FFI"

requirements-completed: [SDK-04, SDK-06]

# Metrics
duration: 5min
completed: 2026-04-04
---
# Phase 05 Plan 04: Lua SDK RuntimeConfig Update Summary

**Updated Lua SDK FFI RuntimeConfigC to match polyplug_abi 24-byte layout, removed duplicate runtime_config.lua and Guard class for instance-based model**

## Performance

- **Duration:** ~5 min (291 seconds)
- **Started:** 2026-04-04T15:47:11Z
- **Completed:** 2026-04-04T15:52:02Z
- **Tasks:** 4
- **Files modified:** 3 (runtime.lua, polyplug.lua, runtime_config.lua deleted)

## Accomplishments

- Deleted duplicate runtime_config.lua that was missing compatibility field
- Updated RuntimeConfigC FFI struct with all 5 fields matching polyplug_abi (24 bytes)
- Added compatibility constants (COMPATIBILITY_STRICT, COMPATIBILITY_RELAXED, COMPATIBILITY_YOLO)
- Removed Guard class to align with instance-based model from Phase 03

## Task Commits

Each task was committed atomically:

1. **Task 1: Remove duplicate runtime_config.lua** - `b2ec9fc` (chore)
2. **Task 2: Update RuntimeConfigC in ffi.cdef** - `89a78db` (feat)
3. **Task 3: Remove Guard class from runtime.lua** - `271d97d` (refactor)
4. **Task 4: Update polyplug.lua exports** - `0717049` (refactor)

## Files Created/Modified

- `sdks/lua/host/polyplug/runtime_config.lua` - Deleted (duplicate, missing compatibility)
- `sdks/lua/host/polyplug/runtime.lua` - Updated FFI struct, added compatibility constants, removed Guard class
- `sdks/lua/host/polyplug.lua` - Removed Guard export

## Decisions Made

- RuntimeConfigC uses explicit padding arrays (`_pad1[3]`, `_pad2[3]`) to match Rust ABI alignment
- Compatibility defaults to Strict (0) when host doesn't specify
- resolve_plugin returns raw cdata handle for instance-based pattern (host accesses interface directly)

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None - straightforward updates matching existing patterns.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Lua SDK now has correct FFI types matching polyplug_abi
- Instance-based model ready for plugin dispatch
- Guard removal aligned with Phase 03 instance model

---
*Phase: 05-sdk-updates*
*Completed: 2026-04-04*

## Self-Check: PASSED

- SUMMARY.md exists at expected location
- runtime_config.lua deleted (verified)
- All 4 task commits found in git log (b2ec9fc, 89a78db, 271d97d, 0717049)