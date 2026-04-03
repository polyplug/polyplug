# ROADMAP: polyplug Error Decoupling

**Created:** 2026-04-03
**Granularity:** Coarse
**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

## Overview

This roadmap addresses error type decoupling, moving loader-specific error variants from the core `LoaderError` enum to their respective loader crates. Each phase delivers a verifiable capability that advances toward full decoupling.

## Phases

- [ ] **Phase 1: Define Loader-Local Error Types** - Move error variants from core to loader crates
- [ ] **Phase 2: Update Loader Implementations** - Loaders use crate-local errors with boundary conversion
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

**Plans:** TBD

---

### Phase 2: Update Loader Implementations

**Goal:** All loaders use their crate-local error types with proper boundary conversion

**Depends on:** Phase 1

**Requirements:** ERR-06

**Success Criteria** (what must be TRUE):
1. Each loader's `load()` method returns its crate-local error type
2. Each loader's `reload()` method returns its crate-local error type
3. Error conversion from crate-local to `RuntimeError::Loader(LoaderError::InitFailed)` exists at cross-crate boundary
4. Core crate has no direct dependency on loader-specific error types

**Plans:** TBD

---

### Phase 3: Verify Compatibility

**Goal:** Error decoupling verified working with all tests passing

**Depends on:** Phase 2

**Requirements:** COMP-01, COMP-02

**Success Criteria** (what must be TRUE):
1. All existing tests pass (`cargo test --workspace`)
2. FFI error messages remain strings at the boundary (no breaking changes)
3. Example hosts compile and run correctly

**Plans:** TBD

---

## Progress

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Define Loader-Local Error Types | 0/1 | Not started | - |
| 2. Update Loader Implementations | 0/1 | Not started | - |
| 3. Verify Compatibility | 0/1 | Not started | - |

## Coverage Map

| Requirement | Phase | Status |
|-------------|-------|--------|
| ERR-01 | Phase 1 | Pending |
| ERR-02 | Phase 1 | Pending |
| ERR-03 | Phase 1 | Pending |
| ERR-04 | Phase 1 | Pending |
| ERR-05 | Phase 1 | Pending |
| ERR-06 | Phase 2 | Pending |
| COMP-01 | Phase 3 | Pending |
| COMP-02 | Phase 3 | Pending |

**Coverage:** 8/8 requirements mapped (100%)

---
*Roadmap created: 2026-04-03*