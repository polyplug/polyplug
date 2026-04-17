---
phase: 17-refactor-contractregistry-to-unified-runtimestore
plan: 02
subsystem: registry
tags: [runtime-store, bundle-data, bundle-descriptor, dependency-resolution, o1-lookup]

# Dependency graph
requires:
  - phase: 17-01
    provides: RuntimeStore rename, PluginSlot/PluginEntry/PluginDescriptor types
provides:
  - BundleData struct with plugin_slots Vec and BundleDescriptor
  - BundleDescriptor struct with id, name, version, runtime, file_path, dependencies
  - BundleDependency struct with name and optional min_version
  - bundle_data HashMap for O(1) bundle slot lookup
  - bundle_name_index HashMap for multi-version bundle resolution
  - register_bundle_metadata() API for populating descriptors post-load
  - list_bundles(), get_bundle_descriptor(), get_bundles_by_name() introspection APIs
  - ManifestData.bundle_dependencies field with "name@version" parsing
affects: [runtime-bundle-loading, hot-reload, dependency-resolution, manifest-parsing]

# Tech tracking
tech-stack:
  added: []
  patterns: [O(1) HashMap lookup replacing O(n) scan, bundle-level dependencies replacing contract-level]

key-files:
  created:
    - crates/polyplug/src/registry/runtime_store.rs (BundleData, BundleDescriptor, BundleDependency added)
  modified:
    - crates/polyplug/src/registry/runtime_store.rs
    - crates/polyplug/src/runtime.rs
    - crates/polyplug/src/loader/manifest.rs
    - crates/polyplug/src/reload.rs (no changes needed, uses get_bundle_plugin_slots which is now O(1))

key-decisions:
  - "Kept RawManifestDependency/ManifestDependency types alongside new bundle_dependencies (used by capability_graph, validate_bundle_compatibility, host_get_dependencies FFI)"
  - "register_bundle_metadata called after loader.load() succeeds, not during plugin registration"

patterns-established:
  - "BundleData as unit of bundle storage: plugin_slots Vec + BundleDescriptor in single HashMap entry"
  - "bundle_name_index for multi-version bundle resolution by name"

requirements-completed: []

# Metrics
duration: 23min
completed: 2026-04-11
---

# Phase 17 Plan 02: BundleData, BundleDescriptor, and Bundle Introspection APIs Summary

**BundleData HashMap replaces single-slot bundle_slots_index with O(1) lookup, BundleDescriptor consolidates bundle metadata in RuntimeStore, bundle_name_index enables multi-version resolution, and ManifestData.bundle_dependencies adds bundle-level dependency parsing.**

## Performance

- **Duration:** 23 min
- **Started:** 2026-04-11T16:10:28Z
- **Completed:** 2026-04-11T16:33:36Z
- **Tasks:** 7
- **Files modified:** 10

## Accomplishments
- O(1) bundle slot lookup via bundle_data HashMap (was O(n) scan through all slots)
- BundleDescriptor consolidates all bundle metadata (id, name, version, runtime, path, deps) in RuntimeStore
- bundle_name_index enables name-to-BundleId resolution for multi-version bundles
- New introspection APIs: list_bundles(), get_bundle_descriptor(), get_bundles_by_name()
- ManifestData.bundle_dependencies field with "name@version" parsing via parsed_bundle_dependencies()
- find_guest_contract_by_bundle now iterates all bundle slots (not just first)
- Runtime calls register_bundle_metadata after successful bundle load

## Task Commits

Each task was committed atomically:

1. **Task 1: Create BundleDescriptor, BundleDependency, BundleData structs** - `925a424` (feat)
2. **Task 2+3: Replace bundle_slots_index with bundle_data HashMap** - `10ca4d5` (feat)
3. **Task 4: Add register_bundle_metadata and introspection APIs** - `fc59869` (feat)
4. **Task 5: Add bundle_dependencies field to ManifestData** - `3b3d933` (feat)
5. **Task 6: Call register_bundle_metadata after bundle load** - `51531b2` (feat)
6. **Task 7: Write tests for new RuntimeStore APIs** - `0be57d3` (test)

## Files Created/Modified
- `crates/polyplug/src/registry/runtime_store.rs` - BundleData, BundleDescriptor, BundleDependency structs; bundle_data/bundle_name_index fields; register_bundle_metadata, list_bundles, get_bundle_descriptor, get_bundles_by_name methods; O(1) get_bundle_plugin_slots; 3 new tests
- `crates/polyplug/src/runtime.rs` - register_bundle_metadata call in load_bundle_with; runtime_language_from_str helper
- `crates/polyplug/src/loader/manifest.rs` - bundle_dependencies field, parsed_bundle_dependencies() method, BundleDependency import
- `crates/polyplug/src/compatibility/mod.rs` - Updated ManifestData struct literals with bundle_dependencies field
- `crates/polyplug/tests/integration_discovery.rs` - Updated ManifestData struct literal
- `crates/polyplug_python/tests/python_loader.rs` - Updated ManifestData struct literals
- `crates/polyplug_lua/tests/lua_loader.rs` - Updated ManifestData struct literals
- `crates/polyplug_js/tests/quickjs_loader.rs` - Updated ManifestData struct literals
- `crates/polyplug_dotnet/tests/dotnet_loader.rs` - Updated ManifestData struct literal

## Decisions Made
- **Kept RawManifestDependency/ManifestDependency alongside new bundle_dependencies** -- These types are deeply integrated into capability_graph.rs, validate_bundle_compatibility(), and the host_get_dependencies FFI callback. Removing them is an architectural change requiring a dedicated plan. The new bundle_dependencies field coexists and will replace the old system in a future plan.
- **Tasks 2 and 3 merged into single commit** -- The struct field change from bundle_slots_index to bundle_data broke compilation for register_guest_contract, find_guest_contract_by_bundle, and get_bundle_plugin_slots. All three needed updating together to maintain compilability.
- **register_bundle_metadata ignores errors with let _ =** -- The metadata registration is supplementary; the core plugin loading already succeeded. Future work can propagate errors if needed.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Updated all ManifestData struct literals across test files**
- **Found during:** Task 5 (manifest.rs changes)
- **Issue:** Adding bundle_dependencies field to ManifestData broke compilation in 7+ test files that construct ManifestData directly
- **Fix:** Added `bundle_dependencies: Vec::new()` to all ManifestData struct literals across polyplug, polyplug_python, polyplug_lua, polyplug_js, and polyplug_dotnet test files
- **Files modified:** integration_discovery.rs, python_loader.rs, lua_loader.rs, quickjs_loader.rs, dotnet_loader.rs, compatibility/mod.rs
- **Committed in:** `3b3d933` (Task 5 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Necessary to maintain compilability across the workspace. No scope creep.

## Issues Encountered
- Pre-existing test failures in ffi_edge_cases (3 tests) -- these require compiled plugin binaries and fail identically before and after our changes. Logged in deferred-items.md.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- RuntimeStore now has complete bundle introspection APIs
- BundleData/bundle_name_index ready for multi-version bundle support
- Bundle-level dependency syntax ready for future dependency resolution refactoring
- RawManifestDependency/ManifestDependency removal deferred to future plan

---
*Phase: 17-refactor-contractregistry-to-unified-runtimestore*
*Completed: 2026-04-11*

## Self-Check: PASSED

All files exist: runtime_store.rs, runtime.rs, manifest.rs
All commits found: 925a424, 10ca4d5, fc59869, 3b3d933, 51531b2, 0be57d3
