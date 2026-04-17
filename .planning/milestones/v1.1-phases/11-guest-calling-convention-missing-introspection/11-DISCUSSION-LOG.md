# Phase 11: Guest Calling Convention & Missing Introspection - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-07
**Phase:** 11-guest-calling-convention-missing-introspection
**Areas discussed:** Instance Naming, Array ABI Type, Instance-to-Contract Mapping, list_bundles Return, get_dependencies Scope, Storage Model, ABI Size, DependencyInfo Struct, find_all_by_contract

---

## Instance Naming

| Option | Description | Selected |
|--------|-------------|----------|
| Keep Contract prefix | GuestContractInstance/HostContractInstance — already clear about what kind of instance | ✓ |
| Drop Contract prefix | GuestInstance/HostInstance — shorter | |
| Full rename | GuestInstance/HostInstance + Guest/Host for interfaces | |
| Semantic rename | PluginInstance/ServiceInstance — different terms for guest vs host | |

**User's choice:** Keep Contract prefix
**Notes:** User confirmed current naming is clear enough

---

## Array ABI Type

| Option | Description | Selected |
|--------|-------------|----------|
| Generic Array<T> | ptr, len, align. CodeGen handles RAII. T must be #[repr(C)] | ✓ |
| Typed array types | BundleIdArray, ContractIdArray, etc. Clearer per-type semantics | |
| Reuse Buffer | Just use Buffer for everything, lose type safety | |

**User's choice:** Generic Array<T>
**Notes:** CodeGen will handle the heavy lifting. Need Vector<T> for dynamic arrays too.

---

## Array Ownership Model

| Option | Description | Selected |
|--------|-------------|----------|
| Caller frees | Callee allocates via rt_ctx.alloc, caller frees. CodeGen generates RAII wrappers | ✓ |
| Explicit owner field | Array has owner: RuntimeContext. rt_ctx.free_array() knows how to free | |
| Instance-arena allocation | Arrays tied to instance lifetime, freed on destroy | |

**User's choice:** Caller frees with CodeGen RAII
**Notes:** Array/Vector should be usable in guest/host contract signatures. CodeGen handles all ownership complexity.

---

## Instance-to-Contract Mapping

| Option | Description | Selected |
|--------|-------------|----------|
| Option A: Embed in instance | instance.data = {contract_id, state_ptr}. Zero lookup | |
| Option B: Runtime map | HashMap<instance_ptr, contract_id>. Keeps instance opaque | |
| Option C: Add field to struct | contract_id: GuestContractId in GuestContractInstance. Zero lookup, type-safe | ✓ |

**User's choice:** Option C with NewType
**Notes:** Use GuestContractId not raw u64. Instance becomes 16 bytes instead of 8.

---

## list_bundles Return Format

| Option | Description | Selected |
|--------|-------------|----------|
| BundleId only | Array<BundleId> — minimal info. Host queries separately if needed | ✓ |
| Full BundleInfo | Array<BundleInfo> with id, name, version, runtime. StringView ownership tricky | |

**User's choice:** BundleId only
**Notes:** Minimal info sufficient. Host can query individual bundles if needed.

---

## get_dependencies Scope

| Option | Description | Selected |
|--------|-------------|----------|
| Plugin introspection only | RuntimeAbi.get_dependencies(rt_ctx) for plugins | |
| Host introspection only | Runtime method for host | |
| Both APIs | RuntimeAbi for plugins + Runtime method for host | ✓ |

**User's choice:** Both APIs
**Notes:** Plugins need to query their own dependencies. Host also needs to query any bundle's dependencies.

---

## Storage Model

| Option | Description | Selected |
|--------|-------------|----------|
| Runtime stores | Runtime stores manifests, host queries via FFI. Single source of truth | ✓ |
| Host stores | Host keeps manifest copy, no FFI overhead but duplication | |

**User's choice:** Runtime stores
**Notes:** Runtime already has bundle_manifests HashMap. Single source of truth.

---

## ABI Compatibility

| Option | Description | Selected |
|--------|-------------|----------|
| Accept size change | RuntimeAbi grows from 64 to ~88 bytes. Breaking OK per PROJECT.md | ✓ |
| Preserve 64-byte layout | Pack functions, versioned structs. More complex | |

**User's choice:** Accept size change
**Notes:** Breaking changes acceptable — crate not published yet.

---

## DependencyInfo Struct

| Option | Description | Selected |
|--------|-------------|----------|
| Contract ID only | Array<GuestContractId>. Plugin calls find_by_contract for more info | |
| Full info | Array<DependencyInfo> with {contract_id, min_version, bundle_id: Option} | ✓ |

**User's choice:** Full info
**Notes:** Mirrors manifest.toml [[dependency]] structure.

---

## find_all_by_contract

| Option | Description | Selected |
|--------|-------------|----------|
| Change to Array return | find_all_by_contract(...) -> Array<ContractHandle>. Single call | ✓ |
| Keep out-param pattern | Existing signature with out buffer and capacity | |

**User's choice:** Change to Array return
**Notes:** Consistent with new Array pattern. Removes two-call pattern.

---

*Discussion log generated: 2026-04-07*