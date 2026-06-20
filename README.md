# polyplug

**High-performance zero/minimal-overhead cross-language cross-platform plugin runtime**

## Overview

polyplug is a plugin runtime that enables seamless cross-language plugin development. Write plugins in Rust, Python, C#, Lua, JavaScript, or C++ — and load them all from a single host application with zero/minimal-overhead FFI dispatch.

**Status: pre-release.** The project is pre-1.0 and not yet published to any package registry — build from source (see [Installation](#installation)). The full test suite runs on Linux, macOS, and Windows.

![one plugin call, end to end — by plugin language](docs/assets/benches/hero.svg)

> One plugin call, end to end — **lower is better**, log scale (a bar twice as long is 10× slower, not 2×).
> Measured locally from live benchmark runs; see [docs/PERFORMANCE.md](docs/PERFORMANCE.md#how-to-read-these-charts) for how to read these charts and reproduce the numbers.

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

polyplug is not yet published to crates.io, PyPI, NuGet, LuaRocks, or npm —
everything builds from this workspace:

```bash
# Core runtime, loaders, and the polyplugc CLI
cargo build --release

# Build the example plugin bundles (all six languages)
bash examples/build_all.sh
```

- **Rust hosts** depend on the `polyplug` and `polyplug_abi` crates by path.
- **Other host languages** use the SDKs under `sdks/<lang>/host` together with
  the built loader cdylibs (`target/release/deps/libpolyplug*.so`).
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