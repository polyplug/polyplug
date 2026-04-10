---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: milestone
status: executing
last_updated: "2026-04-10T12:45:11.130Z"
progress:
  total_phases: 16
  completed_phases: 14
  total_plans: 80
  completed_plans: 78
  percent: 98
---

# STATE: polyplug Architecture Refactor

**Project:** polyplug Architecture Refactor
**Started:** 2026-04-03
**Last Active:** 2026-04-10

## Project Reference

**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

**Goal:** Refactor architecture for instance-based plugin model with FFI-first design

**Constraints:**

- Architecture: Core crate must have zero loader-specific code or dependencies
- Safety: Host must destroy all instances before hot-reload completes
- Compatibility: Breaking changes acceptable — not published yet

## Current Position

Phase: 17 (refactor-contractregistry-to-unified-runtimestore) — EXECUTING
Plan: 1 of 2
**Status:** Executing Phase 17
**Progress:** [██████████] 100%

## Phase Completion Summary

| Phase | Name | Status | Date |
|-------|------|--------|------|
| 01 | ABI Types | Complete | 2026-04-04 |
| 02 | Registry | Complete | 2026-04-04 |
| 03 | Instance Model | Complete | 2026-04-04 |
| 04 | Hot-Reload | Complete | 2026-04-04 |
| 05 | SDK Updates | Complete | 2026-04-04 |
| 06 | Cleanup | Complete | 2026-04-05 |
| 07 | Typed Handles | Complete | 2026-04-05 |
| 08 | Retroactive Verification | Complete | 2026-04-06 |
| 09 | Codegen Test Cleanup | Complete | 2026-04-06 |
| 10 | SDK Cleanup Completion | Complete | 2026-04-06 |
| 11 | Guest Calling Convention | Complete | 2026-04-07 |
| 12 | SDK Instance Model Completion | Complete | 2026-04-08 |
| 13 | C++ Codegen Modernization | Complete | 2026-04-08 |
| 14 | Hot-Reload Documentation | Complete | 2026-04-08 |
| 15 | Final Cleanup | Complete | 2026-04-09 |
| 16 | Milestone Gap Closure | Complete | 2026-04-09 |
| 17 | RuntimeStore Refactor | Planning Complete | 2026-04-10 |

## Phase 17 Plans

**Two-Pass Migration (per CONTEXT.md D-37/D-38):**

- **17-01-PLAN.md** — Pass 1: Rename types, methods, fields
  - ContractRegistry -> RuntimeStore
  - All Registry* -> Plugin* or RuntimeStore*
  - All methods renamed with guest_contract/bundle prefix
  - Wave 1 (independent)

- **17-02-PLAN.md** — Pass 2: Add BundleData, BundleDescriptor, new APIs
  - O(1) get_bundle_plugin_slots via bundle_data HashMap
  - BundleDescriptor for bundle metadata in RuntimeStore
  - bundle_name_index for multi-version support
  - Wave 2 (depends on 17-01)

## Session Continuity

Last session: 2026-04-10T14:30:00.000Z
Completed: Phase 17 Planning
Next: Execute Phase 17 - `/gsd-execute-phase 17`

## Accumulated Context

### Roadmap Evolution

- Phase 17 added: Refactor ContractRegistry to unified RuntimeStore
- Phase 17 plans created: 2026-04-10

---
*State updated: 2026-04-10*
