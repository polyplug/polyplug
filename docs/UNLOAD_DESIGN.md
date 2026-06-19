# True Unload Design — Generation-Counted Handles + Epoch Reclamation

## Overview

polyplug supports **true unload**: `unload_bundle` fully reclaims a loaded bundle's
resources — the registered `GuestContractInterface` `Arc`, the native dylib mapping or
per-bundle VM state, and the bundle's registry indices — at runtime, with no dangling
pointer and no in-flight call left holding freed memory.

Two mechanisms cooperate:

- **Generation-counted handles.** `GuestContractHandle` carries a `generation` field.
  `resolve_guest_contract` compares it against the slot generation; a handle minted
  before its slot was unloaded resolves to `AbiErrorCode::StaleHandle`. Unloading a
  bundle bumps the generation of every slot it owns, atomically invalidating every
  outstanding handle for those slots.
- **Epoch reclamation.** Reads are lock-free and reclamation is deferred through
  `crossbeam-epoch`: the superseded interface `Arc` and the underlying dylib mapping /
  VM state are freed only once no reader is still pinned in the epoch that preceded the
  unload.

`unload_bundle` always reclaims when it is safe to do so — there is no opt-in tier and
no separate "keep it mapped" path. The memory is released as soon as the epoch advances
past every reader that was pinned before the unload landed.

This document mirrors the tone and structure of
[`HOT_RELOAD_DESIGN.md`](./HOT_RELOAD_DESIGN.md).

---

## Memory Model — Why Arena and Unload Are Orthogonal

The recurring question "should the call arena be instance-scoped and freed on unload?"
conflates two independent memory regimes. Every byte polyplug touches belongs to one of
five classes:

| Class | What | Allocated by | Freed by | Reclaimed when | Leaks? |
|---|---|---|---|---|---|
| A — Args | host→guest call inputs | host (stack/buffer) | host | call returns | no |
| B — Returns | guest→host outputs (string/array) | native: none (borrowed view); VM: call arena or `host->alloc` fallback | arena reset / host | next call (arena) | no |
| C — Instance data | per-instance state | native: guest's own allocator (`Box`/`new`); VM: inside the VM | native: `destroy_instance`; VM: VM drop | instance destroyed | only if host skips `destroy_instance` |
| D — Interface vtables | registered `GuestContractInterface` | runtime (`Arc`) | runtime (epoch-deferred on unload) | epoch advances past pinned readers | no |
| E — Dylib mappings / VM state | native code pages; per-bundle VM | `dlopen` / loader | OS / loader (epoch-deferred on unload) | epoch advances past pinned readers | no |

The **call arena lives entirely in class B**. The **unload problem lives entirely in
classes D + E**. They never compose, because you cannot bump-allocate a code mapping or
a registered vtable. No arena of any scope — call-scoped, instance-scoped, or
runtime-scoped — addresses unload.

This follows from facts about the code:

- `HostApi` carries `create_guest_instance` (@160) and `destroy_guest_instance` (@168),
  so the runtime **mediates** instance lifecycle and can attribute each instance to its
  contract. The `create_instance(loader_data, host, args, out_instance)` ABI on
  `GuestContractInterface` is not self-accounting — calling the fn-ptr directly neither
  pins the unload epoch nor updates the runtime's per-contract live-instance counter —
  which is exactly why instance create/destroy is host-mediated.
- Native instance data uses **the guest's own allocator** (`Box` / `new` / `malloc`
  inside the guest dylib), not `host->alloc`. It is outside the runtime's allocation
  bookkeeping.
- VM instance data lives **inside the VM** and dies when the VM is dropped. It is class
  C, not class B.

### Instance-scoped arena is rejected

A per-instance arena lifetime (freed when the instance is destroyed) is rejected on
three axes:

**Performance.** Instance data is allocated once at `create_instance`, not once per
call — it is never on the hot path. An instance-scoped arena captures no hot-path win,
and the generated bridge would have to branch at dispatch on which-arena/which-lifetime
to use, adding cost on the hot path the design exists to keep free.

**Unload.** Even if instance memory were routed through a runtime-owned arena freed en
masse on unload, it would reclaim raw bytes only. The guest's destructor — file
handles, socket teardown, GPU buffer release, any non-POD cleanup — lives in code inside
the unmapped dylib and can never run after the mapping is gone. That is silent resource
leakage of every non-POD instance, strictly worse than the honest "destroy instances
first" contract the ABI already states (`GuestContractInstance` doc: *"must be destroyed
via `destroy_instance` before the bundle is unloaded"*).

**Safety / visibility.** The one genuine benefit — making the runtime aware that an
instance is live — is delivered directly by the live-instance counter (below), without
forcing anyone's allocator.

---

## Epoch Model — Lock-Free Reads, Safe Reclamation

The runtime publishes an immutable `ReadView` of the registry. Reads and writes interact
through `crossbeam-epoch`:

- **Readers** call `crossbeam_epoch::pin()`, atomically load the published `ReadView`,
  and serve the request without taking a lock.
