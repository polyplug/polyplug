---
phase: 06-cleanup
plan: 05
completed: 2026-04-05T12:00:00Z
status: completed
---

# Plan 06-05: Rename Generator File and Function Names

## Summary

Renamed all vtable-related file names and function names in all 6 generators to use interface terminology.

## Changes Made

### Lua Generator (`lua.rs`)
- Output file: `vtable_factories.lua` -> `interface_factories.lua`
- Functions renamed: `generate_lua_host_vtable_factories_file` -> `generate_lua_host_interface_factories_file`
- Functions renamed: `generate_lua_host_vtable_factory` -> `generate_lua_host_interface_factory`
- Functions renamed: `generate_guest_plugin_vtable` -> `generate_guest_plugin_interface`

### Python Generator (`python.rs`)
- Output file: `vtable_factories.py` -> `interface_factories.py`
- Functions renamed: `generate_python_host_vtable_factories_file` -> `generate_python_host_interface_factories_file`
- Functions renamed: `generate_python_host_vtable_factory` -> `generate_python_host_interface_factory`

### C++ Generator (`cpp.rs`)
- Output files: `vtable_factories.hpp` -> `interface_factories.hpp`, `vtables.hpp` -> `interfaces.hpp`
- Functions renamed: `generate_cpp_host_vtable_factories_file` -> `generate_cpp_host_interface_factories_file`
- Functions renamed: `generate_cpp_host_vtable_factory` -> `generate_cpp_host_interface_factory`
- Functions renamed: `generate_vtables_hpp` -> `generate_interfaces_hpp`
- Functions renamed: `generate_cpp_guest_plugin_vtable` -> `generate_cpp_guest_plugin_interface`
- Functions renamed: `generate_cpp_guest_contract_vtable` -> `generate_cpp_guest_contract_interface`

### JS Generator (`js_quickjs.rs`)
- Output files: `vtable_factories.ts` -> `interface_factories.ts`, `vtable.ts` -> `interface.ts`
- Functions renamed: `generate_js_host_vtable_factories_ts` -> `generate_js_host_interface_factories_ts`
- Functions renamed: `generate_js_host_vtable_factory` -> `generate_js_host_interface_factory`
- Functions renamed: `generate_vtable_ts` -> `generate_interface_ts`

### Rust Generator (`rust.rs`)
- Output files: `vtable_factories.rs` -> `interface_factories.rs`, `vtables.rs` -> `interfaces.rs`
- Functions renamed: `generate_host_vtable_factories_file` -> `generate_host_interface_factories_file`
- Functions renamed: `generate_host_vtable_factory` -> `generate_host_interface_factory`
- Functions renamed: `generate_guest_vtables_file` -> `generate_guest_interfaces_file`
- Functions renamed: `generate_guest_plugin_vtable` -> `generate_guest_plugin_interface`

### C# Generator (`csharp.rs`)
- Output files: `VTableFactories.cs` -> `InterfaceFactories.cs`, `Vtables.cs` -> `Interfaces.cs`
- Functions renamed: `generate_cs_host_vtable_factories_file` -> `generate_cs_host_interface_factories_file`
- Functions renamed: `generate_cs_host_vtable_factory` -> `generate_cs_host_interface_factory`
- Functions renamed: `generate_cs_guest_vtables` -> `generate_cs_guest_interfaces`
- Generated class names: `{Contract}Vtables` -> `{Contract}Interfaces`

## Verification

All acceptance criteria passed:
- No `vtable_factories` in generator files (grep returns 0)
- No `vtables.rs|hpp` in generator files (grep returns 0)
- `interface_factories` found in all 6 generators
- `interfaces.rs|hpp` found in guest generators
- `cargo build -p polyplugc` exits with code 0

## Commit

`4cdfcf6` - feat(polyplugc): rename generators to interface terminology, fix ABI templates