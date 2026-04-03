# ROADMAP: polyplug Error Decoupling

**Created:** 2026-04-03
**Granularity:** Coarse
**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

## Overview

This roadmap addresses error type decoupling, moving loader-specific error variants from the core `LoaderError` enum to their respective loader crates. Each phase delivers a verifiable capability that advances toward full decoupling.

## Phases

- [x] **Phase 1: Define Loader-Local Error Types** - Move error variants from core to loader crates (completed 2026-04-03)
- [x] **Phase 2: Update Loader Implementations** - Loaders use unified InitFailed pattern (completed 2026-04-03)
- [ ] **Phase 3: Verify Compatibility** - Ensure tests pass and FFI compatibility maintained

## Phase Details

### Phase 1: Define Loader-Local Error Types

**Goal:** Each loader crate defines and exports its own error type with migrated variants

**Depends on:** Nothing (first phase)

**Requirements:** ERR-01, ERR-02, ERR-03, ERR-04, ERR-05

**Success Criteria** (what must be TRUE):
1. Core `LoaderError` enum contains no Python, Lua, JS, or .NET specific variants
2. `polyplug_python` crate exports `PythonLoaderError` enum with migrated variants
3. `polyplug_lua` crate exports `LuaLoaderError` enum with migrated variants
4. `polyplug_js` crate exports `JsLoaderError` enum with migrated variants
5. `polyplug_dotnet` crate exports `DotnetLoaderError` enum with migrated variants

**Plans:** 5 plans (all complete)

Plans:
- [x] 01-PLAN.md — Define PythonLoaderError in polyplug_python crate (ERR-01) ✓
- [x] 02-PLAN.md — Define LuaLoaderError in polyplug_lua crate (ERR-02) ✓
- [x] 03-PLAN.md — Define JsLoaderError in polyplug_js crate (ERR-03) ✓
- [x] 04-PLAN.md — Define DotnetLoaderError in polyplug_dotnet crate (ERR-04) ✓
- [x] 05-PLAN.md — Strip loader-specific variants from core LoaderError (ERR-01-05 completion) ✓

---

### Phase 2: Update Loader Implementations

**Goal:** All loaders use `LoaderError::InitFailed` directly with descriptive string messages

**Depends on:** Phase 1

**Requirements:** ERR-06

**Success Criteria** (what must be TRUE):
1. Each loader uses `LoaderError::InitFailed { bundle, error }` directly at each error site
2. Error messages are descriptive strings (no intermediate error types)
3. Local error types from Phase 1 (`PythonLoaderError`, etc.) are removed or marked unused
4. All loaders return `RuntimeError::HotReloadDisabled` for unsupported hot-reload (consistency)

**Plans:** 5 plans (all complete)

Plans:
- [x] 02-01-PLAN.md — Update NativeLoader: remove error.rs, inline load_internal, use InitFailed directly
- [x] 02-02-PLAN.md — Update PythonLoader: remove error.rs, replace all error sites with InitFailed
- [x] 02-03-PLAN.md — Update LuaLoader: remove error.rs, replace all error sites with InitFailed
- [x] 02-04-PLAN.md — Update JsLoader: remove error.rs, replace ~45 error sites, fix hot-reload
- [x] 02-05-PLAN.md — Update DotnetLoader: remove error.rs, replace error sites, fix hot-reload

---

### Phase 3: Verify Compatibility

**Goal:** Error decoupling verified working with all tests passing

**Depends on:** Phase 2

**Requirements:** COMP-01, COMP-02

**Success Criteria** (what must be TRUE):
1. All existing tests pass (`cargo test --workspace`)
2. FFI error messages remain strings at the boundary (no breaking changes)
3. Example hosts compile and run correctly

**Plans:** 7 plans

Plans:
- [ ] 03-01-PLAN.md — Fix Python source file (context.rs) to use InitFailed pattern (COMP-01)
- [ ] 03-02-PLAN.md — Fix .NET source files (version.rs, context.rs) to use InitFailed pattern (COMP-01)
- [ ] 03-03-PLAN.md — Fix Python test files to use InitFailed pattern (COMP-01)
- [ ] 03-04-PLAN.md — Fix .NET test files to use InitFailed pattern (COMP-01)
- [ ] 03-05-PLAN.md — Fix Lua test files to use InitFailed pattern (COMP-01)
- [ ] 03-06-PLAN.md — Fix integration_loader_dispatch.rs test file (COMP-01)
- [ ] 03-07-PLAN.md — Run verification tests and confirm FFI compatibility (COMP-01, COMP-02)

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Define Loader-Local Error Types | 5/5 | Complete | 2026-04-03 |
| 2. Update Loader Implementations | 5/5 | Complete | 2026-04-03 |
| 3. Verify Compatibility | 0/7 | In progress | - |

## Coverage Map

| Requirement | Phase | Status |
|-------------|-------|--------|
| ERR-01 | Phase 1 | Complete |
| ERR-02 | Phase 1 | Complete |
| ERR-03 | Phase 1 | Complete |
| ERR-04 | Phase 1 | Complete |
| ERR-05 | Phase 1 | Complete |
| ERR-06 | Phase 2 | Complete |
| COMP-01 | Phase 3 | In progress |
| COMP-02 | Phase 3 | In progress |

**Coverage:** 6/8 requirements complete (75%)

---
*Roadmap created: 2026-04-03*
*Last updated: 2026-04-03 after Phase 3 planning*