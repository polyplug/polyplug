---
phase: 06-cleanup
plan: 03
subsystem: documentation
tags: [documentation, terminology, guest-host, naming]
dependency_graph:
  requires: [06-01]
  provides: [updated-documentation]
  affects: [docs/]
tech_stack:
  added: []
  patterns: [GuestContractInterface, RuntimeAbi, HostContractInterface]
key_files:
  created: []
  modified:
    - docs/ABI_ARCHITECTURE.md
    - docs/ARCHITECTURE_CLARIFICATIONS.md
    - docs/HOST_CONTRACTS.md
    - docs/HOST_CONTRACTS_API.md
    - docs/HOT_RELOAD_DESIGN.md
    - docs/PERFORMANCE.md
    - docs/PLUGIN_INTERFACE_DESIGN.md
    - docs/abi_types.md
decisions:
  - date: 2026-04-04
    decision: "Add terminology notes to all docs explaining v1.1 rename"
    rationale: "Helps readers understand the transition from old naming"
  - date: 2026-04-04
    decision: "Update HOT_RELOAD_DESIGN.md to callback-based model"
    rationale: "Phase 4 removed quiescence waiting; documentation must reflect callback coordination"
  - date: 2026-04-04
    decision: "Remove VTableSlot references from documentation"
    rationale: "VTableSlot wrapper was removed in instance model refactor"
metrics:
  duration: ~25 minutes
  tasks_completed: 6
  files_modified: 8
  lines_changed: 557 (+307/-250)
---

# Phase 06 Plan 03: Update Documentation to Guest/Host Terminology Summary

**One-liner:** Updated all documentation files to use GuestContractInterface/RuntimeAbi terminology, replacing old GuestContractInterface/HostInterface naming throughout.

## Completed Tasks

| Task | Name | Files Modified |
| ---- | ---- | -------------- |
| 1 | Update ABI_ARCHITECTURE.md | docs/ABI_ARCHITECTURE.md |
| 2 | Update HOT_RELOAD_DESIGN.md | docs/HOT_RELOAD_DESIGN.md |
| 3 | Update HOST_CONTRACTS.md and HOST_CONTRACTS_API.md | docs/HOST_CONTRACTS.md, docs/HOST_CONTRACTS_API.md |
| 4 | Update PERFORMANCE.md | docs/PERFORMANCE.md |
| 5 | Update PLUGIN_INTERFACE_DESIGN.md | docs/PLUGIN_INTERFACE_DESIGN.md |
| 6 | Update abi_types.md and ARCHITECTURE_CLARIFICATIONS.md | docs/abi_types.md, docs/ARCHITECTURE_CLARIFICATIONS.md |

## Key Changes

### Terminology Updates

| Old Term | New Term | Context |
|----------|----------|---------|
| GuestContractInterface | GuestContractInterface | Contract implemented by plugins |
| HostInterface | RuntimeAbi | Runtime's ABI provided to guests |
| vtable | interface | When referring to contract interfaces |
| vtable dispatch | contract dispatch | Dispatch mechanism terminology |
| HostContractVTable | HostContractInterface | Host-provided services |

### Architecture Documentation Updates

1. **HOT_RELOAD_DESIGN.md**: Updated from Arc-based reference counting model to callback-based coordination model, reflecting Phase 4 changes that removed quiescence waiting.

2. **ARCHITECTURE_CLARIFICATIONS.md**: Removed VTableSlot references and updated to reflect that interfaces are now stored directly in the registry.

3. **PLUGIN_INTERFACE_DESIGN.md**: Renamed concept to GuestContractInterface design rationale, updated all code examples.

### Code Example Updates

All code examples in documentation now use:
- `create_logger_interface()` instead of `create_logger_vtable()`
- `get_runtime_abi()` instead of `get_host_vtable()`
- `runtime_abi` instead of `host_vtable`
- `interface` instead of `vtable`

## Deviations from Plan

None - plan executed exactly as written.

## Verification Results

```bash
# Verified no old terminology in current descriptions
grep -r "GuestContractInterface|HostInterface" docs/ --include="*.md"
# Only matches in terminology notes explaining the rename

# Verified new terminology is present
grep -rE "GuestContractInterface|RuntimeAbi" docs/ --include="*.md"
# Multiple matches showing new terminology in use
```

## Self-Check: PASSED

- [x] All 8 documentation files exist and are modified
- [x] Commit c44c989 exists with correct message format
- [x] Terminology notes added to all files
- [x] No GuestContractInterface/HostInterface in current descriptions (only in rename notes)
- [x] New terminology used throughout

## Known Stubs

None - this plan updates existing documentation without adding stubs.

## Threat Flags

None - documentation-only changes, no security surface introduced.