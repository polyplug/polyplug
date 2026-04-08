---
phase: 12-sdk-instance-model
plan: 03a
subsystem: SDK
tags: [codegen, cpp, python, instance-wrapper, raii]

# Dependency graph
requires:
  - phase: 12-01
    provides: SDK type import verification
  - phase: 12-02
    provides: JS SDK ABI type naming update
provides:
  - C++ instance wrapper codegen (RAII class with create_instance/destroy_instance lifecycle)
  - Python instance wrapper codegen (__init__/__del__ lifecycle management)
affects: [sdk-07]

# Tech tracking
tech-stack:
  added: []
  patterns: [RAII instance wrapper pattern, instance-based dispatch (GuestContractInstance as first arg)]

key-files:
  created: []
  modified:
    - crates/polyplugc/src/generators/cpp.rs
    - crates/polyplugc/src/generators/python.rs

key-decisions:
  - "C++ wrapper uses static create() factory returning std::optional<Self>"
  - "Python wrapper uses __init__ that raises ValueError on failure, create() returns Optional[Self]"
  - "Both wrappers store interface_, instance_, host_ members for dispatch"
  - "Dispatch signature changed to (GuestContractInstance, args, out) from (args, out)"

patterns-established:
  - "RAII pattern: destructor/__del__ calls destroy_instance, nullifies instance.data"
  - "Factory pattern: create() resolves handle, calls create_instance, returns optional"
  - "Move semantics: move constructor transfers instance, nulls source to prevent double-destroy"
  - "Reset pattern: destroy existing, create new instance for recovery"

requirements-completed: [SDK-07]

# Metrics
duration: 12m
completed: 2026-04-08
---
# Phase 12 Plan 03a: C++ and Python Instance Wrapper Codegen Summary

**Added instance wrapper codegen to C++ and Python generators matching Rust pattern, enabling RAII lifecycle management with create_instance/destroy_instance factory calls.**

## Performance

- **Duration:** 12 min
- **Started:** 2026-04-08T12:43:19Z
- **Completed:** 2026-04-08T12:55:00Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments
- C++ generator produces RAII instance wrapper classes with destructor calling destroy_instance
- Python generator produces instance wrapper classes with __del__ calling destroy_instance
- Both generators use instance-based dispatch (GuestContractInstance as first argument to dispatch calls)
- Factory methods create() resolve handle and call create_instance, returning optional type

## Task Commits

Each task was committed atomically:

1. **Task 1: Add instance wrapper generation to C++ generator** - `5bbbbea` (feat)
2. **Task 2: Add instance wrapper generation to Python generator** - `3c2e0d6` (feat)

## Files Created/Modified
- `crates/polyplugc/src/generators/cpp.rs` - C++ host contract class generation with instance wrapper
- `crates/polyplugc/src/generators/python.rs` - Python host caller class generation with instance wrapper

## Decisions Made

- C++ uses static factory `create(handle, host)` returning `std::optional<Self>` for ergonomic construction
- Python uses `__init__` that raises `ValueError` on failure; `create()` factory wraps in try/catch to return `Optional[Self]`
- Both wrappers store three members: `interface_` (resolved pointer), `instance_` (GuestContractInstance), `host_` (HostInterface pointer)
- Dispatch signature updated to include GuestContractInstance as first parameter, matching Rust SDK pattern
- Move semantics in C++ explicitly transfer instance and null source to prevent double-destroy bug

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

| Check | Result |
|-------|--------|
| cargo test -p polyplugc | PASSED (182 tests) |
| C++ generator has "class.*Contract" | 1 match (generate_cpp_host_contract) |
| C++ generator has create_instance | Multiple matches |
| C++ generator has destroy_instance | Multiple matches |
| Python generator has __del__ | 2 matches |
| Python generator has _instance member | 38 matches |
| Python generator has create_instance | 18 matches |
| Python generator has destroy_instance | 11 matches |

## Security Mitigations (Threat Model)

| Threat ID | Mitigation | Status |
|-----------|------------|--------|
| T-12-03a-01 | Generated code checks instance.data != null before use | Implemented |
| T-12-03a-02 | destroy() sets instance.data = null to prevent reuse | Implemented |
| T-12-03a-03 | RAII/__del__ ensures cleanup on scope exit | Implemented |

## Requirements Satisfied

**SDK-07**: C++ and Python generators produce instance wrapper codegen matching Rust pattern.

## Self-Check: PASSED

- [x] crates/polyplugc/src/generators/cpp.rs modified with instance wrapper generation
- [x] crates/polyplugc/src/generators/python.rs modified with instance wrapper generation
- [x] Commit 5bbbbea exists in git history (C++ instance wrapper)
- [x] Commit 3c2e0d6 exists in git history (Python instance wrapper)
- [x] All acceptance criteria met

---
*Phase: 12-sdk-instance-model*
*Completed: 2026-04-08*