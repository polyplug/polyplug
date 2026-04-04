---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: milestone
status: executing
last_updated: "2026-04-04T12:34:43.268Z"
progress:
  total_phases: 7
  completed_phases: 1
  total_plans: 20
  completed_plans: 19
  percent: 95
---

# STATE: polyplug Architecture Refactor

**Project:** polyplug Architecture Refactor
**Started:** 2026-04-03
**Last Active:** 2026-04-03

## Project Reference

**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

**Goal:** Refactor architecture for instance-based plugin model with FFI-first design

**Constraints:**

- Architecture: Core crate must have zero loader-specific code or dependencies
- Safety: Host must destroy all instances before hot-reload completes
- Compatibility: Breaking changes acceptable — not published yet

## Current Position

Phase: 03 (instance-model) — EXECUTING
Plan: 4 of 5 complete
**Phase:** 3
**Plan:** 04 complete
**Status:** Executing Phase 03
**Progress:** [██████████] 95%

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 0/7 |
| Plans completed | 3 |
| Requirements covered | 50/50 mapped |
| Phase 03-instance-model P01 | 15 | 2 tasks | 3 files |
| Phase 03-instance-model P04 | 261 | 4 tasks | 2 files |

### Plan Execution Times

| Plan | Duration | Tasks | Files |
|------|----------|-------|-------|
| Phase 01-abi-types P01 | 113s | 2 | 2 |
| Phase 01-abi-types P02 | 836s | 7 | 14 |
| Phase 01-abi-types P03 | 300s | 6 | 9 |
| Phase 03-instance-model P02 | 45s | 3 | 7 |
| Phase 03-instance-model P03 | 180s | 3 | 2 |

## Accumulated Context

### Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-03 | Remove "vtable" naming | Confusing terminology; use GuestContractInterface |
| 2026-04-03 | Remove VTableSlot wrapper | Unnecessary indirection; registry stores interfaces directly |
| 2026-04-03 | Instance-based model | Host creates/owns instances, not "guards" |
| 2026-04-03 | create/destroy_instance in interfaces | Contract-specific factory pattern |
| 2026-04-03 | Instance as first dispatch arg | Consistent for native and VM dispatch |
| 2026-04-03 | Hot-reload via callback | Host destroys instances; no Arc quiescence pattern |
| 2026-04-03 | Rename Plugin Contract -> Guest Contract | Clear Host/Guest separation |
| 2026-04-03 | RuntimeAbi naming | Clearer than HostVTable (host != runtime) |
| 2026-04-03 | All public ABI structs repr(C) | Single source of truth, no *C types |
| 2026-04-03 | Host contracts: singleton or multi-instance | Flexibility for host-provided services |
| 2026-04-03 | ContractHandle without generation | Instances destroyed before hot-reload |
| 2026-04-03 | PluginContext init-time only | Two-context model: rt_ctx always, PluginContext during init |
| 2026-04-03 | call_method for cross-dispatch | Plugin-plugin across different dispatch types |
| 2026-04-03 | GuestContractInstance/HostContractInstance opaque handles | Type-safe instance handles, not bare pointers |
| 2026-04-03 | Manifest parsing stays in core | Move TOML dependency later, not blocking this milestone |
| 2026-04-03 | GuestContractId hash prefix: "guest_contract:" | Consistent naming with Guest/Host terminology (breaking change) |
| 2026-04-03 | Deprecation alias PluginContractId = GuestContractId | Smooth migration for dependent code |
| 2026-04-03 | GuestContractInterface 56 bytes | Version 12 bytes causes padding alignment |
| 2026-04-03 | HostContractInterface 64 bytes | singleton bool causes padding cascade |
| 2026-04-03 | RuntimeAbi 64 bytes | call_method + get_host_contract added |
| 2026-04-03 | Legacy aliases PluginInterface/HostVTable | Smooth transition for dependent code |
| 2026-04-03 | Compatibility moved to polyplug_abi/runtime | #[repr(u32)] for FFI stability |
| 2026-04-03 | RuntimeConfig moved to polyplug_abi/runtime | #[repr(C)], 24 bytes, single source of truth |
| 2026-04-03 | ReloadPhaseData as FFI-safe variant | StringView fields, kept Rust ReloadPhase for internal use |
| 2026-04-03 | Rust ReloadPhase enum preserved | String-based convenience, not replaced by FFI variant |
| 2026-04-04 | Double-check locking for singleton cache | Prevents race conditions in singleton instance creation |
| 2026-04-04 | call_method placeholder documented | Requires instance-contract mapping for full implementation |

- [Phase 03-instance-model]: singleton defaults to false via #[serde(default)] - explicit opt-in
- [Phase 03-instance-model]: polyplug_utils visibility fixed: modules public, helper functions added

### Active TODOs

(None — executing phase plans)

### Blockers

- Phase 01 ABI changes not integrated into polyplug crate: GuestContractId/BundleId type mismatches, missing Clone impl, RuntimeConfigC vs RuntimeConfig, HostContractInterface structure changed

### Roadmap Evolution

- Phase 7 added: Replace opaque c_void pointers with typed handles

### Session Continuity

Roadmap created for v1.1 Architecture Refactor:

- Phase 1: ABI Types (19 requirements)
- Phase 2: Registry (6 requirements)
- Phase 3: Instance Model (16 requirements)
- Phase 4: Hot-Reload (6 requirements)
- Phase 5: SDK Updates (7 requirements)
- Phase 6: Cleanup (4 requirements)
- Phase 7: Typed Handles (NEW - replace c_void pointers)

Total: 50+ requirements, 7 phases.

**Project Status:** Phase 01 Plan 03 complete. Next: Phase 01 Plan 04

---
*State initialized: 2026-04-03*
*Roadmap created: 2026-04-03*
