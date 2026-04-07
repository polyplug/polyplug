---
phase: 11-guest-calling-convention-missing-introspection
verified: 2026-04-07T21:30:00Z
status: gaps_found
score: 8/14 requirements verified
overrides_applied: 0
gaps:
  - truth: "Loaders updated to use HostInterface instead of RuntimeAbi"
    status: failed
    reason: "polyplug_python and polyplug_dotnet still import RuntimeAbi and RuntimeContext"
    artifacts:
      - path: "crates/polyplug_python/src/lib.rs"
        issue: "Imports deleted RuntimeContext, HostContext, and renamed RuntimeAbi"
      - path: "crates/polyplug_dotnet/src/context.rs"
        issue: "Uses RuntimeAbi instead of HostInterface"
    missing:
      - "Update polyplug_python/src/lib.rs to use HostInterface, remove RuntimeContext/HostContext imports"
      - "Update polyplug_dotnet/src/context.rs to use HostInterface"
  - truth: "Tests updated to use new self-passing pattern"
    status: failed
    reason: "Test fixtures still use RuntimeContext parameter in callbacks"
    artifacts:
      - path: "crates/polyplug/tests/integration_*.rs"
        issue: "Multiple test files still use RuntimeContext parameter"
      - path: "crates/polyplug/benches/*.rs"
        issue: "Benchmarks still use rt_ctx parameter"
    missing:
      - "Update test fixtures to use *const HostInterface instead of RuntimeContext"
      - "Update benchmarks to use new callback signatures"
  - truth: "Codegen updated to generate new calling convention"
    status: failed
    reason: "polyplugc generators still emit rt_ctx parameter"
    artifacts:
      - path: "crates/polyplugc/src/generators/python.rs"
        issue: "Generates rt_ctx parameter in stub functions"
      - path: "crates/polyplugc/src/generators/lua.rs"
        issue: "Generates rt_ctx parameter in stub functions"
      - path: "crates/polyplugc/src/generators/cpp.rs"
        issue: "Generates rt_ctx in register_contract calls"
    missing:
      - "Update codegen to generate HostInterface parameter instead of rt_ctx"
  - truth: "Workspace compiles after phase completion"
    status: failed
    reason: "polyplug_python and polyplug_dotnet crates do not compile"
    artifacts:
      - path: "crates/polyplug_python"
        issue: "4 compilation errors - unresolved imports for deleted types"
      - path: "crates/polyplug_dotnet"
        issue: "1 compilation error - RuntimeAbi not found"
    missing:
      - "Fix all compilation errors before phase can be considered complete"
  - truth: "VM loaders updated for new polyplug_init signature"
    status: failed
    reason: "polyplug_python and polyplug_js still use old init signature with rt_ctx"
    artifacts:
      - path: "crates/polyplug_python/src/lib.rs"
        issue: "Creates HostContext and passes rt_ctx to polyplug_init"
      - path: "crates/polyplug_js/tests/quickjs_loader.rs"
        issue: "Test fixtures expect rt_ctx parameter"
    missing:
      - "Update VM loaders to use new polyplug_init signature without rt_ctx"
  - truth: "Dependency enforcement works with new TLS pattern"
    status: partial
    reason: "TLS implementation exists in runtime.rs but callers not updated"
    artifacts:
      - path: "crates/polyplug/src/runtime.rs"
        issue: "TLS INIT_BUNDLE_ID implemented but loaders don't call set_init_bundle_id"
---

# Phase 11: Guest Calling Convention & Missing Introspection Verification Report

**Phase Goal:** Rename `RuntimeAbi` to `HostInterface`, create `RuntimeInterface` for symmetric API, delete `RuntimeContext`/`HostContext` wrappers, rename `call_method` to `call_guest_method`, implement guest-to-guest calls, add introspection ABIs, create `Array<T>` type, update all SDKs and codegen.
**Verified:** 2026-04-07T21:30:00Z
**Status:** gaps_found
**Re-verification:** No - initial verification

## Goal Achievement

