# Trust Model — polyplug

This document defines the security boundaries, dependency enforcement mechanisms, and trust assumptions of the polyplug runtime platform.

## Executive Summary

**What this document covers:** How polyplug enforces dependency contracts between plugins without sacrificing performance.

**Key insight:** Dependency enforcement happens once at load time (Phase 1), not on every call (Phase 2). This gives us zero-overhead hot paths while maintaining architectural integrity.

**Trust boundaries:**
- Host app: Fully trusted (unrestricted access)
- Plugins: Semi-trusted (restricted to declared dependencies during init)
- Runtime: Root of trust (enforces all contracts)

**What we protect against:** Undeclared dependencies, version mismatches, use-after-unload, malformed binaries, null pointer inputs.

**What we don't protect against:** Malicious memory access, plugin crashes, privilege escalation. Plugins run in-process with full host privileges.

**Bottom line:** polyplug is an architecture enforcement tool, not a security sandbox. For untrusted plugins, use OS-level isolation (WASM, containers, separate processes).

---

This document defines the security boundaries, dependency enforcement mechanisms, and trust assumptions of the polyplug runtime platform.

## 1. Overview

The polyplug trust model governs how independent plugin bundles interact within a shared process space. Unlike OS-level process isolation, polyplug operates within a single address space, prioritizing performance and architectural integrity over hostile-actor sandboxing.

### Design Philosophy
The model is built on three pillars: **Bundle Identity**, **Declared Dependencies**, and a strictly defined **Enforcement Window**. Our design philosophy favors a "catch-at-load-time" approach. By verifying the dependency graph during the initialization phase, we eliminate the need for expensive per-call authorization checks in the runtime hot path. If a dependency exists and was declared, the call proceeds at the speed of a raw function pointer dereference.

### Scope of Trust
- **Host Application**: Fully trusted (Bundle ID 0). It has unrestricted access to all registered contracts.
- **Plugin Bundles**: Semi-trusted. They are restricted during their initialization phase to only the contracts they explicitly declared in their manifest.
- **Runtime**: The root of trust. It manages the registry, handles dlopen/dlsym operations, and enforces the dependency contracts.

## 2. Bundle Identity

Every plugin bundle is uniquely identified by a `bundle_id`. This 64-bit identifier is the FNV1a-64 hash of the bundle name string provided in the `bundle.toml` (or `manifest.toml`).

### ID Computation
The hash is computed using the FNV-1a algorithm, implemented in `crates/polyplug_utils/src/`.
```rust
// crates/polyplug_utils/src/lib.rs
pub fn bundle_id(name: &str) -> u64 {
    BundleId::new(name).id() // BundleId::new => fnv1a_64(name.as_bytes())
}
```
Contract IDs are namespaced before hashing to keep guest and host contracts in disjoint ID
spaces: `guest_contract_id(name, major)` hashes `"guest_contract:<name>@<major>"` and
`host_contract_id(name, major)` hashes `"host_contract:<name>@<major>"`
(see `crates/polyplug_utils/src/guest_contract_id.rs` and `host_contract_id.rs`).
The use of a 64-bit hash space ensures that for typical deployment sizes (hundreds or thousands of plugins), the probability of a collision is mathematically negligible.

### Deployment Constraints
- **Unique Names**: Bundle names must be unique within a single application deployment. A name collision results in a `bundle_id` collision, which the runtime will reject during the second bundle's registration.
- **Baking the ID**: The `polyplugc` compiler bakes the computed ID into the generated guest code as a constant. This allows the guest to identify itself to the host during the `polyplug_init` call.
- **Enforced at load**: The manifest's declared `id` is no longer trusted blindly. `Manifest::validate` (in `crates/polyplug/src/loader/manifest.rs`) recomputes `polyplug_utils::bundle_id(name)` and rejects the bundle with `LoaderError::BundleTampered { bundle, expected, found }` if the declared `id` does not match. A tampered or hand-edited manifest cannot impersonate another bundle's identity.

### The Null Bundle (ID 0)
A `bundle_id` of `0` is reserved. It represents the "System Context" or "Host Context".
- Internal runtime operations use ID 0 to bypass enforcement.
- The host application itself operates under ID 0.
- Any lookup performed when no `BundleInitGuard` is active defaults to ID 0, effectively disabling enforcement for the host.

