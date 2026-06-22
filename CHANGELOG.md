# Changelog

All notable changes to polyplug are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The ABI is **pre-1.0**: ABI-visible changes are still permitted between releases
and are called out explicitly below. The ABI freezes at 1.0 — see
[`docs/TRUST_MODEL.md`](docs/TRUST_MODEL.md) and
[`docs/ABI_ARCHITECTURE.md`](docs/ABI_ARCHITECTURE.md).

## [Unreleased]

_No changes yet._

## [0.1.2] - 2026-06-22

Republishes the Linux `x86_64` native artifacts built with the portable x86-64
baseline (the 0.1.1 linux-x64 natives could `SIGILL` on older CPUs — see _Fixed_),
and ships bundle signing. A single coherent version across all six registries.

### Added

- **Bundle signing & verification** (`polyplug_signing` crate): detached Ed25519
  `bundle.sig` over a canonical SHA-256 digest of every file in a bundle. The host
  enforces a `SignaturePolicy` (`Off` / `WarnOnly` / `Required`) at load, after
  manifest validation and before any loader runs. Trust is freedom-preserving
  (TOFU) — verification proves a bundle is intact and self-consistently signed
  without requiring the host to pre-know the signer; an opt-in key-pinning layer
  can be added later behind the `BundleVerifier` seam. See
  [`docs/TRUST_MODEL.md`](docs/TRUST_MODEL.md).
- `polyplugc keygen` / `sign` / `verify` subcommands for generating keypairs,
  signing a bundle directory, and verifying one.
- `RuntimeBuilder::signature_policy(..)` (Rust) and the equivalent option in the
  C++, C#, Python, Lua, and JS host SDKs.

### Changed (ABI, pre-1.0)

- `RuntimeConfig` gained a `signature_policy` field (`SignaturePolicy`, `repr(u32)`)
  at offset `0x2C`, filling the former tail padding after `log_max_level`. The
  struct size is unchanged (48 bytes, align 8); hosts that zero-initialize the
  config keep the previous behavior (`Off`). Owner-approved pre-1.0 ABI change.

### Fixed

- Build the Linux `x86_64-unknown-linux-gnu` artifacts with the portable
  x86-64 baseline instead of `-C target-cpu=native`. The committed
  `.cargo/config.toml` had pinned `target-cpu=native`, which compiles for the
  build machine's exact CPU features; the resulting binaries crashed with
  `SIGILL` on any machine lacking those instructions — turning CI red and
  putting the published 0.1.1 linux-x64 native packages (PyPI / npm / NuGet /
  LuaRocks) at risk on older CPUs. Local native builds remain available via a
  user-level `~/.cargo/config.toml` opt-in (documented in `.cargo/config.toml`).

### Security

- Bump `pyo3` `0.28` → `0.29` to clear two advisories in the Python loader's
  transitive dependency: an out-of-bounds read in `PyList`/`PyTuple` iterator
  `nth`/`nth_back` (GHSA-36hh-v3qg-5jq4, high) and a missing `Sync` bound on
  `PyCFunction::new_closure` (GHSA-chgr-c6px-7xpp, moderate).

## [0.1.1] - 2026-06-21

### Fixed

- Packaging fixes so installed packages actually import: PyPI abi types vendored
  + loader natives resolved; npm ships transpiled JS (not raw .ts); jsr declares
  cross-package deps; Lua rockspecs marked LuaJIT-only. 0.1.0 PyPI packages were
  non-functional and are yanked.

## [0.1.0] - 2026-06-21

First public release. polyplug is a universal, cross-language plugin runtime: a
host application loads plugin bundles at runtime and calls into them through a
frozen C ABI, while plugins are authored in any supported language and generated
glue is produced from a single `.toml` contract by the `polyplugc` CLI.

### Added

#### Core runtime & ABI

- Two-export host FFI surface (`polyplug_runtime_create` / `polyplug_runtime_destroy`);
  all other operations flow through the `HostApi` function table (184 bytes, `align = 8`).
- Single canonical plugin entry point: `polyplug_init(host, ctx)`, with registration
  via the self-passing `host->register_guest_contract(host, &descriptor, &interface)`.
