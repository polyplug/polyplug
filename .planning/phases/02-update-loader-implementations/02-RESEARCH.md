# Phase 2: Update Loader Implementations - Research

**Researched:** 2026-04-03
**Domain:** Rust error handling refactoring, loader trait implementations
**Confidence:** HIGH

## Summary

Phase 1 successfully removed loader-specific error variants from the core `LoaderError` enum. Phase 2 now needs to update all loader implementations to use the unified `LoaderError::InitFailed { bundle, error }` pattern directly at error sites, and clean up the unused local error type files created in Phase 1.

The core `LoaderError` enum now contains only generic variants: `InitFailed`, `ManifestParse`, `DuplicateLoader`, `NoLoaderForRuntime`, `InitSymbolMissing`, `BundleReadFailed`, `VersionMismatch`, `FunctionCountMismatch`, `BundleNotADirectory`, `ManifestMissingFile`, and `BundleTampered`. All loader-specific variants (`PythonModuleImportFailed`, `LuaVmInitFailed`, `JsRuntimePanic`, `AssemblyNotFound`, etc.) have been removed from core.

**Primary recommendation:** Replace all loader-specific error construction sites with `LoaderError::InitFailed { bundle: manifest.name.clone(), error: format!("...") }` directly. No intermediate error types needed.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**D-01:** Use `LoaderError::InitFailed` directly with string messages — no local error types
- **Rationale:** Simpler approach; no need for intermediate error enums; string messages are sufficient for diagnostics

**D-02:** Keep error handling inline for all loaders, including `NativeLoader`
- Remove `NativeLoader::load_internal()` method
- Each loader constructs `LoaderError::InitFailed` directly at error sites
- **Rationale:** Consistency and simplicity

**D-03:** All loaders return `RuntimeError::HotReloadDisabled` for unsupported hot-reload
- **Rationale:** Consistency — this is a runtime configuration issue

**D-04:** Remove unused local error types:
- `crates/polyplug_python/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
- `crates/polyplug_lua/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
- `crates/polyplug_js/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
- `crates/polyplug_dotnet/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
- **Note:** `NativeLoaderError` in `polyplug_native` may also be removed if not needed

### Claude's Discretion

- Exact error message strings — make them descriptive and include relevant context
- Order of migration (which loader first) — any order works

### Deferred Ideas (OUT OF SCOPE)

None — discussion stayed within phase scope.

</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| ERR-06 | Update loader `load()` and `reload()` implementations to use `LoaderError::InitFailed` directly with descriptive string messages (no intermediate error types) | Core `LoaderError::InitFailed` pattern documented; all loader error sites identified; migration pattern defined |

</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| thiserror 2.0 | workspace | Error derive macro | Project standard for error types |
| anyhow 1.0 | workspace | Error chaining | Project standard for internal errors |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::format! | N/A | Error message construction | At every error site |

**No installation needed:** All dependencies are already in workspace.

## Architecture Patterns

### Current Error Pattern (Target for Update)

The loaders currently use loader-specific error variants that no longer exist in core:

```rust
// CURRENT (BROKEN - variants removed from core):
Err(RuntimeError::Loader(LoaderError::PythonModuleImportFailed {
    path: bundle_path.to_string_lossy().into_owned(),
    reason: "file does not exist".to_owned(),
}))

Err(RuntimeError::Loader(LoaderError::LuaVmInitFailed {
    reason: format!("failed to set guest package.path: {}", e),
}))

Err(RuntimeError::Loader(LoaderError::JsRuntimePanic {
    runtime: "js-quickjs".to_owned(),
    message: format!("context creation failed: {e}"),
}))
```

### Target Error Pattern

All error sites should use `InitFailed` directly:

```rust
// TARGET (unified pattern):
Err(RuntimeError::Loader(LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("failed to load Python module at {}: file does not exist",
                   bundle_path.to_string_lossy()),
}))

Err(RuntimeError::Loader(LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("Lua VM init failed: package.path injection failed: {e}"),
}))

Err(RuntimeError::Loader(LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("JS runtime panic: context creation failed: {e}"),
}))
```

### Generic Errors (Use Directly from Core)

Some errors are already generic and should be used directly:

