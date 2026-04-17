# Deferred Items (Phase 19, Plan 04)

## Out-of-Scope Discoveries

### 1. C++ handle.hpp has stale `generation` field references
- **File:** `sdks/cpp/host/polyplug/handle.hpp`
- **Issue:** The file references `GuestContractHandle::generation` which no longer exists in the auto-generated `abi.hpp` (D-23: GuestContractHandle has only `index: u32`). The equality operators will fail to compile.
- **Not fixed because:** This file was not in the plan's file list for Task 2. Pre-existing issue not directly caused by this task's changes.
- **Resolution needed:** Update `handle.hpp` equality operators to compare only `index` field, update comments about "generational index".
