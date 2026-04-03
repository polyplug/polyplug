---
phase: 01-define-loader-local-error-types
plan: 05
type: execute
wave: 2
depends_on:
  - 01
  - 02
  - 03
  - 04
files_modified:
  - crates/polyplug/src/error.rs
autonomous: true
requirements:
  - ERR-01
  - ERR-02
  - ERR-03
  - ERR-04
  - ERR-05
must_haves:
  truths:
    - "Core LoaderError enum contains no Python, Lua, JS, or .NET specific variants"
    - "LoaderError::InitFailed remains in core (generic catch-all)"
    - "All loader crates export their error types"
  artifacts:
    - path: "crates/polyplug/src/error.rs"
      provides: "Stripped core loader error"
      does_not_contain: "PythonInitFailed|LuaVmInitFailed|JsRuntimePanic|HostfxrNotFound"
      min_lines: 150
  key_links:
    - from: "crates/polyplug/src/error.rs"
      to: "loader crates"
      via: "Removed variants exist in loader-local error types"
      pattern: "grep -c 'PythonInitFailed' crates/polyplug/src/error.rs returns 0"
---

<objective>
Strip loader-specific variants from core LoaderError enum after all loader-local error types are defined.

Purpose: Complete ERR-01 through ERR-04 by removing migrated variants from core (ERR-05 verification)
Output: crates/polyplug/src/error.rs with loader-specific variants removed, generic variants retained
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
<!-- Current LoaderError variants to remove (source: crates/polyplug/src/error.rs lines 87-148) -->

Variants to REMOVE from LoaderError enum:
```rust
// Lines 87-105 (.NET variants)
HostfxrNotFound,                    // line 88
ClrInitFailed { path, reason },     // line 91
AssemblyNotFound { path },          // line 94
RuntimeVersionMismatch { required, found },  // line 102
InvalidFrameworkVersion { tfm, reason },     // line 105

// Lines 107-114 (Python variants)
PythonInitFailed { reason },        // line 108
PythonModuleImportFailed { path, reason },   // line 111
PythonInitRaisedException { bundle, message }, // line 114

// Lines 116-126 (Lua variants)
LuaVmInitFailed { reason },         // line 117
LuaScriptLoadFailed { path, reason },  // line 120
LuaInitFunctionMissing { bundle },  // line 123
LuaInitRaisedError { bundle, message }, // line 126

// Lines 128-148 (JS variants)
RolldownNotFound { hint },          // line 129
JsRuntimePanic { runtime, message }, // line 132
JsRuntimeInitFailed { reason },     // line 135
ModuleResolutionFailed { reason },  // line 145
JsExecutionFailed { reason },       // line 148
```

Variants to KEEP in LoaderError (generic, not loader-specific):
```rust
InitFailed { bundle, error },       // line 65 — conversion target for loader errors
ManifestParse { path, reason },     // line 68
DuplicateLoader { runtime_name },   // line 72
NoLoaderForRuntime { bundle, runtime_name },  // line 77
InitSymbolMissing { bundle },       // line 98 — used by multiple loaders
BundleReadFailed { path, source },  // line 137
VersionMismatch { contract, required, found },  // line 150
FunctionCountMismatch { contract, expected, found },  // line 158
BundleNotADirectory { path },       // line 166
ManifestMissingFile { bundle },     // line 169
BundleTampered { bundle, expected, found },  // line 173
```
</interfaces>
</context>

<tasks>

