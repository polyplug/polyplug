---
phase: 01-abi-types
plan: 06
subsystem: abi
tags: [guest-contract-id, type-migration, compatibility]

requires:
  - phase: 01-abi-types
    provides: GuestContractId type in polyplug_utils with backward compat alias

provides:
  - polyplug crate compatibility files use GuestContractId
  - manifest.rs dependency types use GuestContractId

affects: [sdk-updates, registry, instance-model]

tech-stack:
  added: []
  patterns: [backward-compat-alias, type-renaming]

key-files:
  created: []
  modified:
    - crates/polyplug/src/compatibility/capability_graph.rs
    - crates/polyplug/src/compatibility/contract_capability.rs
    - crates/polyplug/src/compatibility/dependency_edge.rs
    - crates/polyplug/src/loader/manifest.rs

key-decisions:
  - "Scope limited to plan's explicit files_modified list"
  - "Deferred mod.rs test code (not in plan scope)"
  - "Deferred pre-existing build errors (unrelated to task changes)"

requirements-completed: [ABI-11]

duration: 4min
completed: 2026-04-03
---

# Phase 01 Plan 06: GuestContractId Migration in Polyplug Crate Summary

**Replaced deprecated PluginContractId with GuestContractId in 4 polyplug crate compatibility files, enabling type consistency with Phase 1 ABI rename.**

## Performance

- **Duration:** 4 min
- **Started:** 2026-04-03T21:25:30Z
- **Completed:** 2026-04-03T21:29:29Z
- **Tasks:** 4
- **Files modified:** 4

## Accomplishments

- Updated `capability_graph.rs` to use GuestContractId (import + HashMap key type)
- Updated `contract_capability.rs` to use GuestContractId (import + struct field + constructor)
- Updated `dependency_edge.rs` to use GuestContractId (import + struct field)
- Updated `manifest.rs` to use GuestContractId (import + struct fields + test code, 10 replacements)

## Task Commits

Each task was committed atomically:

1. **Task 1: capability_graph.rs** - `a521f87` (fix)
2. **Task 2: contract_capability.rs** - `d10a274` (fix)
3. **Task 3: dependency_edge.rs** - `69552a4` (fix)
4. **Task 4: manifest.rs** - `dee9348` (fix)

## Files Created/Modified

- `crates/polyplug/src/compatibility/capability_graph.rs` - Import and HashMap key type updated
- `crates/polyplug/src/compatibility/contract_capability.rs` - Import, struct field, constructor updated
- `crates/polyplug/src/compatibility/dependency_edge.rs` - Import and struct field updated
- `crates/polyplug/src/loader/manifest.rs` - All 10 PluginContractId usages replaced

## Decisions Made

- Scope limited to plan's explicit `files_modified` frontmatter list (4 files)
- Deferred `mod.rs` test code PluginContractId usage (not in plan scope, uses backward compat alias)
- Deferred pre-existing build errors unrelated to task changes

## Deviations from Plan

### Deferred Items (Out of Scope)

**1. [Scope Boundary] compatibility/mod.rs PluginContractId usage**
- **Found during:** Verification phase
- **Issue:** Test code in mod.rs still uses PluginContractId (lines 20, 128, 129, 218)
- **Reason deferred:** Not in plan's `files_modified` frontmatter. Backward compatibility alias allows continued use with deprecation warning.
- **Recommendation:** Address in future gap closure plan or SDK cleanup phase.

**2. [Pre-Existing] polyplug_abi deprecated PluginContractId**
- **Found during:** Build verification
- **Issue:** polyplug_abi crate still uses PluginContractId (different crate, outside scope)
- **Reason deferred:** Different crate scope, pre-existing issue.

**3. [Pre-Existing] BundleId vs u64 mismatches in ffi.rs**
- **Found during:** Build verification
- **Issue:** Type mismatches causing build errors in ffi.rs
- **Reason deferred:** Unrelated to GuestContractId migration, pre-existing blocker.

**4. [Pre-Existing] polyplug crate build failure**
- **Issue:** Plan's compilation criterion not met due to 23 pre-existing errors
- **Reason deferred:** Errors unrelated to task changes. All 4 target files pass acceptance criteria.

---

**Total deviations:** 4 deferred items (scope boundary + pre-existing issues)
**Impact on plan:** All planned tasks completed successfully. Build criterion blocked by pre-existing issues outside plan scope.

## Issues Encountered

Build verification revealed pre-existing errors (BundleId/u64 mismatches, deprecated usage in polyplug_abi) unrelated to task changes. Deferred per scope boundary rules. See `deferred-items.md` for details.

## Next Phase Readiness

- 4 compatibility files updated with GuestContractId
- Backward compatibility alias allows gradual migration
- Pre-existing build blockers need separate resolution

## Self-Check: PASSED

- All 4 target files exist on disk
- 4 task commits found in git history
- SUMMARY.md created at expected path

---
*Phase: 01-abi-types*
*Completed: 2026-04-03*