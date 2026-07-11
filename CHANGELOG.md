# Changelog

All notable changes to polyplug are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The ABI is **pre-1.0**: ABI-visible changes are still permitted between releases
and are called out explicitly below. The ABI freezes at 1.0 — see
[`docs/TRUST_MODEL.md`](docs/TRUST_MODEL.md) and
[`docs/ABI_ARCHITECTURE.md`](docs/ABI_ARCHITECTURE.md).

## [Unreleased]

### Fixed

- **Generated Rust code failed a strict host's `clippy` gate.** The Rust host and
  guest SDKs emitted only a handful of `#![allow]`s (`dead_code`, `eq_op`,
  `identity_op`, …), so including the generated module into a crate that runs
  `cargo clippy -- -D warnings` with `clippy::pedantic`/`clippy::nursery` enabled
  broke the build — a multi-argument guest method emits an arg-pack literal
  (`Args { key: key, value: value }`) that trips the default-warn
  `redundant_field_names`, and contract-id constants trip `unreadable_literal`.
  polyplug's own examples never hit this (no multi-argument guest method, no
  pedantic profile); it surfaced integrating a real pedantic host. Generated code
  is machine output that a consumer includes verbatim, so it must suppress its own
  lints: the headers now emit `#![allow(clippy::all)]`, `#![allow(clippy::pedantic)]`,
  and `#![allow(clippy::nursery)]`. Bodies are unchanged.
- **The C++ guest SDK headers were not relocatable.** `sdks/cpp/guest/polyplug/`
  `guest.hpp` and `contract.hpp` included the ABI header by a source-tree-relative
  path (`../../abi/polyplug/abi.hpp`), so vendoring the SDK into another project
  (e.g. copying the headers under one include root) failed to find `abi.hpp`. They
  now use `#include "polyplug/abi.hpp"`, matching the generated guest headers, so
  the SDK resolves both in-tree (`-I sdks/cpp/abi`) and when vendored (`-I
  <include>`).
- **A Python struct with a `Buffer` field failed to load.** The generated types
  module hardcoded `from polyplug_abi import StringView` and never imported
  `Buffer`, so a struct such as `ReadResult { data: Buffer }` emitted
  `("data", Buffer)` with `Buffer` undefined → `NameError` at class creation. The
  module now imports `Buffer` whenever an emitted type references it. Covered by
  `python_buffer_in_struct_types_import`.
- **A Python guest function taking a single `enum` parameter crashed on
  dispatch.** The generated ABI wrapper read it with `Status.from_address(...)`,
  but a generated enum is an `enum.IntEnum` (no `from_address`), raising
  `AttributeError`. It now reads the repr integer in place, matching the
  multi-parameter arg-pack path. Covered by `python_enum_param_dispatches`.
- **A Python struct with an `enum` field could not be instantiated.** The
  generated `class Rec(ctypes.Structure)` emitted `("flag", Status)` in
  `_fields_`, but a generated enum is an `enum.IntEnum` — not a ctypes type — so
  the first use raised `TypeError: this type has no size`. The field now uses the
  enum's repr ctype (matching the already-correct arg-pack path). Lua (`uint32_t`)
  and C++ (`enum class`) were unaffected. Covered by
  `*_struct_with_enum_field_round_trips` across all 6 languages.
- **A struct with an `Array<T>` field generated type declarations in the wrong
  order.** The synthesized `ArrayOf_*` wrapper was emitted *after* the struct that
  embeds it, so the C-family type modules failed before any call ran: the Lua
  `ffi.cdef` raised "declaration specifier expected", and the C++ / Python type
  headers referenced an undeclared type. `polyplugc` now emits types in
  dependency order (a stable topological sort — output for contracts without such
  a field is unchanged). Covered by a `*_struct_with_array_field_round_trips` test
  per language (an `Array<Group>` where `Group` embeds an `Array<StringView>`,
  including an empty nested array).
