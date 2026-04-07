---
phase: 11-guest-calling-convention-missing-introspection
plan: 07
subsystem: loaders
tags: [vm-loaders, hostinterface, self-passing-pattern, tls, codegen]

# Dependency graph
requires:
  - phase: 11-01
    provides: HostInterface struct with self-passing pattern
  - phase: 11-02
    provides: Deleted RuntimeContext/HostContext, renamed RuntimeAbi to HostInterface
provides:
  - All VM loaders updated for HostInterface self-passing pattern
  - TLS bundle_id tracking wired in all VM loaders
  - Rust codegen fixed for new register_contract signature
affects: [codegen, examples, tests]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Self-passing pattern for HostInterface
    - TLS bundle_id tracking for dependency enforcement

key-files:
  created: []
  modified:
    - crates/polyplug_python/src/lib.rs
    - crates/polyplug_dotnet/src/context.rs
    - crates/polyplug_dotnet/src/lib.rs
    - crates/polyplug_lua/src/loader.rs
    - crates/polyplugc/src/generators/rust.rs

key-decisions:
  - "All VM loaders now pass HostInterface pointer directly to polyplug_init (host, ctx) signature"
  - "TLS bundle_id set before polyplug_init and cleared after for get_dependencies introspection"

patterns-established:
  - "Self-passing pattern: HostInterface functions receive host pointer as first parameter"
  - "TLS tracking: set_init_bundle_id before init, clear_init_bundle_id after init"

requirements-completed: []

# Metrics
duration: 15min
completed: 2026-04-07
---

# Phase 11 Plan 07: VM Loader HostInterface Updates Summary

**Updated all VM loaders (Python, .NET, JS, Lua) for HostInterface self-passing pattern and fixed Rust codegen for new register_contract signature.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-07T21:00:00Z
- **Completed:** 2026-04-07T21:15:00Z
- **Tasks:** 4
- **Files modified:** 11

## Accomplishments
- Updated polyplug_python to use HostInterface with self-passing pattern
- Updated polyplug_dotnet InitFn signature to (host, ctx) -> u32
- Verified polyplug_js already uses correct 2-argument signature
- Updated polyplug_lua to use 2-argument polyplug_init signature
- Fixed Rust codegen to emit correct register_contract calls

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix polyplug_python imports and HostInterface usage** - `7ced0c3` (fix)
2. **Task 2: Fix polyplug_dotnet imports and HostInterface usage** - `36d4a8f` (fix)
3. **Task 3: Fix polyplug_js and polyplug_lua if needed** - `22b5354` (fix)
4. **Task 4: Verify workspace compiles** - `b05d2f5` (fix - includes codegen fix)

## Files Created/Modified
- `crates/polyplug_python/src/lib.rs` - Updated imports, HostInterface usage, TLS tracking
- `crates/polyplug_dotnet/src/context.rs` - Updated InitFn type signature
- `crates/polyplug_dotnet/src/lib.rs` - Updated managed_init call to 2-argument signature
- `crates/polyplug_lua/src/loader.rs` - Updated polyplug_init call to 2 arguments
- `crates/polyplugc/src/generators/rust.rs` - Fixed register_contract to use 'host' instead of 'rt_ctx'
- `examples/guests/rust/*/generated/guest/init.rs` - Regenerated with fixed codegen
- `examples/guests/rust/*/generated/guest/interfaces.rs` - Regenerated with HostInterface import

## Decisions Made
- polyplug_js was already using correct 2-argument signature from previous wave - no changes needed
- Rust codegen needed rt_ctx -> host fix for register_contract calls
- Added HostInterface import to interfaces.rs codegen to fix compilation errors

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added HostInterface import to Rust codegen**
- **Found during:** Task 4 (workspace verification)
- **Issue:** Generated interfaces.rs used HostInterface type but didn't import it
- **Fix:** Added `use polyplug_guest::HostInterface;` to codegen template
- **Files modified:** crates/polyplugc/src/generators/rust.rs
- **Verification:** All loaders compile successfully
- **Committed in:** b05d2f5 (codegen fix commit)

**2. [Rule 1 - Bug] Fixed rt_ctx variable in codegen register_contract call**
- **Found during:** Task 4 (workspace verification)
- **Issue:** Generated init.rs used undefined `rt_ctx` variable instead of `host`
- **Fix:** Changed `(host.register_contract)(rt_ctx, ...)` to `(host.register_contract)(host, ...)`
- **Files modified:** crates/polyplugc/src/generators/rust.rs
- **Verification:** Regenerated examples compile
- **Committed in:** b05d2f5 (codegen fix commit)

---

**Total deviations:** 2 auto-fixed (1 missing critical, 1 bug)
**Impact on plan:** Both auto-fixes necessary for correctness. No scope creep.

## Issues Encountered
- Example hosts (examples/hosts/rust) have remaining compilation errors unrelated to loaders - these are out of scope for this gap-closure plan

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All VM loaders compile and use HostInterface self-passing pattern
- TLS bundle_id tracking wired for get_dependencies introspection
- Rust codegen produces correct code for new ABI

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Completed: 2026-04-07*