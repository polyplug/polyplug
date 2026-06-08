# True Unload Design — Generation-Counted Handles

## Overview

This document designs **true unload** for polyplug: the ability to fully reclaim a
loaded bundle's resources (interfaces, native dylib mappings, VM state) at runtime,
in contrast to today's **retire-not-drop** model which retains every superseded
resource for the lifetime of the runtime.

The mechanism is **generation-counted handles**: `GuestContractHandle` grows a
`generation` field, `resolve_guest_contract` validates it, and a precise
**invalidate-then-reclaim** protocol defines exactly when an old generation's
memory may be freed without dangling any pointer or in-flight call.

**Status (2026-06):** Phases 1–4 have shipped.
- **Phase 1** — generation-counted `GuestContractHandle`: 8 bytes, `{ index: u32, generation: u32 }`, `StaleHandle` now produced by `resolve_guest_contract`.
- **Phase 2** — invalidate-only `unload_bundle` on `HostApi` at offset 152; `RuntimeApi` retired entirely; dependent-refusal and cascade unload.
- **Phase 3** — VM true reclaim: Lua/JS loaders drop a quiescent VM on unload; Python purges the bundle's re-keyed `sys.modules` entries.
- **Phase 4** — native opt-in Reclaim: under `UnloadMode::Reclaim` the native loader `dlclose`s the dylib on unload.

The supporting ABI bits — the `UnloadMode { Retire, Reclaim }` enum, `RuntimeConfig.unload_mode` (offset 4; `RuntimeConfig` grew 24→32 bytes), and `ReloadPhaseType::Unloading = 3` with the `ReloadPhase::unloading()` constructor — have also shipped. The unload flow fires a `ReloadPhase::Unloading` callback before invalidate so the host can quiesce.

The "Current State" section below therefore describes history rather than the present for the shipped items. The two items that remain future work are the call-arena **retain-and-rewind** optimization (Phase 1 of §"Core Concepts", a perf change independent of unload — see Deferred Work below) and the **D11 native live-instance counter** (see Decision Points). Phase 5 (.NET collectible ALC) also remains deferred.

This is an **implementation record**; the design rationale below is retained for context. Every claim about current behavior is cited to `file:line` and was verified against the working tree at design time.

This document deliberately mirrors the tone and structure of
[`HOT_RELOAD_DESIGN.md`](./HOT_RELOAD_DESIGN.md).

### Why now

Retire-not-drop trades memory for safety: it never frees a superseded interface or
dylib, so memory grows **monotonically per reload**. The dominant retained cost per
native reload is the entire dylib code-page mapping. For long-running hosts that
reload frequently (the canonical hot-reload use case), this is an unbounded leak by
design. True unload bounds it. The mechanism must be **final before v1.0**, because
`GuestContractHandle`'s layout freezes at v1.0 (TRUST_MODEL.md §7) — a generation
field added after the freeze is impossible.

---

## Memory Model — Why Arena and Unload Are Orthogonal

The recurring question "should the arena be instance-scoped and freed on unload?"
conflates two independent memory regimes. Every byte polyplug touches belongs to one
of five classes:

| Class | What | Allocated by | Freed by | Reclaimed when | Leaks? |
|---|---|---|---|---|---|
| A — Args | host→guest call inputs | host (stack/buffer) | host | call returns | no |
| B — Returns | guest→host outputs (string/array) | native: none (borrowed view); VM: call arena or `host->alloc` fallback | arena reset / host | next call (arena) | no |
| C — Instance data | per-instance state | native: guest's own allocator (`Box`/`new`); VM: inside the VM | native: `destroy_instance`; VM: VM drop | instance destroyed | only if host skips `destroy_instance` |
| D — Interface vtables | registered `GuestContractInterface` | runtime (`Arc`) | runtime | never (retire-not-drop) | yes — monotonic per reload |
| E — Dylib mappings | native code pages | `dlopen` | OS | never (retire-not-drop) | yes — dominant leak |

The **call arena lives entirely in class B**. The **unload problem lives entirely in
classes D + E**. They never compose, because you cannot bump-allocate a code mapping
or a registered vtable. Therefore no arena of any scope — call-scoped, instance-scoped,
or runtime-scoped — addresses unload.

This is not a design opinion; it follows from verified facts about the code:

- Instance create and destroy are **direct vtable calls the runtime never mediates**.
  The generated host callers in `crates/polyplugc/src/generators/rust.rs:1378` and
  `crates/polyplugc/src/generators/cpp.rs:1277` emit the `create_instance` /
  `destroy_instance` calls directly through the resolved `GuestContractInterface`
  pointer. There is no runtime interception point.
- `HostApi` has **no `create_instance` or `destroy_instance` field**
  (`crates/polyplug_abi/src/host/host_api.rs`). Those vtable slots belong to
  `GuestContractInterface`, not to the host table. The runtime cannot intercept
  them without a redesign of the ABI.
- The runtime holds **no live-instance counter**. `RuntimeStoreData` in
  `crates/polyplug/src/runtime_store.rs` carries `slots`, `retired_interfaces`,
  index maps, and `bundle_declared_deps` — no field tracks how many instances any
  bundle has outstanding.
- Native instance data uses **the guest's own allocator** (`Box` / `new` / `malloc`
  inside the guest dylib), not `host->alloc`. It is entirely outside the runtime's
  bookkeeping.
- VM instance data lives **inside the VM** and dies when the VM is dropped. It is
  class C, not class B.

### Rejected: instance-scoped arena

Adding a second arena lifetime (one per instance, freed when the instance is
destroyed) has been evaluated on three axes and rejected on all three.

**Performance.** Instance data is allocated once at `create_instance`, not once per
call — it was never on the hot path. An instance-scoped arena captures no hot-path
win. Worse, the generated bridge would have to branch at dispatch time on which-arena
/ which-lifetime to use (class B call-return data vs hypothetical class C instance
data). That branch is on the hot path, introducing a cost that violates the
zero-overhead pillar the design stands on.