## 3. Declared Dependencies

Dependencies are not discovered implicitly; they must be explicitly declared in the bundle's manifest file. This declaration forms a binding contract between the bundle and the polyplug registry.

### Manifest Example
A typical `bundle.toml` declaration looks like this:
```toml
[bundle]
name = "audio-engine"
runtime = "native"

[[dependency]]
kind = "contract"
contract = "audio.Decoder"
min_version = 1
```

### The Registration Flow
Dependency declaration is wired into `Runtime::load_bundle` (see
`crates/polyplug/src/runtime.rs`). Before the loader runs the plugin's
`polyplug_init`, the runtime:
1. Reads the `[[dependency]]` entries parsed from `manifest.toml` into
   `ManifestData::dependencies` (each `RawManifestDependency` carries an explicit
   `contract_id: GuestContractId`).
2. Collects those `contract_id`s for the bundle.
3. Calls `RuntimeStore::declare_bundle_dependencies(bundle_id, contract_ids)` to
   record them in the registry's allowed-set.
4. If `declare_bundle_dependencies` fails (an internal registry error), the bundle
   load is aborted with `RuntimeError::Registry(..)` before init ever runs.

This step happens **before** the loader's `load()` call so that the allowed-set is
populated by the time the plugin's `polyplug_init` resolves any contract.

### Enforcement Mechanism
`RuntimeStore` maintains a `HashMap<BundleId, HashSet<GuestContractId>>` mapping
each `bundle_id` to its declared `contract_id` set (`bundle_declared_deps`). During
the initialization window, the host's `find_guest_contract` callback
(`host_find_guest_contract`) consults this set via
`RuntimeStore::is_bundle_dependency_declared`. If a plugin attempts to resolve a
contract it did not declare, the callback returns a **null `GuestContractHandle`**
— the C ABI for `find_guest_contract` returns a bare handle with no error channel,
so a null handle is the denial signal, and the plugin never obtains the vtable. A
declared contract resolves normally. When a bundle is unloaded, its entry in
`bundle_declared_deps` is removed.

## 4. Enforcement Window

To maintain maximum performance, polyplug does not enforce dependencies on every single call. Instead, it uses a high-integrity "Enforcement Window" during the plugin's lifecycle.

### Phase 1 vs. Phase 2
The runtime distinguishes between the **Initialization Phase (Phase 1)** and the **Execution Phase (Phase 2)**.

```
### Diagram: Enforcement Flow
```
------------------------------|----------------------------
INIT_BUNDLE_ID != 0           |  INIT_BUNDLE_ID == 0
Strict Enforcement            |  Zero Overhead
Checks manifest declarations  |  Trusts Phase 1 results
Returns null if undeclared    |  Direct pointer dispatch
                              |
[Plugin] -> find_contract()   | [Plugin] -> call_vtable()
    |                         |      |
    v                         |      v
(Check Registry Deps)         | (Direct Dereference)
    |                         |
    +-- Allowed? -> Handle    |
    +-- Denied?  -> Null      |
