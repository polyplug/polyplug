# polyplug Roadmap

This is the living tracker: **what shipped**, **what's coming**, **what we
deferred (and why)**, and **what needs an owner decision**. The ABI freezes at
v1.0 and the project is still pre-1.0, so any item touching `HostApi` /
`RuntimeConfig` / dispatch shape must land *before* the freeze — that window is
the reason "Harden" items are ranked first below.

_Last updated: 2026-06-08._

---

## Status at a glance

| Area | Status |
|---|---|
| Goal 1 — `generate` compiles clean + `validate --bundle-dir` | ✅ Done |
| ~~Goal 2 — Extension system~~ | ❌ Removed (out of scope — two-contract model is complete) |
| Cross-call dispatch (plugin → plugin) | ✅ Done |
| Goal 3 — Call arena for VM dispatch | ✅ Done (perf refinement deferred) |
| Platform support — Windows | ✅ Done |
| Unload — invalidate + opt-in reclaim | ✅ Done |
| FFI panic safety (`catch_unwind` at boundary) | ✅ Done |
| **Fuzzing the ABI boundary** | ✅ Done (3 targets + nightly smoke) |
| **Miri + ASAN in CI** | ✅ Done (nightly) |
| **TSAN for the resolve→dispatch race** | ✅ Done (nightly, concurrent unload stress test) |
| **Supply-chain gate (cargo-deny)** | ✅ Done (nightly) |
| **Cross-language differential parity tests** | ✅ Done (`examples/hosts/parity`, 6 langs × 5 contracts byte-identical) |
| **Published SDK packages (crates.io / PyPI / NuGet / npm / luarocks)** | ⏸ Deferred (owner: not publishing yet) |
| **Quickstart + example gallery** | ✅ Done (B2, #74 — `docs/QUICKSTART.md` guided path + `docs/EXAMPLES.md` gallery) |
| **polyplugc diagnostics (source spans + suggestions)** | ✅ Done (B3, #73 — `file:line:col` + did-you-mean on parse/validate errors) |
| **Guest→guest peer callers + runtime tests** | ✅ Done (#69–#72, #75) — peer callers in all 6 generators; runtime execution tests green for **all 6** languages (rust/lua/js/cpp/csharp/python) |
| **CI cost / caching** | ✅ Done (`rust-cache` on every job; cross-lang jobs main-only) |
| ~~Benchmark regression gate in CI~~ | ❌ Reverted — benchmarks are local-only (owner ruling); run `cargo bench` locally |
| **Comparison benchmark (`counter_inc`)** | ✅ Done — safe dispatch ~0.5 ns over raw FFI; see `benches/README.md` (local-only) |
| **Call-arena retain-and-rewind (perf)** | ✅ Done (ArenaOverflowBlock +used cursor; reset rewinds & retains, free on Drop/teardown; all 6 SDKs + 4 lockstep impls) |
| D11 live-instance counter | ✅ Resolved — host-coordinated, no counter (zero hot-path cost) |
| .NET collectible ALC (true managed unload) | ✅ Done (#68) — per-bundle collectible ALC; Reclaim truly unloads, Retire keeps it |

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
  (`polyplug_abi`, `polyplug_utils`) and ASAN (leak-detection off, because
  retire-not-drop intentionally retains) on the core `--lib` tests. Fixed two
  findings: arena pointer provenance + a leak-clean tracking test.
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
  read while one thread invalidates+re-registers), proving the retire-not-drop
  guarantee (resolve concurrent with unload → valid pointer or clean
  `StaleHandle`, never a use-after-free). A nightly TSAN job runs it under
  `-Zsanitizer=thread` — clean, no data races in the registry locking.
- **A5. Finalize the resolve→dispatch UAF window for 1.0. ✅ Resolved — Option A.**
  Owner ratified the host-coordinated, best-effort `in_dispatch_threads` defense as
  the permanent 1.0 contract (2026-06-09): unload is host-coordinated exactly like
  hot-reload, so the VM dispatch hot path carries zero extra synchronization. Option B
  (per-dispatch weak-upgrade) was rejected — it would tax every VM call with atomic
  weak-upgrades and leak a per-load control block to guarantee what the trusted
  same-process model already delegates to the host.

### Lane B — Adoption / DX ⏸ Deferred (owner: not publishing yet)

Held until the owner decides to publish. Kept here so it isn't lost.

- **B1. Publish SDK packages.** No SDK is installable today (`pip install`,
  `dotnet add package`, `npm i polyplug`, `luarocks install`, crates.io). The
  single biggest blocker to anyone outside the repo authoring a plugin.
  `release.yml` is the starting point. _Outward-facing — owner sign-off on
  names/registries + a decision to publish pre-1.0._
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
  _Follow-up (not yet built — documented in the benches README so it isn't lost;
  it has a caveat, so it is not a clean headline):_
  - **vs sandboxed alternatives** — call-overhead comparison against a WASM
    boundary (wasmtime) and a subprocess/IPC boundary, to quantify what the
    "trusted same-process, native speed" posture buys vs the isolation tiers.
- **C3. ~~Reference tracing extension.~~ ❌ Cancelled (2026-06).** The extension
  concept was removed (out of scope — see "Extension system — Removed"). Tracing
  is an app concern: implement it as a `host.logger`-style host contract.

### Future / bigger bets ("what else would help")

Larger, mostly architectural — each needs an explicit owner decision before
scoping:

- **Sandboxed / untrusted plugin tier.** The trust model is "trusted same
  process." A sandboxed guest target (WASM, or seccomp/process isolation) would
  let hosts load *untrusted* plugins — a natural fit for a "universal plugin
  runtime" and a significant market expansion.
- **Per-call resource limits / timeouts.** Wall-clock + memory caps per guest
  call, so a misbehaving plugin can't hang or exhaust the host.
- **Bundle signing / verification.** `TRUST_MODEL.md` covers identity; add
  cryptographic signing so a host can verify a bundle's origin before loading.
- **Published API-docs site.** `rustdoc` + the `docs/` tree as a browsable site
  (pairs with Lane B adoption).

---

## Resolved (was deferred)

- **D11 native live-instance counter. ✅ Resolved — host-coordinated, no counter (2026-06-09).**
  The host owns the guest-instance lifecycle: generated `new()`/`drop()` host-caller
  wrappers call `create_instance`/`destroy_instance` **directly** on the resolved
  `GuestContractInterface`, so the runtime never sees instance create/destroy (it only
  mediates host-contract singletons via `get_host_contract`). A runtime-visible counter
  would therefore require a new `HostApi` callback (consuming the single free `reserved`
  slot) **plus** an atomic increment/decrement on every instance create/destroy — a
  measurable tax on the instance hot path. That cost is unjustified: the only consumer of
  such a count is a *future* reclaim mode (truly freeing a dylib/VM on unload), and reclaim
  safety is delegated to the host instead, exactly like hot-reload already requires ("all
  instances must be destroyed before calling this") and consistent with the A5/Option-A
  ruling (unload is host-coordinated). The host created the instances, so it already knows
  the live count — a runtime counter would only duplicate host knowledge. **Zero ABI slot
  consumed, zero hot-path cost.** Revisit only if a concrete reclaim-safety policy ever
  needs runtime-visible counts (it would still be a pre-freeze ABI decision at that point).

- **.NET collectible ALC (true managed unload). ✅ Done (#68).** `polyplug_dotnet` now
  loads each bundle's assemblies into its own collectible `AssemblyLoadContext`, keyed by
  bundle id (Path via `LoadFromAssemblyPath` + `AssemblyDependencyResolver`; Bytes via
  `LoadFromStream`). `DotnetLoader::unload` is implemented: under host-attested
  `UnloadMode::Reclaim` (+ `reclaim_safe`) it truly unloads the bundle's ALC so the managed
  assemblies become GC-eligible (proven by a `WeakReference`-after-GC test); under the default
  `Retire` it keeps the ALC rooted (retire-not-drop). `reload` stays disabled and the
  CLR-inits-once-per-process limitation is unchanged — collectible ALC unloads the *bundle's
  assemblies*, not the CLR.

---

## Decisions needed from owner

- **B1 — publishing:** which registries, what package names/namespaces, and is
  publishing pre-1.0 (for battle-testing) desired now or held until 1.0?
- **Lane priority:** which lane do we fund next given the CI-minute constraint?

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
   and the artifact exists; extension matches declared `runtime`; `version`
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
`reserved: *const c_void` (null) at the end of the struct (now offset 160,
after `log`@152). The struct is 168 bytes; `reserved` is silent
forward-compat room (it can later point to a versioned table) with no narrative.

## Cross-call Dispatch (plugin → plugin) ✅ Done

`call_guest_method(host, instance, fn_id, args, out, arena) → AbiError` is the
17th function pointer on `HostApi` (offset 136). A plugin invokes a method on
another plugin's guest contract through the host without holding a raw interface
pointer.

Delivered: `call_guest_method` on `HostApi`; `AbiErrorCode::ReentrantCall = 9`;
host callback re-resolves the target through the registry via
`instance.contract_id` every call (post-reload calls route to the live
interface; retire-not-drop preserved), forwards the arena to VM dispatch, guards
re-entering a VM already dispatching; all 6 SDK ABIs + generators + validator.
**Zero per-call authorization** — trust comes from load-phase declared-dependency
verification (`TRUST_MODEL.md` §5).

## Goal 3 — Call Arena for VM Dispatch ✅ Done

Per-call bump allocator replaces per-value `host->alloc`/`free` in VM dispatch.
Zero API change for plugin authors — hidden inside generated code.

| Language | Guest dispatch | Arena-routed? |
|---|---|---|
| JS (QuickJS) | VM | **Yes** (`allocStringArena`) |
| Lua (LuaJIT) | VM | **Yes** (`alloc_string_arena`) |
| Python | VM (ctypes sret dispatch removed in `fd8cc4ea` — arm64 UB) | **Yes** (`_polyplug_arena_alloc`) |
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
- Windows hot-reload: retire-not-drop loads each version from a distinct on-disk
  filename, so reload never overwrites a mapped file.

## Unload — invalidate + opt-in reclaim ✅ Done

`HostApi.unload_bundle` (offset 144) invalidates a bundle (generation bump →
`StaleHandle`, registry-index removal, dependent-refusal/cascade) and fires a
`ReloadPhaseType::Unloading` callback before invalidation. Reclaim of
loader-owned resources is opt-in via `RuntimeConfig.unload_mode`
(`UnloadMode { Retire (default), Reclaim }`; `RuntimeConfig` is 56 bytes,
`unload_mode` at offset 4). Full model: `docs/UNLOAD_DESIGN.md`; trust posture:
`TRUST_MODEL.md`.

Delivered:

- **Native opt-in reclaim:** under `Reclaim` the native loader `dlclose`s the
  dylib (releases OS resources + the on-disk file lock, notably the Windows DLL
  lock, so a developer can rebuild and reload). Host-attested + best-effort
  `reclaim_safe` (`Arc::strong_count`) net — native dispatch is structurally
  blind to in-flight raw calls, so reclaim is the host's attestation, by design.
- **Python reclaim:** under `Reclaim` the loader purges the bundle's re-keyed
  `sys.modules` entries so a reload re-imports fresh source. Memory-safe
  regardless of in-flight calls (CPython refcount/GC).
- **Lua/JS VM reclaim:** drop the VM if quiescent (`in_dispatch_threads` empty),
  else defer; these loaders govern reclaim by their own quiescence tracking.
- **Reload reuses the unload teardown atom:** `RuntimeStoreData::retire_slot`
  is the single canonical teardown (bump generation + retire-not-drop interface
  + clear entry); both `invalidate_bundle` (unload) and `apply_reload_swap`'s
  dropped-contract branch route through it. Hot-reload continuity preserved —
  the surviving-contract in-place swap does not bump generation, so resolved
  handles stay valid.

## FFI panic safety ✅ Done

Guest panics/exceptions are caught at the FFI boundary (`catch_unwind` in
`crates/polyplug/src/ffi.rs` and the loaders) and converted to `AbiError`
instead of unwinding across the C ABI (undefined behavior). Covered by
`crates/polyplug/tests/integration_panic.rs`.

## Hardening — fuzz, Miri, ASAN, supply-chain ✅ Done

Nightly-only memory-safety and supply-chain checks (`.github/workflows/
nightly.yml`, `schedule` + `workflow_dispatch` — never on push/PR, so zero
per-PR Action minutes).

- **Fuzzing (`fuzz/`):** three `cargo-fuzz` targets over the untrusted-input
  parsers — `fuzz_manifest` (`parse_manifest` + `validate`), `fuzz_contract`
  (`parse_api_str` + `parse_bundle_str`), `fuzz_version` (`Version: FromStr`).
  The nightly job builds all three and smoke-runs each 60s; a crash artifact
  fails the job. (`fuzz/` is a detached cargo-fuzz workspace, the conventional
  layout, so it stays out of the production build graph.)
- **Miri:** nightly UB detection on `polyplug_abi` + `polyplug_utils` (the
  crates with no `dlopen`/FFI execution, which Miri can't interpret).
- **ASAN:** nightly use-after-free / overflow / double-free detection on the
  core `--lib` tests. Leak detection is **off** by design — retire-not-drop
  intentionally retains superseded interfaces/libraries for the runtime
  lifetime, which LSAN would report as leaks.
- **TSAN:** nightly `-Zsanitizer=thread` over `stress_concurrent_registry`
  (pure Rust, no `dlopen`), including `stress_concurrent_unload_with_resolvers`
  which exercises the resolve↔invalidate (resolve→dispatch) race. Confirms the
  registry's `RwLock` access is data-race-free and the retire-not-drop pointer
  guarantee holds under concurrent unload.
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