**Unload.** Even if instance memory were routed through a runtime-owned arena so the
runtime could `free` it en masse on `dlclose`, it would only reclaim raw bytes. The
guest's destructor — file handles, socket teardown, GPU buffer release, any non-POD
cleanup — lives in code inside the unmapped dylib and can **never run** after
`dlclose`. The result is silent resource leakage of every non-POD instance, which is
strictly worse than the honest "destroy instances first" contract the ABI doc already
states (`GuestContractInstance` doc: *"must be destroyed via `destroy_instance` before
the bundle is unloaded"*). An instance-scoped arena trades visible instance bytes for
invisible non-memory resource leaks.

**Safety / visibility.** The one genuine benefit of routing instance allocation
through the runtime — making the runtime *aware* that an instance is live — is
achievable with a cheap per-bundle reference counter, without forcing anyone's
allocator. Even full instance visibility does not fix the dominant residual risk of
native unload: an **in-flight native dispatch call** executing code in the doomed
dylib, because dispatch is a raw pointer call the runtime never sees by design (§§3-4
of this document). The counter approach is examined as D11.

**Conclusion.** Instance-scoped arena is a settled rejection, not an open option.

---

## Current State — Verified Against Code

### Retire-not-drop reclamation

- **Interfaces.** `RuntimeStore` holds `retired_interfaces: Vec<Arc<GuestContractInterface>>`
  (`crates/polyplug/src/runtime_store.rs`, field documented inline). A reload swaps a
  slot's `Arc` under one write lock via `apply_reload_swap`
  (`crates/polyplug/src/runtime_store.rs:788+`); the superseded `Arc` is pushed onto
  `retired_interfaces` and held for the runtime's lifetime. This is what keeps a raw
  `*const GuestContractInterface` valid after a reload.
- **Native libraries.** `NativeLoader` holds `retired: Mutex<Vec<libloading::Library>>`
  (`crates/polyplug_native/src/loader.rs:~25-34`). On reload (Step 8,
  `crates/polyplug_native/src/loader.rs:~333-358`) the old `libloading::Library` is
  `remove`d from the active `libraries` map and pushed onto `retired` — **never**
  `dlclose`d / `FreeLibrary`d — to keep code pages mapped for any in-flight raw
  function pointer.

### Handle and resolution

- `GuestContractHandle` is **4 bytes, index only, no generation**
  (`crates/polyplug_abi/src/plugin/guest_contract_handle.rs`):

  ```rust
  #[repr(C)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub struct GuestContractHandle {
      pub index: u32,
  }
  ```

  Its own doc-comments already promise generation semantics that the code does **not**
  implement: *"Handles become stale after unload. Call `resolve_guest_contract` to
  validate. Returns null pointer if the handle is invalid."* This is the
  **generational-index inconsistency** flagged in TRUST_MODEL.md §5: the doc and
  TRUST_MODEL.md §6 Capabilities Matrix both claim *"Use-after-Unload — caught by
  generational index checks in `GuestContractHandle`"* — but there is no generation
  field and no unload path. **This design closes that gap; it is not a new feature so
  much as making the documented contract true.**

- `resolve_guest_contract` (`crates/polyplug/src/runtime_store.rs:699-725`) takes a
  read lock, bounds-checks `handle.index` against `data.slots.len()`, then returns
  `Ok(interface.as_ref() as *const GuestContractInterface)` if the slot holds an
  interface, else `Err(RegistryError::InvalidHandle)`. **No generation comparison
  exists.** After a reload swap, the same index resolves to whatever interface now
  occupies the slot.

- `AbiErrorCode::StaleHandle = 5` **already exists**
  (`crates/polyplug_abi/src/types/error_code.rs`), documented *"GuestContractHandle is
  invalid (contract unloaded)"*, and the total `from_u32` mapping maps `5 =>
  StaleHandle`. It is currently **never produced** by the resolution path
  (TRUST_MODEL.md §5 confirms this).

### Cross-call (`call_guest_method`) — unload-friendly

- `host_call_guest_method` (`crates/polyplug/src/runtime.rs`, the
  `HostApi::call_guest_method` callback at offset 136) **re-resolves the target
  through the registry via `instance.contract_id` on every call and never caches**
  (verified in the function doc-comment and body). A cross-call issued after a state
  change always routes through the live registry. This is the access pattern an unload
  protocol wants everywhere: validity is re-checked at the registry, not cached in a
  raw pointer.

### In-flight detection primitives (VM loaders)

- Both VM loaders track the threads currently executing a dispatch:
  `in_dispatch_threads: Mutex<Vec<ThreadId>>` in `JsLoaderData`
  (`crates/polyplug_js/src/loader.rs:111`) and `LuaLoaderData`
  (`crates/polyplug_lua/src/loader.rs:78`). Today this drives the same-thread
  re-entrancy guard (`AbiErrorCode::ReentrantCall = 9`); the test at
  `crates/polyplug_js/src/loader.rs` asserts the vec is empty after a dispatch
  returns. **This is a ready-made in-flight-detection signal an unload safe-point can
  consult for VM bundles.** Native dispatch has no equivalent.

### Multi-source loading

- `BundleSource { Path, Code, Bytes }` exists
  (`crates/polyplug/src/loader/bundle_source.rs:30`). It is host-internal, never
  crosses the ABI. Native loader supports `Path` only (no portable in-memory
  `dlopen`); VM/.NET loaders are slated for `Code`/`Bytes`. Relevant here only because
  in-memory VM sources have **no on-disk file lock** to release on unload — easing the
  VM unload story (see §7).

### The two runtime facades — `HostApi` vs `RuntimeApi`

A discrepancy that **must be resolved before wiring unload**:

- `HostApi` is the **live** 152-byte table actually returned by
  `polyplug_runtime_create` (CLAUDE.md §FFI Surface; layout locked by `layout_host_api`).
  It has **no `unload_bundle` field** — it has `load_bundle` (88), `reload_bundle` (96),
  and the cross-call `call_guest_method` (136).
- `RuntimeApi` (`crates/polyplug_abi/src/host/runtime_api.rs`) is a **separate 96-byte**
  `#[repr(C)]` struct whose doc-comment claims it is *"returned to host from
  polyplug_runtime_create()"* and which **already declares `unload_bundle` at offset 24**
  (verified by `layout_runtime_api` test, `runtime_api.rs:254`). But: nothing returns
  it, no `host_unload_bundle` implementation exists anywhere
  (grep for `host_unload_bundle` / `fn .*unload` returns nothing in `crates/*/src`),
  and its only consumers are its own layout tests, the `pub use` in
  `polyplug_abi/src/lib.rs:49`, and the ABI-SDK generator size table
  (`crates/polyplug_abi/build/generate.rs:684` → `("RuntimeApi", 96)`).

`RuntimeApi` is therefore a **defined-but-unwired parallel facade** — a leftover from
an earlier API shape. It is dead-but-load-bearing-looking surface: per CLAUDE.md
"never blindly remove dead code", I do **not** propose deleting it here. Instead it is
**Decision Point D0**: either retire `RuntimeApi` entirely (unload lives on `HostApi`)
or adopt it as the unload home. Recommendation in §6 / Decision Points.

### Test tooling available

- Stress/safety suites already exist: `crates/polyplug/tests/stress_memory.rs`,
  `stress_hot_reload.rs`, `stress_concurrent_registry.rs`, `concurrent_reload.rs`,
  `hot_reload_safety.rs`, `integration_reload_abort.rs`, `integration_panic.rs`.
- `TrackingAllocator` panics on double-free under `cfg(debug_assertions)`, and **ASan
  runs in CI** (TRUST_MODEL.md §6 Capabilities Matrix). **No MIRI** job exists in
  `.github/workflows/` (grep found none). MIRI is largely inapplicable anyway: it
  cannot run `dlopen`/FFI into real cdylibs. UAF coverage for unload must therefore
  come from **ASan + a dedicated unload stress test**, not MIRI (see §9).
- Benches that bound per-call cost: `crates/polyplug/benches/ffi_resolve.rs`
  (resolve path), `contract_dispatch.rs` (full indirect-call dispatch),
  `registry_resolve.rs`, `registry_find.rs`. These are the baselines any
  per-call validation cost (Option A, §2) must be measured against.

---

## Core Concepts

#### Call-arena reset policy (perf, independent of unload)

Today `CallArena::reset` (`crates/polyplug_abi/src/types/call_arena.rs`) frees every
overflow block on each call; for workloads that consistently exceed the inline buffer
this reintroduces one alloc + free per call. The standard arena discipline is
**retain-and-rewind**: keep overflow blocks allocated, rewind the bump pointer to the
start of the first block, and free only on `Drop`. The validity contract is identical
("all arena memory valid until the next reset"), so the guest-facing class-B guarantee
is unchanged. This is a pure class-B performance change tracked separately from
unload; it is pending verification that the guest-facing arena contract does not depend
on the struct's internal layout (the `CallArena` type is not currently `#[repr(C)]`
and not in the frozen ABI surface, so the change is self-contained). Nothing in the
unload phases depends on this optimization; it can ship independently at any time.

