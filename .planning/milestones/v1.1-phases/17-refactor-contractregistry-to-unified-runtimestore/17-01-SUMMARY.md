---
phase: 17-refactor-contractregistry-to-unified-runtimestore
plan: 01
status: complete
retroactive: true
commits:
  - 8cdfb3e refactor(17-01): rename registry types to RuntimeStore
  - e38ab36 refactor(17-01): update Runtime and callers to use RuntimeStore
  - 7b1e0d3 refactor(17-01): fix remaining Runtime method renames
  - 95fd2b1 test: update all test/bench/example files for RuntimeStore rename
  - f02cc32 sdk: update all SDKs for RuntimeStore FFI rename
  - c1b1626 codegen: update polyplugc generators for RuntimeStore rename
---

# Plan 17-01: Pass 1 — Rename ContractRegistry to RuntimeStore

**One-liner:** Renamed ContractRegistry and all related types/methods/fields to RuntimeStore naming convention across all source, tests, SDKs, and codegen (32+ files, 553 lines changed).

## What Was Done

Renamed all ContractRegistry types to RuntimeStore:
- `ContractRegistry` → `RuntimeStore`
- `ContractRegistryData` → `RuntimeStoreData`
- `RegistrySlot` → `PluginSlot`
- `RegistryEntry` → `PluginEntry`
- `bundle_slots_index` field naming conventions applied

Updated all callers:
- `Runtime` struct uses `registry: Arc<RuntimeStore>`
- `reload.rs` uses renamed methods
- `ffi.rs` uses RuntimeStore naming
- All 16+ test files updated
- All 5 SDKs updated (Python, C#, Lua, JS, C++)
- All 7 polyplugc generators updated

## Commits

1. `8cdfb3e` — Rename registry types to RuntimeStore
2. `e38ab36` — Update Runtime and callers to use RuntimeStore
3. `7b1e0d3` — Fix remaining Runtime method renames
4. `95fd2b1` — Update all test/bench/example files for RuntimeStore rename
5. `f02cc32` — Update all SDKs for RuntimeStore FFI rename
6. `c1b1626` — Update polyplugc generators for RuntimeStore rename

## Key Files

### Created
- `crates/polyplug/src/registry/runtime_store.rs` — RuntimeStore, PluginEntry, PluginSlot, RuntimeStoreData

### Modified
- `crates/polyplug/src/registry/mod.rs` — Public export of RuntimeStore
- `crates/polyplug/src/runtime.rs` — Runtime using RuntimeStore
- `crates/polyplug/src/reload.rs` — Reload uses renamed methods
- `crates/polyplug/src/ffi.rs` — FFI uses RuntimeStore naming
- 16 test files — Updated to RuntimeStore naming
- 5 SDK files — Updated FFI naming
- 7 codegen generators — Updated for RuntimeStore API

## Self-Check

- [x] All tasks executed (retroactive — work done before SUMMARY creation)
- [x] Each task committed individually (6 commits)
- [x] SUMMARY.md created
