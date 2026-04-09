# Session Summary: Native Decoupling Refactoring

## Goal

Remove native coupling from the polyplug core crate and move it to polyplug_native crate, making the core runtime loader-agnostic. The plan is defined in `REFACTORING_PLAN_NATIVE_DECOUPLING.md` with 6 phases.

## Instructions

- Follow the refactoring plan from REFACTORING_PLAN_NATIVE_DECOUPLING.md
- **Do NOT compile** - the codebase has 200+ pre-existing errors from a WIP commit. Just stick to the plan.
- The plan has 6 phases to execute in order

## Discoveries

- The codebase has pre-existing compilation errors from a WIP refactoring commit (commit `3c156e5 refactor(polyplug): WIP restructure crate modules and update imports`)
- polyplug_abi was restructured - types are now in submodules (e.g., `polyplug_abi::host::host_vtable::HostInterface`, `polyplug_abi::plugin::GuestContractHandle`)
- VTableSlot was missing from plugin_registry.rs - added it
- RuntimeError was missing from error.rs - added as type alias for RuntimeError
- HostContext needed to be defined in runtime.rs (the ABI version in polyplug_abi has different fields)
- The user explicitly chose to continue without fixing pre-existing errors
- `validate_file()` method was missing from ManifestData - added it
- `parse_from_str()` method was missing from ManifestData - added it
- `parse_manifest()` function was missing - added it
- `scanner` module was missing - created it

## Accomplished

**Phase 1 (COMPLETED):** Updated BundleLoader trait to add `reload()` method
- Added reload() to `loader/bundle_loader.rs`
- Added reload() to all implementations: polyplug_lua, polyplug_python, polyplug_js, polyplug_dotnet, polyplug_native, and test loaders in runtime.rs

**Phase 2 (COMPLETED):** Rewrote reload.rs as generic reload framework
- Contains ReloadPhase enum, ReloadEvent struct, wait_for_quiescence() utility
- Added reload_bundle() and refresh_handle() methods to Runtime via impl block
- Uses BundleId from polyplug_utils

**Phase 3 (COMPLETED):** Created NativeLoader in polyplug_native
- Created `error.rs` with NativeLoaderError enum
- Rewrote `loader.rs` with full implementation (load_internal, load, reload)
- Owns library handles in HashMap<BundleId, libloading::Library>
- Updated lib.rs exports
- Added polyplug_utils and thiserror dependencies to Cargo.toml

**Phase 4 (COMPLETED):** Removing native coupling from core
- Rewrote `loader/mod.rs` - removed NativeBundleLoader struct and load_bundle() function
- Updated `loader/loaded_bundle.rs` - removed library field
- Removed `loaded_libraries` field and `push_library()` from plugin_registry.rs
- Added VTableSlot struct to plugin_registry.rs
- Removed from Runtime: `reload_libraries`, `watcher_thread`, `watcher_stop`, `reload_captured_vtables` fields
- Removed `watch_plugin_dir()` method and Drop impl from Runtime
- Removed auto-registration of native loader from runtime_builder.rs
- Made `loaders` field pub(crate) in Runtime
- Removed libloading dependency from core crate
- Removed notify dependency from core crate
- Removed native-specific error variants from error.rs (LoadFailed, AbiVersionMismatch, MissingSymbol)
- Removed tests for deleted error variants

**Phase 5 (COMPLETED):** Require explicit runtime in manifest
- Removed `default_runtime()` function from manifest.rs
- Changed `runtime` field to use `#[serde(skip_serializing_if = "String::is_empty")]`
- Added `validate()` method to ManifestData
- Added `validate_file()` method to ManifestData
- Added `parse_from_str()` method to ManifestData
- Added `parse_manifest()` function to manifest module
- Created `scanner.rs` module for bundle discovery

**Phase 6 (IN PROGRESS):** Use newtype IDs (BundleId, PluginContractId)
- BundleId and PluginContractId are already being used in most places
- The registry uses newtype IDs for bundle_index, contract_index, declared_deps
- ManifestData uses PluginContractId for contract_id in dependencies
- Remaining: Some function signatures still use raw u64 for bundle_id parameters

## Relevant files / directories

**Modified files:**
- `crates/polyplug/src/loader/bundle_loader.rs` - added reload() method to trait
- `crates/polyplug/src/reload.rs` - complete rewrite as generic framework
- `crates/polyplug/src/runtime.rs` - removed native-specific fields, added HostContext struct, added on_reload_cb() accessor, removed watch_plugin_dir and Drop impl
- `crates/polyplug/src/runtime_builder.rs` - removed auto-registration of native loader, removed watcher fields
- `crates/polyplug/src/loader/mod.rs` - removed NativeBundleLoader and load_bundle(), added scanner module
- `crates/polyplug/src/loader/loaded_bundle.rs` - removed library field
- `crates/polyplug/src/loader/manifest.rs` - added validate(), validate_file(), parse_from_str(), parse_manifest(); removed default_runtime()
- `crates/polyplug/src/loader/scanner.rs` - NEW FILE for bundle discovery
- `crates/polyplug/src/registry/plugin_registry.rs` - removed loaded_libraries, push_library(), added VTableSlot struct
- `crates/polyplug/src/error.rs` - added RuntimeError type alias, removed native-specific error variants
- `crates/polyplug/Cargo.toml` - removed libloading and notify dependencies
- `crates/polyplug_native/src/lib.rs` - updated exports (added error module)
- `crates/polyplug_native/src/error.rs` - NEW FILE with NativeLoaderError
- `crates/polyplug_native/src/loader.rs` - complete rewrite with NativeLoader implementation
- `crates/polyplug_native/Cargo.toml` - added polyplug_utils and thiserror dependencies

**Other loader implementations (updated to add reload()):**
- `crates/polyplug_lua/src/loader.rs`
- `crates/polyplug_python/src/lib.rs`
- `crates/polyplug_js/src/loader.rs`
- `crates/polyplug_dotnet/src/lib.rs`
- `crates/polyplug/tests/integration_version.rs`

**Plan file:**
- `REFACTORING_PLAN_NATIVE_DECOUPLING.md` - contains all 6 phases with detailed instructions

## Next Steps

Phase 6 is mostly complete - the newtype IDs are already in use. The remaining work would be to:
1. Update any remaining function signatures that use raw u64 instead of BundleId
2. Run tests to verify everything works (when the codebase compiles)