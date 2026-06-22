# polyplug

**The native-speed plugin runtime for trusted, first-party extensibility — across six languages.**

## Overview

polyplug lets a single host application load and call plugins written in Rust, C++, C#, Python, Lua, or JavaScript, through a frozen C ABI with near-native dispatch (~2.4 ns/call for native languages). Plugins run as real native code or real language runtimes (CPython, the .NET CLR, LuaJIT, QuickJS) — *not* compiled to WebAssembly — so you keep zero-copy data sharing and full language-native behavior. The `polyplugc` CLI generates the typed glue for each language from a small `.toml` contract.

**polyplug is built for _trusted_ plugins** — code you write or vet (first-party features, partner integrations, a vetted-author ecosystem). It runs plugins in-process with no sandbox. If you need to run _untrusted_ third-party code, use a WebAssembly runtime (Extism, Wasmtime) instead — see [When to use polyplug](#when-to-use-polyplug) below. This is the same trust model the most successful native extension ecosystems use (e.g. VS Code extensions): **vet the author, not the sandbox.**

**Status: 0.1.1 — first functional public release.** Published to crates.io, PyPI, NuGet, LuaRocks, npm, and JSR (see [Installation](#installation)). The ABI is still **pre-1.0** — ABI-visible changes are permitted between releases until the 1.0 freeze (see [docs/TRUST_MODEL.md](docs/TRUST_MODEL.md)). The full test suite runs on Linux, macOS, and Windows.

![one plugin call, end to end — by plugin language](docs/assets/benches/hero.svg)

> One plugin call, end to end — **lower is better**, log scale (a bar twice as long is 10× slower, not 2×).
> Measured locally from live benchmark runs; see [docs/PERFORMANCE.md](docs/PERFORMANCE.md#how-to-read-these-charts) for how to read these charts and reproduce the numbers.

## When to use polyplug

polyplug and WebAssembly plugin runtimes solve different problems. Pick by your trust boundary:

| | **polyplug** (native C ABI) | **WASM runtimes** (Extism, Wasmtime) |
|---|---|---|
| Best for | Trusted / first-party / vetted-author plugins | Untrusted, third-party, multi-tenant plugins |
| Isolation | None — plugins run in-process with host privileges | Sandboxed: memory-isolated, capability-gated |
| Dispatch overhead | ~2.4 ns (native), near-zero | Higher — marshalling + sandbox boundary |
| Data sharing | Zero-copy shared buffers + per-call arena | Copied across the sandbox boundary |
| Languages | Real CPython, .NET CLR, LuaJIT, QuickJS, native Rust/C++/C# | Anything that compiles to WASM (toolchain maturity varies by language) |
| Cold start | None | Per-module instantiation cost |

**Rule of thumb:** if you control or vet your plugin authors and want native speed with real language runtimes, use polyplug. If you must run arbitrary untrusted code safely, use a WASM runtime — we'll happily tell you so.

## Security & trust model

polyplug runs plugins **in-process, with the host's privileges, and no sandbox.** That is the right model for trusted plugins and a deliberate non-goal for untrusted ones. What polyplug *does* guarantee:

- **ABI compatibility is checked at load** — a bundle whose contract version doesn't match is rejected with a clear error, never silent UB.
- **The runtime's own create/destroy path is crash-isolated** — a bug in polyplug's two C ABI exports surfaces as a null/no-op + recorded error, never a host abort.
- **`catch_unwind` at every FFI boundary is real** — `panic = "abort"` is intentionally never set. A *plugin* that lets a panic/exception escape its generated glue is a plugin defect with a defined outcome (process abort).
- **Bundle signing & verification** — a bundle can carry a detached Ed25519 `bundle.sig` over a canonical digest of every file it contains. The host picks a `SignaturePolicy` — `Off` (default), `WarnOnly`, or `Required` — that decides whether an unsigned or tampered bundle is warned about or rejected at load. The model is **freedom-preserving (TOFU)**: verification proves a bundle is intact and self-consistently signed *without* requiring the host to pre-know or allowlist the signer, so app users stay free to load plugins from unknown authors. Sign with `polyplugc keygen` then `polyplugc sign`; an opt-in key-pinning layer can be added later behind the `BundleVerifier` seam without breaking the format.

Planned (post-0.1.x): an **optional process-isolation mode**. See [docs/TRUST_MODEL.md](docs/TRUST_MODEL.md) for the full threat model and [docs/ROADMAP.md](docs/ROADMAP.md) for the security roadmap.

## Features

- **Cross-Language** — Write plugins in Rust, Python, C#, Lua, JavaScript (QuickJS), or C++ (host applications can also be written in any of the six, including JS on Deno)
- **Cross-Platform** — Linux (x64), macOS (x64/ARM64), and Windows (x64)
- **Hot Reload** — Native, Lua, and JS (QuickJS) bundles reload at runtime; the host observes reloads through the `on_reload` phase callback (Python and .NET bundles do not hot-reload)
- **Zero/Minimal-Overhead FFI** — Direct function pointer dispatch with near-zero overhead for native languages (~2.4 ns/call measured), minimal overhead for VM-based languages
- **Lock-Free Reads** — Contract resolution (`find`/`resolve`) serves from an immutable, epoch-published registry snapshot with no lock on the hot path; writers swap the published view atomically on each registration, unload, and reload
- **True Unload** — Unloading a bundle marks its handles stale, removes it from the registry, and reclaims its interface **and** the backing library/VM once no in-flight reader is still pinned (crossbeam-epoch deferred reclamation) — never a retain-forever leak. Native/Lua/JS free the library/VM; Python purges its module cache; .NET unloads its collectible `AssemblyLoadContext`
- **Crash-Isolated Embedding** — A defect in polyplug's *own* runtime create/destroy path can never abort your application: the two C ABI exports (`polyplug_runtime_create` / `polyplug_runtime_destroy`) contain any internal panic and surface it as a null/no-op result plus a recorded `last_error`, so embedding polyplug never crashes the host process on a bug in our code. (A misbehaving *plugin* that lets a panic or exception escape its own generated glue is a plugin defect with a defined outcome — process abort — see [TRUST_MODEL.md](docs/TRUST_MODEL.md).)
- **Runtime Isolation** — The `Runtime` holds no global or thread-local state, so multiple isolated runtimes coexist in one process (CPython and the .NET CLR are the documented once-per-process exceptions)
- **Model-Checked Concurrency** — The epoch publish/reclaim protocol behind lock-free reads and safe unload is exhaustively model-checked with [loom](https://docs.rs/loom)
- **Type-Safe Code Generation** — The `polyplugc` CLI generates type-safe bindings for all languages
- **Multiple Loader Types** — Native, Python, Lua, JavaScript (QuickJS), and .NET loaders

## Quick Start

### Installation

Each language ships the same package shape — `abi`, `host`, `guest`, and one
loader per plugin language. Install the host SDK plus whichever loaders you need:

```bash
# Rust (the polyplug crate IS the runtime)
cargo add polyplug polyplug_abi polyplug_native   # + polyplug_{python,lua,js,dotnet} as needed
cargo install polyplugc                            # contract → bindings CLI

# Python
pip install polyplug polyplug-abi polyplug-loaders-native

# C# / .NET
dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native

# Lua (LuaJIT)
luarocks install polyplug polyplug-loader-native

# JavaScript / TypeScript (Deno runtime required)
deno add jsr:@polyplug/host jsr:@polyplug/loaders-native   # or npm:@polyplug/host
```

- The Rust `polyplug` crate is the runtime engine; every other `host` package is
  an FFI binding that loads it. Guest authors add the `guest` package for their
  language. The `polyplugc` CLI generates the typed glue from a `.toml` contract.
- The JS/TS packages target the **Deno** runtime (they use `Deno.dlopen`); the
  npm packages publish the same code for name reservation and Deno consumption.
- To build from source instead: `cargo build --release` then
  `bash examples/build_all.sh`.
- See [`docs/QUICKSTART.md`](docs/QUICKSTART.md) for the full end-to-end setup.

### Basic Usage

A Rust host builds a `Runtime`, registers the loaders it needs, loads bundles,
then calls plugins through `polyplugc`-generated typed callers (condensed from
[`examples/hosts/rust`](examples/hosts/rust)):

```rust
use polyplug::runtime::Runtime;
use polyplug_native::{NativeConfig, NativeLoader};

fn run() -> Result<(), String> {
    let runtime: Runtime = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig {}))
        // .loader(LuaLoader / JsLoader / PythonLoader / DotnetLoader ...)
        .build()
        .map_err(|e| e.to_string())?;

    // Load a plugin bundle directory (any supported language)
    runtime
        .load_bundle("path/to/bundle".as_ref())
        .map_err(|e| e.to_string())?;

    // Call plugin functions through polyplugc-generated typed callers
    // (find_contract::<MyContract>(&runtime, MY_CONTRACT_ID) -> decode(...) etc.)
    Ok(())
}
```

## Project Structure

```
polyplug/
├── crates/
│   ├── polyplug/           # Rust runtime core
│   ├── polyplug_abi/       # ABI definitions
│   ├── polyplug_utils/     # Shared hash utilities (bundle_id, contract_id)
│   ├── polyplug_native/    # Native (cdylib) bundle loader
│   ├── polyplug_python/    # Python bundle loader
│   ├── polyplug_lua/       # Lua bundle loader
│   ├── polyplug_js/        # JavaScript (QuickJS) bundle loader
│   ├── polyplug_dotnet/    # .NET/C# bundle loader
│   ├── polyplug_codegen/   # Code generation library
│   ├── polyplugc/          # CLI codegen tool
│   └── sdk_validator/      # Validates SDK helpers against the ABI
├── sdks/                   # Cross-language SDKs
│   ├── rust/               # Rust SDK (abi/, guest/ — the host side IS the polyplug crate)
│   ├── cpp/                # C++ SDK (abi/, host/, guest/, loaders/)
│   ├── csharp/             # C# SDK (abi/, host/, guest/, loaders/)
│   ├── python/             # Python SDK (abi/, host/, guest/, loaders/)
│   ├── lua/                # Lua SDK (abi/, host/, guest/, loaders/)
│   └── js/                 # JavaScript SDK (abi/, host/, guest/, loaders/)
└── examples/               # Example hosts and plugins
    ├── api.toml
    ├── hosts/              # One example host per language
    ├── guests/             # Guest sources per language
    ├── plugins/            # Built example bundles (rust/cpp/lua/js/python)
    └── plugins-csharp/     # Built C# example bundles (need the .NET loader)
```

## Documentation

- **Architecture** — See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for diagrams of the runtime pipelines (bundle load, lock-free dispatch, plugin-calls-plugin, loaders, hot-reload, unload)
- **Quick Start** — See [`docs/QUICKSTART.md`](docs/QUICKSTART.md) to write your first plugin in 10 minutes
- **Examples** — See [`docs/EXAMPLES.md`](docs/EXAMPLES.md) for the full reference gallery (30 guests × 6 hosts)
- **Features** — See [`docs/FEATURES.md`](docs/FEATURES.md) for a current-state overview of every shipped feature (arena, hot-reload, unload, platform support, trust model)
- **Workflow** — See [`docs/WORKFLOW.md`](docs/WORKFLOW.md) for the end-to-end host-app and plugin-developer pipelines
- **SDKs** — See `sdks/` for host and guest libraries in each language
- **Design Docs** — See `docs/` for architecture and design decisions

## Code Generation

Use `polyplugc` to generate type-safe bindings and validate the result:

```bash
# Host side: typed callers + registration glue from the app's API
polyplugc generate --api api.toml --lang rust --out generated

# Guest side: contract stubs + ship-ready manifest.toml
polyplugc generate --bundle bundle.toml --lang python --out generated

# Check an assembled bundle directory before shipping
polyplugc validate --bundle-dir dist/my_plugin
```

## Repository

[github.com/polyplug/polyplug](https://github.com/polyplug/polyplug)

## License

MIT License — see [LICENSE](LICENSE) for details.