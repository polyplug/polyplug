---
phase: 01-define-loader-local-error-types
plan: 03
type: execute
wave: 1
depends_on: []
files_modified:
  - crates/polyplug_js/src/error.rs
  - crates/polyplug_js/src/lib.rs
autonomous: true
requirements:
  - ERR-03
must_haves:
  truths:
    - "JsLoaderError enum exists in polyplug_js crate"
    - "JsLoaderError contains RolldownNotFound, JsRuntimePanic, JsRuntimeInitFailed, ModuleResolutionFailed, JsExecutionFailed variants"
    - "polyplug_js lib.rs exports JsLoaderError"
  artifacts:
    - path: "crates/polyplug_js/src/error.rs"
      provides: "JS-specific loader error type"
      contains: "pub enum JsLoaderError"
      min_lines: 30
    - path: "crates/polyplug_js/src/lib.rs"
      provides: "Error module export"
      contains: "pub mod error;"
  key_links:
    - from: "crates/polyplug_js/src/lib.rs"
      to: "error.rs"
      via: "pub mod error;"
      pattern: "pub mod error;\\s*pub use error::JsLoaderError;"
---

<objective>
Define JsLoaderError enum in polyplug_js crate, following NativeLoaderError pattern.

Purpose: Establish loader-local error type for JavaScript-specific failures (ERR-03 partial — create type)
Output: crates/polyplug_js/src/error.rs with exported JsLoaderError enum
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

Core LoaderError variants to migrate (source: crates/polyplug/src/error.rs lines 128-148):
```rust
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
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="false">
  <name>Task 1: Create JsLoaderError enum</name>
  <files>crates/polyplug_js/src/error.rs</files>
  <read_first>
    - crates/polyplug_native/src/error.rs (reference pattern for error enum structure)
    - crates/polyplug/src/error.rs (lines 128-148, source of variants to migrate)
  </read_first>
  <action>
    Create `crates/polyplug_js/src/error.rs` with the following exact content (per D-02, D-03, D-07 from CONTEXT.md):

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

    Notes:
    - Follow NativeLoaderError pattern exactly (same derive, same #[error] format)
    - Keep variant names identical to core LoaderError variants for traceability
    - Keep field names identical (hint, runtime, message, reason)
    - No #[source] attribute needed — all fields are String, not Error types
  </action>
  <verify>
    <automated>grep -q "pub enum JsLoaderError" crates/polyplug_js/src/error.rs && grep -q "RolldownNotFound" crates/polyplug_js/src/error.rs && grep -q "JsRuntimePanic" crates/polyplug_js/src/error.rs && grep -q "JsRuntimeInitFailed" crates/polyplug_js/src/error.rs && grep -q "ModuleResolutionFailed" crates/polyplug_js/src/error.rs && grep -q "JsExecutionFailed" crates/polyplug_js/src/error.rs</automated>
  </verify>
  <done>
    File crates/polyplug_js/src/error.rs exists with JsLoaderError enum containing all five variants.
    grep confirms all five variants present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_js/src/error.rs contains "pub enum JsLoaderError"
    - crates/polyplug_js/src/error.rs contains "#[derive(Debug, Error)]"
    - crates/polyplug_js/src/error.rs contains "RolldownNotFound"
    - crates/polyplug_js/src/error.rs contains "JsRuntimePanic"
    - crates/polyplug_js/src/error.rs contains "JsRuntimeInitFailed"
    - crates/polyplug_js/src/error.rs contains "ModuleResolutionFailed"
    - crates/polyplug_js/src/error.rs contains "JsExecutionFailed"
    - crates/polyplug_js/src/error.rs contains "use thiserror::Error;"
    - File has minimum 22 lines
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Export JsLoaderError from lib.rs</name>
  <files>crates/polyplug_js/src/lib.rs</files>
  <read_first>
    - crates/polyplug_js/src/lib.rs (current state, needs error module added)
    - crates/polyplug_native/src/lib.rs (reference pattern for error module export)
  </read_first>
  <action>
    Update `crates/polyplug_js/src/lib.rs` to add error module export (per D-02 from CONTEXT.md).

    Add two lines after existing module declarations:
    ```rust
    pub mod error;
    pub use error::JsLoaderError;
    ```

    The lib.rs should have these lines added in the module declaration section.

    Expected structure after modification:
    ```rust
    //! polyplug_js — QuickJS in-process JS adapter for polyplug.
    //! ...

    pub mod bridge;
    pub mod config;
    pub mod error;           // NEW LINE
    pub mod ffi;
    pub(crate) mod loader;

    pub use bridge::JsHostBridge;
    pub use config::JsConfig;
    pub use error::JsLoaderError;  // NEW LINE
    pub use loader::JsLoader;
    ```

    Note: Keep alphabetical-ish ordering for modules, add error export line after other pub use statements.
  </action>
  <verify>
    <automated>grep -q "pub mod error;" crates/polyplug_js/src/lib.rs && grep -q "pub use error::JsLoaderError;" crates/polyplug_js/src/lib.rs</automated>
  </verify>
  <done>
    File crates/polyplug_js/src/lib.rs exports error module and JsLoaderError type.
    grep confirms both export lines present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_js/src/lib.rs contains "pub mod error;"
    - crates/polyplug_js/src/lib.rs contains "pub use error::JsLoaderError;"
    - Cargo check for polyplug_js crate passes: cargo check -p polyplug_js exits 0
  </acceptance_criteria>
</task>

</tasks>

<verification>
After both tasks:
- JsLoaderError enum is defined with correct variants
- Error module is exported from lib.rs
- Crate compiles successfully
</verification>

<success_criteria>
- `grep "pub enum JsLoaderError" crates/polyplug_js/src/error.rs` returns match
- `grep "pub mod error;" crates/polyplug_js/src/lib.rs` returns match
- `cargo check -p polyplug_js` exits with code 0
</success_criteria>

<output>
After completion, create `.planning/phases/01-define-loader-local-error-types/01-03-SUMMARY.md`
</output>