# Requirements: polyplug Error Decoupling

**Defined:** 2026-04-03
**Core Value:** Core runtime is loader-agnostic — no loader-specific code or types

## v1 Requirements

### Error Types

- [x] **ERR-01**: Remove `PythonInitFailed`, `PythonModuleImportFailed`, `PythonInitRaisedException` from core `LoaderError` — move to `polyplug_python`
- [x] **ERR-02**: Remove `LuaVmInitFailed`, `LuaScriptLoadFailed`, `LuaInitFunctionMissing`, `LuaInitRaisedError` from core `LoaderError` — move to `polyplug_lua`
- [x] **ERR-03**: Remove `RolldownNotFound`, `JsRuntimePanic`, `JsRuntimeInitFailed`, `ModuleResolutionFailed`, `JsExecutionFailed` from core `LoaderError` — move to `polyplug_js`
- [x] **ERR-04**: Remove `HostfxrNotFound`, `ClrInitFailed`, `AssemblyNotFound`, `RuntimeVersionMismatch`, `InvalidFrameworkVersion` from core `LoaderError` — move to `polyplug_dotnet`
- [x] **ERR-05**: Ensure each loader crate exports its own error type (e.g., `PythonLoaderError`, `LuaLoaderError`)
- [x] **ERR-06**: Update loader `load()` and `reload()` implementations to use `LoaderError::InitFailed` directly with descriptive string messages (no intermediate error types)

### Compatibility

- [x] **COMP-01**: All existing tests pass after error type migration
- [x] **COMP-02**: No breaking changes to public FFI API (error messages are strings at FFI boundary)

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
| ERR-01 | Phase 1 | Complete |
| ERR-02 | Phase 1 | Complete |
| ERR-03 | Phase 1 | Complete |
| ERR-04 | Phase 1 | Complete |
| ERR-05 | Phase 1 | Complete |
| ERR-06 | Phase 2 | Complete |
| COMP-01 | Phase 3 | Complete |
| COMP-02 | Phase 3 | Complete |

**Coverage:**
- v1 requirements: 8 total
- Mapped to phases: 8
- Unmapped: 0 ✓

---
*Requirements defined: 2026-04-03*
*Last updated: 2026-04-03 after roadmap creation*