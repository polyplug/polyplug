---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
plan: 04
subsystem: sdk
tags: [ffi, abi, ctypes, cffi, lua-ffi, deno-ffi, cffi-interface, codegen-imports]

# Dependency graph
requires:
  - phase: 19-02
    provides: "Typed function pointer signatures, size assertions, auto-generated abi.* files with correct struct layouts"
  - phase: 19-03
    provides: "Helper method preservation in abi.* files, auto-generated file headers"
provides:
  - "All 5 SDK host files import FFI struct types from auto-generated abi.* files"
  - "Python polyplug_abi shared package re-exports from auto-generated abi.py"
  - "Lua/JS polyplug_abi re-export files for package conventions"
  - "Zero hand-written FFI struct definitions in any SDK host file"
affects: [19-05, sdk_validator, ci]

# Tech tracking
tech-stack:
  added: []
  patterns: ["SDK host files import from auto-generated abi.* — no hand-written struct definitions", "Python polyplug_abi shared package re-exports via star import from abi module", "JS offset constants imported from abi.ts rather than hand-written inline"]

key-files:
  created:
    - ".planning/phases/19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc/deferred-items.md"
  modified:
    - "sdks/python/host/polyplug/runtime.py"
    - "sdks/python/polyplug_abi/polyplug_abi/abi.py"
    - "sdks/csharp/host/NativeMethods.cs"
    - "sdks/lua/host/polyplug/runtime.lua"
    - "sdks/lua/abi/polyplug_abi.lua"
    - "sdks/js/host/polyplug/mod.js"
    - "sdks/js/abi/polyplug_abi.ts"
    - "sdks/cpp/host/polyplug/runtime.hpp"

key-decisions:
  - "Kept CFUNCTYPE wrapper definitions in Python runtime.py since they are calling-convention helpers, not struct definitions (no _fields_)"
  - "Kept host-specific ffi.cdef block in Lua runtime.lua for FFI function declarations and host-side config types not in abi.lua"
  - "Used imported offset constants in JS mod.js via HOST_INTERFACE_OFFSETS object that maps imported constants"
  - "Updated Python RuntimeConfig usage to 16-byte layout (D-22) with compatibility, hot_reload_enabled, on_reload"
  - "Updated Python HostContractInterface to flat struct (D-25) instead of header+dispatch decomposition"

patterns-established:
  - "SDK host files: zero struct definitions, import from auto-generated abi.*"
  - "Re-export pattern: polyplug_abi.* files re-export from auto-generated abi.* files (D-28)"

requirements-completed: [D-10, D-11, D-26, D-27, D-28, D-34]

# Metrics
duration: 11min
completed: 2026-04-12
---

# Phase 19 Plan 04: SDK Host File Cleanup Summary