```

### The Init-Window Bundle ID
The window is delimited by a per-thread `INIT_BUNDLE_ID` cell
(`crates/polyplug/src/runtime.rs`), driven by the loaders:
- **Entrance**: Immediately before calling `polyplug_init`, the loader sets the
  thread-local `INIT_BUNDLE_ID` to the bundle's ID via `set_init_bundle_id`. Every
  loader (native, Python, .NET, Lua, JavaScript) does this on the same thread that
  runs init.
- **Enforcement**: `host_find_guest_contract` reads `INIT_BUNDLE_ID` via
  `get_init_bundle_id`. When it is non-zero (a bundle's init is in flight), the
  callback verifies the requested `contract_id` against that bundle's declared
  dependencies and returns a null handle on a violation. When it is zero — i.e. a
  host-side lookup outside any init window — no dependency check runs.
- **Exit**: After `polyplug_init` returns, the loader calls `clear_init_bundle_id`,
  resetting `INIT_BUNDLE_ID` to 0 so subsequent host-side lookups are unrestricted.

This state is deliberately a thread-local rather than `Runtime`-instance state. The
init window is a transient, **re-entrant, per-thread** phase (a loader's init may
itself trigger a nested `load_bundle` on the same thread), and loads are
synchronous on the calling thread — so the window naturally tracks the thread of
control. It holds no durable runtime data (the registry, loaded bundles, and config
all remain instance-owned per the runtime-isolation rule); two runtimes never run
init concurrently on the same thread, and runs on different threads get independent
cells.

### Why Hot-paths are Unchecked
Once a plugin has successfully obtained a `GuestContractHandle` during Phase 1, it has effectively "cleared customs." Since the registry and the plugin's dependency set are immutable for the life of the process, there is no architectural reason to re-verify the same contract on every hot-path call.

## 5. Multi-impl Resolution

Polyplug allows multiple bundles to implement the same contract, enabling a rich ecosystem of providers. The runtime provides three distinct query APIs to resolve these implementations.

### Query APIs
1. **`find_guest_contract(contract_id, min_version)`**:
   The standard lookup. It returns the `GuestContractHandle` for the **first registered** provider that satisfies the version requirement. This is deterministic based on the load order.
2. **`find_by_bundle(bundle_id, contract_id, min_version)`**:
   A scoped lookup. This allows a caller to request an implementation from a specific provider bundle, bypassing the default resolution order.
3. **`find_all_by_contract(contract_id, min_version)`**:
   The enumeration API. It returns all providers for a contract. In the C ABI, the caller provides a pre-allocated buffer of `GuestContractHandle` elements which the host populates.

### Implementation Integrity
- **DuplicateProvider Rule**: The same `bundle_id` cannot register the same `contract_id` twice. This prevents internal ambiguity within a single bundle.
- **Cross-Bundle Multi-impl**: Different bundles *can* implement the same contract. The registry tracks these in a `Vec<u32>` of slot indices per contract ID.
- **Handle Validation**: A `GuestContractHandle` is `{ index: u32, generation: u32 }` (8 bytes, align 4). `resolve_guest_contract` bounds-checks the index, confirms the slot still holds an interface, and then compares the handle's `generation` to the slot's current generation — returning `AbiErrorCode::StaleHandle` (5) on mismatch. An out-of-bounds index or an empty slot returns `RegistryError::InvalidHandle`. The slot generation is bumped when a bundle is unloaded, so any handle minted before the unload resolves to `StaleHandle` afterwards. Use-after-unload is therefore actively caught by the generation check. After a hot-reload swap (which does not bump the generation) the same handle remains valid and resolves to the interface now occupying that slot (under the retire-not-drop model, the previously resolved raw pointer also remains valid and continues to serve the old version). The null handle is `{ index: u32::MAX, generation: 0 }`.

### Multi-impl Scenario
Consider an application that supports multiple audio decoders. Both `flac-bundle` and `mp3-bundle` might register the same `audio.Decoder` contract.

1. **`find_guest_contract`**: The first one to register (e.g., `flac-bundle`) will be returned as the system default.
2. **`find_by_bundle`**: The host can explicitly ask for the `mp3-bundle` implementation.
3. **`find_all_by_contract`**: The UI can enumerate all available decoders to show a selection list.

### Cross-call dispatch (plugin → plugin)

`HostApi::call_guest_method(host, instance, fn_id, args, out, arena)` lets one
plugin invoke a method on another plugin's guest contract through the host. The
caller passes a `GuestContractInstance` it already resolved; the host re-resolves
the target through the registry via `instance.contract_id` on **every** call. This
means a call issued after a hot-reload routes to the live (swapped-in) interface,
while the retire-not-drop model keeps any previously resolved pointer valid.

- **Arena forwarding**: the `arena` argument is passed straight to VM dispatch
  (Lua, JS); native dispatch ignores it (native function pointers carry no arena
  slot). A **null arena** means "no arena" and the guest bridge falls back to
  `host->alloc`.
- **Re-entrancy guard**: a cross-call that would re-enter a VM already executing a
  dispatch *on the same thread* returns `AbiErrorCode::ReentrantCall` (9) — nested
  same-thread entry would deadlock or panic the VM's own lock. Concurrent dispatch
  into the same VM from *different* threads is serialized by the VM's internal
  locking and proceeds normally, as do cross-VM calls (e.g. a Lua plugin calling a
  JS plugin).

**Zero per-call authorization.** `call_guest_method` performs **no** dependency
check of its own — it is a Phase 2 hot path (see §4). Trust is established once,
at load time, through the declared-dependency verification that runs during the
init window: while a bundle's `polyplug_init` is in flight (`INIT_BUNDLE_ID != 0`),
`find_guest_contract` / `find_all_guest_contracts` reject any `contract_id` the
calling bundle did not declare in its manifest, returning a null handle. A plugin
can therefore only obtain an instance of a contract it declared a dependency on;
once it has cleared customs in Phase 1, cross-calling that instance is unchecked
by design. Outside any init window (host-side lookups, `INIT_BUNDLE_ID == 0`),
lookups are unrestricted. There is no `find_by_bundle`-style scoped enforcement on
the cross-call path — the instance's own `contract_id` is the only routing input.

### Reference: Frozen Struct Layouts
The following table summarizes the sizes and alignments of the core ABI types on 64-bit systems.

| Type | Size (bytes) | Alignment (bytes) | Key Fields |
|------|--------------|-------------------|------------|
| `HostApi` | 160 | 8 | `runtime` opaque ptr + 19 function pointers |
| `GuestContractInterface` | 24 | 8 | `contract_id`, `functions` ptr |
| `GuestContractHandle` | 8 | 4 | `index: u32`, `generation: u32` |
| `StringView` | 16 | 8 | `ptr`, `len` |
| `AbiError` | 24 | 8 | `code`, `message` (StringView) |

`HostApi`'s 19 function pointers (offsets verified in
`crates/polyplug_abi/src/host/host_api.rs`): `register_guest_contract` (8), `alloc` (16),
`free` (24), `find_guest_contract` (32), `find_all_guest_contracts` (40),
`resolve_guest_contract` (48), `get_host_contract` (56),
`resolve_host_contract_interface` (64), `list_bundles` (72), `get_dependencies` (80),
`load_bundle` (88), `reload_bundle` (96), `register_host_contract` (104),
`register_loader` (112), `get_last_error` (120), `get_error_len` (128),
`call_guest_method` (136), `get_extension` (144), `unload_bundle` (152). There is no
`find_by_bundle` or `resolve_plugin` pointer in `HostApi`.

### Pointer Validity After Resolution
The C ABI deals in raw handles and pointers: `find_guest_contract` returns a
`GuestContractHandle` (a slot index plus generation stamp) and `resolve_guest_contract`
validates the generation then turns it into a `*const GuestContractInterface` borrowed
from the slot's `Arc<GuestContractInterface>`.

There is no `PluginGuard`/`VTableSlot` RAII guard in the runtime. Pointer validity is
guaranteed instead by the retire-not-drop model: the runtime never frees a superseded
interface `Arc` (it is moved into `retired_interfaces`) or a superseded native library (it
is moved into the loader's `retired` list) while the runtime lives. Consequently a
`*const GuestContractInterface` stays valid for the runtime's lifetime even across reloads
and unloads, continuing to serve the version it resolved. To observe a new version after a
reload, a caller must re-`find_guest_contract` and re-`resolve_guest_contract`.
## 6. Threat Model

The polyplug trust model is a **Software Architecture Enforcement Tool**, not a security sandbox. It is designed to prevent architectural erosion in large-scale systems.

### Capabilities Matrix

| Protection Type | Status | Description |
|-----------------|--------|-------------|
| Undeclared Dependencies | **YES** | Caught during initialization lookup. |
| Version Mismatches | **YES** | Rejected by `find_guest_contract` if version < `min_version`. |
| Use-after-Unload | **YES** | Caught by generation check in `resolve_guest_contract`: unload bumps the slot generation and any stale handle returns `AbiErrorCode::StaleHandle` (5). |
| Malformed / corrupted binaries | **YES** | Invalid UTF-8, truncated, wrong magic, missing `init` — all return clean errors. |
| Null pointer inputs to C facade | **YES** | All `polyplug_rt_*` functions null-check every pointer at entry. |
| Double-free of host memory (debug) | **YES** | `TrackingAllocator` panics on double-free in `cfg(debug_assertions)`. ASan in CI. |
| Malicious Memory Access | **NO** | Plugins share the same address space and can read/write any memory. |
| Malicious Symbol Access | **NO** | A plugin can use `dlopen(NULL, ...)` to find host symbols directly. |
| Denial of Service | **NO** | A plugin can loop infinitely or exhaust host memory. |
| Plugin crash isolation | **NO** | A plugin segfault kills the host process — by design (see below). |
| Data exfiltration / privilege escalation | **NO** | Plugins run with the same OS privileges as the host process. |

### The "Trusted Same-Process" Assumption

polyplug assumes that all loaded bundles are authorized to run by the host application. If you require protection against hostile code, you must wrap the polyplug host in an OS-level sandbox (e.g., Firecracker, WebAssembly, or Linux Namespaces).

### Plugin crash isolation — explicit non-goal

**A plugin that segfaults, triggers SIGABRT, or causes any fatal signal kills the host process.** This is intentional and by design.

Isolating plugin crashes requires either:
- **Out-of-process execution** — violates the zero-overhead hot path goal. A single indirect call becomes an IPC round-trip (~microseconds instead of nanoseconds). This is a fundamental incompatibility with polyplug's core design principle.
- **OS-level sandboxing** (seccomp, pledge, etc.) — platform-specific, adds significant complexity, and still cannot prevent all crash vectors.

polyplug's position: the hot path must be a single indirect call. Plugin crash isolation is incompatible with that goal. App developers who need crash isolation must run untrusted plugins in a separate worker process with their own IPC layer. polyplug is not the right tool for untrusted plugin execution.

### Input validation at the host boundary

Even with trusted plugins, malformed or corrupted plugin binaries are a real scenario. polyplug defends against these at load time:

- **Invalid UTF-8** — all strings extracted from plugin binaries are validated via `std::str::from_utf8`. Invalid UTF-8 is a hard load error. The runtime remains healthy after rejecting a bad bundle.
- **Truncated or wrong-magic binaries** — `libloading` returns a clean error; polyplug propagates as `RuntimeError::LoadFailed`.
- **Missing `init` symbol** — returns `RuntimeError::MissingSymbol`. Runtime remains healthy.
- **Unknown runtime value** — returns `RuntimeError::UnknownRuntime`.
- **Null pointer inputs** — all C facade functions null-check every pointer at entry. A null pointer returns a defined error code. No null pointer ever causes UB in polyplug runtime code.

### `from_utf8_unchecked` policy

`std::str::from_utf8_unchecked` is permitted **only** on host-owned data — data created and managed by the polyplug runtime or host application. Every use must have a `// SAFETY:` comment explaining why the data is guaranteed to be valid UTF-8.

