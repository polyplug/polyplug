---
phase: 01-define-loader-local-error-types
plan: 02
type: execute
wave: 1
depends_on: []
files_modified:
  - crates/polyplug_lua/src/error.rs
  - crates/polyplug_lua/src/lib.rs
autonomous: true
requirements:
  - ERR-02
must_haves:
  truths:
    - "LuaLoaderError enum exists in polyplug_lua crate"
    - "LuaLoaderError contains LuaVmInitFailed, LuaScriptLoadFailed, LuaInitFunctionMissing, LuaInitRaisedError variants"
    - "polyplug_lua lib.rs exports LuaLoaderError"
  artifacts:
    - path: "crates/polyplug_lua/src/error.rs"
      provides: "Lua-specific loader error type"
      contains: "pub enum LuaLoaderError"
      min_lines: 25
    - path: "crates/polyplug_lua/src/lib.rs"
      provides: "Error module export"
      contains: "pub mod error;"
  key_links:
    - from: "crates/polyplug_lua/src/lib.rs"
      to: "error.rs"
      via: "pub mod error;"
      pattern: "pub mod error;\\s*pub use error::LuaLoaderError;"
---

<objective>
Define LuaLoaderError enum in polyplug_lua crate, following NativeLoaderError pattern.

Purpose: Establish loader-local error type for Lua-specific failures (ERR-02 partial — create type)
Output: crates/polyplug_lua/src/error.rs with exported LuaLoaderError enum
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

Core LoaderError variants to migrate (source: crates/polyplug/src/error.rs lines 116-126):
```rust
#[error("lua vm init failed: {reason}")]
LuaVmInitFailed { reason: String },

#[error("lua script load failed: path={path}, reason={reason}")]
LuaScriptLoadFailed { path: String, reason: String },

#[error("lua plugin missing polyplug_init function: bundle={bundle}")]
LuaInitFunctionMissing { bundle: String },

#[error("lua polyplug_init raised error: bundle={bundle}, message={message}")]
LuaInitRaisedError { bundle: String, message: String },
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="false">
  <name>Task 1: Create LuaLoaderError enum</name>
  <files>crates/polyplug_lua/src/error.rs</files>
  <read_first>
    - crates/polyplug_native/src/error.rs (reference pattern for error enum structure)
    - crates/polyplug/src/error.rs (lines 116-126, source of variants to migrate)
  </read_first>
  <action>
    Create `crates/polyplug_lua/src/error.rs` with the following exact content (per D-02, D-03, D-06 from CONTEXT.md):

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

    Notes:
    - Follow NativeLoaderError pattern exactly (same derive, same #[error] format)
    - Keep variant names identical to core LoaderError variants for traceability
    - Keep field names identical (reason, path, bundle, message)
    - No #[source] attribute needed — all fields are String, not Error types
  </action>
  <verify>
    <automated>grep -q "pub enum LuaLoaderError" crates/polyplug_lua/src/error.rs && grep -q "LuaVmInitFailed" crates/polyplug_lua/src/error.rs && grep -q "LuaScriptLoadFailed" crates/polyplug_lua/src/error.rs && grep -q "LuaInitFunctionMissing" crates/polyplug_lua/src/error.rs && grep -q "LuaInitRaisedError" crates/polyplug_lua/src/error.rs</automated>
  </verify>
  <done>
    File crates/polyplug_lua/src/error.rs exists with LuaLoaderError enum containing all four variants.
    grep confirms all four variants present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_lua/src/error.rs contains "pub enum LuaLoaderError"
    - crates/polyplug_lua/src/error.rs contains "#[derive(Debug, Error)]"
    - crates/polyplug_lua/src/error.rs contains "LuaVmInitFailed"
    - crates/polyplug_lua/src/error.rs contains "LuaScriptLoadFailed"
    - crates/polyplug_lua/src/error.rs contains "LuaInitFunctionMissing"
    - crates/polyplug_lua/src/error.rs contains "LuaInitRaisedError"
    - crates/polyplug_lua/src/error.rs contains "use thiserror::Error;"
    - File has minimum 20 lines
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Export LuaLoaderError from lib.rs</name>
  <files>crates/polyplug_lua/src/lib.rs</files>
  <read_first>
    - crates/polyplug_lua/src/lib.rs (current state, needs error module added)
    - crates/polyplug_native/src/lib.rs (reference pattern for error module export)
  </read_first>
  <action>
    Update `crates/polyplug_lua/src/lib.rs` to add error module export (per D-02 from CONTEXT.md).

    Add two lines after existing module declarations:
    ```rust
    pub mod error;
    pub use error::LuaLoaderError;
    ```

    The lib.rs should have these lines added in the module declaration section.

    Expected structure after modification:
    ```rust
    //! polyplug_lua: LuaJIT plugin loader for the polyplug runtime.

    pub mod bridge;
    pub mod config;
    pub mod error;           // NEW LINE
    pub mod ffi;
    pub mod loader;

    pub use bridge::LuaHostBridge;
    pub use config::LuaConfig;
    pub use error::LuaLoaderError;  // NEW LINE
    pub use loader::LuaLoader;
    ```

    Note: Keep alphabetical-ish ordering for modules, add error export line after other pub use statements.
  </action>
  <verify>
    <automated>grep -q "pub mod error;" crates/polyplug_lua/src/lib.rs && grep -q "pub use error::LuaLoaderError;" crates/polyplug_lua/src/lib.rs</automated>
  </verify>
  <done>
    File crates/polyplug_lua/src/lib.rs exports error module and LuaLoaderError type.
    grep confirms both export lines present.
  </done>
  <acceptance_criteria>
    - crates/polyplug_lua/src/lib.rs contains "pub mod error;"
    - crates/polyplug_lua/src/lib.rs contains "pub use error::LuaLoaderError;"
    - Cargo check for polyplug_lua crate passes: cargo check -p polyplug_lua exits 0
  </acceptance_criteria>
</task>

</tasks>

<verification>
After both tasks:
- LuaLoaderError enum is defined with correct variants
- Error module is exported from lib.rs
- Crate compiles successfully
</verification>

<success_criteria>
- `grep "pub enum LuaLoaderError" crates/polyplug_lua/src/error.rs` returns match
- `grep "pub mod error;" crates/polyplug_lua/src/lib.rs` returns match
- `cargo check -p polyplug_lua` exits with code 0
</success_criteria>

<output>
After completion, create `.planning/phases/01-define-loader-local-error-types/01-02-SUMMARY.md`
</output>