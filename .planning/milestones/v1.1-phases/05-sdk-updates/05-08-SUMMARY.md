---
phase: 05-sdk-updates
plan: 08
type: execute
wave: 2
depends_on: [05-07]
gap_closure: true
status: completed
completed_at: 2026-04-04
---

## Summary: RuntimeConfigC → RuntimeConfig Rename

**Objective:** Rename RuntimeConfigC to RuntimeConfig in all four native SDKs (Python, C#, Lua, C++) for naming consistency with polyplug_abi.

### Completed Tasks

| Task | Description | Status |
|------|-------------|--------|
| 1 | Rename RuntimeConfigC to RuntimeConfig in Python SDK | ✓ Complete |
| 2 | Rename RuntimeConfigC to RuntimeConfig in C# SDK | ✓ Complete |
| 3 | Rename RuntimeConfigC to RuntimeConfig in Lua SDK | ✓ Complete |
| 4 | Verify RuntimeConfig naming in C++ SDK (from plan 05-07) | ✓ Verified |
| 5 | Cross-SDK verification confirms zero RuntimeConfigC references | ✓ Verified |

### What Was Built

Renamed the legacy "*C" suffixed FFI type to match the canonical polyplug_abi naming exactly:

1. **Python SDK** (`sdks/python/host/polyplug/runtime.py`):
   - Renamed `class RuntimeConfigC` → `class RuntimeConfig`
   - Updated `RuntimeCreateOptionsC._fields_` pointer type
   - Updated `_create_runtime_with_options` instantiation
   - Updated CFFIBackend cdef typedef

2. **C# SDK** (`sdks/csharp/host/NativeMethods.cs` and `Runtime.cs`):
   - Renamed `internal struct RuntimeConfigC` → `internal struct RuntimeConfig`
   - Updated `PolyplugRuntimeSetConfig` parameter type
   - Updated `Runtime.SetConfig()` usage

3. **Lua SDK** (`sdks/lua/host/polyplug/runtime.lua`):
   - Renamed ffi.cdef typedef `RuntimeConfigC` → `RuntimeConfig`
   - Updated `RuntimeCreateOptions.config` pointer type
   - Updated `ffi.new("RuntimeConfig", ...)` instantiation

4. **C++ SDK** (`sdks/cpp/host/polyplug/runtime.hpp`):
   - Verified no RuntimeConfigC references (already renamed in 05-07)

5. **Python test file** (`sdks/python/host/tests/test_runtime_config_c.py`):
   - Updated class and test function names to use RuntimeConfig

### Key Files Modified

- `sdks/python/host/polyplug/runtime.py` — Main Python SDK runtime module
- `sdks/python/host/tests/test_runtime_config_c.py` — Python test file
- `sdks/csharp/host/NativeMethods.cs` — C# P/Invoke declarations
- `sdks/csharp/host/Runtime.cs` — C# Runtime wrapper
- `sdks/lua/host/polyplug/runtime.lua` — Lua SDK runtime module

### Verification Results

- `grep -r "RuntimeConfigC" sdks/` returns 0 matches ✓
- All four SDKs use `RuntimeConfig` naming ✓
- Naming matches polyplug_abi::RuntimeConfig exactly ✓

### Deviations

None - all tasks completed as planned.