- **Writers** (register / unload / reload) build a new `ReadView`, publish it atomically
  under the write lock, and `defer_destroy` the old one. The deferred free runs only
  after every guard that was pinned in the old epoch has unpinned.

The consequence is the soundness backbone of unload: **a reader pinned before an unload
keeps BOTH the old interface `Arc` AND the still-mapped library (or live VM) alive until
it unpins.** There is no window in which the interface is live but its backing library
is unmapped. When the last pre-unload guard unpins, the deferred reclamation drops the
`Arc` and the library/VM together.

### FFI host-caller contract (fast path)

Direct FFI host callers do **not** pin the epoch per call. They rely on the documented
**quiesce-before-unload** contract: the host must ensure no thread is calling into — and
no thread holds a cached pointer into — a bundle before unloading it. Caching a raw
`*const GuestContractInterface` and dereferencing it after the owning bundle is unloaded
is documented **undefined behaviour**. This keeps native dispatch at the speed of a raw
indirect call.

**Generated cached callers** (the host→guest and guest→guest peer callers emitted by
`polyplugc`) go further than bare quiescence. They cache the resolved interface but poll
the registry **revision counter** (one acquire load via `HostApi.revision_counter`)
before each dispatch and re-resolve when it changed, so a reload never leaves them
dereferencing a superseded interface. For the peer caller the provider also cannot simply
vanish: a declared dependency makes the runtime **refuse to unload** a bundle while a
dependent is live (`DependencyInUse`), so the cached interface stays mapped for as long as
the caller can use it, and the revision check turns a *reload* into a clean re-resolve.
Hand-written FFI callers that skip the revision check remain bound by the
quiesce-before-unload contract above.

Runtime-mediated calls do pin the epoch across dispatch and are therefore safe against a
concurrent unload:

- `call_guest_method` (`HostApi` @136)
- `create_guest_instance` (`HostApi` @160)
- `destroy_guest_instance` (`HostApi` @168)

Because these enter the epoch before resolving and stay pinned across the call, an unload
racing one of them cannot free the interface or library out from under the in-flight
dispatch.

### Model-checked with loom