```rust
// ManifestMissingFile - use directly:
Err(RuntimeError::Loader(LoaderError::ManifestMissingFile {
    bundle: manifest.name.clone(),
}))

// InitSymbolMissing - use directly:
Err(RuntimeError::Loader(LoaderError::InitSymbolMissing {
    bundle: bundle_name.clone(),
}))

// BundleTampered - use directly:
Err(RuntimeError::Loader(LoaderError::BundleTampered {
    bundle: path_str,
    expected: expected_bundle_id.id(),
    found: host_ctx.bundle_id,
}))
```

### Hot-Reload Unsupported Pattern

All loaders should use consistent pattern for hot-reload disabled:

```rust
fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
    Err(RuntimeError::HotReloadDisabled)
}
```

**Note:** The JS loader currently uses a different pattern (`LoaderError::JsRuntimePanic`) — this should be changed to `RuntimeError::HotReloadDisabled` per D-03.

### Anti-Patterns to Avoid

- **Intermediate error types:** Don't create/use local error enums like `PythonLoaderError`, `LuaLoaderError` etc.
- **`load_internal()` with separate error type:** NativeLoader's `load_internal()` returns `NativeLoaderError` which is then converted — this adds unnecessary indirection.
- **Inconsistent hot-reload errors:** Using different error types for hot-reload disabled across loaders.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Loader-specific errors | Local error enums with 5-10 variants | `LoaderError::InitFailed { bundle, error }` | Simpler, consistent, sufficient for diagnostics |
| Error conversion | `impl From<LocalError> for LoaderError` | Direct construction | Eliminates conversion layer |

**Key insight:** Error messages are strings at the FFI boundary anyway — structured intermediate error types add complexity without value.

## Runtime State Inventory

> Not applicable — this is a code refactoring phase, not a rename/migration phase.

## Common Pitfalls

### Pitfall 1: Missing Error Context
**What goes wrong:** Generic error messages like "init failed" without details make debugging impossible.
**Why it happens:** Developer assumes the bundle name is sufficient context.
**How to avoid:** Always include specific details in the error string: path, operation, underlying error message.
**Warning signs:** Error messages without file paths, operation names, or root cause.

Example of good error message:
```rust
error: format!("failed to load Python module at {}: {}",
               path, underlying_error)
```

### Pitfall 2: Inconsistent Bundle Name Usage
**What goes wrong:** Using `bundle_path` instead of `manifest.name` for the `bundle` field.
**Why it happens:** Bundle name variable differs across loaders (bundle_name, bundle_path, manifest.name).
**How to avoid:** Always use `manifest.name.clone()` for the `bundle` field in `InitFailed`.
**Warning signs:** `bundle:` field contains file paths instead of bundle names.

### Pitfall 3: Forgetting to Remove Error Imports
**What goes wrong:** Compilation fails after removing error.rs but import remains.
**Why it happens:** `pub use error::LocalLoaderError;` in lib.rs is forgotten.
**How to avoid:** When deleting error.rs, also remove `pub mod error;` and `pub use` from lib.rs.
**Warning signs:** `use crate::error::LocalLoaderError;` still present after deletion.

### Pitfall 4: Hot-Reload Error Inconsistency
**What goes wrong:** Some loaders use `RuntimeError::HotReloadDisabled`, others use `LoaderError::InitFailed`.
**Why it happens:** Loader implementations evolved independently.
**How to avoid:** All loaders must return `RuntimeError::HotReloadDisabled` for unsupported hot-reload (D-03).
**Warning signs:** `LoaderError::InitFailed` or loader-specific error in `reload()` method.

## Code Examples

### Pattern 1: Direct InitFailed Construction

```rust
// Source: crates/polyplug/src/error.rs:61-62
// Target pattern for all loader error sites:

Err(RuntimeError::Loader(LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("descriptive message with context: {}", details),
}))
```

### Pattern 2: NativeLoader Inline (Remove load_internal)

Current structure:
```rust
// crates/polyplug_native/src/loader.rs:51-163
fn load_internal(&self, path: &Path, manifest: &ManifestData, runtime: &Runtime)
    -> Result<libloading::Library, NativeLoaderError> { ... }

fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
    let library = self.load_internal(&bundle_path, manifest, runtime)
        .map_err(|e| RuntimeError::Loader(LoaderError::InitFailed {
            bundle: manifest.name.clone(),
            error: e.to_string(),
        }))?;
    // ...
}
```

