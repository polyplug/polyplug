---
phase: 06-cleanup
verified: 2026-04-05T08:00:00Z
status: gaps_found
score: 2/4 requirements verified
gaps:
  - truth: "No 'vtable' naming remains in codebase (search: vtable, VTable, VTABLE)"
    status: failed
    reason: "28.4KB of vtable matches across generators, SDKs, and tests"
    breakdown:
      generators:
        - file: "crates/polyplugc/src/generators/lua.rs"
          count: 85 matches
          issues:
            - "File names: vtable_factories.lua, vtables.hpp"
            - "Functions: generate_guest_plugin_vtable, generate_lua_host_vtable_factory"
            - "Generated code: store_host_vtable, _vtable, vtable.dispatch"
        - file: "crates/polyplugc/src/generators/python.rs"
          count: 72 matches
          issues:
            - "File names: vtable_factories.py"
            - "Functions: generate_guest_plugin_vtable, generate_guest_contract_vtable"
            - "Generated code: HostContractVTable, _vtable"
        - file: "crates/polyplugc/src/generators/cpp.rs"
          count: 68 matches
          issues:
            - "File names: vtable_factories.hpp, vtables.hpp"
            - "Functions: generate_vtables_hpp, generate_cpp_guest_plugin_vtable"
            - "Generated code: store_host_vtable, vtable_, vtable->dispatch"
        - file: "crates/polyplugc/src/generators/js_quickjs.rs"
          count: 45 matches
          issues:
            - "File names: vtable_factories.ts, vtable.ts"
            - "Functions: generate_js_host_vtable_factories_ts, render_plugin_vtable_quickjs"
            - "Generated code: this.vtable, vtable.dispatch"
        - file: "crates/polyplugc/src/generators/rust.rs"
          count: 22 matches
          issues:
            - "File names: vtable_factories.rs"
            - "Functions: generate_guest_plugin_vtable"
            - "Generated code: store_host_vtable"
      sdks:
        - file: "sdks/python/host/polyplug/runtime.py"
          issues: ["HostContractVTable", "vtable parameter"]
        - file: "sdks/lua/host/polyplug/runtime.lua"
          issues: ["HostContractVTable", "vtable variable"]
        - file: "sdks/cpp/host/polyplug/runtime.hpp"
          issues: ["HostContractVTable", "vtable parameter"]
        - file: "sdks/csharp/host/Runtime.cs"
          issues: ["HostContractVTable usage"]
        - file: "sdks/js/host/polyplug/mod.js"
          issues: ["vtable references"]
      tests:
        - file: "crates/polyplugc/tests/vtable_factories_tests.rs"
          issue: "Test file not renamed to interface_factories_tests.rs"
        - file: "crates/polyplugc/tests/integration_codegen_rust.rs"
          issue: "Uses PluginInterface, HostVTable"
        - file: "crates/polyplugc/tests/smoke.rs"
          issue: "Uses PluginInterface, HostVTable"
        - file: "crates/polyplug/tests/*.rs"
          issue: "Multiple test files use removed aliases"
  - truth: "All tests pass with new instance model and naming"
    status: failed
    reason: "Generated guest code uses outdated ABI structure causing 195+ compilation errors"
    breakdown:
      abi_mismatches:
        - issue: "PluginDescriptor fields"
          expected: "version: Version"
          generated: "version_major/minor/patch: u32"
        - issue: "Error code types"
          expected: "AbiErrorCode enum"
          generated: "u32 constants (ABI_ERROR_GENERIC)"
        - issue: "Registration function"
          expected: "register_contract"
          generated: "register_plugin"
        - issue: "Type names"
          expected: "RuntimeAbi, GuestContractInterface"
          generated: "HostVTable, PluginInterface"
---
# Phase 6: Cleanup Verification Report

**Phase Goal:** Remove all vtable/legacy naming and update to Guest/Host terminology consistently
**Verified:** 2026-04-04T19:30:00Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Goal Achievement

### Observable Truths (Success Criteria from ROADMAP.md)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | No "vtable" naming remains in codebase (search: vtable, VTable, VTABLE) | FAILED | 24.6KB of vtable matches found across generators, SDKs, tests |
| 2 | No *C suffix types in FFI (all types from polyplug_abi are canonical) | VERIFIED | RuntimeConfigC intentional (FFI param struct), others renamed |
| 3 | Documentation uses Guest Contract / Host Contract terminology consistently | VERIFIED | All docs updated with terminology notes |
| 4 | All tests pass with new instance model and naming | FAILED | cargo test --workspace fails with 195+ compilation errors |

**Score:** 2/4 truths verified

### Detailed Gap Breakdown

**CLN-01 (vtable naming):** 28.4KB of vtable matches across codebase:

| Category | Files | Match Count | Key Issues |
|----------|-------|-------------|------------|
| Generators | lua.rs, python.rs, cpp.rs, js_quickjs.rs, rust.rs, csharp.rs | 292 | File names (vtable_factories.*), function names, generated variable names |
| SDKs | python/host, lua/host, cpp/host, csharp/host, js/host | 50+ | HostContractVTable type, vtable parameters |
| Tests | vtable_factories_tests.rs, smoke.rs, integration_*.rs | 40+ | Removed alias imports |

