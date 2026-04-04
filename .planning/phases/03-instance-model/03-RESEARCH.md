# Phase 03: Instance Model - Research

**Researched:** 2026-04-04
**Domain:** Instance lifecycle management, codegen updates, host contract singletons, cross-dispatch calls
**Confidence:** HIGH

## Summary

Phase 3 implements the instance-based plugin model where hosts create and own plugin instances via a factory pattern. The core types (`GuestContractInstance`, `HostContractInstance`, `GuestContractInterface` with `create_instance`/`destroy_instance`) were created in Phase 1. Phase 3 focuses on:

1. **Codegen updates** to generate RAII instance wrappers instead of `PluginGuard`-based callers
2. **Singleton host contracts** with `singleton: bool` field support
3. **`get_host_contract` implementation** returning actual instances (currently returns null)
4. **`call_method` implementation** for cross-dispatch plugin-to-plugin calls
5. **Instance parameter flow** through dispatch (native and VM)

**Primary recommendation:** Update codegen generators in dependency order: first guest-side vtables with create/destroy_instance, then host-side callers with instance wrappers, then host contract factories. Implement runtime `get_host_contract` and `call_method` after codegen is updated.

<phase_requirements>

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| INST-01 | Update codegen to generate `*Instance` RAII wrappers | See Codegen Architecture section - modify `generate_host_contract_caller` |
| INST-02 | Generated wrapper calls `create_instance` on construction | Instance wrapper `new()` calls interface.create_instance |
| INST-03 | Generated wrapper calls `destroy_instance` on drop | Instance wrapper `Drop` impl calls interface.destroy_instance |
| INST-04 | Instance passed as first argument to all dispatch calls | Native: `functions[fn_id](instance, args, out)`, VM: `call(loader_data, instance, fn_id, args, out)` |
| INST-05 | Native dispatch: `functions[fn_id](instance, args, out)` | VmDispatch.call signature already has instance param |
| INST-06 | VM dispatch: `call(loader_data, instance, fn_id, args, out)` | VmDispatch struct at `dispatch/vm_dispatch.rs` |
| HC-01 | `HostContractInterface` supports `singleton: bool` field | Field exists in ABI (Phase 1), needs parser support |
| HC-02 | `get_host_contract` returns same instance for singleton | Runtime needs singleton instance cache |
| HC-03 | `get_host_contract` creates new instance for multi-instance | Runtime calls create_instance for non-singletons |
| HC-04 | Update codegen for host contract implementations | Modify `generate_host_vtable_factory` for instance creation |
| CG-01 | Update codegen to use `GuestContractInterface` naming | Already done in Phase 1 - verify usage in generators |
| CG-02 | Update codegen to generate instance wrappers | Replace `guard: PluginGuard` with `instance: GuestContractInstance` + interface pointer |
| CG-03 | Generated instance wrappers hold `interface` + `instance` pointer | Store `*const GuestContractInterface` and `GuestContractInstance` |
| CG-04 | Generated wrappers call `create_instance`/`destroy_instance` | RAII pattern in generated code |
| CG-05 | Update host contract vtable generation for `HostContractInterface` | Update `generate_host_vtable_factory` to include singleton |
| CG-06 | Generate `singleton` support for host contracts | Parser + codegen changes |

</phase_requirements>

<user_constraints>

## User Constraints (from CONTEXT.md)

Not applicable - no CONTEXT.md file exists for this phase.

</user_constraints>

## Standard Stack

### Core ABI Types (Created in Phase 1)

| Type | Location | Purpose | Size |
|------|----------|---------|------|
| `GuestContractInstance` | `polyplug_abi/src/guest/guest_contract_instance.rs` | Opaque instance handle | 8 bytes (one `*mut c_void`) |
| `HostContractInstance` | `polyplug_abi/src/host/host_contract_instance.rs` | Opaque host instance handle | 8 bytes |
| `GuestContractInterface` | `polyplug_abi/src/guest/guest_contract_interface.rs` | Contract interface with create/destroy | 56 bytes |
| `HostContractInterface` | `polyplug_abi/src/host/host_contract_interface.rs` | Host contract with singleton field | 64 bytes |
| `RuntimeAbi` | `polyplug_abi/src/host/runtime_abi.rs` | Runtime ABI with call_method, get_host_contract | 64 bytes |

