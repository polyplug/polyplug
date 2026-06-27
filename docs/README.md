# polyplug

**A blazing-fast, zero-overhead, cross-language, cross-platform plugin runtime — write both your app *and* its plugins in any of six languages (Rust, C++, C#, Python, Lua, JavaScript), over a frozen C ABI.**

A Rust host can load a Python plugin; a Bun/JavaScript host can load a C++ plugin — any
host language pairs with any guest language, over a frozen C ABI with near-native dispatch
([~2.4 ns/call for native languages](PERFORMANCE.md)). Plugins run as real native code or
real language runtimes (CPython, the .NET CLR, LuaJIT, QuickJS), not WebAssembly — see
[Architecture](ARCHITECTURE.md). The `polyplugc` CLI generates the typed glue for each
language from a small `.toml` contract.

Each language is both a host language (embed the runtime, load and call plugins) and a
guest language (write a plugin other apps load); pick per side independently — see the
per-language **Host** and **Guest** guides in the Languages section.

![one plugin call, end to end — by plugin language](assets/benches/hero.svg)

> One plugin call, end to end — **lower is better**, log scale. Measured locally from live
> benchmark runs; see [Performance](PERFORMANCE.md#how-to-read-these-charts).

## No sandbox, by design — vet the author, not the runtime

| | **polyplug** (native C ABI) | **WASM runtimes** (Extism, Wasmtime) |
|---|---|---|
| Best for | Trusted / first-party / vetted-author plugins | Untrusted, third-party, multi-tenant plugins |
| Isolation | None — plugins run in-process with host privileges | Sandboxed: memory-isolated, capability-gated |
| Dispatch overhead | ~2.4 ns (native), near-zero | Higher — marshalling + sandbox boundary |
| Data sharing | Zero-copy shared buffers + per-call arena | Copied across the sandbox boundary |
| Languages | Real CPython, .NET CLR, LuaJIT, QuickJS, native Rust/C++/C# | Anything that compiles to WASM (toolchain maturity varies) |
| Cold start | None | Per-module instantiation cost |

**Rule of thumb:** if you control or vet your plugin authors, use polyplug; if you must
run arbitrary untrusted code safely, use a WASM runtime. The full threat model is in
[Trust Model](TRUST_MODEL.md).

## What polyplug guarantees

Within the trusted, in-process boundary, polyplug guarantees:

- **ABI compatibility is checked at load** — a bundle whose contract version doesn't
  match is rejected with a clear error, never silent UB.
- **The runtime's own create/destroy path is crash-isolated** — a bug in polyplug's two
  C ABI exports surfaces as a null/no-op plus a recorded error, never a host abort.
- **Lock-free reads + safe true-unload** — contract resolution is lock-free, and
  unloading reclaims the interface and its backing library/VM once no reader is still
  pinned. See [Architecture](ARCHITECTURE.md).
- **Bundle signing** — optional Ed25519 bundle signing. See
  [Trust Model § Bundle Signing](TRUST_MODEL.md#bundle-signing).

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
