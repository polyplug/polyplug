---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: executing
last_updated: "2026-04-03T04:55:15.446Z"
progress:
  total_phases: 3
  completed_phases: 0
  total_plans: 5
  completed_plans: 4
  percent: 80
---

# STATE: polyplug Error Decoupling

**Project:** polyplug Error Decoupling
**Started:** 2026-04-03
**Last Active:** 2026-04-03

## Project Reference

**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

**Goal:** Move loader-specific error variants from core `LoaderError` to respective loader crates

**Constraints:**

- Architecture: Core crate must have zero loader-specific code or dependencies
- Safety: Hot-reload safety contract — hosts must not cache raw function pointers
- Compatibility: Breaking changes acceptable — not published yet

## Current Position

Phase: 01 (define-loader-local-error-types) — EXECUTING
Plan: 4 of 5
**Phase:** 1 - Define Loader-Local Error Types
**Plan:** 01-03 (JsLoaderError) — COMPLETE
**Status:** Executing Phase 01, Plans 01-03 complete
**Progress:** [████████░░] 80%

### Current Plan Context

**Completed Plans in Phase 01:**

| Plan | Error Type | Crate | Status |
|------|------------|-------|--------|
| 01-01 | PythonLoaderError | polyplug_python | COMPLETE |
| 01-02 | LuaLoaderError | polyplug_lua | COMPLETE |
| 01-03 | JsLoaderError | polyplug_js | COMPLETE |
| 01-04 | DotnetLoaderError | polyplug_dotnet | IN PROGRESS |
| 01-05 | NativeLoaderError migration | polyplug_native | PENDING |

**Last Completed (01-03):** JsLoaderError enum defined in polyplug_js crate.

- File: crates/polyplug_js/src/error.rs (created)
- Export: lib.rs updated with error module and JsLoaderError export
- Pattern: Follows NativeLoaderError structure
- Variants: RolldownNotFound, JsRuntimePanic, JsRuntimeInitFailed, ModuleResolutionFailed, JsExecutionFailed

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 0/3 |
| Plans completed | 0/3 |
| Requirements addressed | 0/8 |
| Days in progress | 0 |
| Last activity | 2026-04-03 |
| Phase 01 P02 | 4 | 2 tasks | 2 files |
| Phase 01 P03 | 3 | 2 tasks | 2 files |

## Accumulated Context

### Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-03 | 3 phases (coarse granularity) | Focused refactoring; phases deliver verifiable capabilities |
| 2026-04-03 | Phase 1 covers type definition | Natural grouping: all error types defined before use |
| 2026-04-03 | Phase 2 covers implementation | Single coherent change: update all loaders |
| 2026-04-03 | Phase 3 covers verification | Verification requires complete implementation |

- [Phase 01]: JsLoaderError follows NativeLoaderError pattern for consistency
- [Phase 01]: JS-specific variants kept identical to core LoaderError for traceability

### Active TODOs

- [ ] Begin Phase 1: Define loader-local error types

### Blockers

None.

### Session Continuity

This is a focused refactoring task to decouple error types from the core crate. The native decoupling (Phase 4.6 in prior work) is complete; this roadmap addresses the remaining error type decoupling.

**Next Action:** Run `/gsd:plan-phase 1` to create plan for defining loader-local error types.

---
*State initialized: 2026-04-03*
