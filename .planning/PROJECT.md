# polyplug

## What This Is

A high-performance, zero/minimal-overhead cross-language plugin runtime for Rust. Enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

## Core Value

The core runtime is loader-agnostic — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

## Requirements

### Validated

- ✓ Native loader in separate crate (`polyplug_native`) — decoupled from core
- ✓ `libloading` dependency removed from core `Cargo.toml`
- ✓ No auto-registration of native loader — users must register explicitly
- ✓ `NativeLoader` owns library handles internally — not stored in registry
- ✓ Generic `reload.rs` with `wait_for_quiescence()` utility — all loaders use it
- ✓ `BundleLoader.reload()` method — mandatory for all loaders
- ✓ Python/Lua/JS/.NET loaders already properly decoupled — each owns its VM state
- ✓ Loader-local error types defined (Phase 01) — `PythonLoaderError`, `LuaLoaderError`, `JsLoaderError`, `DotnetLoaderError`
- ✓ Core `LoaderError` stripped of loader-specific variants — only generic Loader variants remain
- ✓ All loaders use `LoaderError::InitFailed` directly (Phase 02) — language-specific error types removed
- ✓ Hot-reload returns `RuntimeError::HotReloadDisabled` consistently across all loaders

### Active

- [ ] Verify compatibility and cross-phase integration (Phase 03)

### Out of Scope

- WASM runtime support — deliberate exclusion, native plugin system is the design
- Plugin sandboxing/security boundaries — host is responsible for trust

## Context

**Active Work:** Phase 03 (verify-compatibility) — ensure cross-phase integration and build health.

**Completed Phases:**
1. ✅ Update `BundleLoader` trait (add reload method)
2. ✅ Create generic reload framework in core
3. ✅ Create `NativeLoader` in `polyplug_native`
4. ✅ Remove native coupling from core
5. ✅ Require explicit `runtime` in manifest
6. ✅ Use newtype IDs (`BundleId`, `PluginContractId`)
7. ✅ Define loader-local error types — each loader has its own error enum
8. ✅ Strip loader-specific variants from core `LoaderError`
9. ✅ Update loader implementations — unified `InitFailed` pattern, removed language-specific error types

**Remaining Work:** Verify compatibility and cross-phase integration.

## Constraints

- **Architecture:** Core crate must have zero loader-specific code or dependencies
- **Safety:** Hot-reload safety contract — hosts must not cache raw function pointers
- **Compatibility:** Breaking changes acceptable — not published yet

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| No WASM runtime | Native shared libraries give zero-overhead dispatch; WASM would add interpretation layer | ✓ Correct |
| Mandatory `reload()` in trait | Every loader must support hot-reload, not just native | ✓ Implemented |
| Library handles owned by loader | Registry only stores vtable pointers; loaders own their resources | ✓ Implemented |
| Fail-fast on stale pointers | If host caches raw pointers after reload, SIGSEGV is a host bug | ✓ Documented in safety contract |

---
*Last updated: 2026-04-03 after Phase 02 completion (loader error unification)*