# polyplug Features — Current State

A reference overview of what polyplug provides today and how each feature
behaves. This is a status/reference document; for the end-to-end pipelines (how
to author APIs, generate, build, assemble, and ship) see
[`WORKFLOW.md`](./WORKFLOW.md). Each section points to the deeper doc or the code
that owns the detail rather than restating it.

Status legend: shipped unless explicitly marked otherwise.

---

## 1. Cross-language plugin runtime

A host application loads plugin bundles at runtime; each bundle exports one or
more guest contracts the host discovers and calls through a frozen C ABI. Plugins
and hosts can be written in any of six languages.

| Role | Languages |
|---|---|
| Guest (plugin) | Rust, C++, C#, Python, Lua, JavaScript (QuickJS) |
| Host (app) | Rust, C++, C#, Python, Lua, JavaScript |

Key ABI facts (verified in `crates/polyplug_abi/src/host/host_api.rs`):

- **FFI surface is exactly two `#[no_mangle]` exports** — `polyplug_runtime_create`
  and `polyplug_runtime_destroy` (`crates/polyplug/src/ffi.rs`). Everything else
  is reached through function-pointer fields on `HostApi`.
- **`HostApi` is 184 bytes, align 8**: one opaque `runtime` pointer plus 21
  function pointers (`call_guest_method` at offset 136, `unload_bundle` at
  offset 144, `log` at offset 152, `create_guest_instance` at offset 160,
  `destroy_guest_instance` at offset 168) followed by a trailing
  `reserved: *const c_void` data pointer at offset 176 (always null;
  forward-compat room only).
  Layout is locked by `layout_host_api` in `host_api.rs`.
- **Plugin entry point is `polyplug_init(const HostApi*, const BundleInitContext*)`**
  (2 args). Plugins register via the self-passing pattern
  `host->register_guest_contract(host, &descriptor, &interface)`.
- **ABI freeze policy:** the ABI freezes at v1.0; the project is pre-1.0 today, so
  ABI-visible changes are permitted **only with explicit owner approval**, never
  unilaterally (CLAUDE.md Rule 7; [`TRUST_MODEL.md`](TRUST_MODEL.md) §7).

See CLAUDE.md (Architecture) for the crate map and loader list.

---

## 2. Code generation (`polyplugc`)

`polyplugc` has exactly two verbs (see [`WORKFLOW.md`](./WORKFLOW.md)):

- **`generate`** — emits both sides of the contract glue. `--api api.toml`
  produces host-side typed callers + registration glue; `--bundle bundle.toml`
  produces guest-side contract stubs, `polyplug_init`, dispatch shims, and a
  ship-ready `manifest.toml` with the precomputed `bundle_id`.
- **`validate --bundle-dir <dir>`** — drives the runtime loader's own manifest
  machinery so the CLI accepts exactly what the runtime would: manifest parses,
  `id == fnv1a_64(name)` (tamper check), the per-platform `[file]` entry resolves
  and exists, the artifact extension matches the declared `loader`, and `version`
  parses.

**Two separate codegen pipelines** (they share no language emitters by design,
per CLAUDE.md):

| Pipeline | Location | Driven by | Produces |
|---|---|---|---|
| ABI-SDK emitters | `crates/polyplug_codegen/src/languages/` | `polyplug_abi`'s build script (`build/generate.rs`) | the `sdks/*/abi` files |
| Contract generators | `crates/polyplugc/src/generators/` | the `polyplugc` CLI | per-contract host/guest bindings |

JS generation targets QuickJS only (`js_quickjs.rs`); there is no `js_deno.rs`.
That one generator emits both the QuickJS **guest** glue and the Deno **host
caller** (`host/callers.ts`, run under Deno against the Deno FFI host SDK). The
Deno host caller marshals the full ABI type universe — primitives (`u64`/`i64`
as native `bigint`), `bool`, enums (read/written UNSIGNED at their repr width),
`StringView`/`Buffer` (host-allocated, freed after dispatch), and one-level
structs — by packing a C-layout argument buffer and reading a C-layout out
buffer (`DataView` + `Deno.UnsafePointer`). Runtime-proven against a native guest
by `integration_host_deno_caller`.

