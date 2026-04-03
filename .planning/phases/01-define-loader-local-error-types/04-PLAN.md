---
phase: 01-define-loader-local-error-types
plan: 04
type: execute
wave: 1
depends_on: []
files_modified:
  - crates/polyplug_dotnet/src/error.rs
  - crates/polyplug_dotnet/src/lib.rs
autonomous: true
requirements:
  - ERR-04
must_haves:
  truths:
    - "DotnetLoaderError enum exists in polyplug_dotnet crate"
    - "DotnetLoaderError contains HostfxrNotFound, ClrInitFailed, AssemblyNotFound, RuntimeVersionMismatch, InvalidFrameworkVersion variants"
    - "polyplug_dotnet lib.rs exports DotnetLoaderError"
  artifacts:
    - path: "crates/polyplug_dotnet/src/error.rs"
      provides: ".NET-specific loader error type"
      contains: "pub enum DotnetLoaderError"
      min_lines: 30
    - path: "crates/polyplug_dotnet/src/lib.rs"
      provides: "Error module export"
      contains: "pub mod error;"
  key_links:
    - from: "crates/polyplug_dotnet/src/lib.rs"
      to: "error.rs"
      via: "pub mod error;"
      pattern: "pub mod error;\\s*pub use error::DotnetLoaderError;"
---

<objective>
Define DotnetLoaderError enum in polyplug_dotnet crate, following NativeLoaderError pattern.

Purpose: Establish loader-local error type for .NET-specific failures (ERR-04 partial — create type)
Output: crates/polyplug_dotnet/src/error.rs with exported DotnetLoaderError enum
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/01-define-loader-local-error-types/01-CONTEXT.md
@.planning/phases/01-define-loader-local-error-types/01-RESEARCH.md

<interfaces>
<!-- Reference pattern from NativeLoaderError (canonical implementation) -->

From crates/polyplug_native/src/error.rs (reference pattern):
```rust
//! Native-specific error types.

use thiserror::Error;

/// Errors from the native loader.
#[derive(Debug, Error)]
pub enum NativeLoaderError {
    #[error("failed to load plugin bundle at `{path}`: {source}")]
    LoadFailed { path: String, #[source] source: libloading::Error },
    // ... other variants
}
```

From crates/polyplug_native/src/lib.rs (export pattern):
```rust
pub mod error;
pub use error::NativeLoaderError;
```

Core LoaderError variants to migrate (source: crates/polyplug/src/error.rs lines 87-105):
```rust
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
```

Note: InitSymbolMissing (line 98-99) is NOT loader-specific — it's used by multiple loaders. Keep in core.
</interfaces>
</context>

<tasks>

<task type="auto" tdd="false">
  <name>Task 1: Create DotnetLoaderError enum</name>
  <files>crates/polyplug_dotnet/src/error.rs</files>
  <read_first>
    - crates/polyplug_native/src/error.rs (reference pattern for error enum structure)
    - crates/polyplug/src/error.rs (lines 87-105, source of variants to migrate)
  </read_first>
  <action>
    Create `crates/polyplug_dotnet/src/error.rs` with the following exact content (per D-02, D-03, D-08 from CONTEXT.md):

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

    Notes:
    - Follow NativeLoaderError pattern exactly (same derive, same #[error] format)
    - Keep variant names identical to core LoaderError variants for traceability
    - Keep field names identical (path, reason, required, found, tfm)
    - HostfxrNotFound has no fields (empty variant)
    - No #[source] attribute needed — all fields are String, not Error types
  </action>
  <verify>
    <automated>grep -q "pub enum DotnetLoaderError" crates/polyplug_dotnet/src/error.rs && grep -q "HostfxrNotFound" crates/polyplug_dotnet/src/error.rs && grep -q "ClrInitFailed" crates/polyplug_dotnet/src/error.rs && grep -q "AssemblyNotFound" crates/polyplug_dotnet/src/error.rs && grep -q "RuntimeVersionMismatch" crates/polyplug_dotnet/src/error.rs && grep -q "InvalidFrameworkVersion" crates/polyplug_dotnet/src/error.rs</automated>
  </verify>
  <done>
    File crates/polyplug_dotnet/src/error.rs exists with DotnetLoaderError enum containing all five variants.
    grep confirms all five variants present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_dotnet/src/error.rs contains "pub enum DotnetLoaderError"
    - crates/polyplug_dotnet/src/error.rs contains "#[derive(Debug, Error)]"
    - crates/polyplug_dotnet/src/error.rs contains "HostfxrNotFound"
    - crates/polyplug_dotnet/src/error.rs contains "ClrInitFailed"
    - crates/polyplug_dotnet/src/error.rs contains "AssemblyNotFound"
    - crates/polyplug_dotnet/src/error.rs contains "RuntimeVersionMismatch"
    - crates/polyplug_dotnet/src/error.rs contains "InvalidFrameworkVersion"
    - crates/polyplug_dotnet/src/error.rs contains "use thiserror::Error;"
    - File has minimum 22 lines
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Export DotnetLoaderError from lib.rs</name>
  <files>crates/polyplug_dotnet/src/lib.rs</files>
  <read_first>
    - crates/polyplug_dotnet/src/lib.rs (current state, needs error module added)
    - crates/polyplug_native/src/lib.rs (reference pattern for error module export)
  </read_first>
  <action>
    Update `crates/polyplug_dotnet/src/lib.rs` to add error module export (per D-02 from CONTEXT.md).

    Add two lines after existing module declarations:
    ```rust
    pub mod error;
    pub use error::DotnetLoaderError;
    ```

    The lib.rs should have these lines added in the module declaration section.

    Expected structure after modification:
    ```rust
    //! polyplug_dotnet — .NET CLR loader adapter for polyplug.

    pub mod config;
    pub(crate) mod context;
    pub mod error;           // NEW LINE
    pub mod ffi;
    pub mod version;
    pub use config::DotnetConfig;
    pub use config::HostfxrLocation;
    pub use error::DotnetLoaderError;  // NEW LINE

    // ... rest of file
    ```

    Note: Keep alphabetical-ish ordering for modules, add error export line after other pub use statements.
  </action>
  <verify>
    <automated>grep -q "pub mod error;" crates/polyplug_dotnet/src/lib.rs && grep -q "pub use error::DotnetLoaderError;" crates/polyplug_dotnet/src/lib.rs</automated>
  </verify>
  <done>
    File crates/polyplug_dotnet/src/lib.rs exports error module and DotnetLoaderError type.
    grep confirms both export lines present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_dotnet/src/lib.rs contains "pub mod error;"
    - crates/polyplug_dotnet/src/lib.rs contains "pub use error::DotnetLoaderError;"
    - Cargo check for polyplug_dotnet crate passes: cargo check -p polyplug_dotnet exits 0
  </acceptance_criteria>
</task>

</tasks>

<verification>
After both tasks:
- DotnetLoaderError enum is defined with correct variants
- Error module is exported from lib.rs
- Crate compiles successfully
</verification>

<success_criteria>
- `grep "pub enum DotnetLoaderError" crates/polyplug_dotnet/src/error.rs` returns match
- `grep "pub mod error;" crates/polyplug_dotnet/src/lib.rs` returns match
- `cargo check -p polyplug_dotnet` exits with code 0
</success_criteria>

<output>
After completion, create `.planning/phases/01-define-loader-local-error-types/01-04-SUMMARY.md`
</output>