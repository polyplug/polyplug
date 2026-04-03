# Deferred Items — Plan 01-04

## Pre-existing Compilation Errors (Out of Scope)

The `cargo check -p polyplug_dotnet` command failed due to pre-existing errors in the `polyplug` core crate, not from changes made in this plan. These errors existed before this plan execution (visible in git status as modified files at session start).

### Errors in `crates/polyplug/src/` (polyplug core crate)

1. **Unresolved imports in runtime_builder.rs:**
   - `CapabilityGraph` not found (help: `capability_graph` exists but inaccessible)
   - `ReloadCb` not found (type alias exists but private)

2. **Missing types in ffi.rs:**
   - `StringViewC` type not found
   - `RuntimeConfigC` type not found
   - `HostContractVTable` not found in `polyplug_abi`

3. **Private module access errors:**
   - `plugin_registry` module is private (multiple files)
   - `manifest` module is private (multiple files)

4. **Type mismatches in ffi.rs:**
   - `BundleId` vs `u64` mismatch

These errors are in the core `polyplug` crate and are unrelated to the `DotnetLoaderError` type creation in this plan. They appear to be from an in-progress refactoring (native decoupling) that is not yet complete.

### Action Taken

Proceeded with plan execution. Documented in SUMMARY.md that cargo check failed due to pre-existing issues.