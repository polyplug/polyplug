---
plan: 07-03
phase: 07-typed-handles
status: completed
commit: 982435f
tasks_completed: 4
tasks_total: 4
---

# Plan 07-03: Update GuestContractInterface, HostContractInterface, VmDispatch

## Summary

Updated three ABI structs to use typed handles:
- GuestContractInterface: `create_instance` and `destroy_instance` use RuntimeContext
- HostContractInterface: `create_instance` and `destroy_instance` use RuntimeContext
- VmDispatch: `call` parameter and `loader_data` field use VmLoaderData

## Files Modified

| File | Change |
|------|--------|
| `crates/polyplug_abi/src/guest/guest_contract_interface.rs` | RuntimeContext in signatures |
| `crates/polyplug_abi/src/host/host_contract_interface.rs` | RuntimeContext in signatures |
| `crates/polyplug_abi/src/dispatch/vm_dispatch.rs` | VmLoaderData in call and field |
| `crates/polyplug/src/runtime.rs` | Test stubs updated |
| `crates/polyplug/src/registry/plugin_registry.rs` | noop functions updated |

## Verification

- `cargo build -p polyplug_abi` ✓
- `cargo build -p polyplug` ✓

## Deviations

None.