# Phase 3: Verify Compatibility - Context

**Gathered:** 2026-04-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Verify that error handling refactoring from Phases 1 and 2 hasn't broken functionality. Tests pass, FFI boundary remains compatible with string-based error messages.

**What this phase delivers:**
- Verification that loader crate tests pass
- Verification that integration tests pass
- Explicit check that FFI error messages remain strings
- Documentation of known build issues (core crate WIP)

**What this phase does NOT include:**
- Fixing pre-existing build errors in core polyplug (WIP refactoring)
- Full workspace build verification
- SDK or example compilation

</domain>

<decisions>
## Implementation Decisions

### Build Scope
- **D-01:** Verify loader crates only — core polyplug has ongoing WIP refactoring and won't compile for a while
- **Rationale:** Core crate build issues are unrelated to error handling changes; loader verification is sufficient for this phase

### Test Scope
- **D-02:** Run loader tests + integration tests
  - `cargo test -p polyplug_python`
  - `cargo test -p polyplug_lua`
  - `cargo test -p polyplug_js`
  - `cargo test -p polyplug_dotnet`
  - Integration tests in `tests/integration/tests/` that test loaders
- **Rationale:** Focused verification on error handling changes without blocking on core WIP issues

### FFI Verification
- **D-03:** Explicit check of error string format at FFI boundary
  - Verify error messages are strings at the FFI boundary
  - Check that `LoaderError::InitFailed` produces string messages
  - No breaking changes to public FFI API
- **Rationale:** User wants explicit verification, not just trusting tests

### Known Issues
- **D-04:** Document that core polyplug crate has pre-existing build errors from WIP refactoring (commit `3c156e5`)
  - These are out of scope for this phase
  - Do not block verification on fixing these

### Claude's Discretion
- Exact test commands and flags
- How to present verification results
- Whether to skip specific failing tests if they're unrelated to error handling

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements
- `.planning/REQUIREMENTS.md` — COMP-01 (tests pass), COMP-02 (FFI compatibility)

### Test Files (Verification Targets)
- `crates/polyplug_python/tests/python_loader.rs`
- `crates/polyplug_lua/tests/lua_loader.rs`
- `crates/polyplug_js/tests/quickjs_loader.rs`
- `crates/polyplug_dotnet/tests/dotnet_loader.rs`
- `tests/integration/tests/integration_python.rs`
- `tests/integration/tests/integration_lua.rs`
- `tests/integration/tests/integration_js.rs`
- `tests/integration/tests/integration_dotnet.rs`
- `tests/integration/tests/cross_language.rs`

### Error Types (FFI Boundary)
- `crates/polyplug/src/error.rs` — Core `LoaderError::InitFailed { bundle: String, error: String }`
- `crates/polyplug_abi/src/` — FFI types (StringView, AbiError)

### Testing Patterns
- `.planning/codebase/TESTING.md` — Test patterns and commands

</canonical_refs>

<code_context>
## Existing Code Insights

### Test Structure
- Loader tests: `crates/polyplug_*/tests/*_loader.rs`
- Integration tests: `tests/integration/tests/integration_*.rs`
- Tests use `#[test]` attribute with built-in assertions

### FFI Error Pattern
```rust
// LoaderError::InitFailed produces string messages
LoaderError::InitFailed {
    bundle: manifest.name.clone(),
    error: format!("descriptive error: {}", details),
}
```

### Known Build Issues
- Core polyplug crate has WIP refactoring issues (commit `3c156e5`)
- Files affected: `crates/polyplug/src/ffi.rs`, `crates/polyplug/src/plugin_registry.rs`
- These issues do not affect loader crate compilation

</code_context>

<specifics>
## Specific Ideas

**Verification sequence:**
1. Build loader crates: `cargo build -p polyplug_python -p polyplug_lua -p polyplug_js -p polyplug_dotnet`
2. Run loader tests: `cargo test -p polyplug_python -p polyplug_lua -p polyplug_js -p polyplug_dotnet`
3. Run integration tests for loaders
4. Explicit FFI check: Verify error messages are strings at boundary

**FFI verification approach:**
- Check that `LoaderError::InitFailed` converts to string for FFI
- Verify `AbiError.message` is a `StringView` (string at boundary)
- No breaking changes to FFI function signatures

</specifics>

<deferred>
## Deferred Ideas

- Full workspace build verification (blocked by core WIP refactoring)
- SDK compilation verification
- Example host compilation verification

</deferred>

---

*Phase: 03-verify-compatibility*
*Context gathered: 2026-04-03*