**generate-e2e guarantee:** all six languages generate code that compiles/loads
with zero hand edits. Proof tests live in
`crates/polyplugc/tests/generate_e2e.rs` (rust → cargo build),
`generate_e2e_native.rs` (cpp → `c++ -shared`, csharp → `dotnet build`), and
`generate_e2e_vm.rs` (python → py_compile + import, lua → luajit load, js → deno
check). Each runs `generate`, drops the output into a minimal project the test
writes, and builds/loads it. The same suite asserts `validate --bundle-dir`
accepts a correct bundle and rejects a missing artifact and a tampered `id`.

---

## 3. Call Arena (Goal 3)

A `CallArena` is a per-call **bump allocator** the host hands to a VM dispatch
call so the guest writes variable-size return values (strings, arrays) into
host-controlled memory without a `host->alloc` round trip per value. It is a
40-byte `#[repr(C)]` struct (`crates/polyplug_abi/src/types/call_arena.rs`): a
primary `[base, end)` bump region (an inline buffer owned by the caller) plus a
fallback chain of host-allocated overflow blocks for returns larger than the
primary region. The caller resets the arena at the start of each call (a pointer
rewind plus a single free pass over any overflow blocks).

**Who benefits and who is excluded** (the exclusions are findings, not gaps):

| Path | Arena-routed? | Why |
|---|---|---|
| JS (QuickJS) guest returns | Yes | loader threads a per-call `arena_ptr` + `bridge` into dispatch; wrapper calls `bridge.arenaAlloc(size, arena_ptr)` (no `globalThis` — Rule 12) |
| Lua (LuaJIT) guest returns | Yes | loader threads `(arena_ptr, arena_alloc)` as the final dispatch args → `alloc_string_arena(arena_alloc, arena_ptr, s)` in the guest SDK |
| Rust / C++ / C# host callers | Yes | per-caller `CallArena` field threaded into VM dispatch |
| Native Rust / C++ / C# guest returns | N/A | returns are borrowed zero-allocation views into guest-owned memory — there is nothing to allocate, so no arena is needed |
| Python guest returns | N/A | Python guests dispatch through `DispatchType::Native` (ctypes function pointers); the native ABI signature carries no arena slot, exactly like native Rust/C++ |

**Lifetime rule:** a view returned from an arena-backed call is valid **until the
next arena-backed call on the same caller**; the caller resets its arena at the
start of each call. Guests never free arena allocations — the arena owns and
reclaims them on reset. To outlive the next call, copy out or use the explicit
`alloc`/`free` path.

**Null-arena fallback:** the VM dispatch signature is always
`call(loader_data, instance, fn_id, args, out, arena)`. A **null arena** means
"no arena": the guest bridge falls back to per-value `host->alloc`, so every path
stays correct whether or not an arena is supplied.

**Zero-alloc proof:** `tests/integration/tests/integration_js.rs` and
`integration_lua.rs` each run a 10,000-iteration string-returning echo loop and
assert the host allocator is hit **zero** times after warmup
(`ARENA_HOST_ALLOC_CALLS`). See [`PERFORMANCE.md`](./PERFORMANCE.md) (Call Arena
section) for the full discussion.

---

## 4. Hot-reload

Hot-reload is supported by the **native, Lua, and JS (QuickJS)** loaders — their
`reload()` re-reads the on-disk source and swaps the live interface. Readers serve
lock-free off an immutable published `ReadView`; a reload republishes a new
`ReadView` under the write lock and `defer_destroy`s the old one through
crossbeam-epoch, so the superseded interface and library are freed only after every
reader that was pinned in the prior epoch unpins.

- **Pinned-reader safety:** a reader holding a `crossbeam_epoch::pin()` guard
  observes a consistent `ReadView` for the whole call; a reader pinned before a
  reload keeps the old interface `Arc` and the still-mapped library alive until it
  unpins. To observe a new version, re-`find_guest_contract` and
  re-`resolve_guest_contract`.
- **Handle generations:** a `GuestContractHandle` survives a hot-reload swap (the
  slot generation is unchanged) and resolves to the new interface.
- **Swap is one write-locked operation** (`apply_reload_swap`): readers observe
  either the complete old or complete new state.
