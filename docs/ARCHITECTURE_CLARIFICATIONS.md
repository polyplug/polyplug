# Architecture Clarifications: Per-Instance Implementations + Caller Wrappers

**Critical clarification about polyplug's instance model.**

## Terminology Note

This document uses the following terminology (current as of the static-free wave):
- **GuestContractInterface**: The interface struct a plugin provides for the host to call
- **HostApi**: The runtime's ABI table provided to guests
- **Instance payload**: The per-instance state carried in `GuestContractInstance.data`

Interfaces are stored in `RuntimeStore` as interface slots guarded by a single `RwLock`. There is no separate slot wrapper struct around individual interfaces.

---

## The Instance Model

### What polyplug HAS

✅ **Real instances** — every `create_instance` call invokes the plugin's author factory
   (`polyplug_create_<plugin>` in Rust/C++, `Set<Plugin>Factory` in C#/Python) and returns a
   fresh implementation carried in `GuestContractInstance.data`
✅ **Per-instance host context** — the `HostApi` pointer is captured at instance creation and
   stored in the instance payload, so every host call routes to the runtime that owns it
✅ **Caller wrappers** — host-side RAII objects that own exactly one instance
   (`create_instance` in `new()`, `destroy_instance` in `drop()`)
✅ **Callback-based lifecycle** — host destroys instances before hot-reload completes

### What polyplug DOESN'T Have

❌ **NO static implementation storage** — generated code and SDKs hold no `OnceLock`,
   module-level, or class-static slot for the implementation or the host pointer
   (CLAUDE.md Rule 12). Two `Runtime`s loading the same plugin binary get fully
   isolated instances.
❌ **NO process-wide host pointer** — helpers like `alloc_string` and `log` take the host
   context explicitly (Rust `HostContext`, C++ `const HostApi*`, C# `IntPtr`, Python `int`)

---

## Architecture Deep Dive

### Plugin Side: Factory + Instance Payload

The generated glue declares an author factory and constructs the implementation per instance:

```rust
// generated/guest/interfaces.rs (Rust shape; other languages mirror it)
unsafe extern "Rust" {
    fn polyplug_create_validator(host: HostContext) -> Box<dyn PipelineValidatorGuestContract>;
}

struct ValidatorPluginState {
    host: HostContext,                                   // captured at creation
    implementation: Box<dyn PipelineValidatorGuestContract>,
}

unsafe extern "C" fn VALIDATOR_create_instance(host: *const HostApi, _args: *const ())
    -> GuestContractInstance
{
    // calls the author factory, boxes the payload, stamps the contract id
}
```

**Consequences:**
- Each `create_instance` produces an independent implementation with its own state
- The dispatch wrappers read the implementation from `instance.data` — a null
  instance is rejected with `InvalidPointer`
- `destroy_instance` drops the payload exactly once
- Re-running `polyplug_init` after an in-place binary overwrite re-creates
  instances through the new factory — no stale static survives a reload

### Host Side: Caller Wrappers Own Instances

```rust
// examples/hosts/rust/generated/host/host_callers.rs
pub struct PipelineValidatorContract {
    interface: *const GuestContractInterface,  // resolved interface pointer
    instance: GuestContractInstance,           // created in new(), destroyed in drop()
    host: *const HostApi,
}
```

The wrapper holds the `*const GuestContractInterface` that `resolve_guest_contract`
returned — there is no RAII guard type around the interface itself. The retire-not-drop
model keeps every resolved interface (and its backing library) alive for the runtime's
lifetime, so the stored pointer stays valid even across a hot-reload. The instance,
by contrast, is owned by the wrapper: `new()` calls `create_instance`, `drop()` calls
`destroy_instance`.

---

## Hot-Reload Implications

### Callback-Based Coordination

The host must destroy all instances when receiving the `Preparing` notification:

```rust
// Hot-reload: Preparing phase
// Host drops all wrappers (each drop calls destroy_instance)
drop(w1); drop(w2); drop(w3);

// Runtime can now safely swap the interface slot via apply_reload_swap
// (under the RuntimeStore RwLock write guard)
```

**Notification flow:**
```
1. Runtime fires Preparing notification
2. Host drops all wrappers for this bundle (instances destroyed)
3. Runtime swaps interface
4. Runtime fires Reloaded notification
5. Host creates new wrappers — create_instance runs against the NEW code,
   so the new factory produces fresh implementations
```

---

## Multiple Implementations Across Bundles

Loading **multiple bundles** that each implement the same contract yields independent
providers (in addition to per-wrapper instances within each):

```rust
runtime.load_bundle("./plugins/validator_v1")?;
runtime.load_bundle("./plugins/validator_v2")?;

let mut handles = [GuestContractHandle::null(); 16];
let count = runtime.find_all_by_contract(VALIDATOR_ID, 1, &mut handles)?;
// count == 2 (one from each bundle)

let wrapper_v1 = ValidatorContract::new(handles[0], runtime)?;  // → bundle A instance
let wrapper_v2 = ValidatorContract::new(handles[1], runtime)?;  // → bundle B instance
```

---

## Terminology Guide

| Term | What It Means | What It DOESN'T Mean |
|------|---------------|---------------------|
| **Caller Wrapper** | Host-side RAII object owning one instance | NOT a shared reference to a singleton |
| **Instance payload** | Factory-created state in `GuestContractInstance.data` | NOT static/global plugin state |
| **Author factory** | `polyplug_create_<plugin>` / `Set<Plugin>Factory` | NOT a registration of a single shared object |
| **Resolved interface pointer** | `*const GuestContractInterface` from `resolve_guest_contract` | NOT instance state |
| **Hot-Reload** | Interface swap via callback coordination | NOT instance migration |

---

## Summary

1. **Plugins export a factory** — the generated `create_instance` calls it per instance
2. **Instances carry their own host context** — no process-wide host pointer exists
3. **Host wrappers own instances** — `new()` creates, `drop()` destroys
4. **Callback coordination** — host destroys instances before hot-reload
5. **Rule 12 holds end to end** — no statics holding runtime or plugin state in any
   SDK or generated file, in any language

**The architecture is: Factory-Created Instances + Callback-Based Coordination.**
