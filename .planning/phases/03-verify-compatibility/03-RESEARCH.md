# Phase 3: Verify Compatibility - Research

**Researched:** 2026-04-03
**Domain:** Test verification, FFI error boundary, build state
**Confidence:** HIGH (direct code inspection and git history)

## Summary

Phase 3 verification faces a significant obstacle: Phase 2 was marked complete but left **incomplete work**. The loader `lib.rs` files were updated to use `LoaderError::InitFailed`, but supporting source files (`context.rs`, `version.rs`) and all test files still reference removed error variants. Additionally, the core polyplug crate has pre-existing WIP build errors unrelated to error handling.

**Primary recommendation:** This phase requires completing Phase 2's incomplete work before verification can proceed. The tests cannot run until source files and test files are updated to use the new `LoaderError::InitFailed` pattern.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Verify loader crates only — core polyplug has ongoing WIP refactoring and won't compile for a while
- **D-02:** Run loader tests + integration tests:
  - `cargo test -p polyplug_python`
  - `cargo test -p polyplug_lua`
  - `cargo test -p polyplug_js`
  - `cargo test -p polyplug_dotnet`
  - Integration tests in `tests/integration/tests/` that test loaders
- **D-03:** Explicit check of error string format at FFI boundary:
  - Verify error messages are strings at the FFI boundary
  - Check that `LoaderError::InitFailed` produces string messages
  - No breaking changes to public FFI API
- **D-04:** Document that core polyplug crate has pre-existing build errors from WIP refactoring (commit `3c156e5`)
  - These are out of scope for this phase
  - Do not block verification on fixing these

### Claude's Discretion
- Exact test commands and flags
- How to present verification results
- Whether to skip specific failing tests if they're unrelated to error handling

### Deferred Ideas (OUT OF SCOPE)
- Full workspace build verification (blocked by core WIP refactoring)
- SDK compilation verification
- Example host compilation verification
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| COMP-01 | All existing tests pass after error type migration | BLOCKED: Tests reference removed error variants; cannot compile |
| COMP-02 | No breaking changes to public FFI API (error messages are strings at FFI boundary) | VERIFIED: FFI uses `.to_string()` conversion; messages are strings |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| Rust test harness | Built-in | Test runner | Default Cargo integration |
| tempfile | 3.x | Test fixtures | Workspace dependency |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| criterion | 0.8 | Benchmarks | Performance testing |

**No installation needed:** Uses built-in Rust test framework.

## Architecture Patterns

### FFI Error Conversion Path
```
LoaderError::InitFailed { bundle, error }
    ↓ RuntimeError::Loader(LoaderError::...)
    ↓ FFI function: runtime.set_last_error(e.to_string())
    ↓ Stored in Mutex<String>
    ↓ Host calls: polyplug_runtime_last_error(buf, len)
    ↓ Returns UTF-8 string bytes
```

**Key insight:** All errors are converted to strings via `.to_string()` at the FFI boundary. The `LoaderError::InitFailed` variant's Display implementation produces human-readable messages containing both `bundle` and `error` fields.