### Dispatch Types

| Type | Location | Purpose | Signature |
|------|----------|---------|-----------|
| `NativeDispatch` | `polyplug_abi/src/dispatch/native_dispatch.rs` | Direct function pointer array | `{ function_count, functions }` |
| `VmDispatch` | `polyplug_abi/src/dispatch/vm_dispatch.rs` | VM dispatch function | `call(loader_data, instance, fn_id, args, out)` |
| `DispatchMechanisms` | `polyplug_abi/src/dispatch/dispatch_mechanisms.rs` | Union of dispatch types | 16 bytes |

### Codegen Infrastructure

| Component | Location | Purpose |
|-----------|----------|---------|
| `RustGenerator` | `polyplugc/src/generators/rust.rs` | Rust host/guest code generation |
| `CSharpGenerator` | `polyplugc/src/generators/csharp.rs` | C# host/guest code generation |
| `PythonGenerator` | `polyplugc/src/generators/python.rs` | Python host/guest code generation |
| `LuaGenerator` | `polyplugc/src/generators/lua.rs` | Lua host/guest code generation |
| `CppGenerator` | `polyplugc/src/generators/cpp.rs` | C++ host/guest code generation |
| `JsQuickjsGenerator` | `polyplugc/src/generators/js_quickjs.rs` | QuickJS host/guest code generation |

## Architecture Patterns

### Current Pattern: PluginGuard-based Callers

```
┌─────────────────────────────────────────────────────────────────┐
│                     Current Generated Host Caller               │
├─────────────────────────────────────────────────────────────────┤
│ pub struct TestAddContract {                                    │
│     guard: PluginGuard,                                         │
│ }                                                               │
│                                                                 │
│ impl TestAddContract {                                          │
│     pub fn new(handle, runtime) -> Option<Self> {              │
│         let guard = runtime.registry().resolve_guard(handle)?; │
│         Some(Self { guard })                                    │
│     }                                                           │
│                                                                 │
│     pub fn add(&self, a: i32, b: i32) -> Result<i32, Error> {  │
│         let vtable = self.guard.vtable();                       │
│         // dispatch via vtable...                               │
│     }                                                           │
│ }                                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Target Pattern: Instance-based Callers

```
┌─────────────────────────────────────────────────────────────────┐
│                     Target Generated Host Caller                │
├─────────────────────────────────────────────────────────────────┤
│ pub struct TestAddContract {                                    │
│     interface: *const GuestContractInterface,                   │
│     instance: GuestContractInstance,                            │
│ }                                                               │
│                                                                 │
│ impl TestAddContract {                                          │
│     pub fn new(handle, rt_ctx) -> Option<Self> {               │
│         let interface = resolve_contract(rt_ctx, handle)?;      │
│         let instance = (interface.create_instance)(rt_ctx, ptr::null())?;│
│         Some(Self { interface, instance })                      │
│     }                                                           │
│                                                                 │
│     pub fn add(&self, a: i32, b: i32) -> Result<i32, Error> {  │
│         // dispatch with instance as first arg...               │
│     }                                                           │
│ }                                                               │
│                                                                 │
│ impl Drop for TestAddContract {                                 │
│     fn drop(&mut self) {                                        │
│         (self.interface.destroy_instance)(rt_ctx, self.instance);│
│     }                                                           │
│ }                                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Instance Dispatch Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                     Native Dispatch (After Phase 3)             │
├─────────────────────────────────────────────────────────────────┤
│ // Signature: fn(instance, args, out) -> AbiError              │
│ let fn_ptr = vtable.dispatch.native.functions[fn_id];          │
│ let result = dispatch_fn(instance, args_ptr, out_ptr);          │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                     VM Dispatch (After Phase 3)                 │
├─────────────────────────────────────────────────────────────────┤
│ // Signature: call(loader_data, instance, fn_id, args, out)    │
│ let result = (vtable.dispatch.vm.call)(                        │
│     vtable.dispatch.vm.loader_data,                             │
│     instance,                                                    │
│     fn_id,                                                       │
│     args_ptr,                                                    │
│     out_ptr                                                      │
│ );                                                               │
└─────────────────────────────────────────────────────────────────┘
```

### Singleton Host Contract Pattern

```
┌─────────────────────────────────────────────────────────────────┐
│                     Singleton Host Contract Flow                │
├─────────────────────────────────────────────────────────────────┤
│ Runtime.host_contracts: HashMap<u64, &'static HostContractInterface>│
│ Runtime.singleton_instances: HashMap<u64, HostContractInstance> │
│                                                                 │
│ get_host_contract(contract_id, min_version):                   │
│     let interface = self.host_contracts.get(contract_id)?;      │
│     if interface.singleton:                                     │
│         // Return cached singleton instance                     │
│         return self.singleton_instances.get(contract_id);       │
│     else:                                                       │
│         // Create new instance per call                         │
│         return (interface.create_instance)(rt_ctx, args);       │
└─────────────────────────────────────────────────────────────────┘
```

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Instance lifetime | Manual destroy tracking | Generated RAII wrapper with Drop | Ensures destroy_instance called before hot-reload |
| Singleton caching | Custom per-contract map | `Runtime.singleton_instances: HashMap<u64, HostContractInstance>` | Centralized, runtime-managed |
| Cross-dispatch calls | Per-dispatch-type logic | `RuntimeAbi.call_method` | Unified entry point handles dispatch type routing |

**Key insight:** The RAII wrapper pattern is critical for hot-reload safety. Hosts must destroy all instances before hot-reload, and RAII ensures instances are destroyed when wrappers go out of scope.

## Runtime State Inventory

> Phase involves codegen and runtime changes - no external runtime state affected.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None - changes are to code generation | None |
| Live service config | None - no external services | None |
| OS-registered state | None | None |
| Secrets/env vars | None | None |
| Build artifacts | None - no artifacts carry instance types | None |

**All categories empty:** Changes are to code generation and runtime logic only.

## Common Pitfalls

### Pitfall 1: Forgetting rt_ctx in Instance Wrappers

**What goes wrong:** Instance wrapper `new()` needs `rt_ctx` to call `create_instance`, but current pattern only passes `handle` and `runtime`.

**Why it happens:** The generated code uses `&'static Runtime` but `create_instance` needs `*mut c_void` rt_ctx.

