---
phase: 13-cpp-codegen-modernization
plan: 01
type: execute
wave: 1
depends_on: []
tags: [codegen, cpp, naming, interface]
requirements: [CG-05]
---

# Phase 13 Plan 01: Rename vtable terminology to interface in C++ codegen

**One-liner:** Renamed all HostContractVTable/_VTABLE terminology in C++ codegen to HostContractInterface/_INTERFACE with inline struct fields and full instance support.

## Summary

This plan modernized the C++ codegen terminology to align with the renamed ABI types from Phase 1. The generated C++ code now uses:

- `HostContractInterface` instead of `HostContractVTable`
- `_INTERFACE` suffix instead of `_VTABLE` for static declarations
- `interface_` member instead of `vtable_` in RAII wrappers
- Inline `HostContractInterface` fields in factory functions (no `HostContractVTableHeader` wrapper)

Additionally, the checkpoint decision (implement-instance) resulted in implementing full host contract instance support in the guest host contract caller, storing both `interface_` and `instance_` members and passing `instance_` to dispatch calls.

## Tasks Completed

| Task | Name | Commit | Files Modified |
|------|------|--------|----------------|
| 1 | Rename guest-side static declarations from _VTABLE to _INTERFACE | c2e340e | cpp.rs (lines 361, 446, 726, 785) |
| 2 | Rename HostContractVTable to HostContractInterface in guest host contract caller | 0793fe9 | cpp.rs (lines 1523-1562, 1625-1637) |
| 3 | Implement host contract instance support (checkpoint: implement-instance) | 1e3dc28 | cpp.rs (lines 1543-1562, 1631-1637) |
| 4 | Update factory functions to use inline HostContractInterface fields | f1c46bb | cpp.rs (lines 1885-2027) |
| 5 | Update unit test assertions for new naming | 7fa7f07 | cpp.rs (test assertions) |

## Decisions Made

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Host contract instance support | implement-instance | Completes INST-04 for all paths, makes generated code fully functional without interim TODO/placeholders. User selected this over defer-instance to ensure complete implementation. |

## Key Changes

### Static Declarations (Task 1)

Changed all guest-side static declarations from `_VTABLE` to `_INTERFACE` suffix:
- `static GuestContractInterface PLUGIN_INTERFACE = {...}`
- `static GuestContractInterface CONTRACT_INTERFACE = {...}`
- Register_contract calls reference `_INTERFACE` not `_VTABLE`

### Guest Host Contract Caller (Tasks 2-3)

Renamed member variable and implemented instance support:
```cpp
// Before:
const HostContractVTable* vtable_;

// After:
const HostContractInterface* interface_;
HostContractInstance instance_;
```

Dispatch calls now pass `instance_` to both native and VM dispatch:
```cpp
// Native dispatch:
err = fn_(instance_, args_ptr, out_ptr);

// VM dispatch:
err = (interface_->dispatch.vm.call)(interface_->dispatch.vm.loader_data, instance_, fn_id, args_ptr, out_ptr);
```

### Factory Functions (Task 4)

Factory functions now emit inline `HostContractInterface` fields matching ABI layout:
- `contract_id` (u64)
- `contract_version` (Version)
- `singleton` (bool)
- `dispatch_type` (DispatchType)
- `runtime` (nullptr, set by runtime during registration)
- `create_instance` stub function
- `destroy_instance` stub function
- `dispatch` union (NativeDispatch or VmDispatch)

No `HostContractVTableHeader` wrapper struct in generated code.

### Test Assertions (Task 5)

Updated test assertions to match new naming:
- `out.contains("const HostContractInterface* interface_")`
- `out.contains("HostContractInstance instance_")`
- `out.contains("create_instance_stub")`
- `out.contains("destroy_instance_stub")`

## Verification Results

| Check | Result | Evidence |
|-------|--------|----------|
| cargo test -p polyplugc --lib | PASSED | 182 tests passed |
| grep -n "HostContractVTable" cpp.rs | 0 matches | No legacy naming remains |
| grep -n "_VTABLE" cpp.rs | 0 matches | All static declarations use _INTERFACE |
| grep -n "interface_->dispatch_type" cpp.rs | 1 match (line 1625) | Direct interface field access |
| grep -n "HostContractInstance instance_" cpp.rs | 2 matches (lines 1156, 1562) | Instance support in both guest and host contract callers |

## Deviations from Plan

### Auto-fixed Issues

None - plan executed as written with checkpoint resolved via user decision.

### Architectural Decision

The plan's checkpoint at Task 3 was resolved with user selecting "implement-instance", which expanded scope beyond naming-only changes to also implement host contract instance support. This was documented in the decision and resulted in additional changes:
- Added `HostContractInstance instance_` member to guest host contract caller
- Updated constructor to receive instance from `get_host_contract`
- Updated dispatch calls to pass `instance_` parameter

This satisfies INST-04 for the host contract caller path (guest calling host contracts).

## Files Modified

| File | Changes |
|------|---------|
| crates/polyplugc/src/generators/cpp.rs | Renamed vtable→interface terminology, added instance support, updated factory functions |

## Metrics

- **Duration:** ~30 minutes (5 commits)
- **Commits:** 5
- **Files Modified:** 1
- **Tests:** 182 passed

## Requirements Addressed

| Requirement | Status | Evidence |
|-------------|--------|----------|
| CG-05 | SATISFIED | Factory functions emit inline HostContractInterface fields with create_instance/destroy_instance stubs |

## Self-Check: PASSED

- [x] All 5 commits exist in git log
- [x] cpp.rs contains `_INTERFACE` (4 matches)
- [x] cpp.rs contains `interface_` member (lines 1561, 2791)
- [x] cpp.rs contains `instance_` member (lines 1156, 1562)
- [x] cpp.rs does NOT contain `HostContractVTable` or `_VTABLE`
- [x] cargo test passes (182 tests)

---
*Summary created: 2026-04-08*