`from_utf8_unchecked` is **never** used on data originating from a plugin binary or passed from a plugin across the ABI boundary. Such data always goes through `std::str::from_utf8` with a hard error on failure.

### `GuestContractInterface` immutability

`GuestContractInterface` pointers are treated as read-only after registration. Casting a `*const GuestContractInterface` to `*mut` and writing to it is undefined behavior. polyplug does not enforce this at runtime — enforcement is bypassable in-process. It is a contract that trusted plugins must uphold.

## 7. ABI Freeze Notice

The core polyplug ABI **freezes at v1.0**. There is no public release yet, so the project is currently pre-1.0: ABI-visible changes are still permitted, but only after explicit owner approval (see CLAUDE.md Rule 7). At and after v1.0 the freeze becomes absolute, ensuring that bundles compiled then remain binary-compatible with future runtime versions.

### Frozen Surface Areas
The following structures have the layouts and sizes that will be frozen at v1.0. At/after v1.0, any modification to these (e.g., adding a field or changing field order) is a breaking change. Sizes are verified by the layout tests in `crates/polyplug_abi`.
- **`HostApi` (160 bytes)**: An opaque `runtime` pointer followed by 19 function pointers (full list in §5).
- **`GuestContractInterface` (24 bytes)**: Fixed header before the function pointer array.
- **`GuestContractHandle` (8 bytes)**: `index: u32` (offset 0) and `generation: u32` (offset 4), align 4.
- **`StringView` (16 bytes)**: 8-byte pointer, 8-byte length.

