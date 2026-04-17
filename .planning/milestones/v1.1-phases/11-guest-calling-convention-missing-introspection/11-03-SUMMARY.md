---
phase: 11-guest-calling-convention-missing-introspection
plan: 03
wave: 3
status: completed
completed: 2026-04-07T17:49:19+02:00
commit: 98d10df
requirements: [D-05, D-06, D-10]
---

## Summary: Enhanced Array<T>, GuestContractInstance, DependencyInfo

**Objective:** Enhance Array<T> with alignment tracking, add contract_id to GuestContractInstance, create DependencyInfo struct.

**Result:** All three type enhancements completed with layout tests passing.

### Tasks Completed

| Task | Status | Detail |
|------|--------|---------|
| 1: Enhance Array<T> | ✓ | Added items: *mut T, len: usize, align: usize (24 bytes) |
| 2: Add contract_id to GuestContractInstance | ✓ | Size changed from 8 to 16 bytes |
| 3: Create DependencyInfo struct | ✓ | 24 bytes with padding (contract_id, min_version, bundle_id) |

### Key Files Modified

| File | Changes |
|------|---------|
| `crates/polyplug_abi/src/types/array.rs` | Enhanced Array<T> with align field, ownership docs |
| `crates/polyplug_abi/src/guest/guest_contract_instance.rs` | Added contract_id field (16 bytes) |
| `crates/polyplug_abi/src/types/dependency_info.rs` | NEW - DependencyInfo struct for introspection |
| `crates/polyplug_abi/src/types/mod.rs` | Added dependency_info module |
| `crates/polyplug_abi/src/lib.rs` | Exported DependencyInfo |

### Verification Results

```
cargo test -p polyplug_abi -- layout_array: 1 passed
cargo test -p polyplug_abi -- layout_guest_contract_instance: 1 passed
cargo test -p polyplug_abi -- layout_dependency_info: 1 passed
```

### Must-Haves Verified

- [x] Array<T> struct has align: usize field for proper caller-frees semantics
- [x] GuestContractInstance struct has contract_id: GuestContractId field (16 bytes total)
- [x] DependencyInfo struct exists with contract_id, min_version, bundle_id fields

### Deviations

None. All requirements met per specification.

### Next Wave

Wave 4 (11-04) will update GuestContractInterface and HostContractInterface to use the self-passing pattern with HostInterface parameter.