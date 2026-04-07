# Phase 11: Guest Calling Convention & Missing Introspection - Context

**Gathered:** 2026-04-07
**Status:** Ready for planning

<domain>
## Phase Boundary

Rename `call_method` to `call_guest_method`, implement guest-to-guest calls with instance-to-contract mapping, add introspection ABIs (`list_bundles`, `get_dependencies`), create `Array<T>` and `Vector<T>` ABI types for FFI, update all SDKs and codegen to support these types in contract signatures.

</domain>

<decisions>
## Implementation Decisions

### Instance Naming
- **D-01:** Keep `GuestContractInstance`/`HostContractInstance` naming — Contract prefix clarifies what kind of instance. No change needed.

### Array/Vector ABI Types
- **D-02:** Generic `Array<T>` for FFI with caller-frees ownership model
  - `Array<T> = { ptr: *mut T, len: usize, align: usize }`
  - Allocated via `rt_ctx.alloc(len * sizeof(T), align)`
  - Freed via `rt_ctx.free(ptr, len * sizeof(T), align)`
  - CodeGen generates RAII wrappers (Rust `Drop`, Python `__del__`, C# `IDisposable`)
  - Support in both guest and host contract function signatures
- `Vector<T>` for dynamic arrays with push/pop (same ownership model, adds `cap` field)

### Instance-to-Contract Mapping
- **D-03:** Add `contract_id: GuestContractId` field to `GuestContractInstance` struct
  - Changes from 8 bytes to 16 bytes (ptr + GuestContractId)
  - Zero lookup overhead for `call_guest_method` dispatch
  - Clear ownership, type-safe contract ID

### list_bundles ABI
- **D-04:** `list_bundles(rt_ctx: RuntimeContext) -> Array<BundleId>`
  - Returns just BundleId (u64) — minimal info
  - Host can query individual bundles if needed via other APIs

### get_dependencies ABI
- **D-05:** Two APIs:
  - **RuntimeAbi.get_dependencies(rt_ctx)** — For plugins to query their own declared dependencies
  - **Runtime.list_bundle_dependencies(bundle_id)** — For host to query any bundle's dependencies (direct Rust method, not in RuntimeAbi)

### Storage Model
- Runtime stores bundle manifests in `bundle_manifests: Mutex<HashMap<String, ManifestData>>`
- Host queries via FFI introspection APIs
- Single source of truth, no duplication

### ABI Compatibility
- **D-06:** Accept RuntimeAbi size change (64 → ~88+ bytes)
  - Breaking changes acceptable per PROJECT.md (not published yet)
  - Plugins compile against SDK which handles struct size

### DependencyInfo Struct
- **D-07:** `DependencyInfo = { contract_id: GuestContractId, min_version: u32, bundle_id: Option<BundleId> }`
  - Mirrors `manifest.toml` `[[dependency]]` structure
  - `get_dependencies` returns `Array<DependencyInfo>`

### find_all_by_contract Update
- **D-08:** Change signature to `find_all_by_contract(rt_ctx, contract_id, min_version) -> Array<ContractHandle>`
  - Replaces out-param pattern
  - Single call, no capacity guessing
  - Consistent with new Array pattern

### Claude's Discretion
- Exact layout of `Array<T>` and `Vector<T>` structs
- Naming of free functions (`rt_ctx.free_array()` vs direct `rt_ctx.free()`)
- Whether to add helper methods to Array/Vector types
- Error handling for allocation failures

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Core ABI Files
- `crates/polyplug_abi/src/host/runtime_abi.rs` — RuntimeAbi struct, all functions to update
- `crates/polyplug_abi/src/types/buffer.rs` — Existing Buffer pattern to reference
- `crates/polyplug_abi/src/types/string_view.rs` — StringView pattern for reference
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` — GuestContractInstance to add contract_id field

### Runtime Implementation
- `crates/polyplug/src/runtime.rs` — host_call_method placeholder, bundle_manifests storage
- `crates/polyplug/src/registry/plugin_registry.rs` — RegistryEntry, contract tracking
- `crates/polyplug/src/loader/manifest.rs` — ManifestData, ManifestDependency types

### CodeGen
- `crates/polyplugc/src/generators/rust.rs` — Rust codegen for Array/Vector support
- `crates/polyplugc/src/generators/python.rs` — Python SDK generation
- `crates/polyplugc/src/generators/csharp.rs` — C# SDK generation
- `crates/polyplugc/src/generators/lua.rs` — Lua SDK generation
- `crates/polyplugc/src/generators/cpp.rs` — C++ SDK generation

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `Buffer` struct pattern: `{ ptr, len, cap }` — can reference for Array/Vector design
- `StringView` struct pattern: `{ ptr, len }` — non-owning view pattern
- `GuestContractInstance` struct: `{ data: *mut c_void }` — add contract_id field
- `bundle_manifests` HashMap in Runtime — already stores manifest data including dependencies

### Established Patterns
- Host allocator for all FFI memory: `rt_ctx.alloc/free`
- `#[repr(C)]` for all FFI types
- Opaque handles with typed wrappers
- CodeGen generates language-specific RAII wrappers

### Integration Points
- RuntimeAbi function signatures (add 3 new functions, rename 1)
- GuestContractInstance struct (add contract_id field)
- PluginContext (bundle_id for get_dependencies)
- CodeGen templates (Array/Vector support in contract signatures)
- All 5 SDKs (Rust, Python, C#, Lua, JS)

</code_context>

<specifics>
## Specific Ideas

- `call_guest_method` naming clarifies that guests (plugins) call other guests
- Array/Vector types should be usable in ANY contract function signature, not just introspection
- `find_all_by_contract` update removes the two-call pattern (call to get count, call with buffer)

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Context gathered: 2026-04-07*