**CLN-04 (tests pass):** Generated code ABI mismatches:

### Requirements Coverage

| Requirement | Description | Status | Evidence |
|-------------|-------------|--------|----------|
| CLN-01 | Remove all "vtable" naming from codebase | FAILED | Extensive vtable terminology in generators, SDKs, tests |
| CLN-02 | Remove *C suffix types from FFI | VERIFIED | RuntimeConfigC intentional, HostVTableStorage renamed, ReloadPhaseC renamed to ReloadPhaseFfi |
| CLN-03 | Update documentation to use Guest/Host terminology | VERIFIED | All docs updated with terminology notes, only historical references to old names |
| CLN-04 | Update tests to use new instance model and naming | FAILED | Tests don't compile, generated code uses wrong ABI structure |

### Required Artifacts (from PLAN frontmatter)

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `crates/polyplug_abi/src/lib.rs` | No legacy aliases | VERIFIED | PluginInterface, HostVTable, PluginDispatch aliases removed |
| `crates/polyplug/benches/contract_dispatch.rs` | Renamed benchmark file | VERIFIED | File exists, uses GuestContractInterface/RuntimeAbi imports |
| `sdks/csharp/guest/RuntimeAbiStorage.cs` | Renamed C# storage class | VERIFIED | HostVTableStorage renamed to RuntimeAbiStorage |
| `sdks/cpp/guest/polyplug/contract.hpp` | interface() method | VERIFIED | vtable() renamed to interface() |
| `crates/polyplugc/tests/interface_factories_tests.rs` | Renamed test file | MISSING | vtable_factories_tests.rs still exists |
| `examples/guests/rust/*/generated/` | Updated generated code | FAILED | Uses old ABI: version_major/minor/patch, u32 error codes, register_plugin |
| `crates/polyplug/tests/*.rs` | Updated test imports | FAILED | Many files still use PluginInterface, HostVTable imports |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|----|--------|---------|
| `polyplug_abi/src/lib.rs` | All crate imports | pub use GuestContractInterface, RuntimeAbi | VERIFIED | Aliases removed, correct exports |
| `polyplugc generators` | Generated code | Template strings | FAILED | Templates use vtable terminology, old ABI structure |
| `examples/guests/` | polyplugc generate | regeneration | FAILED | Generated code outdated, causes compilation failures |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|-------------------|--------|
| `contract_dispatch.rs` benchmark | LAST_INTERFACE | thread_local static | Yes | VERIFIED |
| `integration_load.rs` tests | CAPTURED_VTABLE | thread_local static | Yes | HOLLOW — uses old type names |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `crates/polyplugc/src/generators/rust.rs` | 5 | "vtable stubs" comment | Blocker | Generated code uses old terminology |
| `crates/polyplugc/src/generators/*.rs` | multiple | `vtable_factories` file names | Blocker | Generates vtable-named files |
| `examples/guests/rust/*/init.rs` | 66-68 | `version_major/minor/patch` fields | Blocker | ABI mismatch, tests fail |
| `examples/guests/rust/*/init.rs` | 43 | `ABI_ERROR_GENERIC` u32 constant | Blocker | ABI mismatch, tests fail |
| `examples/guests/rust/*/init.rs` | 72 | `register_plugin` function | Blocker | Old ABI function name |
| `crates/polyplug/tests/*.rs` | multiple | `use polyplug_abi::PluginInterface` | Blocker | Tests use removed type alias |

### Gaps Summary

**CLN-01 (vtable naming):** Despite SUMMARY.md claiming "Removed all legacy vtable terminology", significant vtable terminology remains:

1. **Generators produce vtable-named files:**
   - `host/vtable_factories.lua/py/hpp/cs/rs`
   - `guest/vtables.hpp/cs/rs`

2. **Generators use vtable terminology internally:**
   - Functions: `generate_guest_plugin_vtable`, `generate_vtables_hpp`, `generate_vtable_factories`
   - Generated code: `store_host_vtable`, `get_host_vtable`, `HostContractVTable`
   - Variables: `_vtable`, `vtable_ptr`, `vtable`

3. **Test file not renamed:** `vtable_factories_tests.rs` still exists

4. **SDK files use vtable terminology:** Python, Lua, C++ SDK host contract code

5. **Test imports use removed aliases:** Multiple test files import `PluginInterface`, `HostVTable`

**CLN-04 (tests pass):** Tests fail with 195+ compilation errors due to:

1. Generated guest code uses wrong PluginDescriptor fields (version_major/minor/patch instead of version: Version)
2. Generated guest code uses u32 error codes instead of AbiErrorCode enum
3. Generated guest code uses register_plugin instead of register_contract
4. Generated guest code uses HostVTable/PluginInterface instead of RuntimeAbi/GuestContractInterface
5. Test imports use removed type aliases

### Human Verification Required

None — all failures are programatically detectable compilation errors and grep patterns.

---

_Verified: 2026-04-04T19:30:00Z_
_Verifier: Claude (gsd-verifier)_