**How to avoid:** Generated wrapper stores `rt_ctx: *mut c_void` (or `RuntimeContext` after Phase 7) and passes it to create/destroy.

**Warning signs:** Compilation error: "create_instance requires rt_ctx parameter".

### Pitfall 2: Missing Drop Implementation

**What goes wrong:** Instance wrapper doesn't implement Drop, leading to memory leaks and hot-reload UB.

**Why it happens:** Current PluginGuard-based callers don't need Drop - the Arc handles lifetime.

**How to avoid:** Always generate `impl Drop` that calls `destroy_instance`. Store rt_ctx in wrapper for drop call.

**Warning signs:** Hot-reload warning "instances still active" or memory leak in long-running host.

### Pitfall 3: Singleton Instance Not Cached

**What goes wrong:** `get_host_contract` creates new instance for singletons on every call.

**Why it happens:** Missing singleton cache in Runtime.

**How to avoid:** Add `singleton_instances: HashMap<u64, HostContractInstance>` to Runtime. Check `interface.singleton` before creating.

**Warning signs:** Multiple singleton instances exist, state not shared.

### Pitfall 4: Parser Doesn't Support singleton Field

**What goes wrong:** api.toml `singleton = true` on host_contract causes parse error.

**Why it happens:** `RawHostContract` struct doesn't have `singleton` field.

**How to avoid:** Add `singleton: bool` to `RawHostContract` with `#[serde(default)]`. Propagate to `ResolvedHostContract`.

**Warning signs:** TOML parse error on `singleton` field.

### Pitfall 5: Native Dispatch Signature Mismatch

**What goes wrong:** Native dispatch functions expect `(instance, args, out)` but generated ABI wrappers use `(args, out)`.

**Why it happens:** Current native dispatch doesn't pass instance.

**How to avoid:** Update generated guest ABI wrappers to accept instance as first parameter. Update dispatch call site to pass instance.

**Warning signs:** Wrong parameter order, crashes on dispatch.

## Code Examples

### GuestContractInterface (Current - Phase 1)