### Extensibility via host contracts and extensions
To support future capabilities without breaking the ABI, the host exposes contracts through
`HostApi::get_host_contract(contract_id, min_version)` (and
`resolve_host_contract_interface`). New host-side capabilities are added as new host contracts
that plugins resolve by ID, rather than by extending the frozen `HostApi` struct.
In addition, `HostApi::get_extension(extension_id)` returns host-provided opaque
pointers keyed by a 32-bit FNV-1a hash of the extension name, giving the host a second,
ID-based extension channel that also avoids changing the struct layout.

## Hot-Reload Safety Guarantees

polyplug uses a **retire-not-drop** model for hot-reload. Superseded interfaces and
libraries are never freed while the runtime lives — they are retained so that any
raw pointer handed out before the reload stays valid for the lifetime of the runtime.

- **Interface swap is a single write-locked operation.** `RuntimeStore::apply_reload_swap`
  moves each freshly-registered `Arc<GuestContractInterface>` into the bundle's existing
  pre-reload slot under one write lock, so concurrent readers observe either the complete
  old state or the complete new state — never a half-swapped registry.
- **Superseded interfaces are retired, not dropped.** The old `Arc<GuestContractInterface>`
  is pushed onto `RuntimeStore`'s `retired_interfaces` list and held alive for the lifetime
  of the runtime. A `*const GuestContractInterface` obtained from `resolve_guest_contract`
  therefore stays valid even across reloads; it keeps serving the old version. Callers must
  re-find (`find_guest_contract`) and re-resolve to observe the new version.
