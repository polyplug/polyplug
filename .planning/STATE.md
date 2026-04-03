---
gsd_state_version: 1.0
milestone: v1.0
milestone_name: milestone
status: planning
last_updated: "2026-04-03T08:32:02.010Z"
progress:
  total_phases: 3
  completed_phases: 2
  total_plans: 10
  completed_plans: 10
  percent: 100
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

Phase: 02 (update-loader-implementations) — COMPLETE
Plan: 5 of 5
**Phase:** 3
**Plan:** Not started
**Status:** Ready to plan
**Progress:** [██████████] 100%

### Phase 01 Completion Summary

| Plan | Error Type | Crate | Status |
|------|------------|-------|--------|
| 01-01 | PythonLoaderError | polyplug_python | COMPLETE |
| 01-02 | LuaLoaderError | polyplug_lua | COMPLETE |
| 01-03 | JsLoaderError | polyplug_js | COMPLETE |
| 01-04 | DotnetLoaderError | polyplug_dotnet | COMPLETE |
| 01-05 | Core LoaderError stripped | polyplug | COMPLETE |

**Verification:** Passed (5/5 must-haves verified) — all loader-specific error variants removed from core.

### Phase 02 Completion Summary

| Plan | Loader | Error Sites | Status |
|------|--------|-------------|--------|
| 02-01 | NativeLoader | 6 | COMPLETE |
| 02-02 | PythonLoader | 14 | COMPLETE |
| 02-03 | LuaLoader | 13 | COMPLETE |
| 02-04 | JsLoader | 48 | COMPLETE |
| 02-05 | DotnetLoader | 8 | COMPLETE |

**Verification:** All loaders use LoaderError::InitFailed directly with descriptive string messages.

## Performance Metrics

| Metric | Value |
|--------|-------|
| Phases completed | 2/3 |
| Plans completed | 10/10 (Phase 01 + Phase 02) |
| Requirements addressed | 8/8 (ERR-01-06) |
| Days in progress | 0 |
| Last activity | 2026-04-03 |
| Phase 02 P01 | 5min | 3 tasks | 2 files |
| Phase 02 P02 | 7min | 3 tasks | 2 files |
| Phase 02 P03 | 5min | 3 tasks | 3 files |
| Phase 02 P04 | 15min | 3 tasks | 2 files |
| Phase 02 P05 | 7min | 3 tasks | 2 files |

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
- [Phase 01]: LuaLoaderError follows NativeLoaderError pattern exactly; variant names identical to core LoaderError for traceability
- [Phase 01]: DotnetLoaderError follows NativeLoaderError pattern for consistency
- [Phase 01]: InitSymbolMissing retained in core - used by both Python and .NET loaders (not loader-specific)
- [Phase 01]: InitFailed retained as generic catch-all - loaders convert local errors to this variant
- [Phase 02]: D-01: Use LoaderError::InitFailed for all loader-specific errors with descriptive messages
- [Phase 02]: D-02: Keep error handling inline for all loaders, including NativeLoader
- [Phase 02]: D-03: Use RuntimeError::HotReloadDisabled for hot-reload disabled
- [Phase 02]: D-04: Remove unused local error types
- [Phase 02]: NativeLoaderError removed (02-01): no longer needed with unified InitFailed pattern per D-04
- [Phase 02]: load_internal() inlined (02-01): per D-02, direct error construction without intermediate method
- [Phase 02]: PythonLoaderError type deleted (02-02): per D-04, no longer needed with unified InitFailed pattern
- [Phase 02]: LuaLoaderError type deleted (02-03): per D-04, no longer needed with unified InitFailed pattern
- [Phase 02]: JsLoaderError type deleted (02-04): per D-04, no longer needed with unified InitFailed pattern
- [Phase 02]: JsLoader uses InitFailed pattern at 48 error sites with bundle_name parameter for context

### Active TODOs

- [x] Plan Phase 2: Update loader implementations to use crate-local errors
- [x] Execute Phase 2 after planning complete

### Blockers

None.

### Session Continuity

Phase 02 (update-loader-implementations) is complete. All loaders now use LoaderError::InitFailed directly:

- No loader-specific error types remain
- All error sites use descriptive string messages
- Hot-reload disabled returns RuntimeError::HotReloadDisabled

**Next Action:** Run `/gsd:execute-phase 3` for verification phase.

---
*State initialized: 2026-04-03*
