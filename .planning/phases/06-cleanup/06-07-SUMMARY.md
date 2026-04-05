---
phase: 06-cleanup
plan: 07
completed: 2026-04-05T14:00:00Z
status: completed
---

# Plan 06-07: Update SDK Host Files to Interface Terminology

## Summary

Renamed HostContractVTable to HostContractInterface in all SDK host files and updated related variable/function names.

## Changes Made

### Python SDK
- Renamed ctypes structures: `HostContractVTableHeader` -> `HostContractInterfaceHeader`, `HostContractVTable` -> `HostContractInterface`
- Renamed field: `vtable_version` -> `interface_version`
- Renamed parameters: `vtable_ptr` -> `interface_ptr`
- Renamed storage: `_host_contract_vtables` -> `_host_contract_interfaces`
- Updated docstrings to use "interface" terminology

### Lua SDK
- Renamed FFI cdef structures in runtime.lua and polyplug_abi.lua
- Renamed function parameter `vtable` -> `interface` in `register_host_contract`
- Updated error messages to use "interface"

### C++ SDK
- Renamed FFI declaration parameter `vtable` -> `interface`
- Updated `register_host_contract` method parameter
- Added doc comment describing interface lifetime requirement

### C# SDK
- Renamed method parameter `vtable` -> `hostInterface` (avoiding C# keyword)
- Added doc comments referencing HostContractInterface

### JS SDK
- Renamed JSDoc type reference `HostContractVTable` -> `HostContractInterface`
- Renamed parameter `vtable` -> `hostInterface`

### Rust SDK
- Removed legacy `HostContractVTable` type alias
- Removed legacy `HostContractVTableHeader` type alias
- Updated doc comment for `HostContractInterface`

## Verification

All acceptance criteria passed:
- No `HostContractVTable` in any SDK host files
- `HostContractInterface` found in all 5 SDK host files
- SDK files compile/load successfully

## Commit

`ba79cf3` - refactor(sdk): rename HostContractVTable to HostContractInterface