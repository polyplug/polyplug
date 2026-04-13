---
gsd_state_version: 1.0
milestone: v1.1
milestone_name: milestone
status: executing
last_updated: "2026-04-13T01:02:15.768Z"
progress:
  total_phases: 18
  completed_phases: 17
  total_plans: 91
  completed_plans: 91
  percent: 100
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

Phase: 19 (fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc) — EXECUTING
Plan: 2 of 6
**Status:** Ready to execute
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
| 17 | RuntimeStore Refactor | Complete | 2026-04-10 |
| 18 | FFI Consolidation | Planned | — |

## Phase 18 Plans

**FFI Consolidation (HostInterface-based API):**

- **18-01-PLAN.md** — HostInterface struct changes (add fields, rename fields)
  - Add: load_bundle, reload_bundle, register_host_contract, register_loader, get_last_error, get_error_len
  - Rename: find_by_contract → find_guest_contract, find_all_by_contract → find_all_guest_contracts, resolve_contract → resolve_guest_contract
  - Wave 1 (ABI layer)

- **18-02-PLAN.md** — FFI deletions + Runtime implementation
  - Delete 10 polyplug_runtime_* functions (keep only create/destroy)
  - Implement HostInterface callback functions
  - Wave 2 (depends on 18-01)

- **18-03-PLAN.md** — Python + C# SDK updates
  - Runtime class calls HostInterface methods directly
  - Wave 3 (parallel with 18-04)

- **18-04-PLAN.md** — Lua + JS + C++ SDK updates
  - Runtime class calls HostInterface methods directly
  - Wave 3 (parallel with 18-03)

- **18-05-PLAN.md** — Codegen + tests + verification
  - Update all generators to use HostInterface API
  - Update tests to use new API
  - Wave 4 (depends on 18-02, 18-03, 18-04)

## Session Continuity

Last session: 2026-04-13T01:02:15.766Z
Completed: Phase 17 Execution (RuntimeStore rename complete)
Next: Execute Phase 18 - `/gsd-execute-phase 18`

## Accumulated Context

### Roadmap Evolution

- Phase 17 added: Refactor ContractRegistry to unified RuntimeStore (COMPLETE)
- Phase 18 added: Consolidate FFI to HostInterface (PLANNED)
- Phase 19 added: Fix ABI build script extractor.rs to auto-generate SDK structs then delete hand-written ABI structs from all 5 SDKs

---
*State updated: 2026-04-10*
