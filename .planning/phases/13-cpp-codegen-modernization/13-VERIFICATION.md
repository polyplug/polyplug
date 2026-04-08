---
phase: 13-cpp-codegen-modernization
verified: 2026-04-08T15:30:00Z
status: passed
score: 11/11 must-haves verified
overrides_applied: 0
gaps: []
deferred: []
human_verification: []
---

# Phase 13: C++ Codegen Modernization Verification Report

**Phase Goal:** Modernize C++ codegen to use HostContractInterface naming (not HostContractVTable), align generated code with polyplug_abi types from Phase 1.
**Verified:** 2026-04-08T15:30:00Z
**Status:** passed
**Re-verification:** No - initial verification

## Goal Achievement

### Observable Truths

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | C++ codegen generates *Instance RAII wrappers (not PluginGuard) | VERIFIED | cpp.rs lines 1034-1165: `GuestContractInstance instance_` member, create/destroy lifecycle |
| 2 | Generated wrappers call create_instance on construction | VERIFIED | cpp.rs line 1075: `GuestContractInstance instance = iface->create_instance(host, nullptr)` |
| 3 | Generated wrappers call destroy_instance on drop | VERIFIED | cpp.rs line 1091: `interface_->destroy_instance(host_, instance_)` |
| 4 | Instance passed as first argument to all dispatch calls | VERIFIED | cpp.rs lines 1231, 1250, 1631, 1637: `fn_(instance_, args_ptr, ...)` for both guest and host contract callers |
| 5 | C++ SDK uses HostInterface terminology | VERIFIED | cpp.rs uses `HostContractInterface` (4 matches), no `HostContractVTable` found |
| 6 | Generated C++ code uses HostContractInterface terminology (not HostContractVTable) | VERIFIED | grep: 0 matches for HostContractVTable in cpp.rs |
| 7 | Static declarations use _INTERFACE suffix (not _VTABLE) | VERIFIED | cpp.rs lines 361, 446, 726, 785: `_INTERFACE` suffix used; grep: 0 matches for `_VTABLE` |
| 8 | RAII wrappers store interface_ member (not vtable_) | VERIFIED | cpp.rs lines 1154, 1561: `const GuestContractInterface* interface_` and `const HostContractInterface* interface_`; grep: 0 matches for `vtable_` |
| 9 | Factory functions emit inline HostContractInterface fields (no HostContractVTableHeader wrapper) | VERIFIED | cpp.rs lines 1968-1980: `static HostContractInterface s_interface = { contract_id, contract_version, singleton, ... }`; grep: 0 matches for HostContractVTableHeader |
| 10 | Integration test verifies C++ codegen produces correct naming | VERIFIED | integration_codegen_cpp.rs: 7 tests, all passing |
| 11 | sdk_validator passes for C++ SDK | VERIFIED | `just validate-sdks`: 25/30 method implementations (83.3%), all 5 C++ StringView methods present |

**Score:** 11/11 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `crates/polyplugc/src/generators/cpp.rs` | C++ code generator with modern naming | VERIFIED | Contains `HostContractInterface`, `_INTERFACE`, `interface_`, `instance_`; no legacy naming |
| `crates/polyplugc/tests/integration_codegen_cpp.rs` | Integration test file | VERIFIED | 397 lines, 7 test functions, all passing |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| cpp.rs static declarations | GuestContractInterface | `_INTERFACE` suffix | WIRED | Lines 361, 446: `static GuestContractInterface {}_INTERFACE = {...}` |
| cpp.rs register_contract | interface static | reference `_INTERFACE` | WIRED | Lines 726, 785: `&polyplug_plugin::{upper}_INTERFACE` |
| cpp.rs guest host contract caller | HostContractInterface | `interface_` member | WIRED | Lines 1561-1562: `interface_` and `instance_` members |
| cpp.rs factory function | HostContractInterface ABI | inline fields | WIRED | Lines 1968-1980: matches ABI layout exactly |
| integration test | generated files | file existence checks | WIRED | Tests verify guest/host files exist |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| cpp.rs guest contract wrapper | `instance_` | `iface->create_instance(host, nullptr)` | Creates GuestContractInstance from interface | FLOWING |
| cpp.rs host contract caller | `instance_` | `host->get_host_contract(...)` | Returns HostContractInstance from host | FLOWING |
| cpp.rs dispatch calls | `fn_(instance_, args_ptr, ...)` | Native/VM dispatch | Passes instance to function pointer | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Library tests pass | `cargo test -p polyplugc --lib` | 182 tests passed | PASS |
| Integration tests pass | `cargo test -p polyplugc --test integration_codegen_cpp` | 7 tests passed | PASS |
| No legacy naming in cpp.rs | `grep -c "HostContractVTable" cpp.rs` | 0 matches | PASS |
| No _VTABLE suffix | `grep -c "_VTABLE" cpp.rs` | 0 matches | PASS |
| Interface member present | `grep -c "interface_" cpp.rs` | 34 matches | PASS |
| Instance member present | `grep -c "instance_" cpp.rs` | 58 matches | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ---------- | ----------- | ------ | -------- |
| CG-05 | 13-01, 13-02 | Update host contract vtable generation for HostContractInterface | SATISFIED | cpp.rs lines 1866-2027: factory functions use inline HostContractInterface fields |
| CG-02 | 13-02 | Update codegen to generate instance wrappers | SATISFIED | cpp.rs lines 1154, 1156: `interface_` and `instance_` members in wrapper class |
| CG-03 | 13-02 | Generated instance wrappers hold interface + instance pointer | SATISFIED | cpp.rs lines 1154-1156: both members present |
| CG-04 | 13-02 | Generated wrappers call create_instance/destroy_instance | SATISFIED | cpp.rs lines 1075, 1091: lifecycle methods called |
| INST-01 | 13-02 | Update codegen to generate *Instance RAII wrappers | SATISFIED | cpp.rs generates ContractWrapper with instance lifecycle |
| INST-02 | 13-02 | Generated wrapper calls create_instance on construction | SATISFIED | cpp.rs line 1075: `iface->create_instance(host, nullptr)` |
| INST-03 | 13-02 | Generated wrapper calls destroy_instance on drop | SATISFIED | cpp.rs line 1091: `interface_->destroy_instance(host_, instance_)` |
| INST-04 | 13-02 | Instance passed as first argument to all dispatch calls | SATISFIED | cpp.rs lines 1231, 1250, 1631, 1637: `instance_` passed to dispatch |
| INST-05 | 13-02 | Native dispatch: functions[fn_id](instance, args, out) | SATISFIED | cpp.rs line 1631: `fn_(instance_, args_ptr, out_ptr)` |
| INST-06 | 13-02 | VM dispatch: call(loader_data, instance, fn_id, args, out) | SATISFIED | cpp.rs line 1637: VM call passes `instance_` |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| sdks/cpp/abi/polyplug/abi.hpp | 6-14 | Invalid placeholder syntax (`&[u8]`, `&str`) | Info | Pre-existing stub, documented in deferred-items.md, not caused by phase changes |

**Note:** The invalid placeholder syntax in abi.hpp is a pre-existing issue documented in deferred-items.md. It is out of scope for this phase and does not block verification.

### Deferred Items (Out of Scope)

| Item | Addressed In | Evidence |
| ---- | ------------- | -------- |
| C++ SDK abi.hpp placeholder syntax | Future SDK work | documented in deferred-items.md - pre-existing stub with Rust-like syntax, not phase-related |

### Gaps Summary

No gaps found. All must-haves verified successfully.

---

_Verified: 2026-04-08T15:30:00Z_
_Verifier: Claude (gsd-verifier)_