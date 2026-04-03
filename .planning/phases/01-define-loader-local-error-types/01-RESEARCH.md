# Phase 1: Define Loader-Local Error Types - Research

**Researched:** 2026-04-03
**Domain:** Rust error type definition with thiserror derive
**Confidence:** HIGH

## Summary

This phase creates loader-local error types following the established `NativeLoaderError` pattern. Each loader crate (`polyplug_python`, `polyplug_lua`, `polyplug_js`, `polyplug_dotnet`) will define and export its own error enum with variants migrated from the core `LoaderError` enum. The core crate's `LoaderError` will be stripped of loader-specific variants, retaining only generic variants like `InitFailed`.

**Primary recommendation:** Follow the `NativeLoaderError` pattern exactly — create `error.rs` in each loader crate's `src/` directory, use `thiserror::Error` derive, and export via `pub mod error; pub use error::*LoaderError;` in `lib.rs`.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

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

### Deferred Ideas (OUT OF SCOPE)
None — discussion stayed within phase scope.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ERR-01 | Remove `PythonInitFailed`, `PythonModuleImportFailed`, `PythonInitRaisedException` from core `LoaderError` — move to `polyplug_python` | `NativeLoaderError` pattern (lines 1-37 of `crates/polyplug_native/src/error.rs`) provides exact template |
| ERR-02 | Remove `LuaVmInitFailed`, `LuaScriptLoadFailed`, `LuaInitFunctionMissing`, `LuaInitRaisedError` from core `LoaderError` — move to `polyplug_lua` | Same pattern; variants have `{ bundle, reason/message }` fields |
| ERR-03 | Remove `RolldownNotFound`, `JsRuntimePanic`, `JsRuntimeInitFailed`, `ModuleResolutionFailed`, `JsExecutionFailed` from core `LoaderError` — move to `polyplug_js` | Same pattern; `RolldownNotFound` has `{ hint }` field, others have `{ reason/message }` |
| ERR-04 | Remove `HostfxrNotFound`, `ClrInitFailed`, `AssemblyNotFound`, `RuntimeVersionMismatch`, `InvalidFrameworkVersion` from core `LoaderError` — move to `polyplug_dotnet` | Same pattern; `HostfxrNotFound` has no fields, others have `{ path, reason }` or `{ required, found }` |
| ERR-05 | Ensure each loader crate exports its own error type (e.g., `PythonLoaderError`, `LuaLoaderError`) | Export pattern: `pub mod error; pub use error::*LoaderError;` in `lib.rs` |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| thiserror | 2.0 (workspace) | Error type derive macro | Workspace dependency; already used in all crates |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::fmt::Display | — | Error display trait | Required for all error types via thiserror |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| thiserror | anyhow for errors | anyhow is for application errors; thiserror provides structured library error types with `#[source]` chaining |
| custom Error impl | manual Display/Error impl | More code, no macro benefits; thiserror is the Rust ecosystem standard |

**Installation:**
Already installed as workspace dependency in root `Cargo.toml`:
```toml
thiserror = { workspace = true }
```

**Version verification:**
```bash
# Workspace already defines thiserror 2.0
grep -A1 'thiserror' /mnt/data/Projects/Utils/polyplug/Cargo.toml
```

## Architecture Patterns

### Recommended Project Structure
Each loader crate follows this structure:
```
crates/polyplug_{lang}/
├── src/
│   ├── error.rs       # NEW: Loader-local error enum
│   ├── lib.rs         # ADD: pub mod error; pub use error::*LoaderError;
│   ├── loader.rs      # EXISTING: Uses error types (Phase 2 update)
│   └── ...            # Other existing modules
```

### Pattern 1: thiserror Error Enum
**What:** Structured error enum with `#[derive(Debug, Error)]` and descriptive `#[error(...)]` attributes
**When to use:** All loader-local error types
**Example:**
```rust
// Source: crates/polyplug_native/src/error.rs
//! Native-specific error types.

use thiserror::Error;

/// Errors from the native loader.
#[derive(Debug, Error)]
pub enum NativeLoaderError {
    #[error("failed to load plugin bundle at `{path}`: {source}")]
    LoadFailed {
        path: String,
        #[source]
        source: libloading::Error,
    },

    #[error("ABI version mismatch in `{bundle}`: expected={expected}, found={found}")]
    AbiVersionMismatch {
        bundle: String,
        expected: u32,
        found: u32,
    },

    #[error("missing symbol `{symbol}` in bundle `{bundle}`")]
    MissingSymbol { bundle: String, symbol: String },

    #[error("init failed for bundle `{bundle}`: {error}")]
    InitFailed { bundle: String, error: String },

    #[error("manifest missing file field for bundle `{bundle}`")]
    ManifestMissingFile { bundle: String },

    #[error("bundle `{bundle}` tampered with bundle_id: expected={expected:#x}, found={found:#x}")]
    BundleTampered {
        bundle: String,
        expected: u64,
        found: u64,
    },
}
```