### Test File Organization
```
crates/polyplug_python/tests/python_loader.rs    # Python loader tests
crates/polyplug_lua/tests/lua_loader.rs          # Lua loader tests
crates/polyplug_js/tests/quickjs_loader.rs       # JS loader tests
crates/polyplug_dotnet/tests/dotnet_loader.rs    # .NET loader tests
tests/integration/tests/integration_*.rs         # Cross-loader integration tests
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Error display formatting | Custom formatter | `thiserror::Error` derive | Automatic Display implementation |
| Test fixtures | Manual file creation | `write_temp_bundle()` helper | Reusable, consistent manifest format |

## Critical Discovery: Phase 2 Incomplete

### Source Files Still Using Removed Error Variants

**HIGH confidence — verified by direct grep search:**

| File | Line | Removed Variant | Expected Pattern |
|------|------|-----------------|------------------|
| `crates/polyplug_python/src/context.rs` | 37 | `LoaderError::RuntimeVersionMismatch` | `LoaderError::InitFailed { bundle, error: "version mismatch..." }` |
| `crates/polyplug_dotnet/src/version.rs` | 52, 59, 78, 87 | `LoaderError::AssemblyNotFound` | `LoaderError::InitFailed { bundle, error: "assembly not found..." }` |
| `crates/polyplug_dotnet/src/context.rs` | 91, 98, 104, etc. | `LoaderError::ClrInitFailed` | `LoaderError::InitFailed { bundle, error: "CLR init failed..." }` |

### Test Files Still Using Removed Error Variants

**HIGH confidence — verified by direct grep search:**

| File | Occurrences | Removed Variants |
|------|-------------|------------------|
| `crates/polyplug_python/tests/python_loader.rs` | 7 | `RuntimeVersionMismatch`, `PythonModuleImportFailed`, `PythonInitRaisedException` |
| `crates/polyplug_dotnet/tests/dotnet_loader.rs` | 12 | `AssemblyNotFound`, `RuntimeVersionMismatch`, `ClrInitFailed` |
| `crates/polyplug_lua/tests/lua_loader.rs` | 4 | `LuaScriptLoadFailed`, `LuaInitRaisedError`, `LuaInitFunctionMissing` |
| `tests/integration/tests/integration_python.rs` | 2 | `PythonInitRaisedException`, `RuntimeVersionMismatch` |
| `tests/integration/tests/integration_lua.rs` | 1 | `LuaInitFunctionMissing` |
| `tests/integration/tests/integration_dotnet.rs` | 3 | `RuntimeVersionMismatch`, `AssemblyNotFound` |
| `tests/integration/tests/integration_loader_dispatch.rs` | 4 | `AssemblyNotFound`, `ClrInitFailed`, `PythonModuleImportFailed`, `LuaScriptLoadFailed` |

### Phase 02 Verification Gap

The Phase 02 VERIFICATION.md checked only:
- `crates/polyplug_python/src/lib.rs` (VERIFIED)
- `crates/polyplug_lua/src/loader.rs` (VERIFIED)
- `crates/polyplug_js/src/loader.rs` (VERIFIED)
- `crates/polyplug_dotnet/src/lib.rs` (VERIFIED)

**Missed files:**
- `crates/polyplug_python/src/context.rs`
- `crates/polyplug_dotnet/src/version.rs`
- `crates/polyplug_dotnet/src/context.rs`
- All test files

## Core Crate Build Status

**HIGH confidence — verified by cargo check output:**

The core `polyplug` crate has **114 errors** from WIP refactoring (commit `3c156e5`). These are UNRELATED to error handling:

| Error Type | Location | Cause |
|------------|----------|-------|
| `E0432` unresolved import | `ffi.rs:11` | `VTableSlot` not exported from `plugin_registry` |
| `E0432` unresolved import | `runtime_builder.rs:7` | `CapabilityGraph` not exported |
| `E0432` unresolved import | `runtime_builder.rs:11` | `ReloadCb` not exported |
| `E0425` cannot find type | `ffi.rs:56,60,76,89` | `StringViewC` not defined |
| `E0425` cannot find type | `ffi.rs:150` | `RuntimeConfigC` not defined |
| `E0252` duplicate import | `runtime_builder.rs:8` | `RuntimeError` imported twice |

**Per D-04, these are OUT OF SCOPE for this phase.**

## FFI Boundary Verification

**HIGH confidence — verified by code inspection:**

The error conversion at the FFI boundary uses `.to_string()`:

```rust
// Source: crates/polyplug/src/ffi.rs:247
match runtime.0.load_bundle(std::path::Path::new(s)) {
    Ok(()) => 0u32,
    Err(e) => {
        runtime.0.set_last_error(e.to_string());  // ← String conversion
        1u32
    }
}
```

The `set_last_error` method stores the string:

```rust
// Source: crates/polyplug/src/runtime.rs:234-235
pub(crate) fn set_last_error(&self, msg: impl Into<String>) {
    let mut guard = self.last_error.lock().unwrap_or_else(...);
    guard.clear();
    guard.push_str(&msg.into());
}
```

The `LoaderError::InitFailed` Display implementation (from `thiserror::Error` derive):

```rust
// Source: crates/polyplug/src/error.rs:61-62
#[error("init failed for bundle `{bundle}`: {error}")]
InitFailed { bundle: String, error: String },
```

**COMP-02 is satisfied:** Error messages are strings at the FFI boundary.

## Common Pitfalls

### Pitfall 1: Tests Cannot Compile
**What goes wrong:** Tests reference removed error variants and fail to compile.
**Why it happens:** Phase 2 updated loader `lib.rs` but not test files.
**How to avoid:** Update test assertions to match `LoaderError::InitFailed` pattern.
**Warning signs:** Compilation errors like `no variant named PythonModuleImportFound`.

### Pitfall 2: Supporting Source Files Missed
**What goes wrong:** `context.rs` and `version.rs` still use old variants.
**Why it happens:** These files weren't in the Phase 2 plan scope.
**How to avoid:** Grep for all error variant references, not just in main loader files.
**Warning signs:** Tests fail at runtime with "unexpected error type" assertions.

### Pitfall 3: Core Crate Blocker
**What goes wrong:** Attempting `cargo test --workspace` fails immediately.
**Why it happens:** Core crate has unrelated WIP refactoring errors.
**How to avoid:** Use `-p` flags to test specific crates, not workspace-wide.
**Warning signs:** 114 errors from `ffi.rs`, `runtime_builder.rs`.

## Code Examples

### Correct Error Pattern (Post-Phase-2)
```rust
// Source: crates/polyplug/src/error.rs:61-62
#[error("init failed for bundle `{bundle}`: {error}")]
InitFailed { bundle: String, error: String },
```

### Incorrect Pattern (Still in Tests)
```rust
// Source: crates/polyplug_python/tests/python_loader.rs:301
RuntimeError::Loader(LoaderError::PythonModuleImportFailed { path: p, reason })
// ^^^ This variant no longer exists
```

### Required Update Pattern
```rust
// Tests should match InitFailed pattern:
match err {
    RuntimeError::Loader(LoaderError::InitFailed { bundle, error }) => {
        assert!(error.contains("import failed"), "got: {error}");
        assert!(bundle.contains("expected_name"), "got: {bundle}");
    }
    other => panic!("expected InitFailed, got: {other:?}"),
}
```

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.85+ | Cargo build | ✓ | 1.85.0 | — |
| Python 3.10+ | polyplug_python tests | ✓ | 3.12 | — |
| .NET 10.0 SDK | polyplug_dotnet tests | ✓ | 10.0.100 | — |
| Lua dev headers | polyplug_lua tests | ✓ | 5.4 | — |

**Missing dependencies with no fallback:** None detected.

## Open Questions

1. **How should test assertions be updated?**
   - What we know: Tests check for specific error variants and fields.
   - What's unclear: Whether to preserve field-level assertions or just check message content.
   - Recommendation: Update to match `InitFailed { bundle, error }` pattern, check `error` field contains expected details.

2. **Should supporting source files be updated in Phase 3 or Phase 2 retrofix?**
   - What we know: Phase 2 verification marked these as complete.
   - What's unclear: Whether to treat as Phase 2 scope creep or Phase 3 prerequisite.
   - Recommendation: Update as Phase 3 prerequisite — tests cannot run without these fixes.

## Sources

### Primary (HIGH confidence)
- `crates/polyplug/src/error.rs` — Current `LoaderError` definition (lines 59-126)
- `crates/polyplug/src/ffi.rs` — FFI error conversion (lines 247, 288, 416, 520, 566)
- `crates/polyplug/src/runtime.rs` — Error storage (lines 234-266)
- Git history: Phase 02 commits `0fb17d8`, `6d523da`, `87b1d69`, `5d95ba6`

### Secondary (MEDIUM confidence)
- `.planning/phases/02-update-loader-implementations/02-VERIFICATION.md` — Verification scope (checked lib.rs only)

### Tertiary (LOW confidence)
- None needed — all findings verified by direct code inspection.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — Built-in Rust test framework
- Architecture: HIGH — FFI code inspection complete
- Pitfalls: HIGH — Direct grep search verified all variants
- Phase 2 incomplete: HIGH — Grep results and git history corroborate

**Research date:** 2026-04-03
**Valid until:** 30 days — stable project state