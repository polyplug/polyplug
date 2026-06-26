# Trust Model — polyplug

To *report* a vulnerability privately, see [`SECURITY.md`](../SECURITY.md); the rest of this page is the security *model* — the boundaries, dependency-enforcement mechanisms, and trust assumptions of the polyplug runtime. Its defining property: dependency enforcement happens once at load time (Phase 1), never on the per-call hot path (Phase 2).

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
loader = "native"

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
- **Handle Validation**: A `GuestContractHandle` is `{ index: u32, generation: u32 }` (8 bytes, align 4). `resolve_guest_contract` bounds-checks the index, confirms the slot still holds an interface, and then compares the handle's `generation` to the slot's current generation — returning `AbiErrorCode::StaleHandle` (5) on mismatch. An out-of-bounds index or an empty slot returns `RegistryError::InvalidHandle`. The slot generation is bumped when a bundle is unloaded, so any handle minted before the unload resolves to `StaleHandle` afterwards. Use-after-unload is therefore actively caught by the generation check. After a hot-reload swap (which does not bump the generation) the same handle remains valid and resolves to the interface now occupying that slot. The null handle is `{ index: u32::MAX, generation: 0 }`.

### Multi-impl Scenario
Consider an application that supports multiple audio decoders. Both `flac-bundle` and `mp3-bundle` might register the same `audio.Decoder` contract.

1. **`find_guest_contract`**: The first one to register (e.g., `flac-bundle`) will be returned as the system default.
2. **`find_by_bundle`**: The host can explicitly ask for the `mp3-bundle` implementation.
3. **`find_all_by_contract`**: The UI can enumerate all available decoders to show a selection list.

### Cross-call dispatch (plugin → plugin)

The `polyplugc`-generated **peer caller** is how one plugin invokes a method on
another plugin's guest contract. It resolves the target interface **once** (through
`find_guest_contract`, which enforces the declared-dependency customs check below)
and then dispatches **directly through that cached interface** — no per-call host
round-trip and no epoch pin. Its lifetime safety does **not** rest on the pin: the
declared dependency makes the runtime **refuse to unload the provider** while a
dependent is live, so the cached interface cannot be reclaimed under an in-flight
call, and a hot-reload is caught by an acquire-load of the registry **revision
counter** before each dispatch, which re-resolves the swapped-in interface only when
the registry actually changed. (QuickJS guests cannot dereference a raw pointer, so
a JS peer caller routes through the host-mediated `callGuestMethod` bridge instead.)

There is no longer a `call_guest_method` ABI field. The `polyplugc`-generated peer
callers for all native languages dispatch directly through the cached interface (the
same path as any host→guest caller). For QuickJS, the JS loader's `callGuestMethod`
bridge resolves the interface and dispatches directly without re-entering the host
ABI. The per-call host round-trip and epoch pin that `call_guest_method` provided
are gone; the declared-dependency-refuses-unload guarantee is what keeps the cached
interface valid.

- **Arena forwarding**: the `arena` argument is passed straight to VM dispatch
  (Lua, JS, Python) as an explicit per-call argument — never via a VM global
  (Rule 12); native dispatch ignores it (native function pointers carry no arena
  slot). A **null arena** means "no arena" and the threaded arena allocator falls
  back to `host->alloc`.
- **Re-entrancy guard**: a cross-call that would re-enter a VM already executing a
  dispatch *on the same thread* returns `AbiErrorCode::ReentrantCall` (9) — nested
  same-thread entry would deadlock or panic the VM's own lock. Concurrent dispatch
  into the same VM from *different* threads is serialized by the VM's internal
  locking and proceeds normally, as do cross-VM calls (e.g. a Lua plugin calling a
  JS plugin).

**Zero per-call authorization.** The generated peer caller performs no per-call
dependency check — it is a Phase 2 hot path (see §4). Trust is established once,
at load time, through the
declared-dependency verification that runs during the init window: while a bundle's
`polyplug_init` is in flight (`INIT_BUNDLE_ID != 0`), `find_guest_contract` /
`find_all_guest_contracts` reject any `contract_id` the calling bundle did not
declare in its manifest, returning a null handle. A plugin can therefore only
obtain (and cache) an interface for a contract it declared a dependency on; once it
has cleared customs in Phase 1, cross-calling that instance is unchecked by design. Outside any init window (host-side lookups, `INIT_BUNDLE_ID == 0`),
lookups are unrestricted. There is no `find_by_bundle`-style scoped enforcement on
the cross-call path — the instance's own `contract_id` is the only routing input.