- Out-parameter ABI calling convention shared by every generator, with the canonical
  type universe (primitives, `u64`, `bool`, unsigned enums, `StringView`, `Buffer`,
  by-value structs) marshalled identically across all six languages.
- Host-mediated guest instance lifecycle (`create_guest_instance` / `destroy_guest_instance`)
  so the runtime pins the epoch across construction/destruction and attributes each
  live instance to its contract.
- Self-revalidating generated callers: host→guest and peer callers cache the resolved
  interface for fast dispatch and detect a hot-reload/unload through the new
  `HostApi.revision_counter` field — a pointer to a runtime registry revision counter that
  callers poll with one acquire load before each dispatch (no per-call call into the
  runtime). On a change the caller transparently re-resolves and recreates its instance,
  so a cached interface pointer can never dangle after a reload/unload and authors never
  have to manage cached pointers by hand. Applied across all six language generators.
- Cross-boundary allocation through the `alloc` / `free` `HostApi` fields; all boundary
  strings are UTF-8 `StringView` (ptr + len), never null-terminated C strings.
- Host-provided logging threaded through `RuntimeConfig.log` and the `HostApi.log` field,
  surfaced in every language's guest helpers.
- Multiple isolated `Runtime` instances per process, with no global or thread-local
  runtime state. (External interpreter constraints remain: CPython and the CLR each
  initialize once per process.)

#### Loaders

- Native (`cdylib`), Python, Lua (LuaJIT), JavaScript (QuickJS), and .NET/C# bundle loaders.
- Hot-reload for native, Lua, and JS bundles: `reload()` re-reads on-disk source and
  swaps the live interface, gated on `hot_reload_enabled`. Python and .NET return
  `HotReloadDisabled`.
- Lock-free registry reads via crossbeam-epoch: readers serve from an immutable published
  `ReadView`; reload and unload republish and reclaim the superseded interface `Arc` and
  the underlying dylib/VM through epoch-deferred reclamation (true unload).
- Per-bundle collectible `AssemblyLoadContext` for the .NET loader, keyed by bundle id.

#### Code generation (`polyplugc`)

- Contract-plugin generators for Rust, C++, C#, Python, Lua, and JS (QuickJS), all
  producing the identical ABI mechanism (verified byte-for-byte across a 6×5 parity matrix).
- Global-free VM dispatch for the Python, Lua, and JS guests: `polyplug_init` returns
  `(registrations, abi_error)`; the host pointer, per-call arena allocator, and (for JS)
  the host bridge thread through explicit arguments — nothing is deposited into `_G`,
  module attributes, `globalThis`, or VM userdata.
- Per-instance guest state across all VM languages (no shared/stateless-only guests).
- Generated host→guest, guest→host, and guest→guest (peer) callers in every language,
  with zero-copy borrowed-view returns where the source type allows it.
- Call arena (retain-and-rewind) for per-call scratch allocation on the arena-bearing paths.

#### SDKs & tooling

- Per-language SDKs under `sdks/` (`abi/`, `host/`, `guest/`, `loaders/`) for all six languages.
- `sdk_validator` as the single source of truth for the built-in-type helper surface,
  enforced against every validated target (`--fail-on-missing`).
- Contract/bundle ID helpers in all six ABI SDKs, validator-enforced.
- `to_str` rejects invalid UTF-8 at the boundary in all six languages.
- Cross-language example hosts and guests (6 hosts × 6 guests), plus a byte-identical
  parity harness (`verify_hosts.sh`).

#### Benchmarks & docs

- criterion benchmark suite with committed, regenerated charts and a cross-language
  host×guest dispatch matrix, including a no-polyplug raw-FFI baseline measured in the
  same host loop (see [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md)).
- Nightly hardening lane (Miri / ASAN / TSAN / fuzz) and a criterion regression gate.

[Unreleased]: https://github.com/polyplug/polyplug/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/polyplug/polyplug/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/polyplug/polyplug/releases/tag/v0.1.1
[0.1.0]: https://github.com/polyplug/polyplug/releases/tag/v0.1.0
