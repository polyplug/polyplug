# Requirements: polyplug Error Decoupling

**Defined:** 2026-04-03
**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

## v1 Requirements

### Error Types

- [ ] **ERR-01**: Remove `PythonInitFailed`, `PythonModuleImportFailed`, `PythonInitRaisedException` from core `LoaderError` — move to `polyplug_python`
- [ ] **ERR-02**: Remove `LuaVmInitFailed`, `LuaScriptLoadFailed`, `LuaInitFunctionMissing`, `LuaInitRaisedError` from core `LoaderError` — move to `polyplug_lua`
- [ ] **ERR-03**: Remove `RolldownNotFound`, `JsRuntimePanic`, `JsRuntimeInitFailed`, `ModuleResolutionFailed`, `JsExecutionFailed` from core `LoaderError` — move to `polyplug_js`
- [ ] **ERR-04**: Remove `HostfxrNotFound`, `ClrInitFailed`, `AssemblyNotFound`, `RuntimeVersionMismatch`, `InvalidFrameworkVersion` from core `LoaderError` — move to `polyplug_dotnet`
- [ ] **ERR-05**: Ensure each loader crate exports its own error type (e.g., `PythonLoaderError`, `LuaLoaderError`)
- [ ] **ERR-06**: Update loader `load()` and `reload()` implementations to use crate-local error types, converting to `RuntimeError::Loader(LoaderError::InitFailed)` for cross-crate boundary

### Compatibility

- [ ] **COMP-01**: All existing tests pass after error type migration
- [ ] **COMP-02**: No breaking changes to public FFI API (error messages are strings at FFI boundary)

## v2 Requirements

Deferred to future release.

## Out of Scope

| Feature | Reason |
|---------|--------|
| WASM runtime support | Architectural decision — native plugins are the design |
| Plugin sandboxing | Host responsibility for trust |
| New loader implementations | Out of scope for this refactor |

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| ERR-01 | Phase 1 | Pending |
| ERR-02 | Phase 1 | Pending |
| ERR-03 | Phase 1 | Pending |
| ERR-04 | Phase 1 | Pending |
| ERR-05 | Phase 1 | Pending |
| ERR-06 | Phase 1 | Pending |
| COMP-01 | Phase 1 | Pending |
| COMP-02 | Phase 1 | Pending |

**Coverage:**
- v1 requirements: 8 total
- Mapped to phases: 8
- Unmapped: 0 ✓

---
*Requirements defined: 2026-04-03*
*Last updated: 2026-04-03 after initialization*