- **Guest codegen for non-struct `Array<T>` return elements (Lua, Python).** Only
  `Array<struct>` (the common array-of-records shape) marshaled correctly before;
  arrays whose element is a scalar, a `StringView`, or an enum emitted broken
  guest glue. The Lua generator wrote `ffi.sizeof("u32")` / `ffi.sizeof("<Enum>")`
  (neither is a cdef'd LuaJIT C type) and assigned a Lua string straight into a
  `StringView` cdata; the Python generator referenced an undefined `StringView` /
  enum name in `contracts.py`. Now the Lua marshaler resolves each element to its
  real C type (primitive → C integer/float name, enum → its repr integer,
  `StringView` arena-allocated per element) and the Python marshaler uses the
  enum's repr ctype and imports `StringView` for a `StringView`-array return. A
  round-trip test per language (`*_scalar_string_enum_arrays_round_trip`, covering
  `Array<u32>` at 257 elements, `Array<StringView>`, and `Array<enum>`) locks this
  in. No ABI or API change — generated output for existing `Array<struct>`
  contracts is unchanged.

## [0.1.3] - 2026-07-07

Publishes the `polyplugc` CLI to npm, PyPI, and NuGet (first registry
availability) and routes both code-generation pipelines through langprint for
declaration FORM. Generated output and the ABI are **byte-identical to 0.1.2** —
no runtime, API, or ABI change.

### Added

- **`polyplugc` CLI installable from every SDK registry — no Rust required.** New
  CLI packages distribute the prebuilt `polyplugc` binary, each **embedding** the
  binary so installs are **fully offline** (no download step): `@polyplug/cli` on
  npm (per-platform optional packages, esbuild-style — works with `npm i -g`,
  `bunx`, and `deno install -A npm:@polyplug/cli`), `polyplugc` on PyPI (platform
  wheels — `pip install` / `uv tool install` / `pipx`), and `Polyplug.Cli` on
  NuGet (`dotnet tool install -g`, all three RIDs bundled). `cargo install
  polyplugc` and the prebuilt-binary installer remain. Supported platforms:
  linux-x64, macos-arm64, windows-x64.

### Changed

- **Code generation emits all declaration FORM via langprint 0.2.2** across both
  code-generation pipelines — the `polyplugc` contract generators and the
  `polyplug_codegen` ABI-SDK mirrors (`sdks/*/abi`). Type-mapping LOGIC stays in
  polyplug; the emitted output is byte-identical to 0.1.2, so this is an internal
  refactor with no user-visible change.

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
- **Bundle-signing key pinning (opt-in authenticity).** `RuntimeConfig` gains a
  `trusted_keys: Array<Ed25519PublicKey>` field. Empty (the default) keeps the
  Trust-On-First-Use behaviour — verification proves *integrity* only. When the
  host configures one or more keys (via `RuntimeBuilder::trusted_keys(&[VerifyingKey])`),
  the runtime additionally requires each bundle's embedded `bundle.sig` key to be a
  member of the allowlist, so a bundle re-signed with an attacker key is rejected
  (`LoaderError::UntrustedSigningKey` under `Required`, logged under `WarnOnly`).
  Only public verifying keys are pinned; the private key stays offline. A malformed
  host key fails the load with `LoaderError::MalformedTrustedKey`. New
  `polyplug_signing` surface: `PinnedKeyVerifier`, `verifying_key_from_bytes`, and
  `SigError::UntrustedKey`.
- **JS SDK runs on Node.js (koffi) and Bun (bun:ffi) in addition to Deno**,
  behind one runtime-detected FFI seam (`sdks/js/abi/ffi/`). `getBackend()` picks
  the backend per runtime — `Deno.dlopen` under Deno, the `koffi` C-FFI module
  under Node (an auto-installed **optional** dependency), and the built-in
  `bun:ffi` under Bun — so the same published `@polyplug/host` /
  `@polyplug/loaders-native` packages work on all three. The host-SDK test suite
  runs identically on every runtime, and the install-smoke
  (`examples/smoke/js_install_smoke.sh`) loads the embedded native through the
  Deno, Node, and Bun FFI backends from the published tarballs.

### Changed (ABI, pre-1.0)

- `RuntimeConfig` gained a `signature_policy` field (`SignaturePolicy`, `repr(u32)`)
  at offset `0x2C`, filling the former tail padding after `log_max_level`, and a
  `trusted_keys: Array<Ed25519PublicKey>` field at offset `0x30` (a new `#[repr(C)]`
  32-byte `Ed25519PublicKey` type). The struct grows from 48 to **72 bytes**
  (align 8); every pre-existing field offset is unchanged and all six host SDK abi
  mirrors carry the new type and field. Hosts that zero-initialize the config keep
  the previous behavior (`Off` policy, empty `trusted_keys` = TOFU). Owner-approved
  pre-1.0 ABI change.

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
- **Bundle-signing hardening.** The canonical digest now **rejects symlinks and
  irregular files** (fifo/socket/device) and an **empty bundle**, closing a
  signature-bypass hole where a symlinked artifact was excluded from the digest
  yet still `dlopen`ed. Path relativization is now a hard error (`PathOutsideBundle`)
  instead of a silent absolute-path fallback that could yield a non-reproducible,
  machine-specific digest. The signed message gains a **domain-separation prefix**:
  the fixed tag `polyplug-bundle-sig\0`, a 1-byte algorithm version (`0x01`), and
  the file count (`u64` little-endian) before the per-file entries. The native
  loader now **confines the artifact path** to the bundle directory — a
  `manifest.file` that is a symlink or canonicalizes outside the bundle root
  (`../../evil.so`, an absolute path) is rejected with
  `LoaderError::ArtifactPathEscape`. `save_signing_key` now creates the private key
  file with `0o600` from the start (and tightens a pre-existing `0o644` file before
  writing any secret bytes), eliminating the brief world-readable window.

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
  interface for fast dispatch and detect a hot-reload/unload through the
  `HostApi.registry_revision` callback. The runtime performs the acquire load before
  returning the registry revision value; callers compare that value before each
  dispatch and re-resolve/recreate their instance on a change. A cached interface
  pointer never dangles after a reload/unload, and callers never access runtime-owned
  atomic storage. Applied across all six language generators.
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

[Unreleased]: https://github.com/polyplug/polyplug/compare/v0.1.3...HEAD
[0.1.3]: https://github.com/polyplug/polyplug/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/polyplug/polyplug/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/polyplug/polyplug/releases/tag/v0.1.1
[0.1.0]: https://github.com/polyplug/polyplug/releases/tag/v0.1.0
