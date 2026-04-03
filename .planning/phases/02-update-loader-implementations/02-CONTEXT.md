# Phase 2: Update Loader Implementations - Context

**Gathered:** 2026-04-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Update all loader implementations to use `LoaderError::InitFailed` directly with descriptive string messages. No intermediate error types needed. The `BundleLoader` trait requires `load()` and `reload()` to return `Result<(), RuntimeError>`.

**What this phase delivers:**
- All loaders use `LoaderError::InitFailed { bundle, error }` directly at error sites
- `NativeLoader` updated to match inline pattern (remove `load_internal()`)
- Local error types from Phase 1 (`PythonLoaderError`, `LuaLoaderError`, `JsLoaderError`, `DotnetLoaderError`) removed
- Consistent hot-reload error handling across all loaders

**What this phase does NOT include:**
- Test verification (Phase 3)
- Changes to `BundleLoader` trait signature

</domain>

<decisions>
## Implementation Decisions

### Error Handling Strategy
- **D-01:** Use `LoaderError::InitFailed` directly with string messages — no local error types
- **Rationale:** Simpler approach; no need for intermediate error enums; string messages are sufficient for diagnostics

### Internal Error Handling
- **D-02:** Keep error handling inline for all loaders, including `NativeLoader`
  - Remove `NativeLoader::load_internal()` method
  - Each loader constructs `LoaderError::InitFailed` directly at error sites
- **Rationale:** Consistency and simplicity

### Hot-Reload Error Handling
- **D-03:** All loaders return `RuntimeError::HotReloadDisabled` for unsupported hot-reload
- **Rationale:** Consistency — this is a runtime configuration issue

### Cleanup from Phase 1
- **D-04:** Remove unused local error types:
  - `crates/polyplug_python/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
  - `crates/polyplug_lua/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
  - `crates/polyplug_js/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
  - `crates/polyplug_dotnet/src/error.rs` — delete file, remove `pub mod error;` from lib.rs
- **Note:** `NativeLoaderError` in `polyplug_native` may also be removed if not needed

### Claude's Discretion
- Exact error message strings — make them descriptive and include relevant context
- Order of migration (which loader first) — any order works

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Trait Definition (Immutable)
- `crates/polyplug/src/loader/bundle_loader.rs` — `BundleLoader` trait with `load()` and `reload()` returning `Result<(), RuntimeError>`

### Core Error Types
- `crates/polyplug/src/error.rs` — Core `LoaderError` with `InitFailed { bundle: String, error: String }` variant

### Loader Implementations (Targets for Update)
- `crates/polyplug_native/src/loader.rs` — `NativeLoader` (remove `load_internal()`, remove `NativeLoaderError`)
- `crates/polyplug_python/src/lib.rs` — `PythonLoader` (use `InitFailed` directly)
- `crates/polyplug_lua/src/loader.rs` — `LuaLoader` (use `InitFailed` directly)
- `crates/polyplug_js/src/loader.rs` — `JsLoader` (use `InitFailed` directly)
- `crates/polyplug_dotnet/src/lib.rs` — `DotnetLoader` (use `InitFailed` directly)

### Files to Delete (Phase 1 artifacts no longer needed)
- `crates/polyplug_python/src/error.rs`
- `crates/polyplug_lua/src/error.rs`
- `crates/polyplug_js/src/error.rs`
- `crates/polyplug_dotnet/src/error.rs`
- `crates/polyplug_native/src/error.rs` (if `NativeLoaderError` removed)

</canonical_refs>

<code_context>
## Existing Code Insights

### Pattern to Follow
```rust
// At each error site in load()/reload():
Err(RuntimeError::Loader(LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("descriptive error message: {}", details),
}))
```

### Generic Errors (Use Directly from Core)
- `LoaderError::ManifestMissingFile { bundle }`
- `LoaderError::InitSymbolMissing { bundle }`
- `LoaderError::BundleTampered { bundle, expected, found }`

### Hot-Reload Unsupported
```rust
fn reload(&self, _manifest: &ManifestData, _runtime: &Runtime) -> Result<(), RuntimeError> {
    Err(RuntimeError::HotReloadDisabled)
}
```

</code_context>

<specifics>
## Specific Ideas

**Migration order (suggested):**
1. Delete unused error.rs files from Phase 1
2. Update `NativeLoader` to remove `load_internal()` and `NativeLoaderError`
3. Update `PythonLoader` — replace all error sites with `InitFailed` directly
4. Update `LuaLoader` — replace all error sites with `InitFailed` directly
5. Update `JsLoader` — replace all error sites with `InitFailed` directly
6. Update `DotnetLoader` — replace all error sites with `InitFailed` directly

**Per-loader changes:**
- Remove import of local error type
- At each error site, construct `LoaderError::InitFailed { bundle, error }` with descriptive string
- Generic errors (`ManifestMissingFile`, `InitSymbolMissing`) used directly from core

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---

*Phase: 02-update-loader-implementations*
*Context gathered: 2026-04-03*