### Observable Truths

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | HostInterface struct exists (renamed from RuntimeAbi) | VERIFIED | crates/polyplug_abi/src/host/host_interface.rs:70 - 88 bytes with runtime field at offset 0 |
| 2 | RuntimeInterface struct exists with destroy() function | VERIFIED | crates/polyplug_abi/src/host/runtime_interface.rs:70 - 96 bytes with destroy at offset 88 |
| 3 | RuntimeContext and HostContext deleted | VERIFIED | Files deleted, no references in polyplug_abi |
| 4 | GuestContractInstance has contract_id field (16 bytes) | VERIFIED | crates/polyplug_abi/src/guest/guest_contract_instance.rs:53 - 16 bytes with contract_id at offset 8 |
| 5 | Array<T> has align field (24 bytes) | VERIFIED | crates/polyplug_abi/src/types/array.rs:44 - 24 bytes with align at offset 16 |
| 6 | DependencyInfo struct exists | VERIFIED | crates/polyplug_abi/src/types/dependency_info.rs:41 - 24 bytes |
| 7 | list_bundles ABI implemented | VERIFIED | crates/polyplug/src/runtime.rs:929 - host_list_bundles function exists |
| 8 | get_dependencies ABI implemented | VERIFIED | crates/polyplug/src/runtime.rs:975 - host_get_dependencies uses TLS bundle_id |
| 9 | find_all_by_contract returns Array | VERIFIED | crates/polyplug/src/runtime.rs:725 - Returns Array<PluginHandle> |
| 10 | Loaders updated to use HostInterface | FAILED | polyplug_python and polyplug_dotnet still import RuntimeAbi/RuntimeContext |
| 11 | Tests updated for self-passing pattern | FAILED | Test fixtures still use RuntimeContext parameter |
| 12 | Codegen updated for new calling convention | FAILED | Generators still emit rt_ctx parameter |
| 13 | Workspace compiles | FAILED | polyplug_python (4 errors), polyplug_dotnet (1 error) |
| 14 | Documentation complete | VERIFIED | cargo doc builds without warnings |

