---
phase: 18-consolidate-ffi-to-hostinterface
plan: 04
subsystem: sdk
tags: [ffi, host-interface, sdk, lua, javascript, cpp, self-passing-pattern]

requires:
  - phase: 18
    plan: 02
    provides: FFI surface reduction to create/destroy only
provides:
  - Lua SDK Runtime holds HostInterface pointer
  - JS SDK Runtime holds HostInterface pointer
  - C++ SDK Runtime holds HostInterface pointer
  - All SDKs use self-passing pattern for HostInterface calls
affects: [sdk-hosts, codegen]

tech-stack:
  added: []
  patterns: [host-interface-pointer, self-passing-pattern, ffi-struct-field-access]

key-files:
  created: []
  modified:
    - sdks/lua/host/polyplug/runtime.lua
    - sdks/cpp/host/polyplug/runtime.hpp
    - sdks/js/host/polyplug/mod.js

key-decisions:
  - "All SDKs hold HostInterface pointer (not OpaqueRuntime)"
  - "All operations call through HostInterface struct fields"
  - "Backward compatibility aliases added for renamed methods"
  - "find_by_bundle deprecated (removed from FFI surface)"

patterns-established:
  - "Self-passing pattern: func(host_ptr, args) for all HostInterface calls"
  - "FFI struct field access via offset or dereference"

requirements-completed:
  - D-18-30
  - D-18-31
  - D-18-32

duration: 5min
completed: 2026-04-10
---

# Phase 18 Plan 04: Lua/JS/C++ SDK Updates Summary

**Updated Lua, JavaScript, and C++ SDKs to use HostInterface pointer-based API with self-passing pattern for all operations.**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-10T16:34:53Z
- **Completed:** 2026-04-10T16:39:53Z
- **Tasks:** 5 (combined into single commit)
- **Files modified:** 3

## Accomplishments
- Lua SDK Runtime class holds HostInterface pointer, calls methods through struct fields
- C++ SDK Runtime class holds HostInterface pointer, uses self-passing pattern
- JavaScript SDK Runtime class holds HostInterface pointer, reads fields at offsets
- All SDKs define HostInterface struct (144 bytes, 18 pointer fields) in FFI bindings
- Backward compatibility aliases added for renamed methods (find -> find_guest_contract)

## Task Commits

All tasks combined into single commit:

1. **Task 1-5: Update Lua/JS/C++ SDKs for HostInterface API** - `4463acb` (feat)

**Plan metadata:** To be committed after SUMMARY creation

## Files Created/Modified
- `sdks/lua/host/polyplug/runtime.lua` - Lua SDK with HostInterface-based Runtime
- `sdks/cpp/host/polyplug/runtime.hpp` - C++ SDK with HostInterface-based Runtime
- `sdks/js/host/polyplug/mod.js` - JavaScript SDK with HostInterface-based Runtime

## Decisions Made
- Runtime class holds HostInterface* directly (not separate opaque handle)
- All operations use self-passing pattern: `func(host_ptr, args)`
- Backward compatibility aliases added: `find()` -> `find_guest_contract()`, `findAllByContract()` -> `findAllGuestContracts()`
- `find_by_bundle` method deprecated (returns NULL_HANDLE) since it was removed from FFI surface in 18-02
- HostInterface struct defined in each SDK's FFI bindings with explicit offsets

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None - straightforward SDK updates following Python/C# patterns from 18-03.

## Next Phase Readiness
- All SDKs now use unified HostInterface API
- Ready for codegen updates (18-05) to generate HostInterface-based bindings

## Self-Check: PASSED

---
*Phase: 18-consolidate-ffi-to-hostinterface*
*Completed: 2026-04-10*