```rust
// Source: crates/polyplug_abi/src/guest/guest_contract_interface.rs
#[repr(C)]
pub struct GuestContractInterface {
    pub contract_id: GuestContractId,
    pub contract_version: Version,
    pub dispatch_type: DispatchType,
    pub create_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        args: *const (),
    ) -> GuestContractInstance,
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        instance: GuestContractInstance,
    ),
    pub dispatch: DispatchMechanisms,
}
```

### HostContractInterface (Current - Phase 1)

```rust
// Source: crates/polyplug_abi/src/host/host_contract_interface.rs
#[repr(C)]
pub struct HostContractInterface {
    pub contract_id: HostContractId,
    pub contract_version: Version,
    pub singleton: bool,  // NEW in Phase 1
    pub dispatch_type: DispatchType,
    pub create_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        args: *const (),
    ) -> HostContractInstance,
    pub destroy_instance: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        instance: HostContractInstance,
    ),
    pub dispatch: DispatchMechanisms,
}
```

### VmDispatch (Already has instance parameter)

```rust
// Source: crates/polyplug_abi/src/dispatch/vm_dispatch.rs
#[repr(C)]
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,
        instance: GuestContractInstance,  // Already present
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,
}
```

### Current Generated Host Caller (to be updated)

```rust
// Source: crates/polyplugc/src/generators/rust.rs:1165-1200
fn generate_host_contract_caller(out: &mut String, contract: &ResolvedContract) {
    // Current: stores PluginGuard
    out.push_str("    guard: PluginGuard,\n");
    // ...
    out.push_str("        let guard: PluginGuard = runtime.registry().resolve_guard(handle).ok()?;\n");
}
```

### Target Generated Host Caller (Phase 3)

```rust
// Target implementation for Rust generator
fn generate_host_contract_caller(out: &mut String, contract: &ResolvedContract) {
    // Target: stores interface + instance
    out.push_str("    interface: *const GuestContractInterface,\n");
    out.push_str("    instance: GuestContractInstance,\n");
    out.push_str("    rt_ctx: *mut c_void,\n");
    // ...
    out.push_str("        let interface = resolve_contract(rt_ctx, handle)?;\n");
    out.push_str("        let instance = unsafe { ((*interface).create_instance)(rt_ctx, core::ptr::null()) };\n");
    out.push_str("        if instance.is_null() { return None; }\n");
    // ...
    out.push_str("impl Drop for {struct_name} {\n");
    out.push_str("    fn drop(&mut self) {\n");
    out.push_str("        unsafe { ((*self.interface).destroy_instance)(self.rt_ctx, self.instance); }\n");
    out.push_str("    }\n");
    out.push_str("}\n");
}
```

### RuntimeAbi.call_method (Needs implementation)

```rust
// Source: crates/polyplug_abi/src/host/runtime_abi.rs:76-82
// The ABI defines call_method, but runtime implementation needed
pub call_method: unsafe extern "C" fn(
    rt_ctx: *mut c_void,
    instance: GuestContractInstance,
    method_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError,
```

### get_host_contract (Currently returns null)