**Score:** 8/14 truths verified

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `crates/polyplug_abi/src/host/host_interface.rs` | HostInterface struct | VERIFIED | 88 bytes, runtime at offset 0, 10 function pointers |
| `crates/polyplug_abi/src/host/runtime_interface.rs` | RuntimeInterface struct | VERIFIED | 96 bytes, runtime at offset 0, 11 function pointers |
| `crates/polyplug_abi/src/host/runtime_context.rs` | DELETED | VERIFIED | File does not exist |
| `crates/polyplug_abi/src/host/host_context.rs` | DELETED | VERIFIED | File does not exist |
| `crates/polyplug_abi/src/guest/guest_contract_instance.rs` | 16 bytes with contract_id | VERIFIED | Layout test passes |
| `crates/polyplug_abi/src/types/array.rs` | 24 bytes with align | VERIFIED | Layout test passes |
| `crates/polyplug_abi/src/types/dependency_info.rs` | DependencyInfo struct | VERIFIED | 24 bytes with padding |
| `crates/polyplug_python/src/lib.rs` | Uses HostInterface | FAILED | Still imports RuntimeContext, HostContext, RuntimeAbi |
| `crates/polyplug_dotnet/src/context.rs` | Uses HostInterface | FAILED | Still uses RuntimeAbi |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| HostInterface | Runtime | runtime field at offset 0 | VERIFIED | host_list_bundles extracts runtime from (*this).runtime |
| get_dependencies | TLS bundle_id | get_init_bundle_id() | VERIFIED | runtime.rs:987 - Uses TLS to get caller bundle context |
| polyplug_python | HostInterface | imports | NOT_WIRED | Imports deleted RuntimeContext, HostContext types |
| polyplug_dotnet | HostInterface | imports | NOT_WIRED | Uses RuntimeAbi which was renamed |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| host_list_bundles | manifests | bundle_manifests.lock() | Yes | FLOWING - reads from Runtime.bundle_manifests |
| host_get_dependencies | caller_bundle_id | get_init_bundle_id() | Partial | HOLLOW - TLS set in runtime.rs but loaders don't call set_init_bundle_id |
| host_find_all_by_contract | count | registry.count_by_contract() | Yes | FLOWING - reads from PluginRegistry |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| polyplug_abi tests pass | cargo test -p polyplug_abi --lib | 59 passed | PASS |
| polyplug tests pass | cargo test -p polyplug --lib | 99 passed | PASS |
| polyplug_python builds | cargo build -p polyplug_python | 4 errors | FAIL |
| polyplug_dotnet builds | cargo build -p polyplug_dotnet | 1 error | FAIL |
| polyplug_js builds | cargo build -p polyplug_js | 0 errors | PASS |
| polyplug_native builds | cargo build -p polyplug_native | 0 errors | PASS |
| docs build clean | cargo doc -p polyplug_abi --no-deps | no warnings | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| D-01 | 11-01 | Rename RuntimeAbi to HostInterface | VERIFIED | host_interface.rs exists with HostInterface struct |
| D-02 | 11-01 | Create RuntimeInterface struct | VERIFIED | runtime_interface.rs exists with destroy() function |
| D-03 | 11-02 | Delete RuntimeContext/HostContext | PARTIAL | Types deleted but callers not updated - BREAKS BUILD |
| D-04 | 11-02 | Self-passing pattern for interfaces | VERIFIED | All interface functions take this/self pointer |
| D-05 | 11-03 | Array<T> with align field | VERIFIED | 24 bytes with items, len, align fields |
| D-06 | 11-03 | GuestContractInstance has contract_id | VERIFIED | 16 bytes with data and contract_id fields |
| D-07 | 11-05 | list_bundles introspection API | VERIFIED | host_list_bundles implemented in runtime.rs |
| D-08 | 11-05 | get_dependencies introspection API | VERIFIED | host_get_dependencies uses TLS bundle_id |
| D-09 | 11-03 | DependencyInfo struct | VERIFIED | 24 bytes with contract_id, min_version, bundle_id |
| D-10 | 11-05 | HostInterface introspection | VERIFIED | list_bundles and get_dependencies in HostInterface |
| D-11 | 11-05 | get_dependencies uses TLS | VERIFIED | get_init_bundle_id() called at runtime.rs:987 |
| D-12 | 11-04 | GuestContractInterface uses HostInterface | VERIFIED | create_instance takes *const HostInterface |
| D-13 | 11-04 | HostContractInterface self-passing | VERIFIED | Has runtime field, callbacks take self pointer |
| D-14 | 11-06 | Documentation for interface types | VERIFIED | cargo doc builds without warnings |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| crates/polyplug_python/src/lib.rs | 33 | Import deleted host_context module | Blocker | Build fails |
| crates/polyplug_python/src/lib.rs | 36 | Import deleted RuntimeContext | Blocker | Build fails |
| crates/polyplug_python/src/lib.rs | 120 | Use RuntimeAbi instead of HostInterface | Blocker | Build fails |
| crates/polyplug_dotnet/src/context.rs | 35 | Use RuntimeAbi instead of HostInterface | Blocker | Build fails |

### Human Verification Required

None - all issues are compile-time failures that can be detected programmatically.

### Gaps Summary

**CRITICAL: Phase 11 SUMMARY files claim completion but workspace does not build.**

The core ABI changes (D-01, D-02, D-05, D-06, D-07, D-08, D-09, D-14) were implemented correctly. However, the downstream consumers (loaders, tests, codegen) were NOT updated:

1. **polyplug_python** (4 errors):
   - Imports `polyplug_abi::host::host_context::HostContext` - module deleted
   - Imports `polyplug_abi::RuntimeContext` - type deleted
   - Uses `polyplug_abi::RuntimeAbi` - renamed to HostInterface

2. **polyplug_dotnet** (1 error):
   - Uses `polyplug_abi::RuntimeAbi` - renamed to HostInterface

3. **polyplug_js** (builds OK but tests outdated):
   - Tests still use rt_ctx parameter in polyplug_init signatures

4. **Test fixtures and benchmarks**:
   - 100+ references to RuntimeContext in test callbacks
   - Benchmarks use old rt_ctx parameter

5. **polyplugc codegen**:
   - Python, Lua, C++ generators still emit rt_ctx parameter
   - Should emit HostInterface parameter instead

**The phase was declared complete prematurely.** The types were renamed/deleted, but the callers were never updated, resulting in a broken workspace build.

---

_Verified: 2026-04-07T21:30:00Z_
_Verifier: Claude (gsd-verifier)_