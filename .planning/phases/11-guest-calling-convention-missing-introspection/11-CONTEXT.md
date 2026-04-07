# Phase 11: Guest Calling Convention & Missing Introspection - Context

**Gathered:** 2026-04-07
**Status:** Ready for planning

<domain>
## Phase Boundary

Rename `call_method` to `call_guest_method`, implement guest-to-guest calls with instance-to-contract mapping, add introspection ABIs (`list_bundles`, `get_dependencies`), create `Array<T>` and `Vector<T>` ABI types for FFI, rename `RuntimeAbi` to `HostInterface`, create `RuntimeInterface` for symmetric host API, delete `RuntimeContext` and `HostContext` wrappers, update all SDKs and codegen.

</domain>

<decisions>
## Implementation Decisions

### Interface Naming (Symmetric Design)
- **D-01:** Rename `RuntimeAbi` → `HostInterface`
  - Consistent with `GuestContractInterface`/`HostContractInterface` pattern
  - Clear: runtime provides, guest calls
- **D-02:** Create `RuntimeInterface` struct
  - Symmetric with `HostInterface`
  - Returned from `polyplug_runtime_create()`
  - Contains function pointers for host to call runtime
  - Replaces scattered `polyplug_runtime_*` FFI functions

### Symmetric Interface Architecture
```
| Interface              | Provided by | Called by |
|------------------------|-------------|-----------|
| RuntimeInterface       | Runtime     | Host      |
| HostInterface          | Runtime     | Guest     |
| GuestContractInterface | Guest       | Host      |
| HostContractInterface  | Host        | Guest     |
```

### Delete Wrapper Types
- **D-03:** Delete `RuntimeContext` and `HostContext`
  - These were indirection layers that add confusion
  - Interfaces now directly contain `runtime: *mut c_void` opaque pointer
  - No more `rt_ctx` parameter — functions take `self_ptr: *const Interface` instead
  - SDKs hide the `self_ptr` passing from users

### Interface Structures
```rust
// RuntimeInterface - returned to host from polyplug_runtime_create()
#[repr(C)]
pub struct RuntimeInterface {
    runtime: *mut c_void,  // Opaque pointer to Runtime
    load_bundle: unsafe extern "C" fn(self: *const RuntimeInterface, path: *const c_char) -> AbiError,
    find_by_contract: unsafe extern "C" fn(self: *const RuntimeInterface, contract_id: u64, min_version: u32) -> ContractHandle,
    destroy: unsafe extern "C" fn(self: *const RuntimeInterface),
    // ...
}

// HostInterface - passed to guest during polyplug_init()
#[repr(C)]
pub struct HostInterface {
    runtime: *mut c_void,  // Same opaque pointer
    register_contract: unsafe extern "C" fn(self: *const HostInterface, descriptor: *const PluginDescriptor, interface: *const GuestContractInterface) -> AbiError,
    find_by_contract: unsafe extern "C" fn(self: *const HostInterface, contract_id: u64, min_version: u32) -> ContractHandle,
    alloc: unsafe extern "C" fn(self: *const HostInterface, size: usize, align: usize) -> *mut u8,
    free: unsafe extern "C" fn(self: *const HostInterface, ptr: *mut u8, size: usize, align: usize),
    // ...
}
```

### Function Call Pattern
```c
// ABI level (what function pointers look like):
ContractHandle handle = host->find_by_contract(host, contract_id, version);

// SDK level (what users actually call):
ContractHandle handle = host.find_by_contract(contract_id, version);  // SDK passes self
```

### RuntimeInterface Functions
- `load_bundle(self, path) -> AbiError`
- `reload_bundle(self, bundle_id) -> AbiError`
- `unload_bundle(self, bundle_id) -> AbiError`
- `find_by_contract(self, contract_id, min_version) -> ContractHandle`
- `find_all_by_contract(self, contract_id, min_version) -> Array<ContractHandle>`
- `resolve_contract(self, handle) -> *const GuestContractInterface`
- `get_host_contract(self, contract_id, min_version) -> HostContractInstance`
- `get_last_error(self) -> StringView`
- `destroy(self)` — destroys runtime and frees interface

### HostInterface Functions
- `register_contract(self, descriptor, interface) -> AbiError`
- `alloc(self, size, align) -> *mut u8`
- `free(self, ptr, size, align)`
- `find_by_contract(self, contract_id, min_version) -> ContractHandle`
- `find_all_by_contract(self, contract_id, min_version) -> Array<ContractHandle>`
- `resolve_contract(self, handle) -> *const GuestContractInterface`
- `call_guest_method(self, instance, method_id, args, out) -> AbiError` — RENAMED from call_method
- `get_host_contract(self, contract_id, min_version) -> HostContractInstance`
- `list_bundles(self) -> Array<BundleId>` — NEW
- `get_dependencies(self) -> Array<DependencyInfo>` — NEW

