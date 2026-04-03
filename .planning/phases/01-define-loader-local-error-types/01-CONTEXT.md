# Phase 1: Define Loader-Local Error Types - Context

**Gathered:** 2026-04-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Move loader-specific error variants from core `LoaderError` enum to their respective loader crates. Each loader crate defines, owns, and exports its own error type following the `NativeLoaderError` pattern.

**What this phase delivers:**
- `PythonLoaderError` in `polyplug_python`
- `LuaLoaderError` in `polyplug_lua`
- `JsLoaderError` in `polyplug_js`
- `DotnetLoaderError` in `polyplug_dotnet`
- Core `LoaderError` stripped of Python/Lua/JS/.NET variants

**What this phase does NOT include:**
- Updating loader implementations to use new errors (Phase 2)
- Test verification (Phase 3)

</domain>

<decisions>
## Implementation Decisions

### Error Type Naming
- **D-01:** Follow `NativeLoaderError` naming pattern: `{Language}LoaderError`
  - `PythonLoaderError`
  - `LuaLoaderError`
  - `JsLoaderError`
  - `DotnetLoaderError`

### Error Type Location
- **D-02:** Create `error.rs` in each loader crate's `src/` directory
  - Matches `polyplug_native/src/error.rs` structure
  - Export via `pub mod error;` in `lib.rs`

### Error Conversion
- **D-03:** Each `*LoaderError` implements `std::fmt::Display` via `thiserror`
- **D-04:** Loaders convert to `RuntimeError::Loader(LoaderError::InitFailed { bundle, error })` at cross-crate boundary
  - `LoaderError::InitFailed` remains in core (generic, not loader-specific)
  - Error message string contains the original loader-specific error details

### Variants to Migrate (per crate)
- **D-05:** `polyplug_python`: `PythonInitFailed`, `PythonModuleImportFailed`, `PythonInitRaisedException`
- **D-06:** `polyplug_lua`: `LuaVmInitFailed`, `LuaScriptLoadFailed`, `LuaInitFunctionMissing`, `LuaInitRaisedError`
- **D-07:** `polyplug_js`: `RolldownNotFound`, `JsRuntimePanic`, `JsRuntimeInitFailed`, `ModuleResolutionFailed`, `JsExecutionFailed`
- **D-08:** `polyplug_dotnet`: `HostfxrNotFound`, `ClrInitFailed`, `AssemblyNotFound`, `RuntimeVersionMismatch`, `InvalidFrameworkVersion`

### Claude's Discretion
- Exact variant field names and types — follow `NativeLoaderError` pattern
- Additional error variants discovered during implementation — add as needed
- Documentation comments — add context appropriate to each error

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Pattern
- `crates/polyplug_native/src/error.rs` — Reference implementation for loader-local error type
- `crates/polyplug_native/src/lib.rs` — Shows export pattern: `pub mod error; pub use error::NativeLoaderError;`

### Core Error Types (source of variants to migrate)
- `crates/polyplug/src/error.rs` — Current `LoaderError` enum with loader-specific variants at lines 87-180

### Loader Crate Structures
- `crates/polyplug_python/src/lib.rs` — Target for error module export
- `crates/polyplug_lua/src/lib.rs` — Target for error module export
- `crates/polyplug_js/src/lib.rs` — Target for error module export
- `crates/polyplug_dotnet/src/lib.rs` — Target for error module export

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `NativeLoaderError` in `polyplug_native/src/error.rs`: Provides exact template for error enum structure with `thiserror::Error` derive

### Established Patterns
- Error enum with `#[derive(Debug, Error)]`
- Variant format: `#[error("description: {field}")] { field: Type }`
- Export pattern: `pub mod error; pub use error::XLoaderError;`

### Integration Points
- Each loader's `lib.rs` needs `pub mod error;` and `pub use error::*LoaderError;`
- Core `error.rs` needs variants removed (but `InitFailed` stays — it's the conversion target)

</code_context>

<specifics>
## Specific Ideas

Follow the `NativeLoaderError` pattern exactly. This is a mechanical migration — copy the pattern, adapt the variants.

Key reference: `crates/polyplug_native/src/error.rs` (37 lines, clean thiserror enum)

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 01-define-loader-local-error-types*
*Context gathered: 2026-04-03*