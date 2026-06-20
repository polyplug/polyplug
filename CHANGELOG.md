# Changelog

All notable changes to polyplug are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The ABI is **pre-1.0**: ABI-visible changes are still permitted between releases
and are called out explicitly below. The ABI freezes at 1.0 — see
[`docs/TRUST_MODEL.md`](docs/TRUST_MODEL.md) and
[`docs/ABI_ARCHITECTURE.md`](docs/ABI_ARCHITECTURE.md).

## [Unreleased]

### Breaking

- **`HostApi.call_guest_method` removed.** The dynamic, host-mediated guest→guest
  dispatch field (formerly at offset 136) has been removed from the ABI. All generated
  peer callers (Rust, C++, C#, Python, Lua) now dispatch directly through the cached
  interface; JS peer callers dispatch through the loader-side `callGuestMethod` bridge.
  The struct shrinks from 192 to 184 bytes (21 function pointers); all offsets from
  `unload_bundle` onward shift down by 8: `unload_bundle` 144→136, `log` 152→144,
  `create_guest_instance` 160→152, `destroy_guest_instance` 168→160,
  `revision_counter` 176→168, `reserved` 184→176.

## [0.1.0]

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

[Unreleased]: https://github.com/polyplug/polyplug/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/polyplug/polyplug/releases/tag/v0.1.0
