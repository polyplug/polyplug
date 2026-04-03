---
phase: 01-define-loader-local-error-types
plan: 01
type: execute
wave: 1
depends_on: []
files_modified:
  - crates/polyplug_python/src/error.rs
  - crates/polyplug_python/src/lib.rs
autonomous: true
requirements:
  - ERR-01
must_haves:
  truths:
    - "PythonLoaderError enum exists in polyplug_python crate"
    - "PythonLoaderError contains PythonInitFailed, PythonModuleImportFailed, PythonInitRaisedException variants"
    - "polyplug_python lib.rs exports PythonLoaderError"
  artifacts:
    - path: "crates/polyplug_python/src/error.rs"
      provides: "Python-specific loader error type"
      contains: "pub enum PythonLoaderError"
      min_lines: 20
    - path: "crates/polyplug_python/src/lib.rs"
      provides: "Error module export"
      contains: "pub mod error;"
  key_links:
    - from: "crates/polyplug_python/src/lib.rs"
      to: "error.rs"
      via: "pub mod error;"
      pattern: "pub mod error;\\s*pub use error::PythonLoaderError;"
---

<objective>
Define PythonLoaderError enum in polyplug_python crate, following NativeLoaderError pattern.

Purpose: Establish loader-local error type for Python-specific failures (ERR-01 partial — create type)
Output: crates/polyplug_python/src/error.rs with exported PythonLoaderError enum
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
<!-- Executor should follow this pattern exactly -->

From crates/polyplug_native/src/error.rs (reference pattern):
```rust
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
    // ... other variants
}
```

From crates/polyplug_native/src/lib.rs (export pattern):
```rust
pub mod error;
pub use error::NativeLoaderError;
```

Core LoaderError variants to migrate (source: crates/polyplug/src/error.rs lines 107-114):
```rust
#[error("Python interpreter initialization failed: {reason}")]
PythonInitFailed { reason: String },

#[error("failed to import Python module at `{path}`: {reason}")]
PythonModuleImportFailed { path: String, reason: String },

#[error("Python init function raised exception in bundle `{bundle}`: {message}")]
PythonInitRaisedException { bundle: String, message: String },
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="false">
  <name>Task 1: Create PythonLoaderError enum</name>
  <files>crates/polyplug_python/src/error.rs</files>
  <read_first>
    - crates/polyplug_native/src/error.rs (reference pattern for error enum structure)
    - crates/polyplug/src/error.rs (lines 107-114, source of variants to migrate)
  </read_first>
  <action>
    Create `crates/polyplug_python/src/error.rs` with the following exact content (per D-02, D-03, D-05 from CONTEXT.md):

    ```rust
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

    Notes:
    - Follow NativeLoaderError pattern exactly (same derive, same #[error] format)
    - Keep variant names identical to core LoaderError variants for traceability
    - Keep field names identical (reason, path, bundle, message)
    - No #[source] attribute needed — all fields are String, not Error types
  </action>
  <verify>
    <automated>grep -q "pub enum PythonLoaderError" crates/polyplug_python/src/error.rs && grep -q "PythonInitFailed" crates/polyplug_python/src/error.rs && grep -q "PythonModuleImportFailed" crates/polyplug_python/src/error.rs && grep -q "PythonInitRaisedException" crates/polyplug_python/src/error.rs</automated>
  </verify>
  <done>
    File crates/polyplug_python/src/error.rs exists with PythonLoaderError enum containing all three variants.
    grep confirms: "pub enum PythonLoaderError", "PythonInitFailed", "PythonModuleImportFailed", "PythonInitRaisedException" all present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_python/src/error.rs contains "pub enum PythonLoaderError"
    - crates/polyplug_python/src/error.rs contains "#[derive(Debug, Error)]"
    - crates/polyplug_python/src/error.rs contains "PythonInitFailed"
    - crates/polyplug_python/src/error.rs contains "PythonModuleImportFailed"
    - crates/polyplug_python/src/error.rs contains "PythonInitRaisedException"
    - crates/polyplug_python/src/error.rs contains "use thiserror::Error;"
    - File has minimum 15 lines
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Export PythonLoaderError from lib.rs</name>
  <files>crates/polyplug_python/src/lib.rs</files>
  <read_first>
    - crates/polyplug_python/src/lib.rs (current state, needs error module added)
    - crates/polyplug_native/src/lib.rs (reference pattern for error module export)
  </read_first>
  <action>
    Update `crates/polyplug_python/src/lib.rs` to add error module export (per D-02 from CONTEXT.md).

    Add two lines after existing module declarations:
    ```rust
    pub mod error;
    pub use error::PythonLoaderError;
    ```

    The lib.rs should have these lines added in the module declaration section (after `pub mod ffi;` line).

    Expected structure after modification:
    ```rust
    //! polyplug_python — CPython adapter for the polyplug runtime.
    //! ...

    pub mod bridge;
    pub mod config;
    pub(crate) mod context;
    pub mod error;           // NEW LINE
    pub mod ffi;
    pub use bridge::PythonHostBridge;
    pub use config::PythonConfig;
    pub use error::PythonLoaderError;  // NEW LINE

    // ... rest of file
    ```

    Note: Keep alphabetical-ish ordering for modules, add error export line after other pub use statements.
  </action>
  <verify>
    <automated>grep -q "pub mod error;" crates/polyplug_python/src/lib.rs && grep -q "pub use error::PythonLoaderError;" crates/polyplug_python/src/lib.rs</automated>
  </verify>
  <done>
    File crates/polyplug_python/src/lib.rs exports error module and PythonLoaderError type.
    grep confirms: "pub mod error;" and "pub use error::PythonLoaderError;" both present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_python/src/lib.rs contains "pub mod error;"
    - crates/polyplug_python/src/lib.rs contains "pub use error::PythonLoaderError;"
    - Cargo check for polyplug_python crate passes: cargo check -p polyplug_python exits 0
  </acceptance_criteria>
</task>

</tasks>

<verification>
After both tasks:
- PythonLoaderError enum is defined with correct variants
- Error module is exported from lib.rs
- Crate compiles successfully
</verification>

<success_criteria>
- `grep "pub enum PythonLoaderError" crates/polyplug_python/src/error.rs` returns match
- `grep "pub mod error;" crates/polyplug_python/src/lib.rs` returns match
- `cargo check -p polyplug_python` exits with code 0
</success_criteria>

<output>
After completion, create `.planning/phases/01-define-loader-local-error-types/01-01-SUMMARY.md`
</output>