---
phase: 11-guest-calling-convention-missing-introspection
plan: 09
subsystem: codegen, testing
tags: [codegen, host-interface, self-passing, tests, benchmarks]

# Dependency graph
requires:
  - phase: 11-08
    provides: polyplugc calling convention update
provides:
  - Fixed codegen for HostInterface import and mod.rs generation
  - Updated tests and benchmarks to use HostInterface self-passing pattern
  - Working host and guest examples
affects: [codegen, examples, tests]

# Tech tracking
tech-stack:
  added: []
  patterns: [self-passing pattern, HostInterface as first parameter]

key-files:
  created:
    - examples/hosts/rust/src/generated/mod.rs
    - examples/hosts/rust/src/generated/host/mod.rs
    - crates/polyplug/build.rs
  modified:
    - crates/polyplugc/src/generators/rust.rs
    - crates/polyplug/tests/*.rs
    - crates/polyplug/benches/*.rs
    - examples/hosts/rust/src/main.rs

key-decisions:
  - "HostContractInterface stubs use self-passing pattern with _this: *const HostContractInterface"
  - "Generated mod.rs files force_regenerate=true to ensure module structure stays in sync"
  - "Host example uses as_context_ptr() to get *const HostInterface"

patterns-established:
  - "Self-passing: all HostContractInterface methods take _this: *const HostContractInterface as first param"
  - "Module generation: codegen outputs mod.rs files for proper Rust module structure"

requirements-completed: []

# Metrics
duration: 45min
completed: 2026-04-08
---

# Phase 11 Plan 09: Test Fixture Calling Convention Update Summary

**Fixed codegen to generate proper HostInterface imports, mod.rs files, and updated all tests/benchmarks to use the self-passing HostInterface pattern**

## Performance

- **Duration:** 45 min
- **Started:** 2026-04-08T00:15:00Z
- **Completed:** 2026-04-08T01:00:00Z
- **Tasks:** 5
- **Files modified:** 28

## Accomplishments

- Added HostInterface import to host_callers.rs and interface_factories.rs codegen
- Fixed HostContractInterface stub signatures to use self-passing pattern
- Generated host/mod.rs and root mod.rs for proper Rust module structure
- Updated all tests and benchmarks to use HostInterface instead of RuntimeContext
- Fixed host example to use as_context_ptr() for HostInterface access

## Task Commits

Each task was committed atomically:

1. **Task 1-3: Codegen fixes** - `924a3e7` (fix) - HostInterface import, mod.rs generation, stub signatures
2. **Task 3: Test/benchmark updates** - `4764f64` (fix) - All tests and benchmarks updated to HostInterface pattern
3. **Task 4: Example regeneration** - `dade7a3` (fix) - Regenerated examples with fixed codegen
4. **Build script** - `a58a45b` (chore) - Added build.rs for test fixture paths

## Files Created/Modified

- `crates/polyplugc/src/generators/rust.rs` - Added HostInterface imports, mod.rs generation, runtime field
- `crates/polyplug/tests/*.rs` - Updated to HostInterface self-passing pattern (18 files)
- `crates/polyplug/benches/*.rs` - Updated to HostInterface self-passing pattern (3 files)
- `examples/hosts/rust/src/generated/mod.rs` - Root module declaration
- `examples/hosts/rust/src/generated/host/mod.rs` - Host module declarations
- `examples/hosts/rust/src/main.rs` - Fixed as_context_ptr() usage
- `crates/polyplug/build.rs` - Build script for test fixture paths

## Decisions Made

1. **HostContractInterface self-passing:** Changed stub signatures from `_host: *const HostInterface` to `_this: *const HostContractInterface` to match the actual ABI where the interface passes itself as the first parameter
2. **Force regenerate mod.rs:** Set `force_regenerate: true` for mod.rs files to ensure module structure stays in sync with generated code
3. **Runtime field initialization:** Added `runtime: std::ptr::null_mut()` to HostContractInterface literals since it's filled in by the runtime during registration

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Added missing HostInterface import to interface_factories.rs**
- **Found during:** Task 1 (checking compilation after regenerate)
- **Issue:** interface_factories.rs was missing the HostInterface import, causing compilation failure
- **Fix:** Added `use polyplug_abi::HostInterface;` to imports
- **Files modified:** crates/polyplugc/src/generators/rust.rs
- **Verification:** cargo check passes
- **Committed in:** 924a3e7 (Task 1 commit)

**2. [Rule 1 - Bug] Fixed HostContractInterface stub signatures**
- **Found during:** Task 1 (checking compilation)
- **Issue:** Stub functions used `_host: *const HostInterface` but HostContractInterface expects `_this: *const HostContractInterface`
- **Fix:** Changed stub signatures to match the self-passing pattern for HostContractInterface
- **Files modified:** crates/polyplugc/src/generators/rust.rs
- **Verification:** cargo check passes
- **Committed in:** 924a3e7 (Task 1 commit)

**3. [Rule 2 - Missing Critical] Added runtime field to HostContractInterface literals**
- **Found during:** Task 1 (checking compilation)
- **Issue:** HostContractInterface struct now has a `runtime` field but codegen didn't initialize it
- **Fix:** Added `runtime: std::ptr::null_mut()` to struct literals (filled by runtime during registration)
- **Files modified:** crates/polyplugc/src/generators/rust.rs
- **Verification:** cargo check passes
- **Committed in:** 924a3e7 (Task 1 commit)

**4. [Rule 3 - Blocking] Fixed host example as_context() call**
- **Found during:** Task 4 (checking host example)
- **Issue:** main.rs used `runtime.as_context()` but the method is `as_context_ptr()`
- **Fix:** Changed to `runtime.as_context_ptr()` which returns `*const HostInterface`
- **Files modified:** examples/hosts/rust/src/main.rs
- **Verification:** cargo check passes
- **Committed in:** dade7a3 (Task 4 commit)

---

**Total deviations:** 4 auto-fixed (2 missing critical, 1 bug, 1 blocking)
**Impact on plan:** All auto-fixes necessary for correctness. The codegen was generating incompatible code due to missing imports and incorrect signature patterns. No scope creep.

## Issues Encountered

- Test and benchmark files already had partial updates from previous work - verified they were complete and committed
- Build.rs was untracked - added to repository since it's needed for test fixture path configuration

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- All tests pass (171 tests)
- All examples compile
- Codegen produces correct output for both host and guest sides
- Ready for any remaining gap closure work

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Completed: 2026-04-08*