### Pattern 2: Module Export
**What:** Export error module and type from crate root
**When to use:** All loader crates
**Example:**
```rust
// Source: crates/polyplug_native/src/lib.rs
//! polyplug_native: Native (shared library) plugin loader for the polyplug runtime.

pub mod config;
pub mod error;      // Error module
pub mod loader;

pub use config::NativeConfig;
pub use error::NativeLoaderError;  // Re-export for convenience
pub use loader::NativeLoader;
```

### Anti-Patterns to Avoid
- **Adding `#[source]` for String fields:** Only use `#[source]` for actual error types that implement `std::error::Error`. String fields should use plain `{field}` interpolation.
- **Copying all core variants:** Only migrate loader-specific variants; keep generic variants (`InitFailed`, `ManifestParse`, etc.) in core `LoaderError`.
- **Renaming variants:** Keep variant names identical to their current names for traceability and minimal diff noise.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Error enum with Display | manual `impl Display` | thiserror derive | Macro handles Display, Error, Debug automatically; less boilerplate, more consistent |
| Error chaining | custom `From` impls | `#[source]` attribute | thiserror provides structured chaining with source access |
| Error context | separate context struct | embedded fields | thiserror allows structured fields with `#[error("...")]` templates |

**Key insight:** thiserror is the de facto standard for library error types in Rust. The polyplug project already uses it consistently across all crates.

## Runtime State Inventory

**Phase type:** Code definition only (no rename/refactor/migration of existing runtime state).

**Not applicable:** This phase creates new error type definitions. No databases, OS registrations, services, or build artifacts contain the old variant names. The variants are Rust source code constructs that compile into the binary — no runtime state migration required.

## Common Pitfalls

### Pitfall 1: Variant Field Name Drift
**What goes wrong:** Changing field names during migration (e.g., `reason` to `message`) causes unnecessary diff complexity and potential confusion.
**Why it happens:** Desire to "improve" naming during refactor.
**How to avoid:** Keep existing field names identical; only change the enum name and location.
**Warning signs:** Variant definition shows different field names than original.

### Pitfall 2: Missing `pub mod error;` Export
**What goes wrong:** Creating `error.rs` but forgetting to export from `lib.rs`, making the type inaccessible to crate users.
**Why it happens:** Mechanical oversight when following pattern.
**How to avoid:** Follow the exact `NativeLoaderError` export pattern: `pub mod error; pub use error::*LoaderError;`.
**Warning signs:** Compilation error "cannot find type `PythonLoaderError` in crate `polyplug_python`".

### Pitfall 3: Leaving Generic Variants in Loader Types
**What goes wrong:** Copying generic variants like `ManifestParse` into loader-specific error types.
**Why it happens:** Misunderstanding which variants are loader-specific vs. generic.
**How to avoid:** Only migrate variants with loader-specific prefixes (`Python*`, `Lua*`, `Js*`, `Hostfxr*`, `Clr*`, `Assembly*`, `RuntimeVersion*`, `InvalidFramework*`). Keep `InitFailed`, `ManifestParse`, `DuplicateLoader`, etc. in core.
**Warning signs:** Loader error type contains variants without loader-specific naming.

### Pitfall 4: Removing `InitFailed` from Core
**What goes wrong:** Removing `LoaderError::InitFailed` from core, which is the conversion target for loader-specific errors at cross-crate boundary.
**Why it happens:** Treating `InitFailed` as loader-specific when it's actually the generic catch-all.
**How to avoid:** Keep `LoaderError::InitFailed { bundle, error }` in core — it's used by loaders to wrap their specific errors when crossing into core runtime.
**Warning signs:** `InitFailed` variant removed from `LoaderError` enum.

## Code Examples

Verified patterns from existing code:

### PythonLoaderError (to be created)
```rust
// Pattern based on NativeLoaderError; fields from core LoaderError variants
//! Python-specific error types.

use thiserror::Error;

/// Errors from the Python loader.
#[derive(Debug, Error)]
pub enum PythonLoaderError {
    #[error("Python interpreter initialization failed: {reason}")]
    PythonInitFailed { reason: String },

    #[error("failed to import Python module at `{path}`: {reason}")]
    PythonModuleImportFailed { path: String, reason: String },

    #[error("Python init function raised exception in bundle `{bundle}`: {message}")]
    PythonInitRaisedException { bundle: String, message: String },
}
```

### LuaLoaderError (to be created)
```rust
//! Lua-specific error types.

use thiserror::Error;

/// Errors from the Lua loader.
#[derive(Debug, Error)]
pub enum LuaLoaderError {
    #[error("lua vm init failed: {reason}")]
    LuaVmInitFailed { reason: String },

    #[error("lua script load failed: path={path}, reason={reason}")]
    LuaScriptLoadFailed { path: String, reason: String },

    #[error("lua plugin missing polyplug_init function: bundle={bundle}")]
    LuaInitFunctionMissing { bundle: String },

    #[error("lua polyplug_init raised error: bundle={bundle}, message={message}")]
    LuaInitRaisedError { bundle: String, message: String },
}
```