<task type="auto" tdd="false">
  <name>Task 1: Remove Python variants from LoaderError</name>
  <files>crates/polyplug/src/error.rs</files>
  <read_first>
    - crates/polyplug/src/error.rs (current state, full file)
    - crates/polyplug_native/src/error.rs (reference pattern)
  </read_first>
  <action>
    Remove the following Python-specific variants from LoaderError enum in `crates/polyplug/src/error.rs` (per ERR-01):

    Delete these variant definitions (lines ~107-114):
    ```rust
    #[error("Python interpreter initialization failed: {reason}")]
    PythonInitFailed { reason: String },

    #[error("failed to import Python module at `{path}`: {reason}")]
    PythonModuleImportFailed { path: String, reason: String },

    #[error("Python init function raised exception in bundle `{bundle}`: {message}")]
    PythonInitRaisedException { bundle: String, message: String },
    ```

    Use exact line deletion. Keep all other variants intact, including InitFailed which is generic.

    Important: Do NOT modify any other part of the file — only remove the three Python variants listed above.
  </action>
  <verify>
    <automated>grep -c "PythonInitFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "PythonModuleImportFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "PythonInitRaisedException" crates/polyplug/src/error.rs | grep -q "^0$"</automated>
  </verify>
  <done>
    LoaderError enum no longer contains PythonInitFailed, PythonModuleImportFailed, or PythonInitRaisedException.
    grep -c confirms count of 0 for each removed variant.
  </done>
  <acceptance_criteria>
    - grep -c "PythonInitFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "PythonModuleImportFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "PythonInitRaisedException" crates/polyplug/src/error.rs returns 0
    - grep -q "InitFailed" crates/polyplug/src/error.rs (InitFailed must still exist)
    - grep -q "ManifestParse" crates/polyplug/src/error.rs (ManifestParse must still exist)
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 2: Remove Lua variants from LoaderError</name>
  <files>crates/polyplug/src/error.rs</files>
  <read_first>
    - crates/polyplug/src/error.rs (current state after Task 1)
  </read_first>
  <action>
    Remove the following Lua-specific variants from LoaderError enum in `crates/polyplug/src/error.rs` (per ERR-02):

    Delete these variant definitions (lines ~116-126):
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

    Use exact line deletion. Keep all other variants intact.
  </action>
  <verify>
    <automated>grep -c "LuaVmInitFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "LuaScriptLoadFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "LuaInitFunctionMissing" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "LuaInitRaisedError" crates/polyplug/src/error.rs | grep -q "^0$"</automated>
  </verify>
  <done>
    LoaderError enum no longer contains any Lua-specific variants.
    grep -c confirms count of 0 for each removed variant.
  </done>
  <acceptance_criteria>
    - grep -c "LuaVmInitFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "LuaScriptLoadFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "LuaInitFunctionMissing" crates/polyplug/src/error.rs returns 0
    - grep -c "LuaInitRaisedError" crates/polyplug/src/error.rs returns 0
  </acceptance_criteria>
</task>