### 1. Generation-counted handles

Each registry slot gains a **generation counter**. A handle carries the generation
it was minted against. `resolve` succeeds only if `handle.generation ==
slot.generation`. Unload (and, optionally, reload) **bumps** the slot generation,
which atomically invalidates every previously minted handle for that slot.

This converts the silent "index resolves to whatever now occupies the slot" behavior
into an explicit `StaleHandle` failure — the precondition for ever freeing the old
resource.

### 2. The invalidate-then-reclaim split

Unload happens in two logically separate steps:

1. **Invalidate** (cheap, synchronous, under the write lock): bump the slot
   generation, remove the slot from the public indices, clear the active interface
   `Arc` from the slot. After this, no *new* `resolve` can hand out a pointer to the
   old interface, and every old handle fails with `StaleHandle`.
2. **Reclaim** (deferred to a safe point): drop the old interface `Arc` and `dlclose`
   the dylib **only once we can prove no in-flight call still holds a raw pointer into
   them**. The hard problem is step 2's safe-point proof.

Retire-not-drop is the degenerate case where step 2 never happens. True unload makes
step 2 reachable.

---

## The Fundamental Conflict and the Three Options

`resolve_guest_contract` hands out a **raw** `*const GuestContractInterface` borrowed
from the slot's `Arc` (`runtime_store.rs:699-725`). Once handed out, the runtime has
**no record** of who holds it or for how long. Generation-checking the *handle* stops
*new* resolves, but a caller who already resolved holds a raw pointer with no
generation attached. Freeing the `Arc` while that pointer is live is a use-after-free.
This is the exact reason retire-not-drop exists.

There are three ways to make reclamation sound. The task requires analyzing all
three; A and B are analyzed in full, C is the recommendation.

### Option A — Validate-before-every-use

Change the resolve contract: callers no longer cache the raw pointer. Every method
call re-resolves (handle → generation check → pointer) immediately before use, the
way `call_guest_method` already re-resolves by `contract_id` every call
(`runtime.rs` host_call_guest_method).

- **Soundness:** A free can proceed as soon as the slot generation is bumped, *if*
  the free itself takes the same lock the re-resolve takes. A re-resolve either
  observes the new generation (→ `StaleHandle`, no pointer handed out) or completes
  before the bump under the read lock; the writer that frees waits for the write lock,
  which excludes all readers. No raw pointer outlives its resolve.
- **Cost:** Per-call you add: one `RwLock` read acquisition + bounds check + generation
  compare, on **every** dispatch. Today the hot path after one resolve is a raw
  indirect call (TRUST_MODEL.md §4 "speed of a raw function pointer dereference").
  Option A reintroduces a lock + branch per call. **Measure against
  `benches/contract_dispatch.rs` and `benches/ffi_resolve.rs`**: `ffi_resolve` already
  isolates the resolve cost; Option A makes that cost *mandatory per dispatch* rather
  than once. Expected order: a contended-free `RwLock` read is tens of ns; against a
  raw indirect call of ~1-5 ns this is a **5-20× hot-path regression** for the
  resolve-then-call-N-times pattern. This **violates the zero-overhead hot-path pillar**
  (TRUST_MODEL.md §4, §6 crash-isolation rationale).