The `loom_epoch_model` crate exhaustively model-checks the protocol above with
[loom](https://docs.rs/loom). It reproduces the published-`AtomicPtr` read path and the
pin / defer / reclaim collector, then explores every thread interleaving of a reader
racing a writer that swaps the published view and defers the old one:

- **`pinned_reader_never_observes_reclaim`** — a reader that holds its guard across the
  dereference (the shipped `RuntimeStore` read path) never observes a reclaimed view,
  under every interleaving.
- **`unpinned_reader_race_is_detected_by_loom`** — a reader that drops its guard *before*
  the dereference races reclamation, and loom finds the use-after-free. This is the exact
  bug fixed by pinning the epoch across resolve→deref; the test asserts loom still
  detects it, so the proof keeps its teeth.

Scope: this verifies the safety *contract* `crossbeam-epoch` provides and that polyplug
relies on. `crossbeam-epoch`'s *implementation* of that contract is loom-checked upstream
in the crossbeam repo; it cannot be driven by loom from a downstream crate without
rebuilding the whole crossbeam stack under its internal `--cfg crossbeam_loom`. The crate
is compiled only under `--cfg loom`, so a normal build never pulls loom in. Run it with
`just loom` (or `RUSTFLAGS="--cfg loom" cargo test -p loom_epoch_model --release`).

---

## Handles and Resolution

`GuestContractHandle` is 8 bytes, `{ index: u32, generation: u32 }`, align 4. Every
registry slot carries a generation counter; `find_guest_contract` /
`find_all_guest_contracts` stamp the current slot generation into the handles they mint.

`resolve_guest_contract` takes the read lock, bounds-checks `handle.index`, then compares
`handle.generation` against the slot generation. A mismatch yields
`RegistryError::StaleHandle`, which the FFI shim maps to `AbiErrorCode::StaleHandle`. The
added cost is a single `u32` compare under the read lock already held, on resolve only —
never on dispatch.

Generation behaviour across the two lifecycle operations:

- **Unload** bumps the slot generation. A `GuestContractHandle` minted before the unload
  resolves to `AbiErrorCode::StaleHandle`.
- **Hot-reload** swaps the slot's interface in place **without** bumping the generation.
  A handle minted before the swap stays valid and resolves to the new interface.

---

## Live-Instance Accounting

The runtime keeps a per-contract live-instance counter keyed by contract id —
`Mutex<HashMap<GuestContractId, u64>>` on `Runtime`. `create_guest_instance` and
`destroy_guest_instance` are host-mediated precisely so the runtime can attribute each
instance to its contract; the bare `create_instance(args)` ABI carries no contract
context to attribute by.

Only **stateful** instances are counted: an instance with non-null `instance.data`
increments the counter, and a stateless (null-`data`) instance is ignored to avoid false
warnings. On reload or unload, if a contract still has live stateful instances, the
runtime emits a **"live guest instance"** warning that names the use-after-free hazard —
the instance's vtable (native) or backing object (VM) is about to be invalidated while a
host-held instance still references it.

---

## Unload Flow

1. **Quiesce callback.** The unload fires a `ReloadPhase::Unloading`
   (`ReloadPhaseType::Unloading = 3`, constructed via `ReloadPhase::unloading()`)
   callback before invalidation, so the host can drop caller wrappers and stop calling
   into the bundle.
2. **Invalidate (under the registry write lock).** Bump the generation of every slot the
   bundle owns, remove the bundle from `guest_contract_index`, `bundle_name_index`, and
   `bundle_declared_deps`, and publish a new `ReadView` that no longer contains the
   bundle. After this, every old handle resolves to `StaleHandle` and every fresh
   `call_guest_method` re-resolve fails cleanly — no resolve can hand out a pointer into
   the doomed interface.
3. **Defer reclamation.** `defer_destroy` the superseded `ReadView` and the bundle's
   interface `Arc`; the loader hands its dylib mapping / VM to the same epoch-deferred
   path. The actual free runs once the epoch advances past every reader pinned before the
   unload.

Re-resolution through `call_guest_method` routes by `instance.contract_id`; once the slot
is gone from the index, that re-resolve returns `NotFound` / `StaleHandle` and can never
reach a freed interface.

---

## Per-Loader Reclaim

| Loader | Reclaim mechanism |
|---|---|
| **Native (cdylib)** | The `libloading::Library` is dropped via the epoch-deferred path once the epoch advances past pinned readers; the drop `dlclose`s / `FreeLibrary`s the mapping. On Windows this also releases the on-disk file lock, enabling overwrite/delete of the old DLL. Native dispatch is a raw fn pointer the runtime never sees, so safety against in-flight native calls rests on the host's quiesce-before-unload contract. |
| **Lua** | The per-bundle `Lua` VM is dropped via the same epoch-deferred path. The VM and the interface `Arc` are freed together once readers pinned before the unload have unpinned. |
| **JS (QuickJS)** | The per-bundle QuickJS `Context` + `Runtime` are dropped via the epoch-deferred path, identically to Lua. |
| **Python** | CPython is single-init per process and cannot be torn down. On unload the loader **purges the bundle's re-keyed `sys.modules` entries** so a later load re-imports fresh source. This is memory-safe regardless of in-flight calls — CPython refcounts/GC keep referenced objects alive, and purging only drops the import cache. Honest verdict: **module-cache purge, not interpreter unload.** |
| **.NET / C#** | The CLR is single-init per process. Each bundle loads into a per-`(runtime, bundle)` **collectible `AssemblyLoadContext`** keyed by bundle id. Unload calls `AssemblyLoadContext.Unload()`; the ALC's assemblies become eligible for GC reclamation once all references and native frames into it clear (GC-driven, no hard timing guarantee). C#-guest bundles register native fn pointers, so the host-cached-pointer UB caveat applies to them as it does to native bundles. |

---

## Dependent Bundles

Dependency trust is established once at load and never re-checked on the hot path
(TRUST_MODEL.md §3–4). Unloading a provider that a loaded consumer declared a dependency
on therefore breaks an assumption the consumer baked in at load. The runtime **refuses**
such an unload by default — it consults the reverse mapping of `bundle_declared_deps` and
returns `RuntimeError::DependencyInUse { provider, dependents }` if any loaded bundle
declared this one. A deliberate cascade is available via `unload_bundle_cascade`, which
unloads dependents first, recursively. Orphaning (unloading anyway and letting consumers
hit `StaleHandle` later) is unsafe under the no-recheck hot path and is not offered.

`reload()` and `unload_bundle` are distinct operations with opposite contracts: reload
keeps already-resolved interface pointers valid across an in-place swap; unload
invalidates handles and reclaims the backing resources.

---

## Sizes

| Type | Size | Layout |
|---|---|---|
| `GuestContractHandle` | 8 bytes, align 4 | `{ index: u32, generation: u32 }` |
| `HostApi` | 192 bytes, align 8 | 1 runtime ptr + 22 fn-ptr fields + 1 trailing `reserved` data ptr |
| `RuntimeConfig` | 48 bytes, align 8 | no unload-mode field |

`ReloadPhaseType::Unloading = 3` with the `ReloadPhase::unloading()` constructor models
the quiesce callback fired before invalidation.

---

## See Also

- [HOT_RELOAD_DESIGN.md](./HOT_RELOAD_DESIGN.md) — the `Preparing`/`Reloaded`/`Failed`
  callback contract this design reuses for unload coordination
- [TRUST_MODEL.md](TRUST_MODEL.md) — §5 handle validation; §7 ABI freeze timing;
  Hot-Reload Safety Guarantees
- [PERFORMANCE.md](./PERFORMANCE.md) — zero-overhead hot-path rationale (why direct FFI
  host callers do not pin per call)