<task type="auto" tdd="false">
  <name>Task 3: Remove JS and .NET variants from LoaderError</name>
  <files>crates/polyplug/src/error.rs</files>
  <read_first>
    - crates/polyplug/src/error.rs (current state after Tasks 1-2)
  </read_first>
  <action>
    Remove the following JS-specific and .NET-specific variants from LoaderError enum in `crates/polyplug/src/error.rs` (per ERR-03, ERR-04):

    Delete these .NET variant definitions (lines ~87-105):
    ```rust
    #[error("hostfxr not found: searched DOTNET_ROOT, PATH, and well-known paths")]
    HostfxrNotFound,

    #[error("CLR initialization failed for runtime config `{path}`: {reason}")]
    ClrInitFailed { path: String, reason: String },

    #[error("assembly not found at path `{path}`")]
    AssemblyNotFound { path: String },

    #[error("init symbol missing in assembly `{bundle}`: expected `[UnmanagedCallersOnly] polyplug_init`")]
    InitSymbolMissing { bundle: String },

    #[error(".NET runtime version mismatch: required={required}, found={found}")]
    RuntimeVersionMismatch { required: String, found: String },

    #[error("invalid .NET framework version in TFM `{tfm}`: {reason}")]
    InvalidFrameworkVersion { tfm: String, reason: String },
    ```

    Delete these JS variant definitions (lines ~128-148):
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

    Wait: InitSymbolMissing is used by both Python and .NET loaders (see polyplug_python/src/lib.rs line 207). This is NOT loader-specific — it should STAY in core. Only remove the other .NET variants.

    Corrected removal list for .NET:
    ```rust
    HostfxrNotFound,
    ClrInitFailed { path, reason },
    AssemblyNotFound { path },
    RuntimeVersionMismatch { required, found },
    InvalidFrameworkVersion { tfm, reason },
    ```

    Do NOT remove InitSymbolMissing — it's generic.

    Use exact line deletion. Keep all generic variants intact including InitFailed, InitSymbolMissing, etc.
  </action>
  <verify>
    <automated>grep -c "HostfxrNotFound" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "ClrInitFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "AssemblyNotFound" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "RuntimeVersionMismatch" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "InvalidFrameworkVersion" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "RolldownNotFound" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "JsRuntimePanic" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "JsRuntimeInitFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "ModuleResolutionFailed" crates/polyplug/src/error.rs | grep -q "^0$" && grep -c "JsExecutionFailed" crates/polyplug/src/error.rs | grep -q "^0$"</automated>
  </verify>
  <done>
    LoaderError enum no longer contains any JS or .NET-specific variants (except InitSymbolMissing which stays).
    grep -c confirms count of 0 for each removed variant.
  </done>
  <acceptance_criteria>
    - grep -c "HostfxrNotFound" crates/polyplug/src/error.rs returns 0
    - grep -c "ClrInitFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "AssemblyNotFound" crates/polyplug/src/error.rs returns 0
    - grep -c "RuntimeVersionMismatch" crates/polyplug/src/error.rs returns 0
    - grep -c "InvalidFrameworkVersion" crates/polyplug/src/error.rs returns 0
    - grep -c "RolldownNotFound" crates/polyplug/src/error.rs returns 0
    - grep -c "JsRuntimePanic" crates/polyplug/src/error.rs returns 0
    - grep -c "JsRuntimeInitFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "ModuleResolutionFailed" crates/polyplug/src/error.rs returns 0
    - grep -c "JsExecutionFailed" crates/polyplug/src/error.rs returns 0
    - grep -q "InitSymbolMissing" crates/polyplug/src/error.rs (InitSymbolMissing must still exist - generic)
    - grep -q "InitFailed" crates/polyplug/src/error.rs (InitFailed must still exist)
  </acceptance_criteria>
</task>

</tasks>

<verification>
After all three tasks:
- Core LoaderError contains only generic variants
- All loader-specific variants removed
- InitFailed and InitSymbolMissing remain (generic catch-all and multi-loader variant)
- All loader crates export their error types (verified by depending on Plans 01-04)

Run verification:
```bash
# Verify all loader-specific variants removed
grep -E "PythonInitFailed|PythonModuleImportFailed|PythonInitRaisedException|LuaVmInitFailed|LuaScriptLoadFailed|LuaInitFunctionMissing|LuaInitRaisedError|RolldownNotFound|JsRuntimePanic|JsRuntimeInitFailed|ModuleResolutionFailed|JsExecutionFailed|HostfxrNotFound|ClrInitFailed|AssemblyNotFound|RuntimeVersionMismatch|InvalidFrameworkVersion" crates/polyplug/src/error.rs || echo "All loader-specific variants removed"

# Verify generic variants remain
grep "InitFailed" crates/polyplug/src/error.rs
grep "InitSymbolMissing" crates/polyplug/src/error.rs
```
</verification>

<success_criteria>
- `grep -c "PythonInitFailed" crates/polyplug/src/error.rs` returns 0
- `grep -c "LuaVmInitFailed" crates/polyplug/src/error.rs` returns 0
- `grep -c "JsRuntimePanic" crates/polyplug/src/error.rs` returns 0
- `grep -c "HostfxrNotFound" crates/polyplug/src/error.rs` returns 0
- `grep "InitFailed" crates/polyplug/src/error.rs` returns match (generic variant retained)
- `grep "InitSymbolMissing" crates/polyplug/src/error.rs` returns match (generic variant retained)
- All loader error types exported in their respective crates (dependency on Plans 01-04)
</success_criteria>

<output>
After completion, create `.planning/phases/01-define-loader-local-error-types/01-05-SUMMARY.md`
</output>