- **Verdict:** Rejected as the *default*. Correct and simple, but it taxes every host
  that never unloads. It is acceptable only as an **opt-in mode** for hosts that value
  unload over peak dispatch throughput.

### Option B — Epoch / quiescence (RCU-style grace period)

Keep raw-pointer resolve. Track in-flight readers with an epoch scheme: a reader
"enters" before using a resolved pointer and "exits" after. Unload bumps the
generation (invalidate), then **defers the free until every reader that could hold an
old pointer has exited** (quiescence). This is classic RCU / `crossbeam-epoch`.

- **Soundness:** A grace period guarantees no thread is between enter/exit with an old
  pointer when the free runs. Sound *if* every raw-pointer use is bracketed by
  enter/exit.
- **The bracketing problem:** polyplug's whole point is that the host caches the raw
  interface pointer and calls it directly with **zero runtime involvement** per call
  (the generated caller wrappers in HOT_RELOAD_DESIGN.md hold the resolved interface).
  The runtime never sees the call, so it cannot bracket it. To make epoch work you'd
  have to route every dispatch back through a runtime enter/exit — which **is Option A's
  cost** plus epoch bookkeeping. The only place the runtime *already* sees every call
  is `call_guest_method` (VM cross-calls), and the VM loaders *already* track
  `in_dispatch_threads` — so epoch/quiescence is **cheaply achievable for VM dispatch
  but not for the native host-cached-pointer hot path.**
- **History:** TRUST_MODEL.md §7 note records that an earlier ref-counted reclamation
  design *"that used those mechanisms [generation counter, quiescence spin,
  QuiescenceTimeout] was removed in favor of retire-not-drop."* Re-introducing a
  global quiescence scheme would re-introduce exactly what was deleted. The memory
  (`MEMORY.md`) corroborates the archive "tried unload+load and it was reverted."
- **Verdict:** Rejected as a *global* scheme for native. Viable and cheap **only for
  VM dispatch**, where the runtime already mediates every call and already tracks
  in-flight threads.

### Option C — Hybrid: immediate invalidate, deferred free at a runtime-chosen safe point (RECOMMENDED)

Split by resource type and by what the runtime can actually observe:

1. **Invalidate immediately, always.** `unload_bundle` bumps the slot generation,
   removes the slot from `guest_contract_index` / `bundle_name_index`, clears
   `bundle_declared_deps`, and clears the slot's active `Arc` — all under the single
   `RuntimeStore` write lock (the same lock `apply_reload_swap` uses). After this,
   every old handle resolves to `StaleHandle` and every fresh `call_guest_method`
   re-resolve returns `NotFound`/`StaleHandle`. **This is fully sound with no per-call
   cost** because it only stops *future* resolves.

2. **Move the old `Arc` + dylib to retire storage (unchanged from today).** The raw
   pointers already handed out keep working against retired memory. So far this is
   retire-not-drop — i.e. **invalidate-only unload is already shippable as Phase 1 with
   zero UAF risk and zero hot-path cost**, and it already delivers the *semantic*
   win (handles go stale, the registry shrinks, `find` stops returning the bundle).

