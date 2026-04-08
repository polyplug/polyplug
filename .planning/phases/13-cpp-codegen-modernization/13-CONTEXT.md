# Phase 13: C++ Codegen Modernization - Context

**Gathered:** 2026-04-08
**Status:** Ready for planning

<domain>
## Phase Boundary

Modernize C++ codegen to use modern `HostContractInterface`/`*Interface` naming instead of legacy `HostContractVTable`/`*VTABLE` terminology. The instance model functionality (create_instance/destroy_instance) is already implemented — this phase is purely about naming consistency.

</domain>

<decisions>
## Implementation Decisions

### Naming Modernization
- **D-01:** Rename `HostContractVTable` → `HostContractInterface` in all generated code
- **D-02:** Rename `HostContractVTableHeader` → embed in `HostContractInterface` or use correct modern naming
- **D-03:** Rename `_VTABLE` suffix → `_INTERFACE` for static interface declarations
- **D-04:** Rename variable names `vtable_` → `interface_` in RAII wrappers
- **D-05:** Update all comments referencing "vtable" to use "interface" terminology

### C++ Standard
- **D-06:** Target C++17 for generated code (existing standard used)
- **D-07:** Continue using `std::optional`, `std::string_view` (C++17 features)

### Testing
- **D-08:** Create `integration_codegen_cpp.rs` test file
- **D-09:** Test should verify:
  - Generated files exist (host/types.hpp, guest/interfaces.hpp, etc.)
  - Generated code contains `HostContractInterface` not `HostContractVTable`
  - Generated code contains `_INTERFACE` not `_VTABLE`
  - Instance wrapper class exists with create/destroy lifecycle

### SDK Validation
- **D-10:** Run `sdk_validator` after changes to ensure C++ SDK consistency
- **D-11:** SDK files must be "real and working" — no hacks

### Claude's Discretion
- Exact test file structure matching other language integration tests
- Whether to add additional helper utilities to C++ SDK

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### C++ Codegen
- `crates/polyplugc/src/generators/cpp.rs` — C++ code generator (rename HostContractVTable → HostContractInterface)
- `crates/polyplugc/src/generators/python.rs` — Reference for instance wrapper pattern
- `crates/polyplugc/src/generators/csharp.rs` — Reference for IDisposable pattern

### ABI Types (Correct Naming)
- `crates/polyplug_abi/src/host/host_contract_interface.rs` — HostContractInterface definition
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` — GuestContractInterface definition
- `crates/polyplug_abi/src/host/runtime_interface.rs` — RuntimeInterface definition

### C++ SDK
- `sdks/cpp/host/polyplug.hpp` — Host-side convenience header
- `sdks/cpp/guest/polyplug_guest.hpp` — Guest-side convenience header
- `sdks/cpp/abi/polyplug/abi.hpp` — C ABI definitions

### SDK Validation
- `sdk_validator.yaml` — SDK validation configuration
- `crates/sdk_validator/src/ast_grep.rs` — Validation logic

### Existing Integration Tests (Pattern to Follow)
- `crates/polyplugc/tests/integration_codegen_python.rs` — Python integration test pattern
- `crates/polyplugc/tests/integration_codegen_csharp.rs` — C# integration test pattern

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- C++ instance wrapper class already exists (lines 1034-1165 in cpp.rs)
- `create_instance`/`destroy_instance` stubs already generated
- Instance passed as first arg to dispatch calls already implemented

### Established Patterns
- `_VTABLE` static variable pattern (needs rename to `_INTERFACE`)
- `HostContractVTable` struct pattern (needs rename to `HostContractInterface`)
- RAII wrapper with move semantics, destructor calls `destroy_instance`

### Integration Points
- `generate_cpp_host_contract()` — host caller class generation
- `generate_cpp_guest_plugin_interface()` — guest interface generation
- `generate_cpp_host_interface_factories_file()` — host contract factory generation

### Files to Update in cpp.rs
- Lines 361, 446: `{}_VTABLE` → `{}_INTERFACE`
- Lines 1523, 1548, 1552: `HostContractVTable*` → `HostContractInterface*`
- Lines 1895-2000: Factory functions with `HostContractVTable`
- All variable names `vtable_` → `interface_`
- All comments mentioning "vtable" → "interface"

</code_context>

<specifics>
## Specific Ideas

- Follow the exact pattern from other generators (Python, C#, Lua, JS) for naming
- Generated code should compile against `polyplug_abi` types without modification
- The SDK validator should pass for C++ after changes

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---
*Phase: 13-cpp-codegen-modernization*
*Context gathered: 2026-04-08*