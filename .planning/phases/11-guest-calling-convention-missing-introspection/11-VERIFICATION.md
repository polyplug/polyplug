---
phase: 11-guest-calling-convention-missing-introspection
verified: 2026-04-07T22:55:00Z
status: verified
score: 14/14 requirements verified
overrides_applied: 0
gaps: []
---

# Phase 11: Guest Calling Convention & Missing Introspection Verification Report

**Phase Goal:** Rename `RuntimeAbi` to `HostInterface`, create `RuntimeInterface` for symmetric API, delete `RuntimeContext`/`HostContext` wrappers, rename `call_method` to `call_guest_method`, implement guest-to-guest calls, add introspection ABIs, create `Array<T>` type, update all SDKs and codegen.
**Verified:** 2026-04-07T22:55:00Z
**Status:** verified
**Re-verification:** Yes - gap closure plans 07-09 fixed all issues

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
| 10 | Loaders updated to use HostInterface | VERIFIED | polyplug_python, polyplug_dotnet, polyplug_lua, polyplug_js use HostInterface |
| 11 | Tests updated for self-passing pattern | VERIFIED | Tests and benchmarks use *const HostInterface |
| 12 | Codegen updated for new calling convention | VERIFIED | Generators emit HostInterface parameter |
| 13 | Workspace compiles | VERIFIED | cargo build --workspace succeeds, 353 tests pass |
| 14 | Documentation complete | VERIFIED | cargo doc builds without warnings |

**Score:** 14/14 truths verified

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
| `crates/polyplug_python/src/lib.rs` | Uses HostInterface | VERIFIED | Imports HostInterface, passes to polyplug_init |
| `crates/polyplug_dotnet/src/context.rs` | Uses HostInterface | VERIFIED | Uses HostInterface pointer |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| HostInterface | Runtime | runtime field at offset 0 | VERIFIED | host_list_bundles extracts runtime from (*this).runtime |
| get_dependencies | TLS bundle_id | get_init_bundle_id() | VERIFIED | runtime.rs:987 - Uses TLS to get caller bundle context |
| polyplug_python | HostInterface | imports | VERIFIED | Imports HostInterface, passes to polyplug_init |
| polyplug_dotnet | HostInterface | imports | VERIFIED | Uses HostInterface pointer in context.rs |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
| -------- | ------------- | ------ | ------------------ | ------ |
| host_list_bundles | manifests | bundle_manifests.lock() | Yes | FLOWING - reads from Runtime.bundle_manifests |
| host_get_dependencies | caller_bundle_id | get_init_bundle_id() | VERIFIED | loaders call set_init_bundle_id before init |
| host_find_all_by_contract | count | registry.count_by_contract() | Yes | FLOWING - reads from PluginRegistry |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| polyplug_abi tests pass | cargo test -p polyplug_abi --lib | 59 passed | PASS |
| polyplug tests pass | cargo test -p polyplug --lib | 99 passed | PASS |
| polyplug_python builds | cargo build -p polyplug_python | 0 errors | PASS |
| polyplug_dotnet builds | cargo build -p polyplug_dotnet | 0 errors | PASS |
| polyplug_js builds | cargo build -p polyplug_js | 0 errors | PASS |
| polyplug_native builds | cargo build -p polyplug_native | 0 errors | PASS |
| docs build clean | cargo doc -p polyplug_abi --no-deps | no warnings | PASS |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| D-01 | 11-01 | Rename RuntimeAbi to HostInterface | VERIFIED | host_interface.rs exists with HostInterface struct |
| D-02 | 11-01 | Create RuntimeInterface struct | VERIFIED | runtime_interface.rs exists with destroy() function |
| D-03 | 11-02 | Delete RuntimeContext/HostContext | VERIFIED | Types deleted, all callers updated to HostInterface |
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

None - all previous anti-patterns were resolved by gap closure plans 07-09.

### Human Verification Required

None - all issues resolved programmatically.

### Gaps Summary

**RESOLVED:** All gaps identified in initial verification were fixed by gap closure plans:
- 11-07: Updated VM loaders (polyplug_python, polyplug_dotnet, polyplug_lua, polyplug_js) to use HostInterface
- 11-08: Updated polyplugc codegen to emit HostInterface parameter instead of rt_ctx
- 11-09: Updated tests and benchmarks to use new self-passing pattern

The workspace now compiles successfully with 353 tests passing.

---

_Verified: 2026-04-07T22:55:00Z_
_Verifier: Claude (gsd-verifier)_