3. **Actually free at a safe point, per resource class:**
   - **VM bundles (Lua, JS):** free is reclaimable **under host coordination**. The VM
     is owned solely by its loader; once the slot is invalidated and
     `in_dispatch_threads` is observed empty, the loader drops the VM, the interface
     `Arc`, and any in-memory `BundleSource::Code`; otherwise it retires (defers). No
     raw native pointers were ever exposed for VM dispatch (dispatch goes through
     `call_guest_method` → loader → VM). **Caveat (see §7 Correction):** the
     `in_dispatch_threads` check is **best-effort, not a guarantee** — there is a
     resolve→dispatch window where `host_call_guest_method` has released the registry
     lock but the VM has not yet registered the call, so a call racing a cross-thread
     unload could free a VM under it. VM unload is therefore **host-coordinated** (the
     host must not call a bundle concurrently with unloading it), with
     `in_dispatch_threads` as defense-in-depth.
   - **Native bundles:** the host may hold cached raw interface/function pointers the
     runtime cannot see. The runtime **cannot prove quiescence** for these without
     Option A's per-call tax. Therefore native `dlclose` is gated on an **explicit host
     attestation**: the host must have dropped all caller wrappers for the bundle
     (exactly the `Preparing`-callback contract that hot-reload already defines,
     HOT_RELOAD_DESIGN.md §4). The runtime fires the unload equivalent of `Preparing`,
     performs the same informational `Arc::strong_count` leak check, and:
       - **Default (`UnloadMode::Retire`):** does **not** `dlclose`; behaves exactly
         like today (retire-not-drop) but with the slot invalidated. Zero new risk.
       - **`UnloadMode::Reclaim` (opt-in, with `force` semantics):** `dlclose`s the
         dylib after the callback returns. This is a **host-asserted safe point**: the
         host has promised no raw pointer survives. If the host lied, it is a host bug
         (consistent with HOT_RELOAD_DESIGN.md's "safety contract is on the host" and
         TRUST_MODEL.md's trusted-same-process assumption). The leak check downgrades
         a likely-unsafe reclaim: if `strong_count > 1`, the runtime **refuses to
         `dlclose`** (falls back to retire) and emits a warning, unless `force` is set.

**Why C is right:** it keeps the **zero-overhead hot path intact** (no per-call check
in the default and native paths), delivers **real, safe reclamation for VM bundles
immediately**, makes the **documented `StaleHandle`/use-after-unload contract true**
for all bundle types, and confines the genuinely-unprovable case (native cached
pointers) behind an explicit, opt-in, host-attested mode that mirrors the existing
hot-reload coordination contract. It also lets us **ship value in phases** (§9):
invalidate-only first, VM reclaim second, native opt-in reclaim last.

---

## Detailed Design

### 1. Handle layout change (ABI — owner approval required, pre-1.0)

```rust
// crates/polyplug_abi/src/plugin/guest_contract_handle.rs
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuestContractHandle {
    /// Slot index in the registry array.
    pub index: u32,
    /// Generation the handle was minted against. Bumped on unload (and optionally
    /// reload). A mismatch on resolve yields StaleHandle.
    pub generation: u32,
}
// New size: 8 bytes, align 4. (Was 4 bytes.)
```

- **Why `u32` generation:** keeps align 4, total 8 bytes, no padding. A `u32` wraps
  after 4.29e9 unload/reload cycles of a single slot; at that scale wrap-collision is
  negligible and a wrapped generation only risks a *false-valid* resolve, which the
  bounds/empty-slot check still mostly catches. (A `u16`+`u16` index split was
  considered and rejected: 65 535 slots is too few for large deployments.)
- The null handle becomes `{ index: u32::MAX, generation: 0 }` (or a documented
  sentinel); update `GuestContractHandle::NULL` accordingly.

**ABI artifacts affected (every one must change in lockstep — Rule 10 ABI parity):**

| Artifact | Change |
|---|---|
| `crates/polyplug_abi/src/plugin/guest_contract_handle.rs` | add `generation: u32`, update NULL, doc |
| `layout_*` test for the handle in `polyplug_abi` | 4 → 8 bytes, add `offset_of!(.., generation) == 4` |
| `HostApi::find_guest_contract` / `find_all_guest_contracts` / `resolve_guest_contract` signatures | unchanged *types* (still pass/return `GuestContractHandle`), but the now-8-byte struct changes the **calling convention by value** — every caller recompiles |
| `RuntimeApi` (if kept, D0) | same handle type ripples through its `find_*`/`resolve_*` fields |
| `Array<GuestContractHandle>` (find_all buffer) | element stride 4 → 8; host pre-allocated buffers double; `benches/ffi_find_all.rs` updates |
| SDK ABIs ×5 — `sdks/{rust,cpp,csharp,python,lua,js}/abi/` | `GuestContractHandle` struct/marshalling: C++ `abi.hpp`, C# `Abi.cs`, Python `abi.py` (ctypes), Lua `abi.lua` (FFI), JS `abi.ts` |
| SDK layout tests ×5 — `sdks/*/.../test_layout.*` + `csharp/abi.tests/LayoutTests.cs` | assert new 8-byte layout |
| `sdk_validator` | its handle-size expectation |
| `crates/polyplug_abi/build/generate.rs` | emits the SDK handle structs; the generator (not the generated files) is the edit site (Rule 10) |
| `TRUST_MODEL.md` §5/§6/§7 table | `GuestContractHandle` 4 → 8 bytes, **remove the "no generation" caveat and the "StaleHandle not produced" sentence**, flip the §6 use-after-unload row from aspirational to real |
| `CLAUDE.md` Quick Reference / FENV notes | any handle-size mention |

### 2. Slot generation and the invalidate path

`PluginSlot` gains `generation: u32` (server-side; not ABI). The slot's generation is
copied into every handle minted by `find_guest_contract` / `find_all`.

`resolve_guest_contract` (`runtime_store.rs:699`) gains, after the bounds check and
before dereferencing the slot:

```text
if handle.generation != slot.generation { return Err(RegistryError::StaleHandle { index }) }
```

`RegistryError` gains a `StaleHandle { index: u32 }` variant; the FFI shim maps it to
`AbiErrorCode::StaleHandle` (5, already in the ABI). The cost is **one `u32` compare
under the read lock already held** — negligible, and only on resolve, not on dispatch.

`unload_bundle` adds a `RuntimeStore` method (mirrors `apply_reload_swap`'s
single-write-lock discipline):

```text
fn invalidate_bundle(&self, bundle_id) under write lock:
    for each slot owned by bundle_id:
        slot.generation = slot.generation.wrapping_add(1)
        retire slot.interface (push Arc into retired_interfaces)   // or hand to caller for free
        slot.interface = None
    remove bundle from guest_contract_index, bundle_name_index, bundle_declared_deps
```

### 3. In-flight calls during unload

Enumerate the paths (the task requires each shown sound):

- **Native host-cached pointer mid-call while `unload_bundle` lands.** In
  `UnloadMode::Retire` (default): sound, identical to retire-not-drop — the dylib stays
  mapped, the `Arc` stays in `retired_interfaces`. In `UnloadMode::Reclaim`: the host
  attested (via the unload `Preparing` callback) that it dropped all wrappers before
  the call returned; if a call is genuinely still executing, that is the host
  violating the contract — same trust posture as hot-reload's `Preparing`
  (HOT_RELOAD_DESIGN.md §4). The leak check (`strong_count > 1`) is the runtime's
  best-effort refusal to `dlclose` when wrappers clearly remain.
- **Cross-call via `call_guest_method` re-resolution.** Re-resolution goes through the
  registry by `instance.contract_id` (`runtime.rs` host_call_guest_method). After
  `invalidate_bundle` removes the slot from the index, a re-resolve **fails cleanly**
  (`NotFound`/`StaleHandle`) — it cannot route to a freed interface. Sound by
  construction; this is the unload-friendly pattern the task calls out.
- **VM dispatch holding `loader_data`.** Reclaim of a VM bundle defers (retires)
  unless `in_dispatch_threads` is observed empty (the vec at
  `crates/polyplug_js/src/loader.rs:111` / `crates/polyplug_lua/src/loader.rs:78`).
  While any thread is *visibly* mid-dispatch, the loader is not dropped. **This is
  best-effort, not a guarantee** (see §7 Correction): `host_call_guest_method` releases
  the registry lock before the VM registers the call in `in_dispatch_threads`, so a
  resolve→dispatch window remains where a racing cross-thread unload is not yet
  observable. VM unload is therefore host-coordinated; `in_dispatch_threads` is
  defense-in-depth, not a complete proof of quiescence.
- **Unload-during-call from the same thread (re-entrant unload).** A guest cannot call
  `unload_bundle` re-entrantly into its own VM without tripping the existing
  `ReentrantCall = 9` guard. Host-initiated unload from another thread serializes on
  the registry write lock.
- **Reload-during-unload / unload-during-reload.** Both take the single registry write
  lock; they cannot interleave. Generation bump composes: a reload that *also* bumps
  generation (D3) and an unload that bumps generation are both monotonic.

### 4. Native dylib unmapping

- **Timing.** `dlclose` / `FreeLibrary` happens **only** in `UnloadMode::Reclaim`,
  **after** the unload `Preparing` callback returns and **after** the leak check
  passes (or `force` overrides), and **after** `invalidate_bundle` has removed the slot
  so no new resolve can hand out a pointer into the doomed mapping.
- **Caller guarantees required before unmap.** (a) No host caller wrapper holds the
  interface (`Arc::strong_count == 1`, i.e. only the registry/retire ref). (b) No
  in-flight native call (host-attested — runtime cannot verify; this is the residual
  trust). (c) No outstanding `GuestContractInstance` from this bundle (see §5).
- **Windows specifics.** On Windows the mapped DLL **file is locked while loaded**;
  `FreeLibrary` releases the lock and enables overwrite/delete of the on-disk file —
  this is precisely what a recompile-and-reload workflow needs (today retire-not-drop
  *prevents* deleting the old DLL on Windows). `FreeLibrary` is refcounted by the OS;
  `libloading::Library::drop` calls it. So reclaim is simply: `remove` from `retired`
  (or never retire) and drop the `Library`. Add a Windows-specific unload stress test
  asserting the old DLL file becomes deletable post-reclaim.
- **Interaction with retire storage.** `NativeLoader::retired`
  (`loader.rs:~25-34`) becomes a **per-bundle** structure (`HashMap<BundleId,
  Vec<Library>>` or tagged entries) so reclaim can drop *that bundle's* retired
  libraries. Today it is an untagged `Vec`; reclaiming one bundle must not drop
  another's retired mapping. This is the one loader-internal data-structure change.

### 5. Instances after unload

`GuestContractInstance` carries `data` (opaque, guest-owned) and `contract_id`
(stamped at creation) — verified in
`crates/polyplug_abi/src/guest/guest_contract_instance.rs`. Its doc already says it
*"must be destroyed via `destroy_instance` before the bundle is unloaded."*

- **Policy:** `destroy_instance` for an unloaded contract is **unavailable in Reclaim
  mode** — the `destroy_instance` function pointer lives in the (freed) interface
  vtable. Therefore unload must require instances destroyed **first**, exactly like
  hot-reload's `Preparing`.
- **Leak-vs-invalidate:** in `Retire` mode, a surviving instance is harmless (vtable
  still mapped); in `Reclaim` mode a surviving instance is a host bug. The runtime
  cannot enumerate guest-owned instances, so it relies on the same host attestation +
  `strong_count` heuristic. For VM bundles, instance data lives inside the VM and is
  dropped with the VM — so VM reclaim does not leak instances.

### 6. API surface

```rust
impl Runtime {
    /// Invalidate a bundle's handles and remove it from the registry. Whether the
    /// underlying dylib/VM is actually freed depends on `UnloadMode` and the bundle
    /// kind. Fires the on_reload callback with a new ReloadPhaseType::Unloading
    /// (or a sibling UnloadPhase) so the host can drop wrappers first.
    pub fn unload_bundle(&self, bundle_id: BundleId) -> Result<(), RuntimeError>;
}
```

- **Semantics & errors.**
  - Bundle not loaded → `RuntimeError::BundleNotFound`.
  - In-flight refusal: in `Reclaim` mode, if the leak check shows `strong_count > 1`
    (or a VM's `in_dispatch_threads` is non-empty), **refuse to free**, fall back to
    invalidate-only (Retire), and return `Ok` with a warning **unless** `force` is set,
    in which case proceed (caller-asserted). Recommendation: provide
    `unload_bundle_forced(bundle_id)` rather than a bool param, to keep the common path
    obvious.
  - Invalidate-only never fails on in-flight state (it only stops future resolves).
- **Dependent bundles** (declared `[[dependency]]` consumers). Per TRUST_MODEL.md §3-4,
  dependency trust is established **once at load** and never re-checked on the hot path.
  Unloading a provider a consumer declared a dependency on therefore breaks an
  assumption the consumer baked in at Phase 1. Options: **refuse** (safest — error
  `RuntimeError::DependencyInUse { provider, dependents }` if any loaded bundle declared
  this one), **cascade** (unload dependents first, recursively — risky, surprising,
  and the archive's cascade-reload work showed cascade depth is a footgun), or
  **orphan** (unload anyway, consumers get `StaleHandle` on their next resolve).
  **Recommendation: refuse by default** (consult `bundle_declared_deps` reverse-mapped),
  with an explicit `unload_bundle_cascade` for the deliberate case. Orphan is unsafe
  with the no-recheck hot path and is not offered.
- **`reload()` interop — does reload become unload+load?** **No.** Argued against:
  (a) the archive already tried unload+load reload and it was reverted (MEMORY.md
  `fork-resolved-queue`); (b) reload's value is **pointer stability across the swap**
  for hosts that *don't* coordinate teardown, which retire-not-drop + in-place
  `apply_reload_swap` provides and unload+load destroys; (c) reload and unload have
  opposite contracts (reload keeps old pointers valid; unload invalidates them).
  Keep them distinct. Reload **may** optionally bump generation (D3) so that
  `find`-then-`resolve` after reload yields a handle that won't silently resolve to the
  old slot content — but the *retire-not-drop* old-pointer guarantee stays.

### 7. VM vs native asymmetry — honest per-loader verdicts

**Correction (honest safety model).** An earlier revision of this table called VM
reclaim "fully safe via quiescence." That was an **overstatement**. There is an
inherent **resolve→dispatch window**: `host_call_guest_method` releases the registry
lock *before* the VM registers the call in `in_dispatch_threads`, so a call racing a
cross-thread unload could free a VM out from under it. Therefore **unload (VM and
native alike) is HOST-COORDINATED, exactly like hot-reload**: the host must not call a
bundle concurrently with unloading it (the trusted-same-process model). The
`in_dispatch_threads` check is **best-effort defense-in-depth** — it retires
(drop-deferred) instead of dropping when a dispatch is *visibly* in flight — **not a
complete guarantee**. Native reclaim follows the same host-coordinated model, plus the
structural-blindness caveat below (native dispatch is a raw fn pointer the runtime
never sees, so it cannot even attempt the in-flight check).

| Loader | True reclaim feasible? | Mechanism / verdict |
|---|---|---|
| **Native (cdylib)** | Yes, host-attested | Invalidate always; `dlclose` only in opt-in Reclaim mode after the `Unloading` callback + leak check. Native dispatch is zero-overhead (raw fn pointers into the library), so the runtime is **structurally blind** to in-flight native calls and cannot verify safety. Selecting Reclaim is the host's **attestation** that no thread is calling / holds a pointer into the bundle. A best-effort `Arc::strong_count` net (`reclaim_safe`) defers to retire when an `Arc` holder remains, but cannot see raw in-flight calls. |
| **Lua** | Yes, host-coordinated | Per-VM, runtime mediates dispatch. Drops the `Lua` VM + `Arc` when `in_dispatch_threads` is observed empty; retires (defers) otherwise. Best-effort, not a guarantee (resolve→dispatch window). The Lua loader governs reclaim by its own `in_dispatch_threads` quiescence and ignores `UnloadMode`/`reclaim_safe`. (Reload already disabled; unload is independent.) |
| **JS (QuickJS)** | Yes, host-coordinated | Same as Lua via `in_dispatch_threads` (`js/loader.rs:111`). Drops the `Context`/`Runtime` + `Arc` when quiescent. Governs reclaim by its own `in_dispatch_threads` and ignores `UnloadMode`/`reclaim_safe`. |
| **Python** | **Partial — invalidate yes, true free no** | CPython is single-init per process (`PYTHON_INIT: OnceLock<()>`). The interpreter can't be torn down. Shipped behaviour: under `UnloadMode::Reclaim` the loader **purges the bundle's re-keyed `sys.modules` entries** so a reload re-imports fresh source; under `Retire` it keeps them. Memory-safe regardless of in-flight calls — CPython refcounts/GC keep referenced objects alive, so purging only drops the import cache. Honest verdict: **module purge, not interpreter unload.** |
| **.NET / C#** | **Partial — requires collectible ALC, not built today** | CLR is single-init (`CLR_CONTEXT: OnceCell`, `polyplug_dotnet/src/context.rs`). .NET *does* support unload via **collectible `AssemblyLoadContext` + `AssemblyLoadContext.Unload()`** — but only if each bundle is loaded into its own collectible ALC, and unload is *cooperative* (GC reclaims after all references drop, no hard guarantee of timing). The current loader uses a per-assembly delegate-loader cache on a shared context, **not** per-bundle collectible ALCs. Honest verdict: **true .NET unload is a larger loader rework (one collectible ALC per bundle); for this design, .NET gets invalidate-only**, with collectible-ALC reclaim deferred to future work. Note C#-guest bundles register **native fn ptrs** (like native bundles), so even with ALC unload, the host-cached-pointer caveat applies. |

### 8. Migration & compatibility

- **Default behavior stays retire-not-drop.** Recommend a `RuntimeConfig` knob
  `unload_mode: UnloadMode { Retire (default), Reclaim }` (added pre-1.0 alongside the
  handle change; `RuntimeConfig` is not frozen-listed but is `#[repr(C)]` — owner
  approval per Rule 7). Hosts that never call `unload_bundle` are completely unaffected;
  existing hot-reload semantics are byte-for-byte unchanged.
- **The handle size change is the only unavoidable break.** Every host/plugin
  recompiles against the 8-byte handle. This is acceptable **only pre-1.0** — which is
  exactly why this must land before the v1.0 freeze. After 1.0 it is impossible.
- **Doc updates:** TRUST_MODEL.md §5/§6/§7 (handle size, generation now real,
  StaleHandle now produced, use-after-unload now caught), CLAUDE.md FFI/Quick-Reference
  handle-size notes, HOT_RELOAD_DESIGN.md cross-reference, a new "Unload" section in
  FEATURES.md.

### 9. Cost / risk table + phased plan

| Risk | Severity | Mitigation |
|---|---|---|
| UAF on native `dlclose` with live cached pointer | **High** | Reclaim is opt-in + host-attested + leak-checked; default Retire is UAF-free |
| Handle ABI break ripples to 5 SDKs + validator | Medium | Single coordinated PR; generator-driven; layout tests gate it |
| Generation wrap (u32) false-valid resolve | Low | 4.29e9 cycles/slot; bounds+empty check still mostly catches |
| Per-bundle retire bookkeeping bug drops wrong dylib | Medium | Key `retired` by `BundleId`; unload stress test per bundle |
| Dependent-bundle orphaning | Medium | Refuse-by-default; explicit cascade opt-in |
| .NET/Python "unload" overpromised | Medium | Honest verdicts above; ship invalidate-only for both |

**Phases (each independently shippable):** Phases 1–4 have **SHIPPED**; Phase 5 is deferred.

1. **Phase 1 — Generation field + StaleHandle (ABI). [SHIPPED]** Add `generation` to handle,
   slot generation, resolve check, `RegistryError::StaleHandle` → `AbiErrorCode::StaleHandle`.
   Update all 5 SDK ABIs + layout tests + validator + generator. *No unload yet — this
   alone makes the documented generation/StaleHandle contract true and is the only ABI
   break.* Tests: layout tests; a resolve-after-bump returns StaleHandle.
2. **Phase 2 — Invalidate-only `unload_bundle` (Retire mode). [SHIPPED]** Add
   `RuntimeStore::invalidate_bundle`, `Runtime::unload_bundle`, dependent-refusal,
   unload phase callback. No freeing. Wire/resolve the `HostApi` vs `RuntimeApi` D0
   question here. Tests: extend `stress_hot_reload.rs` / `concurrent_reload.rs` with
   unload; assert handles go stale, `find` stops returning, retire storage still keeps
   old pointers valid (no UAF), ASan clean.
3. **Phase 3 — VM true reclaim. [SHIPPED]** Free Lua/JS VMs at the
   `in_dispatch_threads`-empty safe point (best-effort, host-coordinated — see §7
   Correction); Python `sys.modules` purge under `UnloadMode::Reclaim`. Tests:
   load→unload→reload loop asserts bounded memory (no monotonic growth) for VM bundles.
4. **Phase 4 — Native opt-in Reclaim. [SHIPPED]** `UnloadMode::Reclaim`, per-bundle
   `NativeLoader::retired`, `dlclose`/`FreeLibrary` gated on the best-effort
   `reclaim_safe` (`Arc::strong_count`) net + the `Unloading` callback. Tests in
   `crates/polyplug_native/src/loader.rs`: Reclaim-mode unload `dlclose`s a quiescent
   bundle; `reclaim_safe=false` retires (defers) instead; the on-disk DLL becomes
   removable after Reclaim-mode unload (Windows file-lock release).
5. **Phase 5 (future, not gated by this doc) — .NET collectible ALC** per-bundle for
   true managed unload. **[DEFERRED]** — C# guests register native fn ptrs, so even
   with ALC unload the host-cached-pointer caveat applies.

### Deferred Work

Two unload-area items remain deliberately deferred (recorded here, not abandoned):

- **Call-arena retain-and-rewind (perf).** Deferred: it changes the arena's
  free-on-`reset()` contract to a teardown/`Drop` model that ripples into the C++ and
  other generated host callers (only exercised in CI), so it is a separate dedicated
  effort. Nothing in the unload phases depends on it; it can ship independently at any
  time. Tracked as the "Call-arena reset policy" note in §"Core Concepts".
- **D11 — native live-instance counter.** Deferred. The host owns the instance
  lifecycle: `create_instance` / `destroy_instance` are direct guest-vtable calls the
  runtime never mediates. A runtime-side counter via `get_extension` would therefore
  either duplicate knowledge the host already has, or require auto-increment code
  emitted into every native host-caller generator (CI-only validation). Not added
  pre-freeze without an explicit owner decision. See Decision Point D11.

---

## Decision points for the owner

- **D0 — `HostApi` vs `RuntimeApi` for the unload entry point.** `RuntimeApi` already
  declares `unload_bundle` at offset 24 but is unwired and unconsumed; `HostApi` is the
  live table and has no unload field. **Recommendation:** add `unload_bundle` to
  `HostApi` (the real table) and **retire `RuntimeApi`** (surface it to you for
  removal rather than maintaining two parallel facades) — do not silently delete it.
- **D1 — Reclamation strategy.** Options A (per-call validate), B (global quiescence),
  C (hybrid invalidate-now/free-at-safe-point). **Recommendation: C.**
- **D2 — Default unload behavior.** **Recommendation:** `UnloadMode::Retire` default
  (zero change for existing hosts); `Reclaim` opt-in via `RuntimeConfig`.
- **D3 — Does reload bump generation?** **Recommendation:** yes (so post-reload handles
  are explicit), while keeping retire-not-drop's old-pointer validity. Argue if you'd
  rather reload leave generation untouched for max compatibility.
- **D4 — Dependent-bundle policy.** refuse / cascade / orphan. **Recommendation:**
  refuse by default, explicit `unload_bundle_cascade` opt-in, no orphan.
- **D5 — `reload = unload + load`?** **Recommendation: no** — keep distinct (archive
  reverted this; opposite contracts).
- **D6 — Generation width.** `u32` (8-byte handle, align 4) vs other splits.
  **Recommendation: `u32` generation, 8-byte handle.**
- **D7 — Force semantics.** Separate `unload_bundle_forced` vs a `force: bool` param.
  **Recommendation:** separate method to keep the safe path obvious.
- **D8 — Native Reclaim residual trust.** Accept that native `dlclose` relies on host
  attestation (cannot prove quiescence without Option A's hot-path tax). **Recommendation:**
  accept it — consistent with TRUST_MODEL.md's trusted-same-process posture and
  hot-reload's existing `Preparing` host-contract.
- **D9 — Phasing.** Ship Phases 1-4 as listed; .NET collectible ALC (Phase 5) deferred.
  **Recommendation:** approve the phase order; each phase is independently shippable.
- **D10 — ABI break timing.** The 8-byte handle is a hard break and **must** land
  pre-1.0. **Recommendation:** approve the handle change now, before the v1.0 freeze.
- **D11 — Instance-liveness gating (optional, native-only).** If the runtime should
  *refuse* a native `Reclaim` unload while instances are live — rather than relying
  solely on the `Arc::strong_count` heuristic — implement a per-bundle live-instance
  counter as an **extension via `get_extension` (offset 144)**, not as a core-ABI
  change and not as an arena. Host SDK caller wrappers (generated by `polyplugc`)
  would bump the counter at `create_instance` and decrement at `destroy_instance`;
  these are the teardown path, never the dispatch hot path, so the zero-overhead
  pillar is untouched. The counter lives behind `get_extension`, keeping the 152-byte
  `HostApi` unchanged and the approach strictly opt-in — consistent with CLAUDE.md §7
  ("new functionality goes through the extension system"). **Honest caveat:** a
  live-instance counter *detects* that instances remain; it does not make
  destructor-less reclaim safe, because the destructors live in the dylib being
  closed. The "destroy instances first" host contract remains mandatory; the counter
  only allows the runtime to enforce it rather than assume it.
  **Recommendation:** build D11 in Phase 4 alongside native Reclaim, not before.

---

## See Also

- [HOT_RELOAD_DESIGN.md](./HOT_RELOAD_DESIGN.md) — the `Preparing`/`Reloaded`/`Failed`
  callback contract this design reuses for unload coordination
- [TRUST_MODEL.md](../TRUST_MODEL.md) — §5 handle validation & the generational-index
  inconsistency this design closes; §7 ABI freeze timing; Hot-Reload Safety Guarantees
- [PERFORMANCE.md](./PERFORMANCE.md) — zero-overhead hot-path rationale (why Option A is
  rejected as default)