**Deleted all hand-written FFI struct definitions from 5 SDK host files (Python, C#, Lua, JS, C++) replacing them with imports from auto-generated abi.* files, saving ~1850 lines of error-prone duplicate definitions**

## Performance

- **Duration:** 11 min
- **Started:** 2026-04-12T23:42:03Z
- **Completed:** 2026-04-12T23:53:28Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- Removed all ctypes.Structure subclasses from Python runtime.py (RuntimeConfig, HostInterface, HostContractInterface header+dispatch, ReloadPhaseFfi, CFUNCTYPE typedefs)
- Removed all [StructLayout] structs and [UnmanagedFunctionPointer] delegates from C# NativeMethods.cs
- Removed HostInterface, HostContractInterface, RuntimeConfig ffi.cdef definitions from Lua runtime.lua
- Replaced hand-written HOST_INTERFACE_OFFSETS in JS mod.js with imports from auto-generated abi.ts
- Removed hand-written HostInterface struct and old 24-byte RuntimeConfig from C++ runtime.hpp
- Updated Python RuntimeConfig to new 16-byte layout (D-22) and HostContractInterface to flat struct (D-25)
- Created re-export files for polyplug_abi shared packages in Python, Lua, and JS

## Task Commits

Each task was committed atomically:

1. **Task 1: Delete hand-written FFI structs from Python and C# SDK host files** - `66f4ffe` (feat)
2. **Task 2: Delete hand-written FFI structs from Lua, JS, and C++ SDK host files** - `ae5b5d6` (feat)

## Files Created/Modified
- `sdks/python/host/polyplug/runtime.py` - Removed ~700 lines of hand-written structs, replaced with imports from polyplug_abi
- `sdks/python/polyplug_abi/polyplug_abi/abi.py` - Replaced hand-maintained file with re-exports from auto-generated abi.py
- `sdks/csharp/host/NativeMethods.cs` - Removed ~230 lines of structs/delegates, replaced with using Polyplug.Abi
- `sdks/lua/host/polyplug/runtime.lua` - Removed ~80 lines of ffi.cdef struct definitions, kept host-specific declarations
- `sdks/lua/abi/polyplug_abi.lua` - Replaced hand-maintained file with re-exports from abi.lua
- `sdks/js/host/polyplug/mod.js` - Replaced hand-written offset constants with imports from abi.ts
- `sdks/js/abi/polyplug_abi.ts` - Replaced hand-maintained file with re-exports from abi.ts
- `sdks/cpp/host/polyplug/runtime.hpp` - Removed ~90 lines of hand-written structs, uses abi.hpp types

## Decisions Made
- **Kept Python CFUNCTYPE wrappers:** The CFUNCTYPE definitions in runtime.py are calling-convention helpers used to wrap raw function pointers, not struct definitions (_fields_). They stay in the host file as they describe how to CALL the functions, not the struct layouts.
- **Kept Lua host-specific ffi.cdef:** The FFI function declarations (polyplug_runtime_create/destroy) and host-side config types (HostRuntimeConfig, RuntimeCreateOptions) are not ABI types -- they are host-specific and remain in runtime.lua.
- **JS offset import mapping:** Rather than changing every HOST_INTERFACE_OFFSETS reference throughout mod.js, the imported constants are mapped into the same object structure. This preserves all existing callHostMethod references.
- **Python RuntimeConfig adapter:** The new 16-byte RuntimeConfig (D-22) has different fields than the old 24-byte version. The set_config method now accepts a simpler config object with just hot_reload_enabled and compatibility.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Updated Python register_host_contract to use flat HostContractInterface**
- **Found during:** Task 1 (Python runtime.py update)
- **Issue:** The old HostContractInterface used a header+dispatch decomposition pattern. The auto-generated version (D-25) is a flat struct with contract_id, contract_version, singleton, dispatch_type, runtime, create_instance, destroy_instance, dispatch fields directly.
- **Fix:** Rewrote the register_host_contract method to populate the flat struct with Version import for contract_version field.
- **Files modified:** sdks/python/host/polyplug/runtime.py
- **Committed in:** 66f4ffe

**2. [Rule 2 - Missing Critical] Updated CFFIBackend to use c_void_p for options**
- **Found during:** Task 1 (Python runtime.py update)
- **Issue:** CFFIBackend referenced old RuntimeConfig field names (hot_reload_max_retries, etc.) from the 24-byte layout. Simplified to pass void pointer since the new RuntimeConfig is created via ctypes and passed by address.
- **Files modified:** sdks/python/host/polyplug/runtime.py
- **Committed in:** 66f4ffe

---

**Total deviations:** 2 auto-fixed (2 missing critical functionality)
**Impact on plan:** Both auto-fixes update the host code to match the new struct layouts from auto-generated abi.* files. No scope creep.

## Deferred Issues
- C++ `handle.hpp` references `GuestContractHandle::generation` which no longer exists in auto-generated abi.hpp (D-23). Not in plan scope -- logged in deferred-items.md.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 5 SDK host files clean of hand-written FFI struct definitions
- All imports from auto-generated abi.* files verified
- Build passes
- Ready for Plan 19-05 (final verification and PluginRegistrar removal)

---
*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Completed: 2026-04-12*

## Self-Check: PASSED

- All 8 modified files verified present
- Commit 66f4ffe (Task 1) verified in git log
- Commit ae5b5d6 (Task 2) verified in git log
- cargo build -p polyplug_abi: success
- grep _fields_ python/runtime.py: 0
- grep StructLayout csharp/NativeMethods.cs: 0 (only in comment)
- grep 'struct HostInterface {' cpp/runtime.hpp: 0
- All 5 SDK host files import from abi.* files
- No stale generation references in modified host files
