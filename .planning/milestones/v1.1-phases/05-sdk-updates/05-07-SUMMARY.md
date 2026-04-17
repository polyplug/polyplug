---
phase: 05-sdk-updates
plan: 07
type: execute
wave: 1
gap_closure: true
status: completed
completed_at: 2026-04-04
---

## Summary: C++ SDK Instance Model Update

**Objective:** Update C++ SDK to match instance-based model: remove PluginGuard class, add FFI RuntimeConfig struct in global namespace (24 bytes), add compatibility field to high-level RuntimeConfig, and update resolve_plugin to return raw handle.

### Completed Tasks

| Task | Description | Status |
|------|-------------|--------|
| 1 | Remove PluginGuard class from runtime.hpp | ✓ Complete |
| 2 | Move FFI RuntimeConfig struct to global namespace | ✓ Complete |
| 3 | Add compatibility field to high-level RuntimeConfig | ✓ Complete |
| 4 | Update resolve_plugin to return raw handle | ✓ Complete |
| 5 | Update set_config conversion with compatibility field | ✓ Complete |

### What Was Built

Updated C++ SDK (`sdks/cpp/host/polyplug/runtime.hpp` and `runtime_config.hpp`) to match the instance-based model used by other SDKs:

1. **PluginGuard removal**: Deleted the entire PluginGuard class (59 lines). The instance-based model does not use RAII guards - hosts call `release_plugin()` explicitly when done with handles.

2. **FFI RuntimeConfig struct**: Added a 24-byte `RuntimeConfig` struct in global namespace (after extern "C" block, before namespace polyplug). This matches the polyplug_abi layout exactly with the compatibility field at offset 20.

3. **High-level RuntimeConfig**: Added `uint32_t compatibility{0U}` field to the polyplug::RuntimeConfig class in runtime_config.hpp.

4. **resolve_plugin return type**: Changed from `PluginGuard` to `const ResolveHandle*` (raw pointer). Added documentation explaining the instance-based workflow.

5. **release_plugin method**: Added explicit cleanup method for handles.

6. **set_config conversion**: Updated to use `::RuntimeConfig` (global namespace FFI struct) and convert the compatibility field.

### Key Files Modified

- `sdks/cpp/host/polyplug/runtime.hpp` — Core runtime wrapper with FFI struct and resolve changes
- `sdks/cpp/host/polyplug/runtime_config.hpp` — High-level config with compatibility field

### Verification Results

- `grep -c "class PluginGuard"` returns 0 ✓
- `grep -c "RuntimeConfigC"` returns 0 ✓
- FFI struct in global namespace (not inside `namespace polyplug`) ✓
- `static_assert(sizeof(RuntimeConfig) == 24)` present ✓
- compatibility field in both files ✓
- resolve_plugin returns `const ResolveHandle*` ✓
- release_plugin method added ✓

### Deviations

None - all tasks completed as planned.