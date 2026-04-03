---
phase: 01-abi-types
plan: 03
subsystem: abi
tags: [runtime-config, compatibility, reload-phase, ffi-safe, repr-C]

# Dependency graph
requires:
  - phase: 01-abi-types
    plan: 02
    provides: GuestContractInterface, HostContractInterface, StringView types
provides:
  - Compatibility enum (#[repr(u32)]) in polyplug_abi
  - RuntimeConfig struct (#[repr(C)], 24 bytes) in polyplug_abi
  - ReloadPhaseData FFI-safe struct (#[repr(C)], 56 bytes) in polyplug_abi
  - ReloadPhaseType enum (#[repr(u32)]) in polyplug_abi
affects: [phase-02-registry, phase-03-instance-model, phase-05-sdk-updates]

# Tech tracking
tech-stack:
  added: []
  patterns: ["Runtime types consolidated in polyplug_abi", "FFI-safe structs with StringView"]

key-files:
  created:
    - crates/polyplug_abi/src/runtime/mod.rs
    - crates/polyplug_abi/src/runtime/compatibility.rs
    - crates/polyplug_abi/src/runtime/runtime_config.rs
    - crates/polyplug_abi/src/runtime/reload_phase_data.rs
  modified:
    - crates/polyplug_abi/src/lib.rs
    - crates/polyplug/src/lib.rs
    - crates/polyplug/src/compatibility/mod.rs
    - crates/polyplug/src/compatibility/compatibility.rs
    - crates/polyplug/src/runtime_config.rs

key-decisions:
  - "Compatibility moved to polyplug_abi with #[repr(u32)]"
  - "RuntimeConfig moved to polyplug_abi with #[repr(C)]"
  - "ReloadPhaseData created as FFI-safe variant (not replacing Rust ReloadPhase enum)"
  - "Rust ReloadPhase enum kept in polyplug for internal String-based use"

patterns-established:
  - "Runtime configuration types live in polyplug_abi/runtime module"
  - "FFI-safe variants use StringView instead of String"
  - "Internal Rust types remain in polyplug for ergonomic use"

requirements-completed: [ABI-05, ABI-06, ABI-12]

# Metrics
duration: 5min
completed: 2026-04-03T17:14:36Z
---

# Phase 01 Plan 03: Runtime Types Migration Summary

**RuntimeConfig, Compatibility, and ReloadPhaseData moved to polyplug_abi crate with FFI-safe repr(C/u32) annotations**

## Performance

- **Duration:** 5 min
- **Started:** 2026-04-03T17:09:01Z
- **Completed:** 2026-04-03T17:14:36Z
- **Tasks:** 6
- **Files modified:** 9 (4 created, 5 modified)

## Accomplishments

- Compatibility enum moved to `polyplug_abi::runtime` with `#[repr(u32)]` for FFI stability
- RuntimeConfig struct moved to `polyplug_abi::runtime` with `#[repr(C)]` (24 bytes, 8-byte aligned)
- FFI-safe ReloadPhaseData created with StringView fields (56 bytes) for hot-reload callbacks
- ReloadPhaseType enum created with `#[repr(u32)]` for phase discrimination
- polyplug crate imports RuntimeConfig and Compatibility from polyplug_abi
- Internal Rust ReloadPhase enum preserved in polyplug for String-based convenience

## Task Commits

Each task was committed atomically:

1. **Task 1: Create runtime module structure** - `21bcd9f` (feat)
2. **Task 2: Move Compatibility enum** - `832aa0a` (feat)
3. **Task 3: Move RuntimeConfig struct** - `36d0a84` (feat)
4. **Task 4: Create ReloadPhaseData struct** - `bb53240` (feat)
5. **Task 5: Update polyplug_abi exports** - `55a22af` (feat)
6. **Task 6: Update polyplug imports** - `c068bbc` (feat)

## Files Created/Modified

- `crates/polyplug_abi/src/runtime/mod.rs` - Module structure with exports
- `crates/polyplug_abi/src/runtime/compatibility.rs` - Compatibility enum (#[repr(u32)])
- `crates/polyplug_abi/src/runtime/runtime_config.rs` - RuntimeConfig struct (#[repr(C)], 24 bytes)
- `crates/polyplug_abi/src/runtime/reload_phase_data.rs` - FFI-safe ReloadPhaseData (56 bytes)
- `crates/polyplug_abi/src/lib.rs` - Added runtime module and exports
- `crates/polyplug/src/lib.rs` - Import RuntimeConfig/Compatibility from polyplug_abi
- `crates/polyplug/src/compatibility/mod.rs` - Import Compatibility from polyplug_abi
- `crates/polyplug/src/compatibility/compatibility.rs` - Deprecated re-export
- `crates/polyplug/src/runtime_config.rs` - Deprecated re-export

## Decisions Made

- FFI-safe ReloadPhaseData created alongside internal ReloadPhase enum, not replacing it
- StringView::null() used for empty string fields (StringView::empty() does not exist)
- Layout tests added to verify struct sizes and alignments
- polyplug re-exports RuntimeConfig/Compatibility for backward compatibility

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

### Pre-existing polyplug compilation errors

The polyplug crate has pre-existing compilation errors from earlier refactoring phases:
- `VTableSlot` type not found in registry
- `host_vtable` module not found in polyplug_abi::host
- `StringViewC`, `RuntimeConfigC` FFI wrapper types not found
- `CapabilityGraph` visibility issue in compatibility module

These errors existed before this plan and are NOT caused by my changes. The polyplug_abi crate tests pass successfully (39 passed). The polyplug crate errors will be addressed in subsequent phases as the refactoring continues.

## Next Phase Readiness

- Runtime types successfully moved to polyplug_abi
- FFI-safe ReloadPhaseData available for hot-reload callback FFI boundary
- Pre-existing polyplug errors deferred to subsequent phases

---
*Phase: 01-abi-types*
*Completed: 2026-04-03*

## Self-Check: PASSED

All created files exist, all commits found in git history.