### JsLoaderError (to be created)
```rust
//! JavaScript-specific error types.

use thiserror::Error;

/// Errors from the JS loader.
#[derive(Debug, Error)]
pub enum JsLoaderError {
    #[error("rolldown not found on PATH — js-quickjs pack requires rolldown. {hint}")]
    RolldownNotFound { hint: String },

    #[error("JS runtime \"{runtime}\" panicked during bundle load: {message}")]
    JsRuntimePanic { runtime: String, message: String },

    #[error("JS runtime initialization failed: {reason}")]
    JsRuntimeInitFailed { reason: String },

    #[error("module resolution failed: {reason}")]
    ModuleResolutionFailed { reason: String },

    #[error("failed to execute JS script: {reason}")]
    JsExecutionFailed { reason: String },
}
```

### DotnetLoaderError (to be created)
```rust
//! .NET-specific error types.

use thiserror::Error;

/// Errors from the .NET loader.
#[derive(Debug, Error)]
pub enum DotnetLoaderError {
    #[error("hostfxr not found: searched DOTNET_ROOT, PATH, and well-known paths")]
    HostfxrNotFound,

    #[error("CLR initialization failed for runtime config `{path}`: {reason}")]
    ClrInitFailed { path: String, reason: String },

    #[error("assembly not found at path `{path}`")]
    AssemblyNotFound { path: String },

    #[error(".NET runtime version mismatch: required={required}, found={found}")]
    RuntimeVersionMismatch { required: String, found: String },

    #[error("invalid .NET framework version in TFM `{tfm}`: {reason}")]
    InvalidFrameworkVersion { tfm: String, reason: String },
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Loader-specific variants in core `LoaderError` | Loader-local error types in each crate | This phase | Core crate becomes loader-agnostic; loader crates own their error semantics |

**Deprecated/outdated:**
- `LoaderError::PythonInitFailed`, `LoaderError::PythonModuleImportFailed`, `LoaderError::PythonInitRaisedException`: Move to `PythonLoaderError`
- `LoaderError::LuaVmInitFailed`, `LoaderError::LuaScriptLoadFailed`, `LoaderError::LuaInitFunctionMissing`, `LoaderError::LuaInitRaisedError`: Move to `LuaLoaderError`
- `LoaderError::RolldownNotFound`, `LoaderError::JsRuntimePanic`, `LoaderError::JsRuntimeInitFailed`, `LoaderError::ModuleResolutionFailed`, `LoaderError::JsExecutionFailed`: Move to `JsLoaderError`
- `LoaderError::HostfxrNotFound`, `LoaderError::ClrInitFailed`, `LoaderError::AssemblyNotFound`, `LoaderError::RuntimeVersionMismatch`, `LoaderError::InvalidFrameworkVersion`: Move to `DotnetLoaderError`

## Open Questions

1. **Should loader error types implement `From<*LoaderError>` for `LoaderError::InitFailed`?**
   - What we know: D-04 specifies conversion to `RuntimeError::Loader(LoaderError::InitFailed { bundle, error })`
   - What's unclear: Whether to implement `From` trait or do manual conversion in loader code
   - Recommendation: **Phase 2 decision** — this phase only defines types. Recommend implementing `From` trait in Phase 2 for ergonomic conversion, but defer implementation.

## Environment Availability

**Step 2.6: SKIPPED** — No external dependencies identified. This phase only requires:
- Rust toolchain (already available, workspace compiles)
- thiserror crate (already workspace dependency)
- No new CLI tools, services, or runtimes needed

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_native/src/error.rs` — Reference implementation for loader-local error type (exact pattern to follow)
- `crates/polyplug_native/src/lib.rs` — Export pattern: `pub mod error; pub use error::NativeLoaderError;`
- `crates/polyplug/src/error.rs` (lines 87-180) — Source of variants to migrate

### Secondary (MEDIUM confidence)
- `crates/polyplug_python/src/lib.rs` — Target crate structure, currently lacks error module
- `crates/polyplug_lua/src/lib.rs` — Target crate structure, currently lacks error module
- `crates/polyplug_js/src/lib.rs` — Target crate structure, currently lacks error module
- `crates/polyplug_dotnet/src/lib.rs` — Target crate structure, currently lacks error module

### Tertiary (LOW confidence)
- None — all information derived from direct source code reading

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — thiserror is already workspace dependency, pattern verified in NativeLoaderError
- Architecture: HIGH — exact pattern exists in polyplug_native crate, no ambiguity
- Pitfalls: HIGH — derived from existing code analysis and CONTEXT.md decisions

**Research date:** 2026-04-03
**Valid until:** Stable — pattern is well-established in Rust ecosystem