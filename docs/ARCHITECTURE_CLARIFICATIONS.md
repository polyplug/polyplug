# Architecture Clarifications: the instance model

polyplug creates a real instance per `create_instance` call in every language,
and the host owns each instance through an RAII caller wrapper. This page is the
authoritative description of that instance model — what `create_instance`
produces, how the two dispatch families carry per-instance state, and how the
host-side wrappers own it. For the runtime pipelines that move data across the
boundary (load, dispatch, reload, unload), see
[Architecture — runtime pipelines](ARCHITECTURE.md).

See the [glossary](glossary.md) for `GuestContractInterface`, `HostApi`,
*instance*, and *instance payload*.

Interfaces live in `RuntimeStore` as interface slots guarded by a single
`RwLock`; there is no separate slot-wrapper struct around an individual
interface.

## What the instance model guarantees

- **Real instances, every language** — every `create_instance` invokes the
  plugin's author factory and returns a fresh implementation. This holds for
  **native-dispatch** guests (Rust/C++/C#, factory `polyplug_create_<plugin>` /
  `Set<Plugin>Factory`) **and** **VM-dispatch** guests (Python/Lua/JS, factory
  `set_<plugin>_factory` / `set<Plugin>Factory`). The two families carry the
  instance differently — see [the two dispatch families](#the-two-dispatch-families).
- **Per-instance host context** — the `HostApi` pointer is captured at instance
  creation and stored in the instance payload, so every host call routes to the
  runtime that owns it.
- **Caller wrappers** — host-side RAII objects that own exactly one instance
  (`create_instance` in `new()`, `destroy_instance` in `drop()`).
- **Callback-based lifecycle** — the host destroys instances before hot-reload
  completes.

What the model excludes (CLAUDE.md Rule 12):

- **No static implementation storage** — generated code and SDKs hold no
  `OnceLock`, module-level, or class-static slot for the implementation or the
  host pointer. Two `Runtime`s loading the same plugin binary get fully isolated
  instances.
- **No process-wide host pointer** — helpers like `alloc_string` and `log` take
  the host context explicitly (Rust `HostContext`, C++ `const HostApi*`, C#
  `IntPtr`, Python `int`).

## Factory and instance payload

The generated glue declares an author factory and constructs the implementation
per instance:

```rust,ignore
// generated/guest/interfaces.rs (Rust shape; other languages mirror it)
unsafe extern "Rust" {
    fn polyplug_create_validator(host: HostContext) -> Box<dyn PipelineValidatorGuestContract>;
}

struct ValidatorPluginState {
    host: HostContext,                                   // captured at creation
    implementation: Box<dyn PipelineValidatorGuestContract>,
}

unsafe extern "C" fn VALIDATOR_create_instance(
    _loader_data: polyplug_abi::dispatch::VmLoaderData,  // ignored by native; VM loaders use it
    host: *const HostApi,
    _args: *const (),
    out_instance: *mut GuestContractInstance,            // out-param ABI — no return value
) {
    // calls the author factory, boxes the payload, stamps the contract id,
    // and writes the handle through `out_instance`
}
```

Consequences:

- Each `create_instance` produces an independent implementation with its own state.
- The dispatch wrappers read the implementation from `instance.data` (see the two
  families below for how a null/zero instance is handled).
- `destroy_instance` drops the payload exactly once.
- Re-running `polyplug_init` after an in-place binary overwrite re-creates
  instances through the new factory — no stale static survives a reload.

## The two dispatch families

`GuestContractInstance.data` means something different per dispatch family, but
both deliver real per-instance state:

| | Native dispatch (Rust/C++/C#) | VM dispatch (Python/Lua/JS) |
|---|---|---|
| What `instance.data` holds | A raw pointer to the boxed implementation payload | A **non-zero id** into the loader's per-contract instance registry (the impl object itself stays inside the VM) |
| `create_instance` | Calls the author factory, boxes the payload, stamps the contract id | Calls the registered factory inside the VM, mints a non-zero id, stores the impl in the loader's per-contract `instances` map keyed by that id, stamps the handle |
| Dispatch resolves the impl | `&*(instance.data as *const State)` | id 0 → a per-contract `default_impl` built once at load (stateless / low-level paths); non-zero id → the impl in the `instances` map; the resolved impl is passed as the handler's first argument |
| Null / zero `instance.data` | Rejected with `InvalidPointer` | Resolves to the per-contract `default_impl` (a valid stateless instance) |
| `destroy_instance` | Drops the box exactly once | Removes the id from the `instances` map (dropping the VM-side impl) |

The VM family must mint **non-zero** ids because `GuestContractInstance::is_null`
keys on `data` — id 0 is reserved for "the default instance". Ids start at 1.

## Caller wrappers own instances

```rust,ignore
// examples/hosts/rust/generated/host/host_callers.rs
pub struct PipelineValidatorContract {
    interface: *const GuestContractInterface,  // resolved interface pointer
    instance: GuestContractInstance,           // created in new(), destroyed in drop()
    host: *const HostApi,
}
```

The wrapper holds the `*const GuestContractInterface` that
`resolve_guest_contract` returned — a plain raw pointer with no RAII guard around
the interface itself. Its validity is governed by the epoch/quiesce contract (see
[Pipeline 2 — lock-free registry read](ARCHITECTURE.md#pipeline-2--lock-free-registry-read)
and the [*epoch*](glossary.md) / [*unload*](glossary.md) glossary entries): the
stored pointer survives a hot-reload, still serving the version it resolved, and
runtime-mediated calls pin a crossbeam-epoch guard so a concurrent unload cannot
free it mid-dispatch. Using the pointer **after** its owning bundle is unloaded is
undefined behaviour — the host must quiesce before unloading. The instance, by
contrast, is owned by the wrapper: `new()` calls `create_instance`, `drop()` calls
`destroy_instance`.

## Hot-reload coordination

The host destroys all instances when it receives the `Preparing` notification, so
the runtime can swap the interface slot with no live instance pointing at the old
code (the runtime-side mechanics are [Pipeline 6 — hot-reload](ARCHITECTURE.md#pipeline-6--hot-reload)):

```rust,ignore
// Hot-reload: Preparing phase
// Host drops all wrappers (each drop calls destroy_instance)
drop(w1); drop(w2); drop(w3);
```

Notification flow from the instance perspective:

1. Runtime fires `Preparing`.
2. Host drops all wrappers for this bundle (instances destroyed).
3. Runtime swaps the interface.
4. Runtime fires `Reloaded`.
5. Host creates new wrappers — `create_instance` runs against the **new** code, so
   the new factory produces fresh implementations.

## Multiple implementations across bundles

Loading **multiple bundles** that each implement the same contract yields
independent providers (in addition to per-wrapper instances within each):

```rust,ignore
runtime.load_bundle("./plugins/validator_v1")?;
runtime.load_bundle("./plugins/validator_v2")?;

let mut handles = [GuestContractHandle::null(); 16];
let count = runtime.find_all_by_contract(VALIDATOR_ID, 1, &mut handles)?;
// count == 2 (one from each bundle)

let wrapper_v1 = ValidatorContract::new(handles[0], runtime)?;  // → bundle A instance
let wrapper_v2 = ValidatorContract::new(handles[1], runtime)?;  // → bundle B instance
```
