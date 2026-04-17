---
phase: 12-sdk-instance-model
plan: 03b
subsystem: SDK
tags: [codegen, lua, csharp, js, instance-wrapper, raii]

# Dependency graph
requires:
  - phase: 12-03a
    provides: C++/Python instance wrapper pattern reference
provides:
  - Lua instance wrapper codegen (closure with __gc metamethod)
  - C# instance wrapper codegen (IDisposable class)
  - JS QuickJS instance wrapper codegen (class with destroy method)
affects: [sdk-07]

# Tech tracking
tech-stack:
  added: []
  patterns: [RAII instance wrapper pattern, closure-based wrapper (Lua), IDisposable pattern (C#), explicit destroy (JS)]

key-files:
  created: []
  modified:
    - crates/polyplugc/src/generators/lua.rs
    - crates/polyplugc/src/generators/csharp.rs
    - crates/polyplugc/src/generators/js_quickjs.rs

key-decisions:
  - "Lua wrapper uses closure-based table with __gc metamethod for automatic GC cleanup"
  - "C# wrapper uses sealed class implementing IDisposable for deterministic cleanup"
  - "JS wrapper uses class with explicit destroy() method (no deterministic cleanup in JS)"
  - "All wrappers store interface, instance, host members for dispatch"
  - "All wrappers call create_instance on construction, destroy_instance on cleanup"

patterns-established:
  - "Lua: closure table with is_valid(), destroy(), reset() methods; __gc metamethod"
  - "C#: IDisposable with Dispose() calling destroy_instance, Create() factory method"
  - "JS: class with private #instance, #interface, #host fields; explicit destroy()"
  - "Null-check pattern: all wrappers check instance.data != null before use"
  - "Nullify pattern: destroy sets instance.data = null to prevent reuse"

requirements-completed: [SDK-07]

# Metrics
duration: 15m
completed: 2026-04-08
---
# Phase 12 Plan 03b: Lua, C#, and JS Instance Wrapper Codegen Summary

**Added instance wrapper codegen to Lua, C#, and JS QuickJS generators matching Rust pattern, enabling RAII lifecycle management with create_instance/destroy_instance factory calls.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-04-08T12:30:00Z
- **Completed:** 2026-04-08T13:06:00Z
- **Tasks:** 4 (3 auto + 1 checkpoint approved)
- **Files modified:** 3

## Accomplishments
- Lua generator produces closure-based wrapper tables with `__gc` metamethod for automatic cleanup
- C# generator produces `IDisposable` sealed classes with `Dispose()` calling `destroy_instance`
- JS QuickJS generator produces classes with `destroy()` method and private instance fields
- All generators use instance-based dispatch (GuestContractInstance as first argument to dispatch calls)
- Factory methods resolve handle and call `create_instance`, returning optional type

## Task Commits

Each task was committed atomically:

1. **Task 1: Add instance wrapper generation to Lua generator** - `79877b2` (feat)
2. **Task 2: Add instance wrapper generation to C# generator** - `bf758a6` (feat)
3. **Task 3: Add instance wrapper generation to JS QuickJS generator** - `3e528fa` (feat)
4. **Task 4: Verify all generators produce instance wrappers** - checkpoint approved by user

## Files Created/Modified
- `crates/polyplugc/src/generators/lua.rs` - Lua host contract caller generation with instance wrapper
- `crates/polyplugc/src/generators/csharp.rs` - C# host caller class generation with instance wrapper
- `crates/polyplugc/src/generators/js_quickjs.rs` - JS QuickJS host caller class generation with instance wrapper

## Decisions Made

- Lua uses closure-based table pattern (no classes in Lua), with metatable containing `__gc` metamethod
- C# uses `sealed class` implementing `IDisposable` for proper RAII pattern with `Dispose()`
- JS QuickJS uses ES6-style class with private `#instance`, `#interface`, `#host` fields
- Lua wrapper exposes `is_valid()`, `destroy()`, `reset()` methods in the closure table
- C# wrapper uses static `Create(handle, host)` factory returning nullable reference
- JS wrapper uses static `Create(handle, host)` factory returning nullable instance
- All wrappers check `instance.data != null` before operations and nullify after destroy

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

| Check | Result |
|-------|--------|
| cargo test -p polyplugc | PASSED (182 tests) |
| Lua generator has __gc | 3 matches |
| Lua generator has create_instance | Multiple matches |
| Lua generator has destroy_instance | Multiple matches |
| C# generator has IDisposable | 2 matches |
| C# generator has create_instance | Multiple matches |
| C# generator has destroy_instance | Multiple matches |
| JS generator has class.*Contract | 4 matches |
| JS generator has create_instance | Multiple matches |
| JS generator has destroy_instance | Multiple matches |

## Security Mitigations (Threat Model)

| Threat ID | Mitigation | Status |
|-----------|------------|--------|
| T-12-03b-01 | Generated code checks instance.data != null before use | Implemented |
| T-12-03b-02 | destroy() sets instance.data = null to prevent reuse | Implemented |
| T-12-03b-03 | __gc (Lua), IDisposable (C#), explicit destroy() (JS) ensure cleanup | Implemented |

## Requirements Satisfied

**SDK-07**: All generators (Rust, C++, Python, Lua, C#, JS QuickJS) produce instance wrapper codegen matching Rust pattern.

Combined with 12-03a, SDK-07 is now fully satisfied across all supported languages.

## Self-Check: PASSED

- [x] crates/polyplugc/src/generators/lua.rs modified with instance wrapper generation
- [x] crates/polyplugc/src/generators/csharp.rs modified with instance wrapper generation
- [x] crates/polyplugc/src/generators/js_quickjs.rs modified with instance wrapper generation
- [x] Commit 79877b2 exists in git history (Lua instance wrapper)
- [x] Commit bf758a6 exists in git history (C# instance wrapper)
- [x] Commit 3e528fa exists in git history (JS instance wrapper)
- [x] All acceptance criteria met
- [x] User approved checkpoint

---
*Phase: 12-sdk-instance-model*
*Completed: 2026-04-08*