Target structure (inline):
```rust
fn load(&self, manifest: &ManifestData, runtime: &Runtime) -> Result<(), RuntimeError> {
    // Step 1: dlopen the library
    let library: libloading::Library = unsafe {
        libloading::Library::new(&bundle_path).map_err(|e| {
            RuntimeError::Loader(LoaderError::InitFailed {
                bundle: manifest.name.clone(),
                error: format!("failed to load plugin library at {}: {}",
                               bundle_path.display(), e),
            })
        })?
    };

    // Step 2: Check ABI version...
    // All error sites inline with InitFailed directly
}
```

### Pattern 3: Delete Local Error Module

```rust
// BEFORE (lib.rs):
pub mod error;
pub use error::LocalLoaderError;

// AFTER (lib.rs):
// (error module deleted, pub mod and pub use removed)
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Loader-specific error variants in core | Unified `InitFailed` with string messages | Phase 1 (2026-04-03) | Core crate is loader-agnostic |
| Intermediate local error types | Direct `InitFailed` construction | Phase 2 (target) | Simpler error handling, no conversion |

**Deprecated/outdated:**
- Local error enums (`PythonLoaderError`, `LuaLoaderError`, `JsLoaderError`, `DotnetLoaderError`, `NativeLoaderError`): No longer needed after Phase 2.

## Open Questions

1. **Should NativeLoaderError be kept or removed?**
   - What we know: D-04 mentions it "may also be removed if not needed"
   - What's unclear: Whether there's any external dependency on `NativeLoaderError`
   - Recommendation: Remove it — it's only used internally by `load_internal()` which is being removed anyway.

## Environment Availability

> SKIPPED — This phase has no external dependencies. It's purely code refactoring within the existing Rust workspace.

## Validation Architecture

> SKIPPED — workflow.nyquist_validation is set to false in config.json. Test verification is deferred to Phase 3.

## Sources

### Primary (HIGH confidence)
- `crates/polyplug/src/error.rs` — Core error types after Phase 1 (verified: loader-specific variants removed)
- `crates/polyplug/src/loader/bundle_loader.rs` — BundleLoader trait signature (verified: returns `Result<(), RuntimeError>`)
- `crates/polyplug_native/src/loader.rs` — NativeLoader implementation (verified: uses `load_internal()`)
- `crates/polyplug_python/src/lib.rs` — PythonLoader implementation (verified: uses removed variants)
- `crates/polyplug_lua/src/loader.rs` — LuaLoader implementation (verified: uses removed variants)
- `crates/polyplug_js/src/loader.rs` — JsLoader implementation (verified: uses removed variants)
- `crates/polyplug_dotnet/src/lib.rs` — DotnetLoader implementation (verified: uses removed variants)

### Secondary (MEDIUM confidence)
- `.planning/phases/02-update-loader-implementations/02-CONTEXT.md` — User decisions locked for this phase
- `.planning/REQUIREMENTS.md` — ERR-06 requirement specification

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH — No new dependencies needed; using existing thiserror pattern
- Architecture: HIGH — Pattern is already defined in CONTEXT.md; all error sites identified
- Pitfalls: HIGH — Based on observed patterns in existing code

**Research date:** 2026-04-03
**Valid until:** 30 days (stable Rust patterns)

---

## Loader Error Sites Inventory

For implementation reference, here are all error sites in each loader that need updating:

### NativeLoader (crates/polyplug_native/src/loader.rs)

| Line | Current Error | Target |
|------|---------------|--------|
| 173-176 | `InitFailed` (already correct) | Keep |
| 182-184 | `ManifestMissingFile` (generic) | Keep |
| 62-65 | `NativeLoaderError::LoadFailed` | `InitFailed` with path and source |
| 71-77 | `NativeLoaderError::MissingSymbol` (abi_version) | `InitFailed` with symbol name |
| 80-84 | `NativeLoaderError::AbiVersionMismatch` | `InitFailed` with version details |
| 104-107 | `NativeLoaderError::MissingSymbol` (polyplug_init) | `InitFailed` with symbol name |
| 139-143 | `NativeLoaderError::BundleTampered` | Use `LoaderError::BundleTampered` directly |
| 156-159 | `NativeLoaderError::InitFailed` | Already matches pattern (just needs to be returned directly) |

### PythonLoader (crates/polyplug_python/src/lib.rs)

| Line | Current Error | Target |
|------|---------------|--------|
| 73-75 | `ManifestMissingFile` (generic) | Keep |
| 79-84 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 89-94 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 121-126 | `PythonInitRaisedException` | `InitFailed` with bundle and message |
| 129-133 | `PythonInitRaisedException` | `InitFailed` with bundle and message |
| 136-141 | `PythonInitRaisedException` | `InitFailed` with bundle and message |
| 144-152 | `PythonInitRaisedException` | `InitFailed` with bundle and message |
| 156-160 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 166-169 | `PythonInitFailed` | `InitFailed` with reason |
| 171-175 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 180-184 | `PythonInitFailed` | `InitFailed` with reason |
| 186-190 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 198-203 | `PythonModuleImportFailed` | `InitFailed` with path and reason |
| 207-211 | `InitSymbolMissing` (generic) | Keep |
| 231-236 | `PythonInitRaisedException` | `InitFailed` with bundle and message |
| 248 | `HotReloadDisabled` | Already correct |

### LuaLoader (crates/polyplug_lua/src/loader.rs)

| Line | Current Error | Target |
|------|---------------|--------|
| 122-125 | `ManifestMissingFile` (generic) | Keep |
| 127-131 | `LuaScriptLoadFailed` | `InitFailed` with path and reason |
| 149-155 | `LuaVmInitFailed` | `InitFailed` with reason |
| 163-166 | `LuaVmInitFailed` | `InitFailed` with reason |
| 170-175 | `LuaScriptLoadFailed` | `InitFailed` with path and reason |
| 182-186 | `LuaVmInitFailed` | `InitFailed` with reason |
| 193-196 | `LuaVmInitFailed` | `InitFailed` with reason |
| 198-203 | `LuaScriptLoadFailed` | `InitFailed` with path and reason |
| 215-219 | `LuaInitFunctionMissing` | `InitFailed` with bundle name |
| 248-254 | `LuaInitRaisedError` | `InitFailed` with bundle and message |
| 260-264 | `LuaInitFunctionMissing` | `InitFailed` with bundle name |
| 281-285 | `LuaInitRaisedError` | `InitFailed` with bundle and message |
| 308-313 | `LuaInitRaisedError` | `InitFailed` with bundle and message |
| 381-384 | `LuaInitRaisedError` | `InitFailed` with bundle and message |
| 395 | `HotReloadDisabled` | Already correct |

### JsLoader (crates/polyplug_js/src/loader.rs)

| Line | Current Error | Target |
|------|---------------|--------|
| 251-255 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 260-263 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 266-270 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 274-278 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 292-297 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 301-305 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 321-325 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 330-334 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 359-362 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 366-370 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 389-393 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 397-401 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 422-425 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 428-431 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 479-483 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 487-492 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 519-523 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 527-531 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 548-551 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 557-561 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 577-580 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 586-590 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 610-613 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 619-622 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 640-643 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 649-653 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 672-675 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 683-686 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 722-725 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 730-734 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 751-754 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 762-764 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 779-781 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 828-831 | `ManifestParse` | `InitFailed` with path and reason (or keep ManifestParse) |
| 836-839 | `JsRuntimeInitFailed` | `InitFailed` with reason |
| 843-847 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 866-869 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 875-878 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 889-893 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 898-902 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 906-911 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 917-920 | `InitSymbolMissing` (generic) | Keep |
| 942-945 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 950-954 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 959-963 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 973-977 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 985-988 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 1033-1036 | `JsRuntimePanic` | `InitFailed` with runtime and message |
| 1047-1050 | `JsRuntimePanic` | Change to `RuntimeError::HotReloadDisabled` per D-03 |

### DotnetLoader (crates/polyplug_dotnet/src/lib.rs)

| Line | Current Error | Target |
|------|---------------|--------|
| 110-112 | `ManifestMissingFile` (generic) | Keep |
| 115-118 | `AssemblyNotFound` | `InitFailed` with path |
| 124-128 | `AssemblyNotFound` | `InitFailed` with path |
| 48-51 | `InvalidFrameworkVersion` | `InitFailed` with tfm and reason |
| 54-57 | `InvalidFrameworkVersion` | `InitFailed` with tfm and reason |
| 67-70 | `InvalidFrameworkVersion` | `InitFailed` with tfm and reason |
| 73-76 | `InvalidFrameworkVersion` | `InitFailed` with tfm and reason |
| 84-87 | `RuntimeVersionMismatch` | `InitFailed` with required and found versions |
| 144-147 | `InitSymbolMissing` (generic) | Keep |
| 149-153 | `InitSymbolMissing` (generic) | Keep |
| 185-188 | `InitFailed` | Already correct |
| 199-202 | `InitFailed` | Change to `RuntimeError::HotReloadDisabled` per D-03 |

---

*Research complete. Ready for planning.*