### Instance Naming
- **D-04:** Keep `GuestContractInstance`/`HostContractInstance` naming — Contract prefix clarifies what kind of instance. No change needed.

### Array/Vector ABI Types
- **D-05:** Generic `Array<T>` for FFI with caller-frees ownership model
  - `Array<T> = { ptr: *mut T, len: usize, align: usize }`
  - Allocated via `host->alloc(self, len * sizeof(T), align)`
  - Freed via `host->free(self, ptr, len * sizeof(T), align)`
  - CodeGen generates RAII wrappers (Rust `Drop`, Python `__del__`, C# `IDisposable`)
  - Support in both guest and host contract function signatures
- `Vector<T>` for dynamic arrays with push/pop (same ownership model, adds `cap` field)

### Instance-to-Contract Mapping
- **D-06:** Add `contract_id: GuestContractId` field to `GuestContractInstance` struct
  - Changes from 8 bytes to 16 bytes (ptr + GuestContractId)
  - Zero lookup overhead for `call_guest_method` dispatch
  - Clear ownership, type-safe contract ID

### list_bundles ABI
- **D-07:** `list_bundles(self: *const HostInterface) -> Array<BundleId>`
  - Returns just BundleId (u64) — minimal info
  - Host can query individual bundles if needed via other APIs

### get_dependencies ABI
- **D-08:** `get_dependencies(self: *const HostInterface) -> Array<DependencyInfo>`
  - For plugins to query their own declared dependencies
  - PluginContext still has bundle_id, implementation uses it to look up deps

### Storage Model
- Runtime stores bundle manifests in `bundle_manifests: Mutex<HashMap<String, ManifestData>>`
- Introspection APIs query from runtime
- Single source of truth, no duplication

### ABI Compatibility
- **D-09:** Accept interface size changes
  - Breaking changes acceptable per PROJECT.md (not published yet)
  - Plugins compile against SDK which handles struct size

### DependencyInfo Struct
- **D-10:** `DependencyInfo = { contract_id: GuestContractId, min_version: u32, bundle_id: Option<BundleId> }`
  - Mirrors `manifest.toml` `[[dependency]]` structure
  - `get_dependencies` returns `Array<DependencyInfo>`

### find_all_by_contract Update
- **D-11:** Change signature to `find_all_by_contract(self, contract_id, min_version) -> Array<ContractHandle>`
  - Replaces out-param pattern
  - Single call, no capacity guessing
  - Consistent with new Array pattern

### Files to Delete
- `crates/polyplug_abi/src/host/runtime_context.rs`
- `crates/polyplug/src/host/host_context.rs`

### Claude's Discretion
- Exact layout of `Array<T>` and `Vector<T>` structs
- Whether to add helper methods to Array/Vector types
- Error handling for allocation failures
- Exact function list in RuntimeInterface

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Core ABI Files
- `crates/polyplug_abi/src/host/runtime_abi.rs` — RuntimeAbi → HostInterface
- `crates/polyplug_abi/src/host/runtime_context.rs` — DELETE
- `crates/polyplug_abi/src/types/buffer.rs` — Existing Buffer pattern to reference
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` — Add contract_id field

### Runtime Implementation
- `crates/polyplug/src/runtime.rs` — Main runtime, interface creation
- `crates/polyplug/src/host/host_context.rs` — DELETE
- `crates/polyplug/src/ffi.rs` — FFI functions to restructure
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
- `GuestContractInstance` struct: `{ data: *mut c_void }` — add contract_id field
- `bundle_manifests` HashMap in Runtime — already stores manifest data

### Established Patterns
- Interface struct with function pointers (GuestContractInterface, HostContractInterface)
- `#[repr(C)]` for all FFI types
- Opaque `*mut c_void` for internal pointers in ABI structs
- CodeGen generates language-specific RAII wrappers
- SDK hides self-pointer passing from users

### Integration Points
- HostInterface (was RuntimeAbi) — rename, add functions, change signatures
- RuntimeInterface — NEW struct
- GuestContractInstance — add contract_id field
- Delete RuntimeContext, HostContext
- All 5 SDKs (Rust, Python, C#, Lua, JS)
- `polyplug_runtime_create()` — returns `*const RuntimeInterface`

</code_context>

<specifics>
## Specific Ideas

- `call_guest_method` naming clarifies that guests (plugins) call other guests
- Array/Vector types should be usable in ANY contract function signature, not just introspection
- Symmetric interface naming: RuntimeInterface (host calls), HostInterface (guest calls)
- No wrapper types — interfaces directly contain opaque runtime pointer
- SDKs provide clean API: `host.find_by_contract(id, ver)` not `host->find_by_contract(host, id, ver)`

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---
*Phase: 11-guest-calling-convention-missing-introspection*
*Context gathered: 2026-04-07*