- **Superseded native libraries are retired, not `dlclose`d.** The native loader moves the
  old `libloading::Library` into its `retired` list instead of unloading it, keeping the old
  code pages mapped so any in-flight raw function pointer stays valid.
- **Retired interfaces and libraries intentionally accumulate** for the lifetime of the
  runtime. This is a deliberate trade of memory for pointer-validity safety in the absence
  of generation/quiescence tracking.
- **Reload failure leaves the active version untouched.** If `loader.reload()` (which calls
  `polyplug_init`) fails, no interface swap occurs and the pre-reload state remains live.
- **Phase callbacks.** `reload_bundle` fires the host's `on_reload` callback with a
  `ReloadPhase { phase_type, bundle_id, bundle_name, reason }`: `Preparing` before the
  swap (host must destroy all live instances here), `Reloaded` after a successful swap, and
  `Failed` (with a reason string) on any failure. After the `Preparing` callback returns,
  the runtime emits an informational warning if `Arc::strong_count > 1` for any slot
  (instances may not have been destroyed) but proceeds with the reload regardless.
- **Hot-reload must be enabled in config.** `reload_bundle` and the native loader's
  `reload()` both return `RuntimeError::HotReloadDisabled` when `hot_reload_enabled` is false.
- **VM-backed bundles are not reloadable.** The Python, .NET, Lua, and JavaScript loaders all
  return `RuntimeError::HotReloadDisabled` from `reload()` (Python and .NET due to
  process-level single-initialization constraints of their interpreters/CLR; Lua and JS are
  disabled in this version).

> **Note:** `GuestContractHandle` carries a generation counter (bumped on unload; verified
> by `resolve_guest_contract`), but there is no `ArcSwap`, no `PluginGuard`/`VTableSlot`
> quiescence spin, no `QuiescenceTimeout`, and no cascade-depth cap in the current code. An
> earlier ref-counted reclamation design that used those mechanisms was removed in favor of
> the retire-not-drop model described above. Hot-reload remains retire-not-drop. **Unload**
> defaults to retire-not-drop (`UnloadMode::Retire`) but can opt into true resource reclaim
> (`UnloadMode::Reclaim` — native `dlclose`, Python `sys.modules` purge, Lua/JS VM drop);
> see the Unload Reclaim Trust Model below.

## 8. Future Work

The trust model continues to evolve as polyplug expands its reach into more dynamic environments.

### Unload ✅ done

`HostApi.unload_bundle(this, bundle_id)` (19th pointer, offset 152) is live.
`Runtime::unload_bundle(bundle_id)` refuses if any still-loaded bundle declared a
dependency on a contract this bundle provides (`RuntimeError::DependencyInUse`);
`Runtime::unload_bundle_cascade(bundle_id)` unloads dependents first. Unload bumps the
slot generation (all stale handles return `AbiErrorCode::StaleHandle`), removes the bundle
from all registry indices, retires the interface `Arc`, and fires the `on_reload` callback
with `ReloadPhaseType::Unloading` (3) before invalidation so the host can quiesce. See
`docs/UNLOAD_DESIGN.md`.

#### Unload Reclaim Trust Model

