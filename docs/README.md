# polyplug

**The native-speed plugin runtime for trusted, first-party extensibility — across six languages.**

polyplug lets a single host application load and call plugins written in **Rust, C++,
C#, Python, Lua, or JavaScript**, through a frozen C ABI with near-native dispatch
(~2.4 ns/call for native languages). Plugins run as real native code or real language
runtimes (CPython, the .NET CLR, LuaJIT, QuickJS) — *not* compiled to WebAssembly — so
you keep zero-copy data sharing and full language-native behavior. The `polyplugc` CLI
generates the typed glue for each language from a small `.toml` contract.

![one plugin call, end to end — by plugin language](assets/benches/hero.svg)

> One plugin call, end to end — **lower is better**, log scale (a bar twice as long is
> 10× slower, not 2×). Measured locally from live benchmark runs; see
> [Performance](PERFORMANCE.md#how-to-read-these-charts) for how to read these charts.

## Built for *trusted* plugins — vet the author, not the sandbox

polyplug is built for **trusted** plugins: code you write or vet — first-party features,
partner integrations, a vetted-author ecosystem. It runs plugins **in-process with the
host's privileges and no sandbox.** If you need to run **untrusted** third-party code,
use a WebAssembly runtime (Extism, Wasmtime) instead. This is the same trust model the
most successful native extension ecosystems use (e.g. VS Code extensions): **vet the
author, not the sandbox.**

| | **polyplug** (native C ABI) | **WASM runtimes** (Extism, Wasmtime) |
|---|---|---|
| Best for | Trusted / first-party / vetted-author plugins | Untrusted, third-party, multi-tenant plugins |
| Isolation | None — plugins run in-process with host privileges | Sandboxed: memory-isolated, capability-gated |
| Dispatch overhead | ~2.4 ns (native), near-zero | Higher — marshalling + sandbox boundary |
| Data sharing | Zero-copy shared buffers + per-call arena | Copied across the sandbox boundary |
| Languages | Real CPython, .NET CLR, LuaJIT, QuickJS, native Rust/C++/C# | Anything that compiles to WASM (toolchain maturity varies) |
| Cold start | None | Per-module instantiation cost |

**Rule of thumb:** if you control or vet your plugin authors and want native speed with
real language runtimes, use polyplug. If you must run arbitrary untrusted code safely,
use a WASM runtime — we'll happily tell you so. The full threat model is in
[Trust Model](TRUST_MODEL.md).

## What polyplug guarantees

Within the "trusted, in-process" boundary, polyplug still makes hard guarantees:

- **ABI compatibility is checked at load** — a bundle whose contract version doesn't
  match is rejected with a clear error, never silent UB.
- **The runtime's own create/destroy path is crash-isolated** — a bug in polyplug's two
  C ABI exports surfaces as a null/no-op plus a recorded error, never a host abort.
- **Lock-free reads + safe true-unload** — contract resolution serves from an
  epoch-published snapshot; unloading reclaims the interface *and* the backing
  library/VM once no reader is still pinned (model-checked with
  [loom](https://docs.rs/loom)).
- **Bundle signing & verification** — a bundle can carry a detached Ed25519 `bundle.sig`
  over a canonical digest of every file it contains. The host picks a `SignaturePolicy`
  — `Off` (default), `WarnOnly`, or `Required`. The default model is **TOFU** (integrity
  without pre-known signers); a host that trusts specific authors opts into **key
  pinning** for authenticity. See [Trust Model § Bundle Signing](TRUST_MODEL.md#bundle-signing)
  and [Feature Guide § 11](FEATURES.md).

## Where to go next

- **New here?** Start with the [Quick Start](QUICKSTART.md), then browse the
  [Examples](EXAMPLES.md).
- **Embedding polyplug?** Read the [Architecture Overview](ARCHITECTURE.md) and the
  [Host Contracts](HOST_CONTRACTS.md) guide.
- **Writing a plugin?** See [Plugin Interface Design](PLUGIN_INTERFACE_DESIGN.md) and the
  [Feature Guide](FEATURES.md).
- **Deploying to production?** Read [Debugging Native Crashes](DEBUGGING_NATIVE_CRASHES.md)
  and the [Trust Model](TRUST_MODEL.md).
- **Going deep on the ABI?** [ABI Architecture](ABI_ARCHITECTURE.md),
  [ABI Types](abi_types.md), and the generated [API Reference](API_REFERENCE.md).

## Status

Published to crates.io, PyPI, NuGet, LuaRocks, npm, and JSR. The ABI is **pre-1.0** —
ABI-visible changes are permitted between releases (see [Trust Model](TRUST_MODEL.md)).
The full test suite runs on Linux, macOS, and Windows. Found a vulnerability? Report it
privately — see the [Security Policy](security-policy.md).