### Reference: Frozen Struct Layouts
The following table summarizes the sizes and alignments of the core ABI types on 64-bit systems.

| Type | Size (bytes) | Alignment (bytes) | Key Fields |
|------|--------------|-------------------|------------|
| `HostApi` | 184 | 8 | `runtime` opaque ptr + 21 function pointers + trailing `reserved` data ptr |
| `GuestContractInterface` | 56 | 8 | `contract_id`, `contract_version`, `dispatch_type`, `create_instance`, `destroy_instance`, `dispatch` union |
| `GuestContractHandle` | 8 | 4 | `index: u32`, `generation: u32` |
| `StringView` | 16 | 8 | `ptr`, `len` |
| `AbiError` | 24 | 8 | `code`, `message` (StringView) |

`HostApi`'s 21 function pointers (offsets verified in
`crates/polyplug_abi/src/host/host_api.rs`): `register_guest_contract` (8), `alloc` (16),
`free` (24), `find_guest_contract` (32), `find_all_guest_contracts` (40),
`resolve_guest_contract` (48), `get_host_contract` (56),
`resolve_host_contract_interface` (64), `list_bundles` (72), `get_dependencies` (80),
`load_bundle` (88), `reload_bundle` (96), `register_host_contract` (104),
`register_loader` (112), `get_last_error` (120), `get_error_len` (128),
`unload_bundle` (136), `log` (144),
`create_guest_instance` (152), `destroy_guest_instance` (160), `revision_counter` (168),
`reserved` (176, data pointer — always null). There is no
`find_by_bundle`, `call_guest_method`, or `resolve_plugin` pointer in `HostApi`.

### Pointer Validity After Resolution
The C ABI deals in raw handles and pointers: `find_guest_contract` returns a
`GuestContractHandle` (a slot index plus generation stamp) and `resolve_guest_contract`
validates the generation then turns it into a `*const GuestContractInterface` borrowed
from the slot's `Arc<GuestContractInterface>`.

There is no `PluginGuard`/`VTableSlot` RAII guard in the runtime. Pointer validity for
runtime-mediated calls is guaranteed instead by crossbeam-epoch. A reader pins the epoch
(`crossbeam_epoch::pin()`), atomically loads the immutable published `ReadView`, and serves
the call lock-free; a writer republishes a new `ReadView` under the write lock and
`defer_destroy`s the old one, whose deferred free runs only after every guard pinned in the
old epoch unpins. A reader pinned before a reload or unload therefore keeps both the old
interface `Arc` and the still-mapped library alive until it unpins — there is no window in
which a live interface points at an unmapped library. To observe a new version after a
reload, a caller must re-`find_guest_contract` and re-`resolve_guest_contract`.

Direct FFI host-callers do **not** pin per call — they take the fast path and rely on the
documented quiesce-before-unload contract. Caching a raw `*const GuestContractInterface`
and using it after the owning bundle is unloaded is **undefined behaviour**; the host must
quiesce all callers of a bundle before unloading it.
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

### Runaway-plugin watchdog / per-call resource limits — explicit non-goal

**polyplug does not enforce per-call wall-clock timeouts, memory caps, or any in-runtime resource limit on a dispatch.** This is intentional and, like crash isolation, follows from the zero-overhead hot path.

Detecting that a *specific* call exceeded a deadline requires recording when that call started, in a place a monitor thread can read. That recording lives on the dispatch hot path and is not free: a monotonic clock read is ~15–30 ns, and even the cheapest design (no clock read on the hot path, just an atomic "in-flight" flag set at call start and cleared at call end, with the watchdog stamping its own observation time) still costs two atomic stores per call plus cache-coherence traffic. polyplug's safe dispatch is ~0.5 ns over raw FFI; any of these would multiply that by 2–60×. The zero-overhead invariant is non-negotiable, so the watchdog is not built.

There is also no safe way to *interrupt* a running native call: in-process native code is not asynchronously cancellable (it may hold a lock or be mid-allocation), so even a watchdog that detected an overrun could not stop it without risking corruption.

polyplug's position: **per-call timeouts are an application/host concern.** A host that needs to bound a call's duration runs it on a worker thread it controls and enforces its own deadline *around* the polyplug call, leaving dispatch zero-overhead — the same pattern by which tracing is implemented as a `host.logger`-style host contract rather than baked into the runtime.