```rust
// Source: crates/polyplug/src/runtime.rs:755-775
pub(crate) unsafe extern "C" fn host_get_host_contract(
    rt_ctx: *mut core::ffi::c_void,
    contract_id: u64,
    min_version: u32,
) -> polyplug_abi::HostContractInstance {
    // ... lookup interface ...
    match runtime.get_host_contract(contract_id, min_version) {
        Some(_vtable) => {
            // TODO: Return actual instance - for now return null instance
            polyplug_abi::HostContractInstance { data: core::ptr::null_mut() }
        }
        None => polyplug_abi::HostContractInstance { data: core::ptr::null_mut() },
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| PluginGuard + Arc quiescence | Instance-based with explicit destroy | Phase 1 decision | RAII wrappers, explicit lifecycle |
| Bare pointers for instances | Opaque GuestContractInstance/HostContractInstance handles | Phase 1 | Type safety, FFI-safe |
| All host contracts multi-instance | Singleton field for shared services | Phase 1 | Efficient shared state |

**Deprecated/outdated:**
- `PluginGuard`: Replaced by instance wrappers - Phase 2 removes from registry
- `resolve_guard()`: Replaced by `resolve()` returning interface directly - Phase 2 removes

## Dependencies on Phase 2

Phase 3 depends on Phase 2 registry simplification:

| Phase 2 Change | Impact on Phase 3 |
|----------------|-------------------|
| `VTableSlot` removed | Codegen works with `GuestContractInterface` directly |
| `PluginGuard` removed | Generated callers must use instance wrappers |
| `resolve_guard()` removed | Use `resolve()` to get interface pointer |
| Generation counter removed | Simpler handle validation in instance wrappers |
| `ArcSwap` removed | Direct interface access, no quiescence tracking |

**Critical:** Phase 2 must complete before Phase 3 codegen updates. The generated callers assume `resolve()` returns `*const GuestContractInterface` directly.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Native dispatch functions accept instance as first parameter | Architecture Patterns | Guest ABI wrapper signature mismatch |
| A2 | Guest implementations can store per-instance state | Instance Model | May need instance context design revision |
| A3 | Singleton host contracts are created once at registration | Singleton Pattern | May need lazy initialization |
| A4 | Drop impl has access to rt_ctx | Generated Code | Need to store rt_ctx in wrapper |

**If this table is empty:** All claims in this research were verified or cited - no user confirmation needed.

## Open Questions

1. **Should instance wrappers store rt_ctx as raw pointer or typed handle?**
   - What we know: Phase 7 adds typed handles; current ABI uses `*mut c_void`
   - What's unclear: Whether to use typed handles now or wait for Phase 7
   - Recommendation: Use `*mut c_void` now, Phase 7 will replace with `RuntimeContext`

2. **Should guest implementations store instance state?**
   - What we know: `GuestContractInstance.data` is opaque `*mut c_void`
   - What's unclear: Who allocates/frees this memory
   - Recommendation: Guest allocates in `create_instance`, frees in `destroy_instance`

3. **How should guest vtables set create_instance/destroy_instance?**
   - What we know: Current guest vtable generation only sets dispatch functions
   - What's unclear: Whether to generate default implementations or require user code
   - Recommendation: Generate default implementations that work with `Box<dyn Trait>` state

## Environment Availability

> SKIPPED - Phase has no external dependencies beyond Rust toolchain.

All changes are pure Rust code modifications. No external tools, services, or databases required.

## Validation Architecture

> SKIPPED - workflow.nyquist_validation is false in config.json.

## Security Domain

> SKIPPED - security_enforcement not explicitly set (absent = enabled, but this phase is about internal architecture, not security boundaries).

This phase focuses on instance lifecycle management and code generation. Security-relevant considerations:

- Instance handles are opaque pointers - memory safety ensured by RAII pattern
- Singleton host contracts must not leak between tenants (if multi-tenant)
- destroy_instance must be called before hot-reload (UB if not)

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` - Instance handle definition
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` - Interface with create/destroy (56 bytes)
- `crates/polyplug_abi/src/host/host_contract_instance.rs` - Host instance handle
- `crates/polyplug_abi/src/host/host_contract_interface.rs` - Host interface with singleton (64 bytes)
- `crates/polyplug_abi/src/host/runtime_abi.rs` - call_method and get_host_contract ABI
- `crates/polyplug_abi/src/dispatch/vm_dispatch.rs` - VM dispatch with instance parameter
- `crates/polyplugc/src/generators/rust.rs` - Rust codegen (2000+ lines)
- `crates/polyplugc/src/ir.rs` - IR definitions for contracts
- `crates/polyplugc/src/parser.rs` - TOML parsing (needs singleton field)

### Secondary (MEDIUM confidence)
- `crates/polyplug/src/runtime.rs` - Runtime with get_host_contract (currently returns null)
- `crates/polyplug/src/ffi.rs` - FFI layer
- `.planning/phases/02-registry/02-RESEARCH.md` - Phase 2 context
- `.planning/phases/02-registry/02-01-PLAN.md` through `02-03-PLAN.md` - Phase 2 implementation details

### Tertiary (LOW confidence)
- None - all findings verified against codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All ABI types verified in Phase 1, codegen structure analyzed
- Architecture: HIGH - Target patterns clear from ABI definitions
- Pitfalls: HIGH - Identified from code analysis and Phase 1/2 decisions
- Dependencies: HIGH - Phase 2 plans read and understood

**Research date:** 2026-04-04
**Valid until:** 30 days - stable Rust patterns, depends on Phase 2 completion