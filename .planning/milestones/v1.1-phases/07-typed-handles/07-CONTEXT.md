# Phase 7: Typed Handles - Context

**Gathered:** 2026-04-05 (assumptions mode)
**Status:** Ready for planning

<domain>
## Phase Boundary

Replace all `*mut c_void` and `*const c_void` in public ABI with meaningful typed handles. This improves type safety and self-documentation at the FFI boundary without changing runtime behavior.

</domain>

<decisions>
## Implementation Decisions

### Typed Handle Pattern
- **D-01:** Follow existing `GuestContractInstance`/`HostContractInstance` pattern: `#[repr(C)]` struct with single `data: *mut c_void` field
- **D-02:** New handles: `RuntimeContext` (replaces rt_ctx) and `VmLoaderData` (replaces loader_data)
- **D-03:** Handles are opaque — no methods, just type-safe wrappers around raw pointers

### RuntimeContext
- **D-04:** Replaces `rt_ctx: *mut c_void` in all RuntimeAbi function signatures; wraps `*mut HostContext` (the existing opaque struct)
- **D-04a:** Use `#[repr(C)]` for consistency with GuestContractInstance/HostContractInstance pattern (not `#[repr(transparent)]`)
- **D-05:** Internally points to `HostContext` struct (already exists in host/host_context.rs)
- **D-06:** Created by runtime, passed to plugins during init, used for all ABI calls

### VmLoaderData
- **D-07:** Replaces `loader_data: *mut c_void` in VmDispatch struct
- **D-08:** VM-specific state managed by each loader (Python, Lua, JS)
- **D-09:** Opaque to core runtime — loaders know their own state layout

### FFI Impact
- **D-10:** All RuntimeAbi function pointer fields use `RuntimeContext` instead of `*mut c_void`
- **D-11:** GuestContractInterface.create_instance/destroy_instance use `RuntimeContext`
- **D-12:** HostContractInterface.create_instance/destroy_instance use `RuntimeContext`
- **D-13:** VmDispatch.call uses `VmLoaderData` instead of bare pointer

### Claude's Discretion
- Exact internal pointer casting strategy
- Whether to add helper methods for type conversion
- Naming of any conversion traits/functions

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Existing Patterns
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` — Existing opaque handle pattern to follow
- `crates/polyplug_abi/src/host/host_contract_instance.rs` — Another opaque handle example
- `crates/polyplug_abi/src/host/runtime_abi.rs` — All functions that need rt_ctx → RuntimeContext
- `crates/polyplug_abi/src/dispatch/vm_dispatch.rs` — VmDispatch.loader_data → VmLoaderData
- `crates/polyplug_abi/src/host/host_context.rs` — Internal struct that RuntimeContext wraps

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `GuestContractInstance` pattern: `#[repr(C)] pub struct { pub data: *mut c_void }` — copy this for new handles
- `HostContext` struct already exists — RuntimeContext wraps this
- All loaders already have their VM state — VmLoaderData wraps their existing pointers

### Established Patterns
- Opaque handles are always `#[repr(C)]` for FFI stability
- Single `data` field with raw pointer — type safety without overhead
- No methods on opaque handles — just type-safe wrappers

### Integration Points
- RuntimeAbi function signatures (8 functions use rt_ctx)
- VmDispatch struct (loader_data field)
- GuestContractInterface (create_instance, destroy_instance)
- HostContractInterface (create_instance, destroy_instance)
- FFI layer in polyplug crate (ffi.rs functions)

</code_context>

<specifics>
## Specific Ideas

No specific requirements — follow the established opaque handle pattern consistently.

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope.

</deferred>

---
*Phase: 07-typed-handles*
*Context gathered: 2026-04-05*