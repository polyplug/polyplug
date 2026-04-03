---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: Architecture Refactor
status: planning
last_updated: "2026-04-03T12:00:00.000Z"
progress:
  total_phases: 0
  completed_phases: 0
  total_plans: 0
  completed_plans: 0
  percent: 0
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

Phase: Not started (defining requirements)
Plan: —
Status: Defining requirements
**Phase:** —
**Plan:** Not started
**Status:** Milestone v1.1 initialized
**Progress:** [----------] 0%

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
| 2026-04-03 | Rename Plugin Contract → Guest Contract | Clear Host/Guest separation |
| 2026-04-03 | RuntimeAbi naming | Clearer than HostVTable (host ≠ runtime) |
| 2026-04-03 | All public ABI structs repr(C) | Single source of truth, no *C types |
| 2026-04-03 | Host contracts: singleton or multi-instance | Flexibility for host-provided services |
| 2026-04-03 | ContractHandle without generation | Instances destroyed before hot-reload |
| 2026-04-03 | PluginContext init-time only | Two-context model: rt_ctx always, PluginContext during init |
| 2026-04-03 | call_method for cross-dispatch | Plugin-plugin across different dispatch types |

### Active TODOs

(None — milestone just initialized)

### Blockers

None.

### Session Continuity

Milestone v1.1 initialized after deep exploration of:
- Current architecture (ABI, utils, runtime, registry, dispatch)
- PluginGuard/VTableSlot patterns
- BundleLoader and bundle loading flow
- Codegen system (host/guest SDK generation)
- Manifest and dependency handling

Key architectural decisions made:
1. Instance model replaces "guard" model
2. Factory pattern in interfaces (create/destroy_instance)
3. Callback-based hot-reload (host destroys instances)
4. Clear naming: Guest Contract, Host Contract, RuntimeAbi

**Project Status:** Ready for requirements definition.

---
*State initialized: 2026-04-03*