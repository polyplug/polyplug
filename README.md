# polyplug

**High-performance zero/minimal-overhead cross-language cross-platform plugin runtime**

## Overview

polyplug is a plugin runtime that enables seamless cross-language plugin development. Write plugins in Rust, Python, C#, Lua, JavaScript, or C++ — and load them all from a single host application with zero/minimal-overhead FFI dispatch.

**Just add polyplug as a dependency and it works on Linux, macOS, and Windows.** No manual downloads, no build-from-source requirements, no obstacles.

## Features

- **Cross-Language** — Write plugins in Rust, Python, C#, Lua, JavaScript/Deno, or C++
- **Cross-Platform** — Works on Linux (x64), macOS (x64/ARM64), and Windows (x64) with zero setup
- **Hot Reload** — Reload plugins at runtime with notification system for seamless updates
- **Zero/Minimal-Overhead FFI** — Direct function pointer dispatch with no runtime overhead for native languages, minimal overhead for VM-based languages
- **Type-Safe Code Generation** — The `polyplugc` CLI generates type-safe bindings for all languages
- **Singleton Plugin Implementations** — Each contract has one implementation; host creates caller wrappers with Arc-based lifecycle
- **Multiple Loader Types** — Native, Python, Lua, JavaScript (QuickJS), and .NET loaders

## Quick Start

### Installation

#### Rust

```toml
# Cargo.toml
[dependencies]
polyplug = "0.1"
polyplug_abi = "0.1"
```

#### Python

```bash
pip install polyplug
# Native library is bundled - no additional setup needed
```

#### C# / .NET

```bash
dotnet add package Polyplug
# Native libraries for all platforms are bundled in the package
```

#### Lua (LuaRocks)

```bash
luarocks install polyplug
# Native library is bundled
```

#### JavaScript / Deno

```typescript
import { Runtime } from "@polyplug/runtime";
// Native library auto-detected and loaded
```

#### C++

```cmake
find_package(Polyplug REQUIRED)
# Downloads native library if not found locally
```

### Basic Usage

```rust
use polyplug::{PluginHost, PluginDescriptor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = PluginHost::new()?;
    
    // Load a plugin from any supported language
    let plugin = host.load("path/to/plugin.polyplug")?;
    
    // Call plugin functions through generated bindings
    let result = plugin.my_function(&input)?;
    
    Ok(())
}
```

## Project Structure

```
polyplug/
├── crates/
│   ├── polyplug/           # Rust runtime core
│   ├── polyplug_abi/       # ABI definitions
│   ├── polyplug_guest/     # Guest library for Rust plugins
│   ├── polyplugc/          # CLI codegen tool
│   └── polyplug_codegen/   # Code generation library
├── sdks/                   # Cross-language SDKs (host + guest + ABI)
│   ├── csharp/             # C# SDK (abi/, host/, guest/, loaders/)
│   ├── python/             # Python SDK (abi/, host/, guest/, loaders/)
│   ├── cpp/                # C++ SDK (abi/, host/, guest/, loaders/)
│   ├── lua/                # Lua SDK (abi/, host/, guest/, loaders/)
│   └── js/                 # JavaScript SDK (abi/, host/, guest/, loaders/)
└── examples/               # Example hosts and plugins
    ├── hosts/
    └── guests/
```

## Documentation

- **Features** — See [`docs/FEATURES.md`](docs/FEATURES.md) for a current-state overview of every shipped feature (arena, hot-reload, extensions, platform support, trust model)
- **Workflow** — See [`docs/WORKFLOW.md`](docs/WORKFLOW.md) for the end-to-end host-app and plugin-developer pipelines
- **SDKs** — See `sdks/` for host and guest libraries in each language
- **Examples** — See `examples/` for complete working examples
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