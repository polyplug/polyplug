---
phase: 11-guest-calling-convention-missing-introspection
plan: 04
wave: 4
status: completed
completed: 2026-04-07T18:30:00+02:00
commit: 3717702
requirements: [D-12, D-13]
---

## Summary: Interface Callback Updates

**Objective:** Update GuestContractInterface and HostContractInterface to use self-passing pattern and HostInterface parameter.

**Result:** GuestContractInterface updated; HostContractInterface already had the correct structure.

### Tasks Completed

| Task | Status | Detail |
|------|--------|---------|
| 1: Update GuestContractInterface | ✓ | create/destroy_instance now take *const HostInterface |
| 2: Update HostContractInterface | ✓ | Already had runtime field and self-passing pattern |
| 3: Update registry test callbacks | ✓ | noop_create_instance/noop_destroy_instance updated |

### Key Files Modified

| File | Changes |
|------|---------|
| `crates/polyplug_abi/src/guest/guest_contract_interface.rs` | Updated create/destroy_instance signatures, added compile test |
| `crates/polyplug/src/runtime.rs` | Updated stub_create_instance/stub_destroy_instance |
| `crates/polyplug/src/registry/plugin_registry.rs` | Updated noop_create_instance/noop_destroy_instance |
| `crates/polyplug_abi/src/dispatch/vm_dispatch.rs` | Fixed test for 16-byte GuestContractInstance |

### Verification Results

```
cargo test -p polyplug_abi: 57 passed
cargo test -p polyplug --lib: 99 passed
cargo build -p polyplug: success
```

### Must-Haves Verified

- [x] GuestContractInterface create_instance/destroy_instance take *const HostInterface parameter
- [x] HostContractInterface has runtime: *mut c_void field
- [x] HostContractInterface create_instance/destroy_instance take self pointer parameter

### Deviations

None. HostContractInterface already had the correct structure from earlier phases.

### Next Wave

Wave 5 (11-05) will add introspection ABIs (list_bundles, get_dependencies) to HostInterface and RuntimeInterface, and change find_all_by_contract to return Array<ContractHandle>.