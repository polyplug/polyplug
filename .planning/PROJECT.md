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

### Active

- [ ] Move loader-specific error variants from core `LoaderError` to respective crates
  - Python: `PythonInitFailed`, `PythonModuleImportFailed`, `PythonInitRaisedException`
  - Lua: `LuaVmInitFailed`, `LuaScriptLoadFailed`, `LuaInitFunctionMissing`, `LuaInitRaisedError`
  - JS: `RolldownNotFound`, `JsRuntimePanic`, `JsRuntimeInitFailed`, `ModuleResolutionFailed`, `JsExecutionFailed`
  - .NET: `HostfxrNotFound`, `ClrInitFailed`, `AssemblyNotFound`, `RuntimeVersionMismatch`, `InvalidFrameworkVersion`

### Out of Scope

- WASM runtime support — deliberate exclusion, native plugin system is the design
- Plugin sandboxing/security boundaries — host is responsible for trust

## Context

**Active Refactoring:** 28 modified files from native decoupling work. The `REFACTORING_PLAN_NATIVE_DECOUPLING.md` document defined 6 phases:

1. ✅ Update `BundleLoader` trait (add reload method)
2. ✅ Create generic reload framework in core
3. ✅ Create `NativeLoader` in `polyplug_native`
4. ✅ Remove native coupling from core
5. ✅ Require explicit `runtime` in manifest
6. ✅ Use newtype IDs (`BundleId`, `PluginContractId`)

**Remaining Work:** Error type decoupling (Phase 4.6 was partially complete). Core `LoaderError` still contains loader-specific variants that should live in their respective loader crates.

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
*Last updated: 2026-04-03 after codebase map and refactoring status review*