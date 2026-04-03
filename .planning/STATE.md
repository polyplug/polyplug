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

**Phase:** 1 - Define Loader-Local Error Types
**Plan:** None assigned
**Status:** Not started
**Progress:** `░░░░░░░░░░` 0%

### Current Plan Context

None — awaiting first plan.

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 0/3 |
| Plans completed | 0/3 |
| Requirements addressed | 0/8 |
| Days in progress | 0 |
| Last activity | 2026-04-03 |

## Accumulated Context

### Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-03 | 3 phases (coarse granularity) | Focused refactoring; phases deliver verifiable capabilities |
| 2026-04-03 | Phase 1 covers type definition | Natural grouping: all error types defined before use |
| 2026-04-03 | Phase 2 covers implementation | Single coherent change: update all loaders |
| 2026-04-03 | Phase 3 covers verification | Verification requires complete implementation |

### Active TODOs

- [ ] Begin Phase 1: Define loader-local error types

### Blockers

None.

### Session Continuity

This is a focused refactoring task to decouple error types from the core crate. The native decoupling (Phase 4.6 in prior work) is complete; this roadmap addresses the remaining error type decoupling.

**Next Action:** Run `/gsd:plan-phase 1` to create plan for defining loader-local error types.

---
*State initialized: 2026-04-03*