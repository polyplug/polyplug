# polyplug

**High-performance zero-overhead cross-language cross-platform plugin runtime**

## Overview

polyplug is a plugin runtime that enables seamless cross-language plugin development. Write plugins in Rust, Python, C#, Lua, JavaScript, or C++ — and load them all from a single host application with zero-overhead FFI dispatch.

**Just add polyplug as a dependency and it works on Linux, macOS, and Windows.** No manual downloads, no build-from-source requirements, no obstacles.

## Features

- **Cross-Language** — Write plugins in Rust, Python, C#, Lua, JavaScript/Deno, or C++
- **Cross-Platform** — Works on Linux (x64), macOS (x64/ARM64), and Windows (x64) with zero setup
- **Hot Reload** — Reload plugins at runtime with notification system for seamless updates
- **Zero-Overhead FFI** — Direct function pointer dispatch with no runtime overhead
- **Type-Safe Code Generation** — The `polyplugc` CLI generates type-safe bindings for all languages
- **Factory Method Pattern** — Safe instance management with host-controlled lifecycles
- **Multiple Loader Types** — Native, Python, Lua, JavaScript, Deno, and .NET loaders

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
├── host-libs/              # Host libraries for each language
│   ├── rust/
│   ├── python/
│   ├── csharp/
│   ├── lua/
│   ├── js/
│   └── cpp/
├── guest-libs/             # Guest libraries for plugin authors
│   ├── rust/
│   ├── python/
│   ├── csharp/
│   ├── lua/
│   ├── js/
│   └── cpp/
└── examples/               # Example hosts and plugins
    ├── hosts/
    └── guests/
```

## Documentation

- **Host Libraries** — See `host-libs/` for integrating polyplug into your application
- **Guest Libraries** — See `guest-libs/` for writing plugins in each language
- **Examples** — See `examples/` for complete working examples

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate Rust bindings
polyplugc generate --bundle bundle.toml --lang rust --out src/generated

# Generate Python bindings
polyplugc generate --bundle bundle.toml --lang python --out src/generated

# Generate C# bindings
polyplugc generate --bundle bundle.toml --lang csharp --out src/generated
```

## Repository

[github.com/polyplug/polyplug](https://github.com/polyplug/polyplug)

## License

MIT License — see [LICENSE](LICENSE) for details.