### Failure responsibility at the ABI boundary

Who is responsible for turning a failure into an `AbiError` is fixed by contract, not by a catch-all guard somewhere in the runtime:

- **Each language converts its own failures.** A plugin's generated glue is responsible for catching *its own* language's failures (Rust `panic!` → `catch_unwind`; C++ `throw` → `catch(...)`; C# exception → `try/catch`; Lua/JS error → `pcall`/`try`) and returning `AbiError { code: Panic, … }`. This conversion happens *inside* the plugin, before control returns across the C ABI. It is zero happy-path cost — table-driven exception handling adds nothing to the ~2.4 ns native dispatch when no failure occurs.
- **The runtime never absorbs foreign failures.** polyplug does **not** wrap calls *into* a plugin (`polyplug_init`, native dispatch) in `catch_unwind`. Such a guard would be a false promise: it cannot catch a C/C++ exception (only Rust panics), and a modern Rust plugin's own `extern "C"` boundary aborts on a panic that escapes its glue — that abort fires first. An unwind or exception that *leaks across* the ABI is therefore a plugin defect with a defined outcome — **process abort** — identical to the SIGSEGV case above. The native loader pushes/pops its init-window bundle id around the `polyplug_init` call for dependency enforcement; it does not, and cannot meaningfully, contain a foreign unwind.
- **The two runtime exports are the only runtime-side guards.** `polyplug_runtime_create` and `polyplug_runtime_destroy` each wrap their body in `catch_unwind`. These guards exist solely for the **embedder guarantee**: a bug in polyplug's *own* create/destroy code surfaces as a null/no-op result (plus a recorded `last_error`), never as a panic that aborts the embedding host process. They do not — and are not meant to — catch plugin failures. The `HostApi` field operations are intentionally unguarded: a bug in the runtime there fails fast.

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
- **`HostApi` (184 bytes)**: An opaque `runtime` pointer followed by 21 function pointers and a trailing `reserved` data pointer (full list in §5).
- **`GuestContractInterface` (56 bytes)**: `contract_id`, `contract_version`, `dispatch_type`, the `create_instance`/`destroy_instance` callbacks, and the `dispatch` union.
- **`GuestContractHandle` (8 bytes)**: `index: u32` (offset 0) and `generation: u32` (offset 4), align 4.
- **`StringView` (16 bytes)**: 8-byte pointer, 8-byte length.

### Extensibility via host contracts
To support future capabilities without breaking the ABI, the host exposes contracts through
`HostApi::get_host_contract(contract_id, min_version)` (and
`resolve_host_contract_interface`). New host-side capabilities are added as new host contracts
that plugins resolve by ID, rather than by extending the frozen `HostApi` struct.
The trailing `reserved: *const c_void` field (offset 184) is the only sanctioned
post-freeze expansion slot; producers set it to null, consumers must not read it.

## Hot-Reload Safety Guarantees

Hot-reload swaps a bundle's live interface in place without freezing the runtime, and the
trust-relevant guarantee is that the swap is **memory-safe under concurrent readers**. A
reader pins a crossbeam-epoch guard before resolving an interface, so a `*const
GuestContractInterface` resolved under that pin keeps both the old interface `Arc` and its
still-mapped library alive until the guard unpins — there is no window in which a live
interface points at an unmapped library. To observe the new version, a caller
re-`find_guest_contract`s and re-`resolve_guest_contract`s.

- **Reload failure leaves the active version untouched.** If `loader.reload()` (which calls
  `polyplug_init`) fails, no interface swap occurs and the pre-reload state stays live.
- **Hot-reload must be enabled in config.** `reload_bundle` and the native loader's
  `reload()` both return `RuntimeError::HotReloadDisabled` when `hot_reload_enabled` is false.
- **Reloadability by loader.** Native, Lua, and JavaScript (QuickJS) bundles are reloadable.
  The Python and .NET loaders return `RuntimeError::HotReloadDisabled` from `reload()` because
  CPython and the CLR initialize once per process.
- **Handles survive a reload.** A hot-reload swap does **not** bump the slot generation, so a
  `GuestContractHandle` minted before the swap stays valid and resolves to the new
  interface — contrast unload, which bumps the generation (see §5 and the Unload Trust Model
  below).

For the swap mechanism, the three-phase `Preparing` / `Reloaded` / `Failed` callback
contract, the informational live-instance leak check, and reload serialization, see
[`HOT_RELOAD_DESIGN.md`](HOT_RELOAD_DESIGN.md).

## 8. Security & Lifecycle Hardening

The trust model continues to evolve as polyplug expands its reach into more dynamic
environments. The features below are implemented; one planned item closes the section.

### Bundle Signing

A bundle directory can carry a detached `bundle.sig` file produced by the
`polyplug_signing` crate (Ed25519 over SHA-256, pure Rust). The host's
`RuntimeConfig.signature_policy` (`SignaturePolicy`: `Off` = 0 default,
`WarnOnly` = 1, `Required` = 2) is enforced at load, immediately after manifest
validation and before any loader runs (see `Runtime::load_manifest_with_source`).
`Required` rejects an unsigned bundle with `LoaderError::UnsignedBundle` and a
tampered/invalid one with `LoaderError::SignatureVerificationFailed`; `WarnOnly`
logs the same failure at `LogLevel::Warn` and continues; `Off` skips the check.

**Canonical digest.** The signed message is a SHA-256 over a deterministic buffer
built from every file in the bundle except `bundle.sig`. The buffer is prefixed,
in order, by a fixed domain-separation tag (`polyplug-bundle-sig\0`, 20 bytes), a
1-byte algorithm version (`0x01`), and the file count as a `u64` little-endian;
then, for each file sorted by its `/`-relative path, it appends the path's UTF-8
bytes, a `0x00` separator, and the SHA-256 of the file's contents. Symlinks and
irregular files (fifo/socket/device) are **rejected**, and an empty bundle is
**rejected** — a signable bundle is a non-empty plain tree of regular files and
directories. Because symlinks are excluded from the digest but would still be
`dlopen`ed, rejecting them closes a signature-bypass hole; the native loader
additionally confines the artifact path to the bundle directory (no `../`
traversal, no symlink escape) as defense-in-depth. Any added, removed, or modified
file changes the digest, so the signature covers the manifest and every artifact
uniformly. `bundle.sig` (6-byte magic + version + 32-byte verifying key + 64-byte
signature) embeds the signer's **public** key, which therefore travels with the
bundle.

**Trust is freedom-preserving (TOFU), by design.** Verification proves a bundle is
**intact** and **self-consistently signed** — nothing more. It deliberately does
**not** require the host to pre-know or allowlist the signer's key: an application
that loads third-party plugins almost never knows every author up front, and
forcing an allowlist would defeat the whole point of an open plugin ecosystem.
App users stay free to load plugins from unknown authors; the policy only governs
*tamper detection*, not *author approval*. `verify_bundle` returns the embedded
`VerifyingKey` so a host that *wants* stricter trust can opt into the key-pinning
layer below — the `BundleVerifier` trait is the seam for that (`Ed25519Verifier`
is the TOFU default; `PinnedKeyVerifier` is the pinning implementation).

**Key pinning (`trusted_keys`) — authenticity on top of integrity.** A host that
*does* know which authors it trusts can pin them by populating
`RuntimeConfig.trusted_keys` (an `Array<Ed25519PublicKey>`, where each
`Ed25519PublicKey` is the raw 32-byte verifying-key encoding). The two layers are
distinct and complementary:

- **TOFU (empty `trusted_keys`, the default)** gives *integrity + self-consistency*:
  the bundle is intact and signed by *some* key, but not necessarily by anyone you
  trust. Self-signing is normal and expected here — it is not forging.
- **Pinning (non-empty `trusted_keys`)** adds *authenticity*: after the normal
  Ed25519 verification succeeds, the runtime additionally requires the key embedded
  in `bundle.sig` to be a member of the allowlist. A bundle re-signed with an
  attacker's key — which passes TOFU because its self-signature is internally
  valid — is rejected with `LoaderError::UntrustedSigningKey` (under `Required`) or
  logged and skipped (under `WarnOnly`). Only **public** verifying keys are pinned;
  the private signing key stays offline with the author. An empty allowlist would
  reject every bundle, so the runtime only switches to the pinning verifier when at
  least one key is configured. `RuntimeBuilder::trusted_keys(&[VerifyingKey])` is
  the Rust-host ergonomic entry point, and every other host SDK exposes the
  equivalent builder setter (`trusted_keys` / `TrustedKeys` / `trustedKeys` for
  cpp/csharp/python/lua/js); the keys are copied into the runtime during `create`,
  which then owns them for its lifetime, so a host SDK only lends its buffer for
  that call and may release it as soon as `create` returns (the persisted config
  never points at freed caller storage). A malformed key in the host allowlist fails the load with
  `LoaderError::MalformedTrustedKey`. Pinning never weakens the open-ecosystem
  default — it is purely opt-in.

**Tooling.** `polyplugc keygen --out <dir>` writes `signing.key` (private, `0o600`
on Unix) and `verifying.key` (public); `polyplugc sign --bundle-dir <dir> --key
<signing.key>` runs the same checks as `validate --bundle-dir` then writes
`bundle.sig`; `polyplugc verify --bundle-dir <dir>` exits non-zero on any failure.

**ABI note.** `signature_policy` was added at offset `0x2C` of `RuntimeConfig`,
filling the 4-byte tail padding after `log_max_level`. Key pinning then added
`trusted_keys` (a 24-byte `Array<Ed25519PublicKey>`) at offset `0x30`, growing the
struct from 48 to **72 bytes**, align 8 (both owner-approved pre-1.0 ABI changes).
The new `Ed25519PublicKey` type is a `#[repr(C)]` 32-byte (`align 1`) value. Every
pre-existing field offset is unchanged; all six host SDK abi mirrors carry the new
type and field and default `trusted_keys` to an empty `Array`, so existing hosts
that zero-initialize the config get TOFU and are unaffected.

### Unload

`HostApi.unload_bundle(this, bundle_id)` (offset 136) is live.
`Runtime::unload_bundle(bundle_id)` refuses if any still-loaded bundle declared a
dependency on a contract this bundle provides (`RuntimeError::DependencyInUse`);
`Runtime::unload_bundle_cascade(bundle_id)` unloads dependents first. Unload bumps the
slot generation (all stale handles return `AbiErrorCode::StaleHandle`), removes the bundle
from all registry indices, reclaims the superseded interface `Arc` and the loader-owned
mapping / VM state via epoch-deferred reclamation, and fires the `on_reload` callback
with `ReloadPhaseType::Unloading` (3) before invalidation so the host can quiesce. See
`docs/UNLOAD_DESIGN.md`.

#### Unload Trust Model

Unload always reclaims when safe — there is no opt-in mode and no retain tier. The
generation bump and index removal are unconditional and fully safe. Loader-owned resources
(the superseded interface `Arc` and the mapping / VM) are `defer_destroy`d under the write
lock through crossbeam-epoch and run only once no reader is still pinned in the prior epoch.

- **Runtime-mediated calls are epoch-safe.** `create_guest_instance` (offset 152) and
  `destroy_guest_instance` (offset 160) pin the epoch across their operation, so a
  lifecycle call racing a concurrent unload keeps the interface and its mapping alive
  until the guard unpins.
- **Direct FFI host-callers are host-coordinated.** Direct FFI callers do not pin per call;
  they take the fast path and rely on the documented quiesce-before-unload contract. The
  host must not call a bundle concurrently with unloading it (trusted-same-process model).
  Caching a raw `*const GuestContractInterface` and using it after the bundle is unloaded is
  **undefined behaviour** — the same posture as hot-reload's `Preparing` contract.

The per-loader reclaim mechanics — native `dlclose` / `FreeLibrary`, Lua/JS VM drop, Python
`sys.modules` purge, and .NET collectible-ALC unload — are in
[`UNLOAD_DESIGN.md`](UNLOAD_DESIGN.md).

### Hot-Reload
Native, Lua, and JavaScript (QuickJS) bundles support hot-reload; the Python and .NET loaders
return `HotReloadDisabled` (CPython / CLR single-initialization). Reload is driven explicitly
by `reload_bundle` — there is no file-watcher and no automatic retry. See the Hot-Reload
Safety Guarantees section above and [`HOT_RELOAD_DESIGN.md`](HOT_RELOAD_DESIGN.md).

### Scripting and JS Bindings
Python, Lua, and JavaScript (QuickJS) plugins are implemented. All respect the same trust model rules — scripted plugins have their own bundle ID and declare dependencies in `bundle.toml`. The runtime enforces these through the same `INIT_BUNDLE_ID` mechanism used by native code.

### Priority Resolution (planned)
A weighting system for multi-impl providers is planned for a future version. This will allow the host or a "Coordinator Bundle" to assign priorities to implementations, ensuring that `find_guest_contract` returns the "best" provider rather than just the first one registered.
