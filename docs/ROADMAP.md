# polyplug Roadmap

This is the living tracker: **what shipped**, **what's coming**, **what we
deferred (and why)**, and **what needs an owner decision**. The ABI is **pre-1.0**
and stays there for the foreseeable future — there is **no scheduled 1.0 freeze**,
so ABI-visible changes (`HostApi` / `RuntimeConfig` / dispatch shape) remain
permitted between releases. The "Harden" lane is complete and the SDKs are
published; the open work below is adopter-facing DX and polish.

_Last updated: 2026-06-23._

---

## Post-0.1.0 priorities (ranked by adopter demand)

polyplug targets **trusted, first-party plugins** — native speed, real language
runtimes, zero-copy. It is **not** a sandbox; for untrusted code use WASM/Extism.
These items, ranked by demand from adopter research, strengthen that model rather
than abandon it.

| Rank | Item | Summary |
|---|---|---|
| ~~G1~~ ✅ | ~~Bundle signing + verification~~ **Done** | Shipped: detached Ed25519 `bundle.sig` over a canonical digest, enforced at load via `SignaturePolicy` (`Off`/`WarnOnly`/`Required`); freedom-preserving TOFU. See `TRUST_MODEL.md` § Bundle Signing and the `polyplug_signing` crate. |
| ~~G4~~ ✅ | ~~Published SDKs per registry~~ **Done** | All six registries publish from `release.yml`: crates.io, PyPI, NuGet, npm, JSR, LuaRocks. Latest release 0.1.1; see [Installation](../README.md#installation). |
| ~~G5~~ ✅ | ~~Docs website + native-crash debugging guide~~ **Done** | Shipped: an mdBook site over the `docs/` tree plus the workspace rustdoc API reference, deployed to GitHub Pages by `docs.yml`, and a new [Debugging Native Crashes](DEBUGGING_NATIVE_CRASHES.md) guide (symbols, core dumps, sanitizers) for trusted-plugin deployments. |
| G2 ⏸ | Optional process-isolation mode | Opt-in, per-plugin out-of-process execution — the credible answer to "I sometimes need isolation" while keeping the native fast path the default. **Deferred** (owner: not now); needs a design/scoping decision before building. |
| ~~G3~~ ❌ | ~~Resource limits / runaway-plugin watchdog~~ **Non-goal** | A per-call wall-clock watchdog cannot be built without hot-path overhead: knowing a call ran too long requires recording when it started (a clock read ~15–30 ns, or ≥2 atomic stores/call). That violates the ~0.5 ns zero-overhead dispatch invariant, which is sacred. **Per-call timeouts are a host-side concern** — run the call on a worker thread you control and enforce your own deadline, outside polyplug (same pattern as tracing-via-host-contract). |

---

## Status at a glance

| Area | Status |
|---|---|
| Goal 1 — `generate` compiles clean + `validate --bundle-dir` | ✅ Done |
| ~~Goal 2 — Extension system~~ | ❌ Removed (out of scope — two-contract model is complete) |
| Cross-call dispatch (plugin → plugin) | ✅ Done |
| Goal 3 — Call arena for VM dispatch | ✅ Done (perf refinement deferred) |
| Platform support — Windows | ✅ Done |
| Unload — true unload (epoch-deferred reclaim) | ✅ Done |
| Bundle signing + verification (Ed25519, `SignaturePolicy`) | ✅ Done |
| FFI panic safety (per-language self-catch + embedder-guarded exports) | ✅ Done |
| **Fuzzing the ABI boundary** | ✅ Done (3 targets + nightly smoke) |
| **Miri + ASAN in CI** | ✅ Done (nightly) |
| **TSAN for the resolve→dispatch race** | ✅ Done (nightly, concurrent unload stress test) |
| **Supply-chain gate (cargo-deny)** | ✅ Done (nightly) |
| **Cross-language differential parity tests** | ✅ Done (`examples/hosts/parity`, 6 langs × 5 contracts byte-identical) |
| **Published SDK packages (crates.io / PyPI / NuGet / npm / JSR / luarocks)** | ✅ Done — all six registries publish from `release.yml`; latest release 0.1.1 |
| **Quickstart + example gallery** | ✅ Done (B2, #74 — `docs/QUICKSTART.md` guided path + `docs/EXAMPLES.md` gallery) |
| **polyplugc diagnostics (source spans + suggestions)** | ✅ Done (B3, #73 — `file:line:col` + did-you-mean on parse/validate errors) |
| **Guest→guest peer callers + runtime tests** | ✅ Done (#69–#72, #75; direct-dispatch upgrade #104–#107) — peer callers in all 6 generators; runtime execution tests green for **all 6** languages (rust/lua/js/cpp/csharp/python). Native-dispatch languages (rust/cpp/csharp/python/lua) now dispatch **directly through the cached interface** (no `call_guest_method` round-trip, no per-call resolve, no epoch pin); JS stays bridge-mediated (QuickJS cannot deref raw pointers) |
| **CI cost / caching** | ✅ Done (`rust-cache` on every job; cross-lang jobs main-only) |
| ~~Benchmark regression gate in CI~~ | ❌ Reverted — benchmarks are local-only (owner ruling); run `cargo bench` locally |
| **Comparison benchmark (`counter_inc`)** | ✅ Done — safe dispatch ~0.5 ns over raw FFI; see `benches/README.md` (local-only) |
| **Call-arena retain-and-rewind (perf)** | ✅ Done (ArenaOverflowBlock +used cursor; reset rewinds & retains, free on Drop/teardown; all 6 SDKs + 4 lockstep impls) |
| Live-instance counter (per-contract, host-mediated) | ✅ Done — counts stateful instances; reload/unload-with-live warning |
| .NET collectible ALC (true managed unload) | ✅ Done (#68) — per-bundle collectible ALC; unload always calls `AssemblyLoadContext.Unload()` |
| **Real per-instance state — VM guests + host contracts (all 6 langs)** | ✅ Done (#74) — replaced the VM `create/destroy_instance` stubs; python/lua/js guests AND lua + Deno host-contract providers now build a fresh impl per `create_instance` and route dispatch by instance id (native = boxed payload, null rejected; VM = non-zero id keyed, id 0 → per-contract default). Owner-approved layout-neutral `loader_data` ABI change |
| **JS (Deno) host-contract provider** | ✅ Done (#85) — native dispatch via `Deno.UnsafeCallback` + per-instance, marshalling the **full ABI type universe** (no per-language limitation) |
| **Lua/JS host-SDK suites in CI** | ✅ Done (#26) — the `test` job runs `just test-host-lua test-host-js`; closes the no-cargo-coverage gap that previously hid two bugs until after merge |
| **JS host SDK on Node.js + Bun (not just Deno)** | ✅ Done — one runtime-detected FFI seam (`sdks/js/abi/ffi/`): Deno → `Deno.dlopen`, Node → `koffi` (optional dep, lazy-loaded), Bun → built-in `bun:ffi`. Same 57-test suite runs green on all three (`test-host-js` / `-node` / `-bun`); the install-smoke loads the embedded native via the Node and Bun FFI backends from the published tarballs |
| **Incremental codegen writes** | ✅ Done (#31) — `polyplugc generate` skips bindings whose content is unchanged (mtime-preserving, no downstream rebuild cascade) and always rewrites `manifest.toml` via the wired `force_regenerate` flag |
| **Documentation site + API reference (G5)** | ✅ Done — mdBook site over `docs/` + workspace rustdoc, deployed to GitHub Pages by `docs.yml`; new [Debugging Native Crashes](DEBUGGING_NATIVE_CRASHES.md) guide |

---

## Coming up — candidate work (prioritized)

Three lanes. The order reflects the pre-1.0 freeze: correctness of the ABI shape
is irreversible after 1.0, adoption enables the battle-testing that validates the
shape, and perf/ops are continuous. Each item notes CI-minute cost because the
owner is currently budget-constrained.

### Lane A — Harden for the 1.0 freeze (correctness is forever)

- **A1. Fuzz the ABI boundary. ✅ Done.** `cargo-fuzz` targets over the
  untrusted-input surfaces — manifest TOML parser, contract `.toml` parser
  (`parse_api_str`/`parse_bundle_str`), and `Version` parsing — in `fuzz/`, with
  a 60s/target nightly smoke run. See _Hardening_ in the shipped section.
- **A2. Miri + ASAN. ✅ Done.** Nightly Miri on the pure-logic crates
  (`polyplug_abi`, `polyplug_utils`) and ASAN (leak-detection off, because a
  reader pinned in the prior epoch keeps a superseded interface/library alive
  until it unpins) on the core `--lib` tests. Fixed two findings: arena pointer
  provenance + a leak-clean tracking test.
- **A3. Cross-language differential parity. ✅ Done.** `examples/hosts/parity`
  loads each of the 6 guest languages' implementations of the 5 rich pipeline
  contracts and asserts every language returns byte-identical, golden output —
  locking the "all generators produce identical ABI mechanics" invariant
  (CLAUDE.md §10) against drift. Complements `cross_language.rs` (which only
  checks `add(3,5)==8`) by exercising `StringView` marshaling, the host-allocator
  return path, and host-contract callbacks. Wired into `verify_hosts.sh` (CI
  Examples job). Caught a real bug on first run: three hand-written Lua example
  guests double-converted a `StringView` into the SDK helpers and silently
  returned `INVALID:*` — fixed to native Lua string ops.
  _Follow-up: ✅ Done (#77). The Lua `to_str` (and `split`/`strip_prefix`/
  `starts_with`/`ends_with` which delegate to it) now raise a clear error on
  non-StringView input instead of silently returning `""`. Fixed at the
  `HELPER_LUA` emitter + the hand-written guest SDK; runtime test added._
- **A4. TSAN for the resolve→dispatch race. ✅ Done.** Added
  `stress_concurrent_unload_with_resolvers` (resolver threads `find`+`resolve`+
  read while one thread unloads+re-registers), proving the epoch guarantee
  (resolve concurrent with unload → valid pointer or clean `StaleHandle`, never a
  use-after-free: a reader pinned before the unload keeps the old interface `Arc`
  and still-mapped library alive until it unpins). A nightly TSAN job runs it
  under `-Zsanitizer=thread` — clean, no data races in the registry locking.
- **A5. Finalize the resolve→dispatch UAF window for 1.0. ✅ Resolved — Option A.**
  Owner ratified the host-coordinated, best-effort `in_dispatch_threads` defense as
  the permanent 1.0 contract (2026-06-09): unload is host-coordinated exactly like
  hot-reload, so the VM dispatch hot path carries zero extra synchronization. Option B
  (per-dispatch weak-upgrade) was rejected — it would tax every VM call with atomic
  weak-upgrades and leak a per-load control block to guarantee what the trusted
  same-process model already delegates to the host.

### Lane B — Adoption / DX

- **B1. Publish SDK packages. ✅ Done.** All six registries publish from
  `release.yml`: `cargo add polyplug` (crates.io), `pip install` (PyPI),
  `dotnet add package` (NuGet), `npm i` / JSR, and `luarocks install`. Latest
  release 0.1.1 (0.1.2 prepared, release-on-demand). See
  [Installation](../README.md#installation).
- **B2. Quickstart + example gallery. ✅ Done (#74).** `docs/QUICKSTART.md` is a
  guided "write your first plugin in language X" path; `docs/EXAMPLES.md` is a
  gallery of the reference plugins. (Shipped independently of B1's publish gate.)
- **B3. polyplugc diagnostics. ✅ Done (#73).** Contract-parse/validate errors now
  carry `file:line:col` source spans (via `toml::Spanned`) plus did-you-mean
  suggestions (Levenshtein over known type/contract names). _Not scaffolding_ —
  owner ruled out `polyplugc new`.

### Lane C — Performance & ops (continuous)

- **C1. CI cost reduction. ✅ Mostly done.** `Swatinem/rust-cache@v2` is already
  on every CI job and cross-language jobs run main-only. Remaining ideas if
  pressure persists: smarter matrix triggers, job consolidation.
- **C2. Benchmark regression gate. ❌ Reverted — benchmarks are local-only
  (owner ruling 2026-06-10).** A nightly `benches` job briefly ran the hot-path
  benches in CI, but the owner ruled benchmarks are a **local** tool, not a CI
  gate: they load native fixtures, embed VMs, and are too sensitive to
  shared-runner noise to gate on. The whole `benches` job was removed from
  `nightly.yml`. The benches themselves remain (run `cargo bench -p polyplug`),
  and `scripts/check_bench_regression.py` is kept as a **local** before/after
  comparison helper. See `crates/polyplug/benches/README.md`.
- **C4. Comparison / marketing benchmark. ✅ Done.** `counter_inc`
  (`cargo bench -p polyplug --bench counter_inc`) runs the same 1M-iteration
  `counter = inc(counter)` loop through four mechanisms — a direct `#[inline(never)]`
  call (floor), raw `dlsym` by-value FFI, the ptr-in/ptr-out ABI convention
  statically linked, and full polyplug resolved dispatch over a loaded `.so` —
  so each per-call delta isolates one cost. Result: polyplug's safe dispatch is
  **~0.5 ns over hand-rolled raw FFI** and within **2× of an un-inlinable direct
  call** (~440 M calls/s). Documented in `docs/PERFORMANCE.md` ("The safety tax")
  and `crates/polyplug/benches/README.md`. **Local-only** (per the C2 ruling) —
  not wired into CI.
  - **Payload-scaling bench. ✅ Done — `payload_scaling`.** Same byte-fill work
    (N bytes) `native_direct` vs `polyplug_dispatch` across N ∈ [0, 16384] using
    `memory_plugin` (no fixture change). Shows the dispatch overhead is a *fixed*
    ~0.25–0.5 ns that collapses to <0.3% (within measurement noise) by a few KB —
    the honest real-world view. Local-only; see `benches/README.md`.
  - **Cross-language dispatch matrix. ✅ Done.** `counter_inc` gained a C++
    native-dispatch arm (`polyplug/dispatch_cpp`, `libtest_plugin_cpp.so`)
    alongside the Rust one, proving native dispatch is language-agnostic
    (Rust ~2.5 ns, C++ ~2.7 ns — same path, same contract). The VM tier
    (Lua/JS/Python/.NET) is documented as the per-loader `dispatch_benchmark.rs`
    benches, kept in a *separate* tier because those measure the interpreter's
    call cost, not polyplug's, and are not apples-to-apples with native. Matrix +
    regenerate recipe in `benches/README.md`. Local-only.
  - **One-time cost amortization. ✅ Done — `amortization`.** Measures the costs
    *around* the hot path: `load_bundle` (~13 µs, once per bundle), `find_and_resolve`
    (~22 ns, cached in real use), and native `hot_reload_swap` (~17 µs, once per
    reload). Shows the load cost amortizes below one dispatch (~2.5 ns) past a few
    thousand calls and is noise by 1 M. Reuses the integration tests' `TestNativeLoader`
    (the `polyplug` crate cannot dev-depend on `polyplug_native` — that would be a
    dependency cycle). Local-only; see `benches/README.md`.
- **C3. ~~Reference tracing extension.~~ ❌ Cancelled (2026-06).** The extension
  concept was removed (out of scope — see "Extension system — Removed"). Tracing
  is an app concern: implement it as a `host.logger`-style host contract.

### Future / bigger bets ("what else would help")

Larger, mostly architectural — each needs an explicit owner decision before
scoping:

- **Sandboxed / untrusted plugin tier.** The trust model is "trusted same
  process." A sandboxed guest target (seccomp / process isolation) would
  let hosts load *untrusted* plugins — a natural fit for a "universal plugin
  runtime" and a significant market expansion.

**Explicit non-goal — in-runtime per-call resource limits / timeouts.** A
watchdog that enforces a per-call wall-clock deadline must record when each call
starts (a clock read ~15–30 ns, or ≥2 atomic stores/call), which taxes the
~0.5 ns zero-overhead dispatch path. That invariant is sacred, so polyplug will
not add it. Hosts that need a timeout run the call on a worker thread they
control and enforce their own deadline — outside polyplug — the same way tracing
is an app concern (see C3).

---

## Resolved (was deferred)

- **Live-instance counter (per-contract, host-mediated). ✅ Done.**
  `Runtime` carries a per-contract live-instance counter keyed by contract id. Host-mediated
  `create_guest_instance` (`HostApi` @160) and `destroy_guest_instance` (`HostApi` @168)
  attribute each guest instance to its contract and keep the count current. Only stateful
  instances (non-null `instance.data`) are counted; stateless/VM-singleton contracts add
  nothing. When a reload or unload runs while a contract still has live stateful instances,
  the runtime emits a "live guest instance" use-after-free warning so the host knows it
  tore down a contract whose instances are still reachable. The count drives the true-unload
  path's safety signalling and is delegated to the host for quiescing, consistent with the
  A5/Option-A ruling (unload is host-coordinated).

- **.NET collectible ALC (true managed unload). ✅ Done (#68).** `polyplug_dotnet`
  loads each bundle's assemblies into its own per-(runtime, bundle) collectible
  `AssemblyLoadContext`, keyed by bundle id (Path via `LoadFromAssemblyPath` +
  `AssemblyDependencyResolver`; Bytes via `LoadFromStream`). `DotnetLoader::unload` always
  calls `AssemblyLoadContext.Unload()` on the bundle's ALC, so the managed assemblies become
  GC-eligible and are reclaimed once outstanding references and native frames clear (proven
  by a `WeakReference`-after-GC test). `reload` stays disabled and the CLR-inits-once-per-process
  limitation is unchanged — collectible ALC unloads the *bundle's assemblies*, not the CLR.

---

## Decisions needed from owner

- **1.0 ABI freeze — pull the trigger?** Lane A (the irreversible-before-freeze
  correctness work) is complete and the ABI is hardened. Declaring 1.0 is now an
  owner call, not a blocked engineering task.
- **B1 — publishing:** which registries, what package names/namespaces, and is
  publishing pre-1.0 (for battle-testing) desired now or held until 1.0?
- **Lane priority:** which lane do we fund next given the CI-minute constraint?
- _(Resolved 2026-06-17: the `[[plugin]] version` field was **removed** — field,
  parse, schema requirement, and all fixtures/examples. A bundle is the single
  deployable/load/reload unit, so any plugin change forces a bundle-version bump;
  a per-plugin version carried no independent semantics and never affected
  resolution (which uses the contract `@N`). The bundle version is the one
  artifact version; `[[plugin]]` now takes only `name` + `implements`.)_

_(Resolved 2026-06-09: the python peer-caller reachability question (#75) was
decided "fix it, mirror lua/js/cpp" and is shipped — see the status table.)_

---
---

# Shipped (reference)

Detailed records of completed work. Kept for the non-obvious constraints and
delivered-surface lists.

## Goal 1 — `generate` compiles clean + `validate --bundle-dir` ✅ Done

`polyplugc` does two things: `generate` and `validate` (there is **no `pack`
command** — owner ruling). Users build the generated glue with their own
toolchains and assemble the bundle directory themselves.

1. **`generate` output compiles with zero hand edits** for all 6 languages
   (`rust`, `cpp`, `csharp`, `python`, `lua`, `js-quickjs`): emits guest-side
   contract glue + a ship-ready `manifest.toml` with precomputed `bundle_id`.
2. **`validate --bundle-dir <dir>`** drives the runtime loader's own manifest
   machinery (`polyplug::loader::parse_manifest` + `ManifestData::validate`) so
   the CLI accepts exactly what the runtime would: manifest parses;
   `id == fnv1a_64(name)` (tamper check); per-platform `[file]` entry resolves
   and the artifact exists; extension matches declared `loader`; `version`
   parses.

e2e proof: `crates/polyplugc/tests/generate_e2e.rs` (rust → cargo build),
`generate_e2e_native.rs` (cpp → c++ -shared, csharp → dotnet build),
`generate_e2e_vm.rs` (python → py_compile + import, lua → luajit load, js →
deno check), `cli_validation.rs` (validate accepts correct, rejects missing
artifact + tampered id).

## Extension system — ❌ Removed (2026-06)

The extension mechanism (`get_extension` + side-table + `fnv1a_32`) was removed
entirely. Owner ruling: the two-contract model — **guest contracts** (plugin
functionality) + **host contracts** (services the host exposes to guests, e.g.
the `host.logger` example) — is the complete model. Extensions forced an unused
concept on every user and shipped zero consumers; anything they could carry is
already an app-defined host contract.

The `get_extension` slot on `HostApi` was replaced by a single trailing
`reserved: *const c_void` (null) at the end of the struct (offset 176, after the
instance-lifecycle callbacks and `revision_counter`). The struct is 184 bytes — the tail is
`unload_bundle`@136, `log`@144, `create_guest_instance`@152,
`destroy_guest_instance`@160, `revision_counter`@168, `reserved`@176. `reserved` is silent forward-compat
room (it can later point to a versioned table) with no narrative.

## Cross-call Dispatch (plugin → plugin) ✅ Done

Plugin→plugin peer calls are generated by `polyplugc` as typed **peer callers**
that cache the resolved interface and dispatch **directly** through it — the same
path as a host→guest caller — polling `HostApi.revision_counter` to catch a reload
and relying on the declared dependency (provider unload refused while a dependent is
live) for lifetime safety. The hot path carries no per-call resolve and no epoch pin.
QuickJS is the one exception: it cannot deref a raw interface pointer, so a JS peer
caller dispatches through the loader-side `callGuestMethod` bridge.

Delivered: `AbiErrorCode::ReentrantCall = 9`; re-entrancy guard on same-thread
VM re-entry; arena forwarded to VM dispatch; all 6 SDK ABIs + generators + validator.
`call_guest_method` was initially delivered as the host-mediated capability, then
**removed from the ABI** when the direct-dispatch upgrade landed (#104–#107) and
no remaining caller needed it.
**Zero per-call authorization** — trust comes from load-phase declared-dependency
verification (`TRUST_MODEL.md` §5).

## Goal 3 — Call Arena for VM Dispatch ✅ Done

Per-call bump allocator replaces per-value `host->alloc`/`free` in VM dispatch.
Zero API change for plugin authors — hidden inside generated code.

| Language | Guest dispatch | Arena-routed? |
|---|---|---|
| JS (QuickJS) | VM | **Yes** (`allocStringArena`) |
| Lua (LuaJIT) | VM | **Yes** (`alloc_string_arena`) |
| Python | VM (ctypes sret dispatch removed in `fd8cc4ea` — arm64 UB) | **Yes** (threaded `arena_alloc` dispatch arg) |
| Rust (host caller) | Native | N/A — returns are borrowed views, zero-alloc |
| C++ (host caller) | Native/VM | N/A — borrowed views, zero-alloc |
| C# | Native | N/A — borrowed views, zero-alloc |

**Native Rust/C++/C# are excluded by design, not omission:** their guest
functions return borrowed `StringView`/`Array` views into guest-owned memory
already valid for the call — the return marshal is a pointer write with no
allocation, so there is nothing for an arena to replace. Native callers pass a
null arena.

Lifetime rule: an arena-backed return is valid until the next arena-backed call
on the same caller. The host always passes the arena slot in the canonical
6-arg VM dispatch signature `call(loader_data, instance, fn_id, args, out,
arena)`; a null arena means "no arena" and the guest bridge falls back to
per-value `host->alloc`.

## Platform Support — Windows ✅ Done

Full workspace test suite passes on `windows-latest` (`cargo test --workspace
--no-fail-fast`). Green as of 2026-06-07.

Non-obvious constraints (do not regress):

- All shared-library naming uses the real Rust cdylib convention per OS
  (`<name>.dll` no `lib` prefix on Windows, `lib<name>.dylib` macOS,
  `lib<name>.so` Linux).
- `polyplug_dotnet` hostfxr auto-discovery is OS-aware.
- MSVC env activation is ordered **after** all bash-shell cargo steps — Git
  Bash's GNU `/usr/bin/link` shadows MSVC `link.exe` once MSVC env is set.
- Fabricated-TOML tests normalize embedded absolute paths to forward slashes;
  MSBuild paths strip the `\\?\` verbatim prefix from `canonicalize()`.
- LuaJIT built from source via `msvcbuild.bat` (no prebuilt binaries).
- `polyplug_dotnet/src/context.rs` uses `into_temp_path()` to close the
  runtimeconfig.json write handle before hostfxr reads it — Windows mandatory
  file sharing otherwise fails with `ERROR_SHARING_VIOLATION`.
- Windows hot-reload: each version loads from a distinct on-disk filename, so
  reload never overwrites a mapped file (the superseded mapping is freed later via
  epoch-deferred reclamation, once no reader is still pinned in the prior epoch).

## Unload — true unload (epoch-deferred reclaim) ✅ Done

`HostApi.unload_bundle` (offset 136) bumps slot generations (resolved handles go
`StaleHandle`), removes the bundle from every registry index (with
dependent-refusal/cascade), and fires a `ReloadPhaseType::Unloading` (= 3)
callback before invalidation so the host can quiesce. It then reclaims the
superseded interface `Arc` **and** the loader's dylib mapping / VM state via
crossbeam-epoch deferred reclamation — freed once no reader is still pinned in
the prior epoch. There is no opt-in mode and no retained tier:
`RuntimeConfig` is 72 bytes with no unload-mode field. Full model:
`docs/UNLOAD_DESIGN.md`; trust posture: `TRUST_MODEL.md`.

Reads are lock-free: a reader takes a pin guard over the immutable published
`ReadView`; writers republish and `defer_destroy` the old view, which is freed
once all old-epoch guards unpin. A reader pinned before the unload keeps the old
interface `Arc` and its still-mapped library alive until it unpins.

This publish/reclaim protocol is model-checked with loom (`loom_epoch_model`
crate, run via `just loom`): one test proves the pinned read path is
use-after-free-free across every interleaving, a second proves loom still
detects the UAF when a reader drops its guard before the dereference. See
`docs/UNLOAD_DESIGN.md` → *Epoch Model* → *Model-checked with loom*.

Two caller contracts:

- **Runtime-mediated lifecycle calls are safe under concurrent unload.**
  `create_guest_instance` (@152) and `destroy_guest_instance` (@160) pin the epoch
  across their operation, so the interface and library can't be freed mid-call.
  (The former `call_guest_method` field also pinned the epoch but has been removed
  from the ABI — peer callers now dispatch directly.)
- **Direct FFI host-callers are fast and do not pin per call.** Quiesce-before-unload
  is the documented contract; using a cached raw interface pointer after unload is
  UB.

Per-loader reclaim:

- **Native:** the loader's `Library` is dropped via the epoch-deferred path, so
  `dlclose`/`FreeLibrary` releases OS resources + the on-disk file lock (notably
  the Windows DLL lock, so a developer can rebuild and reload) once no reader is
  pinned in the prior epoch.
- **Lua/JS:** the per-bundle VM is dropped through the same epoch-deferred path.
- **Python:** unload always purges the bundle's re-keyed `sys.modules` entries so a
  reload re-imports fresh source — a module-cache purge, because CPython's
  single-init interpreter can't be torn down per bundle. Memory-safe regardless of
  in-flight calls (CPython refcount/GC).
- **.NET:** unload always calls `AssemblyLoadContext.Unload()` on the bundle's
  per-(runtime, bundle) collectible ALC; the assemblies are GC-reclaimed once
  outstanding references and native frames clear. `reload` stays disabled; the CLR
  is single-init per process.
- **Shared teardown:** reload and unload share one canonical teardown atom on the
  store (bump generation + epoch-reclaim the superseded interface). Both
  `invalidate_bundle` (unload) and `apply_reload_swap`'s dropped-contract branch
  route through it. Hot-reload continuity is preserved — the surviving-contract
  in-place swap does not bump generation, so resolved handles stay valid.

## FFI panic safety ✅ Done

Failure-to-`AbiError` conversion is owned per-language, not by a runtime-side
catch-all (see "Failure responsibility at the ABI boundary" in
`docs/TRUST_MODEL.md`):

- **Each language's generated glue self-catches** its own failures (Rust
  `catch_unwind`, C++ `catch(...)`, C# `try/catch`, Lua/JS `pcall`/`try`) and
  returns `AbiError { code: Panic }` *before* crossing the C ABI — zero
  happy-path cost. Covered by `crates/polyplug/tests/integration_panic.rs`,
  which drives a generated dispatch wrapper through a real plugin panic and
  asserts the host sees `AbiErrorCode::Panic` with no process abort.
- **The two C ABI exports** (`polyplug_runtime_create` / `polyplug_runtime_destroy`
  in `crates/polyplug/src/ffi.rs`) wrap their bodies in `catch_unwind` solely for
  the *embedder guarantee* — a bug in polyplug's own create/destroy path never
  aborts the host process. The runtime does **not** wrap calls *into* a plugin
  (`polyplug_init`, native dispatch): an unwind/exception escaping a plugin's own
  glue is a plugin defect with a defined outcome (process abort).

## Hardening — fuzz, Miri, ASAN, supply-chain ✅ Done

Nightly-only memory-safety and supply-chain checks (`.github/workflows/
nightly.yml`, `schedule` + `workflow_dispatch` — never on push/PR, so zero
per-PR Action minutes).

- **Concurrency suite (`crates/polyplug/tests/concurrency/`, `--test concurrency`):**
  one organized home for every parallel/concurrent test, one module per concurrent
  surface — `dispatch` (resolve+call while a reload/unload swaps the slot in
  flight), `reload` (concurrent reload of the same bundle + deterministic
  critical-section mutual exclusion), `registry` (concurrent register/find/
  resolve/swap + duplicate-registration races for guest, host, and loader),
  `load_unload` (retire/reclaim races, no UAF on invalidate), `multi_runtime`
  (Rule-12 isolation: N runtimes built/used/destroyed across threads stay fully
  independent), and `logger` (concurrent `host->log` funnel thread-safety). It
  pairs deterministic interleaving probes with high-iteration stress tests — the
  latter are the TSAN job's race oracle below. The suite exists because the
  historical concurrent-reload flake passed in isolation and only failed under
  full-suite parallel load, so scattered non-deterministic stress tests alone were
  proven insufficient.
- **Fuzzing (`fuzz/`):** three `cargo-fuzz` targets over the untrusted-input
  parsers — `fuzz_manifest` (`parse_manifest` + `validate`), `fuzz_contract`
  (`parse_api_str` + `parse_bundle_str`), `fuzz_version` (`Version: FromStr`).
  The nightly job builds all three and smoke-runs each 60s; a crash artifact
  fails the job. (`fuzz/` is a detached cargo-fuzz workspace, the conventional
  layout, so it stays out of the production build graph.)
- **Miri:** nightly UB detection on `polyplug_abi` + `polyplug_utils` (the
  crates with no `dlopen`/FFI execution, which Miri can't interpret).
- **ASAN:** nightly use-after-free / overflow / double-free detection on the
  core `--lib` tests. Leak detection is **off** by design — a reader pinned in
  the prior epoch keeps a superseded interface/library alive until it unpins, and
  in a short-lived test a still-pinned or not-yet-collected epoch garbage entry
  reads to LSAN as a leak.
- **TSAN:** nightly `-Zsanitizer=thread` over the pure-Rust epoch race surface of
  the concurrency suite (`crates/polyplug/tests/concurrency/` — `--test concurrency`,
  scoped by positive filters to the `registry`/`load_unload` modules plus the
  `RuntimeStore` swap-vs-dispatch tests, which have no `dlopen`). Includes the
  resolve↔unload (resolve→dispatch) race. Confirms the registry's `RwLock` access
  is data-race-free and the epoch guarantee holds under concurrent unload (a reader
  pinned before the unload never observes a freed interface). The Runtime-level
  reload tests in the same suite load real `.so` bundles (uninstrumented under
  `-Zbuild-std`) and stay out of TSAN scope by design.
- **Supply-chain (`deny.toml`):** `cargo-deny` checks advisories (deny yanked +
  known CVEs), licenses (permissive allow-list; first-party workspace crates are
  `publish = false` + skipped), and sources (crates.io only).

Findings fixed while standing this up:

- **Arena pointer provenance:** `CallArena::bump` derived the aligned pointer
  via an integer→pointer cast, which Miri can't track. Now derived from the
  source pointer via `wrapping_add(offset)` — same address, provenance preserved,
  no behavior/layout change.
- **Security advisory:** bumped `rustls-webpki` 0.103.9 → 0.103.13 to clear
  RUSTSEC-2026-0104 (a panic reachable in CRL parsing; transitive via the .NET
  hostfxr downloader).
- **Leak-clean test:** the intentional-leak test that proves `assert_no_leaks`
  panics now catches the panic and frees, so it stays clean under Miri's leak
  checker (keeping leak detection on for everything else).
