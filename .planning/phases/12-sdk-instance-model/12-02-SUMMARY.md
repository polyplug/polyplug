---
phase: 12-sdk-instance-model
plan: 02
subsystem: SDK
tags: [typescript, abi, naming, types]
requires: []
provides: [SDK-05]
affects: [sdks/js/abi/polyplug_abi.ts]
tech_stack:
  added: []
  patterns: [TypeScript interface naming alignment, ABI type coverage]
key_files:
  created: []
  modified: [sdks/js/abi/polyplug_abi.ts]
decisions:
  - TypeScript ABI types use GuestContractInterface naming (not PluginInterface)
  - TypeScript ABI types use HostInterface naming (not HostVTable)
  - Added RuntimeInterface, GuestContractInstance, HostContractInstance interfaces
metrics:
  duration: "6m"
  tasks: 4
  files: 1
  completed_date: "2026-04-08"
---

# Phase 12 Plan 02: JS SDK ABI Type Naming Update Summary

Aligned JS SDK TypeScript types with current polyplug_abi naming conventions, satisfying SDK-05.

## One-Liner

Updated TypeScript ABI types from legacy naming (PluginInterface, HostVTable) to current conventions (GuestContractInterface, HostInterface), adding complete interface coverage for instance model.

## Tasks Completed

| Task | Name | Commit | Status |
|------|------|--------|--------|
| 1 | Rename PluginInterface to GuestContractInterface | e2813cc | Complete |
| 2 | Rename HostVTable to HostInterface | e2813cc | Complete |
| 3 | Add RuntimeInterface and instance interfaces | e2813cc | Complete |
| 4 | Verify TypeScript compilation | (checkpoint approved) | Verified |

## Key Changes

### Interface Naming Updates

- `PluginInterface` → `GuestContractInterface` (line 230)
- `HostVTable` → `HostInterface` (line 265)
- `PluginDispatch` → `GuestContractDispatch` (line 192)

### New Interfaces Added

- `GuestContractInstance` (line 106): `data: bigint`, `contract_id: bigint`
- `HostContractInstance` (line 119): `data: bigint`
- `RuntimeInterface` (line 301): 13 function fields for host-side runtime control
- `DependencyInfo` (line 331): `bundle_id`, `contract_id`, `min_version`

### HostInterface Fields Updated

- `register_contract` (line 269): renamed from `register_plugin`
- `find_by_contract` (line 275): contract lookup function
- `find_all_by_contract` (line 277): batch contract lookup
- `resolve_contract` (line 279): handle-to-interface resolution
- `call_guest_method` (line 281): cross-dispatch method calling
- `get_host_contract` (line 283): host contract instance retrieval
- `list_bundles` (line 285): introspection API
- `get_dependencies` (line 287): dependency introspection

### GuestContractInterface Fields

- `create_instance` (line 242): factory function for instance creation
- `destroy_instance` (line 247): destructor for instance cleanup
- `dispatch` (line 249): GuestContractDispatch union

### ABI_EXPECTED_SIZES Updated

All 20 struct size entries verified correct:
- GuestContractInterface: 56 bytes
- HostInterface: 88 bytes
- RuntimeInterface: 96 bytes
- GuestContractInstance: 16 bytes
- HostContractInstance: 8 bytes

## Verification Results

| Check | Result |
|-------|--------|
| `deno check sdks/js/mod.ts` | PASSED (no output) |
| `deno check sdks/js/abi/polyplug_abi.ts` | PASSED |
| GuestContractInterface present | 1 match |
| HostInterface present | 1 match |
| PluginInterface absent | 0 matches |
| HostVTable absent | 0 matches |

## Deviations from Plan

None - plan executed exactly as written.

## Requirements Satisfied

**SDK-05**: JS SDK TypeScript types use current polyplug_abi naming (GuestContractInterface, HostInterface, not PluginInterface/HostVTable).

## Self-Check: PASSED

- [x] sdks/js/abi/polyplug_abi.ts exists and contains correct interfaces
- [x] Commit e2813cc exists in git history
- [x] TypeScript compilation passes without errors
- [x] All acceptance criteria met