The invalidation above is unconditional and fully safe. Whether loader-owned resources are
*freed* is governed by `RuntimeConfig.unload_mode` (`UnloadMode { Retire (default),
Reclaim }`):

- **`Retire` (default):** the library/VM stays mapped after unload — any raw pointer
  resolved before the unload stays valid for the runtime's lifetime (retire-not-drop). No
  new trust assumptions.
- **`Reclaim` (opt-in) is host-coordinated**, exactly like hot-reload: the host must not
  call a bundle concurrently with unloading it (trusted-same-process model). The runtime's
  quiescence/leak checks are best-effort defense-in-depth, not a complete guarantee.
  - **Native (`dlclose`) is host-attested.** Native dispatch is raw function pointers into
    the library, so the runtime is **structurally blind** to in-flight native calls and
    cannot verify safety. Selecting `Reclaim` is the host's attestation that no thread is
    calling, or holds a pointer into, the bundle. A best-effort `Arc::strong_count` net
    (`reclaim_safe`) defers to retire when an `Arc` holder remains, but it cannot see raw
    in-flight calls. If the host's attestation is wrong, it is a host bug — the same posture
    as hot-reload's `Preparing` contract.
  - **VM reclaim (Lua/JS) is best-effort, not a guarantee.** The loaders drop a VM only
    when `in_dispatch_threads` is observed empty. But `host_call_guest_method` releases the
    registry lock *before* the VM registers the call in `in_dispatch_threads`, leaving a
    resolve→dispatch window where a call racing a cross-thread unload could free a VM under
    it. So VM unload relies on the same host-coordination contract; `in_dispatch_threads` is
    defense-in-depth.
  - **Python reclaim is memory-safe regardless of in-flight calls** — it only purges the
    bundle's re-keyed `sys.modules` entries (the import cache); CPython refcounts/GC keep
    any referenced objects alive.

### Hot-Reload ✅ done
Hot-reload is implemented for native bundles using the retire-not-drop model: superseded
interfaces and libraries are retained for the lifetime of the runtime so previously resolved
pointers stay valid, and the active version is swapped under a single write lock. Reload is
driven explicitly by `reload_bundle`; there is no file-watcher and no automatic retry (both
were deliberately removed). VM-backed bundles (Python, .NET, Lua, JS) are not reloadable and
return `HotReloadDisabled`. See the Hot-Reload Safety Guarantees section above.

### Scripting and JS Bindings ✅ done (Epics 10–11, 11.5)
Python, Lua, and JavaScript (QuickJS) plugins are implemented. All respect the same trust model rules — scripted plugins have their own bundle ID and declare dependencies in `bundle.toml`. The runtime enforces these through the same `INIT_BUNDLE_ID` mechanism used by native code.

### Priority Resolution
A weighting system for multi-impl providers is planned for a future version. This will allow the host or a "Coordinator Bundle" to assign priorities to implementations, ensuring that `find_guest_contract` returns the "best" provider rather than just the first one registered.

### WASM sandbox support
Future versions will support WASM plugins in a sandboxed execution environment, providing crash isolation without IPC overhead. This is the planned path for untrusted plugin execution.

## Plugin crash isolation

Plugins run in-process. A plugin that dereferences a null pointer, causes a stack
overflow, or triggers any hardware exception (SIGSEGV, SIGBUS, SIGILL) takes down
the entire host process. **This is expected and intentional behaviour.**

Isolating plugin crashes would require either:
- Out-of-process execution with IPC — violates the zero-overhead hot-path goal
- OS-level sandboxing (seccomp, pledge) — platform-specific, adds significant complexity

Neither is acceptable for v1.

App developers who need crash isolation should run plugins in a separate worker process
and communicate via IPC. polyplug does not provide this facility.



// =============================================================================
// ABI FREEZE TARGET — v1.0 (currently pre-1.0, no public release yet)
// =============================================================================
//
// The following types and function signatures constitute the polyplug ABI that
// freezes at v1.0. While pre-1.0, changes to #[repr(C)] structs, function pointer
// signatures, or the field order of HostApi ARE permitted, but ONLY after
// explicit owner approval — never unilaterally. At and after v1.0, NO such changes
// are permitted.
//
// New functionality should go through the host contract mechanism (get_host_contract)
// or get_extension. For rationale and trust model, see TRUST_MODEL.md.
// =============================================================================
