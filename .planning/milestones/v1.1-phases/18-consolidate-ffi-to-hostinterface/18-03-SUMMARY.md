---
phase: 18
plan: 03
subsystem: sdks
tags: [sdk, host-interface, python, csharp, ffi-consolidation]
requires: [18-02]
provides: [hostinterface-based-sdks]
affects: [sdks/python/host/polyplug/runtime.py, sdks/csharp/host/NativeMethods.cs, sdks/csharp/host/Runtime.cs, sdks/csharp/abi/Abi.cs]
tech-stack:
  added: []
  patterns: [host-interface-struct, function-pointer-calls, self-passing-pattern]
key-files:
  created: []
  modified:
    - sdks/python/host/polyplug/runtime.py
    - sdks/csharp/host/NativeMethods.cs
    - sdks/csharp/host/Runtime.cs
    - sdks/csharp/abi/Abi.cs
decisions:
  - D-18-28: Python Runtime holds HostInterface pointer
  - D-18-29: C# Runtime holds HostInterface pointer
metrics:
  duration: ~30min
  tasks_completed: 4
  files_modified: 4
---

# Phase 18 Plan 03: Update Python/C# SDKs for HostInterface Summary

**One-liner:** Updated Python and C# SDKs to hold HostInterface pointer and call all operations through struct fields, eliminating 11 separate FFI function imports.

## Changes Made

### Task 1 & 2: Python SDK

**Backend Protocol updated:**
- `create_host_interface()` - Returns HostInterface* (not opaque runtime)
- `destroy_host_interface(host)` - Destroys HostInterface and runtime
- `load_host_interface(host)` - Loads HostInterface struct from pointer

**HostInterface struct defined (144 bytes, 18 fields):**
- All function pointers at fixed offsets matching Rust ABI
- Self-passing pattern: each field takes HostInterface* as first param

**Runtime class:**
- Holds `_host` pointer (HostInterface*)
- Loads `_host_struct` via `HostInterface.from_address(host)`
- Caches CFUNCTYPE wrappers for function pointer calls
- All methods call through struct fields (e.g., `_load_bundle_fn(_host, buf, len)`)

### Task 3: C# NativeMethods

**Reduced FFI exports to 2:**
- `PolyplugRuntimeCreate` - Returns HostInterface*
- `PolyplugRuntimeDestroy` - Takes HostInterface*
- Removed 11 individual FFI function imports

**HostInterface struct (144 bytes):**
- 18 `nint` fields for function pointers
- Layout verified against Rust offset tests

### Task 4: C# Runtime

**Runtime class:**
- Holds `_host` pointer and `_hostStruct`
- Uses `Marshal.GetDelegateForFunctionPointer<T>()` to call through struct fields
- Added internal constructor for RuntimeBuilder compatibility
- Error handling via HostInterface.get_last_error/get_error_len fields

### Blocking Fix (Rule 3)

**sdks/csharp/abi/Abi.cs:**
- Added missing `StringView` struct definition
- Required by StringViewHelper.cs which was causing build failures

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Added missing StringView struct to C# Abi.cs**
- **Found during:** Task 4 verification (C# build)
- **Issue:** StringViewHelper.cs referenced `StringView` type that was not defined in Abi.cs
- **Fix:** Added `StringView` struct with `[StructLayout(LayoutKind.Sequential)]` and `Ptr/Len` fields
- **Files modified:** sdks/csharp/abi/Abi.cs
- **Commit:** d7dfe99

## Test Results

- Python syntax check: Passed
- C# dotnet build: Passed (0 errors, 0 warnings)

## Known Stubs

None - all SDK methods call through HostInterface fields.

## Threat Flags

None - no new security-relevant surface. HostInterface pointer validity checked before each call.