- **Opt-in:** `reload_bundle` returns `RuntimeError::HotReloadDisabled` unless
  `hot_reload_enabled` is set in `RuntimeConfig`.
- **Model-checked:** the publish/reclaim protocol that underpins both lock-free
  reads and safe unload is exhaustively model-checked with [loom](https://docs.rs/loom)
  (the `loom_epoch_model` crate, run via `just loom`). It proves a reader that
  pins across its dereference never observes reclaimed memory, and that dropping
  the guard early would race reclamation — making the pin demonstrably necessary.
  See [`UNLOAD_DESIGN.md`](./UNLOAD_DESIGN.md) → *Epoch Model* → *Model-checked with loom*.
- **Three-phase callback:** `Preparing` (host destroys its caller wrappers),
  `Reloaded`, `Failed`. The post-`Preparing` leak check is informational and
  never blocks. Failure leaves the active version untouched.
- **Cascade reload:** reloading a bundle that others depend on is supported.
- **Python and .NET are not reloadable:** both loaders return
  `HotReloadDisabled` from `reload()` unconditionally (interpreter/CLR
  once-per-process constraints). Lua and JS (QuickJS) reload like native does.
- **Windows-safe:** each version loads from a distinct on-disk filename (e.g.
  `reload_plugin_v1` vs `_v2`), so a reload never overwrites a file while it is
  mapped — the Windows DLL file-lock that would break overwrite-in-place reload
  does not apply.

Full design and flow: [`HOT_RELOAD_DESIGN.md`](./HOT_RELOAD_DESIGN.md);
safety guarantees: [`TRUST_MODEL.md`](TRUST_MODEL.md) (Hot-Reload Safety).

---

## 5. Unload (true unload)

`HostApi.unload_bundle(this, bundle_id)` is the 18th `HostApi` function pointer
(offset 144). It tears a bundle out of the runtime and reclaims its resources
through epoch-deferred reclamation — there is no opt-in mode and no "keep mapped"
tier; unload always reclaims once it is safe to do so.

- **Index removal + generation bump:** unload bumps the slot generation for every
  contract the bundle registered and removes the bundle from all registry indices.
  All previously minted `GuestContractHandle`s for those slots return
  `AbiErrorCode::StaleHandle` (5) on the next `resolve_guest_contract` call;
  `find_guest_contract` and `list_bundles` no longer return it.
- **Epoch reclamation:** the superseded interface `Arc` and the loader-owned
  resource (dylib mapping or VM state) are handed to crossbeam-epoch via
  `defer_destroy`. The deferred free runs only after every reader pinned in the
  prior epoch unpins, so a reader that pinned (`crossbeam_epoch::pin()`) before the
  unload keeps both the old interface `Arc` and the still-mapped library alive
  until it unpins. No reader ever observes freed memory.
- **Runtime-mediated calls are safe; raw FFI must quiesce:** calls that go through
  the runtime — `call_guest_method` (offset 136), `create_guest_instance`
  (offset 160), `destroy_guest_instance` (offset 168) — pin the epoch across
  dispatch and are always safe against a concurrent unload. Direct FFI callers do
  **not** pin per call (the fast path); they rely on the documented
  quiesce-before-unload contract. Caching a raw `*const GuestContractInterface` and
  using it after the owning bundle is unloaded is **undefined behaviour** — the
  host must quiesce that bundle before unloading it.
- **Live-instance warning:** the runtime keeps a per-contract live-instance counter
  keyed by contract id. Because `create_guest_instance` / `destroy_guest_instance`
  are host-mediated, every stateful instance (non-null `instance.data`) is
  attributed to a contract. Unloading (or reloading) while stateful instances are
  still live emits a "live guest instance" warning naming the use-after-free hazard.
- **Dependency refusal:** `Runtime::unload_bundle(bundle_id)` returns
  `RuntimeError::DependencyInUse` if any still-loaded bundle declared a dependency on
  a contract this bundle provides. Use `Runtime::unload_bundle_cascade(bundle_id)` to
  unload dependents first.
- **Unloading callback:** before invalidation the runtime fires the `on_reload`
  callback with `ReloadPhaseType::Unloading` (3) so the host can quiesce.
- **Per-loader reclaim:**
  - **Native (cdylib):** `dlclose` / `FreeLibrary` via the epoch-deferred drop of
    the `libloading::Library`, releasing OS resources and the on-disk file lock
    (notably the Windows DLL lock).
  - **Lua / JS (QuickJS):** the per-bundle VM is dropped through the same epoch
    path.
  - **Python:** CPython is single-init per process, so unload purges the bundle's
    re-keyed `sys.modules` entries (a module-cache purge, not an interpreter
    unload). This is memory-safe regardless of in-flight calls — CPython
    refcounts/GC keep referenced objects alive; only the import cache is dropped.
  - **.NET/C#:** the CLR is single-init per process; each (runtime, bundle) pair
    gets a collectible `AssemblyLoadContext`, and unload always calls
    `AssemblyLoadContext.Unload()`, GC-reclaimed once references and native frames
    clear. C#-guest bundles register native function pointers, so the
    host-cached-pointer UB caveat above applies to them too.

See [`UNLOAD_DESIGN.md`](./UNLOAD_DESIGN.md) for the full model.

---

## 6. Host contracts

Host contracts provide **bidirectional** communication: plugins call back into
host-provided services (logging, metrics, config, etc.).

- The host registers an implementation via `register_host_contract`; plugins
  resolve it via `get_host_contract` / `resolve_host_contract_interface`.
- **Version negotiation** (`crates/polyplug_abi/src/types/version.rs`,
  `Version::is_compatible`): a host interface satisfies a request when
  `major == required.major && minor >= required.minor`. Requesting `min_version`
  effectively at a minor of 0 accepts any minor of the same major (a wildcard for
  minor); a major mismatch always fails.
- **`user_data`-carried impls:** for native (Rust/C++/C#) and Python host providers the
  registrant's single implementation pointer lives in the `user_data` field of
  `HostContractInterface` (offset 40); `create_instance` / `destroy_instance` recover it
  via `(*this).user_data`. No static or thread-local storage. The runtime stores the
  pointer only — it never reads, writes, or frees the pointee. (C# and Python additionally
  hold a managed-side reference so the GC does not collect the impl object.) The **Lua**
  and **JavaScript (Deno)** host providers instead build a fresh implementation from a
  registered factory per `create_instance`, keying real per-instance state by a non-zero
  id — so `singleton = false` contracts give independent instances (the Deno provider uses
  native dispatch via `Deno.UnsafeCallback`). See
  [`HOST_CONTRACTS.md`](./HOST_CONTRACTS.md) § Singleton vs Per-Instance Host Contracts.
- **Contract ID namespacing:** host contracts hash `"host_contract:<name>@<major>"`
  and guest contracts hash `"guest_contract:<name>@<major>"`, keeping the two ID
  spaces disjoint.

Full tutorial and per-language examples: [`HOST_CONTRACTS.md`](./HOST_CONTRACTS.md).

---

## 7. Cross-dispatch (plugin → plugin)

A plugin can invoke a method on another plugin's guest contract through the host,
without holding a raw interface pointer of its own.

- `call_guest_method(host, instance, fn_id, args, out, arena, out_err) -> ()` is the
  17th `HostApi` function pointer (offset 136). The caller passes a
  `GuestContractInstance` it already resolved; the host re-resolves the target
  through the registry via `instance.contract_id` on **every** call, so a call
  made after a reload routes to the live (swapped-in) interface. The dispatch pins
  the epoch for its duration, so the target interface and library stay alive for
  the whole call even if an unload races it.
- **Arena threading:** the `arena` argument is forwarded to VM dispatch (Lua, JS)
  exactly like the per-call arena in §4 of [`ROADMAP.md`](ROADMAP.md); native
  dispatch ignores it (native function pointers carry no arena slot). A **null
  arena** means "no arena" — the guest bridge falls back to `host->alloc`.
- **Re-entrancy guard:** a cross-call that would re-enter a VM already executing a
  dispatch *on the same thread* returns `AbiErrorCode::ReentrantCall` (9) — nested
  same-thread entry would deadlock or panic the VM's own lock. Concurrent dispatch
  into the same VM from different threads is serialized by the VM's internal
  locking; cross-VM calls (e.g. a Lua plugin calling a JS plugin) are fine.
- **Trust:** there is **zero per-call authorization**. Trust is established once at
  load time through declared-dependency verification — see
  [`TRUST_MODEL.md`](TRUST_MODEL.md) (Cross-call dispatch).

---

## 8. Runtime isolation

Multiple `Runtime` instances can coexist in one process, each owning its own
`RuntimeStore`, loaded bundles, and configuration. No globals or thread-locals
hold runtime state (CLAUDE.md Rule 12). The init-window `INIT_BUNDLE_ID` is a
per-thread cell by design — it is transient, re-entrant, per-thread phase state,
not durable runtime data (see [`TRUST_MODEL.md`](TRUST_MODEL.md) §4).

**Documented external exceptions** (interpreter/CLR constraints, not polyplug
choices):

- **CPython** initializes once per process — the `polyplug_python` loader uses a
  single init; multiple runtimes share the interpreter and can see each other's
  Python modules/state.
- **.NET CLR** initializes once per process — same constraint; runtimes share the
  CLR and the loader cache.

Native, Lua, and JavaScript (QuickJS) loaders are fully isolated: each bundle gets
its own VM. For full isolation with Python or .NET, use separate processes.

---

## 9. Platform support

| Platform | Status |
|---|---|
| Linux (x86_64) | Full |
| macOS (x86_64 / aarch64) | Full |
| Windows (x86_64) | In progress — see below |

Windows status (honest, per [`ROADMAP.md`](ROADMAP.md) Platform Support):

- The workspace is **Windows-correct at the source level**. All shared-library
  naming uses the real cdylib convention per OS (`<name>.dll` with no `lib`
  prefix on Windows, `lib<name>.dylib` on macOS, `lib<name>.so` on Linux); the
  native loader uses cross-platform `libloading` and `PathBuf::join`;
  `polyplug_dotnet` hostfxr discovery is OS-aware; `manifest.toml` per-platform
  `[file]` tables resolve the `windows` key.
- `cargo check --target x86_64-pc-windows-msvc` is clean for every pure-Rust
  crate. The only cross-check failures are vendored native C build scripts
  (`pyo3-ffi`, `rquickjs-sys`, `tree-sitter`) that build natively on a Windows
  runner.
- **CI:** a `windows-latest` job builds the full workspace (all six loaders) and
  runs `cargo test --workspace --lib`.
- **Pending (separate work item):** native-loader **integration** tests need their
  pre-built `.so` fixtures rebuilt and committed as Windows `.dll` artifacts (plus
  per-platform `[file]` manifest tables); only then can the Windows CI job drop
  the `--lib` scoping and run the fixture-loading suites.

---

## 10. Trust model

polyplug is a software-architecture enforcement tool, not a security sandbox:
host fully trusted (bundle ID 0), plugins semi-trusted (restricted at init to
their declared dependencies), runtime the root of trust. Bundle identity is the
FNV1a-64 hash of the bundle name, and `Manifest::validate` recomputes it and
rejects a mismatch with `LoaderError::BundleTampered { bundle, expected, found }`,
so a hand-edited manifest cannot impersonate another bundle. Plugins run
in-process with full host privileges — a plugin crash takes down the host by
design, and there is no protection against malicious memory access (use OS-level
isolation for untrusted code). Full detail: [`TRUST_MODEL.md`](TRUST_MODEL.md).

---

## 11. Performance posture

- **Native path is near-zero overhead:** the hot path is one guard load, one
  pointer dereference, and one indirect call (~2.4 ns for a trivial native
  dispatch, measured). VM loaders add bounded overhead (~8 ns .NET, ~34 ns Lua,
  ~98 ns QuickJS; Python ~62 ns, dominated by GIL acquisition).
- **The Call Arena** removes the per-value `host->alloc` round trip from VM return
  paths (see §3), turning steady-state string returns into a pointer increment.
- Native guest returns are already zero-allocation borrowed views, so no arena is
  needed there.

Numbers, methodology, and per-loader benchmark tables:
[`PERFORMANCE.md`](./PERFORMANCE.md) (run `cargo bench -p polyplug` and the
per-loader `cargo bench` targets to reproduce).
