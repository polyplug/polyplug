# polyplug C++ SDK

Complete C++ support for polyplug plugin runtime.

## Structure

```
sdks/cpp/
├── abi/           # ABI type definitions (auto-generated from Rust)
├── host/          # Host runtime library for C++ applications
├── guest/         # Guest library for C++ plugin authors
└── loaders/       # Loader implementations
```

## Installation

### Via vcpkg

```bash
vcpkg install polyplug
```

### Via Conan

```bash
conan install --requires=polyplug/0.1
```

### Manual

Download from [releases](https://github.com/polyplug/polyplug/releases) and link manually.

## Quick Start

### Host Application

```cpp
#include <polyplug/runtime.hpp>

auto runtime = polyplug::Runtime::Builder()
    .PluginDir("./plugins")
    .Build();

// Load a plugin bundle
runtime.LoadBundle("./plugins/my_plugin");

// Use generated host callers to interact with plugins
auto decoder = PipelineDecoder::Create(runtime);
if (decoder) {
    auto result = decoder->Decode(input);
}
```

### Plugin Author

```cpp
#include <polyplug/guest.hpp>

POLYPLUG_PLUGIN_INIT {
    registrar.Register<PipelineDecoder>(std::make_unique<DecoderImpl>());
}

class DecoderImpl : public IPipelineDecoder {
public:
    std::string Decode(std::string_view input) override {
        return fmt::format("DECODED:{}", input);
    }
};
```

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate C++ bindings from api.toml
polyplugc generate --api api.toml --lang cpp --out ./generated

# Generate C++ bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang cpp --out ./src/generated
```

## Components

### ABI (`abi/`)

Auto-generated from Rust ABI definitions. Contains:
- `StringView` — UTF-8 string view (non-owning)
- `Buffer` — Byte buffer with host allocator
- `AbiError` — Error code and message
- `PluginHandle` — Opaque plugin reference
- `PluginInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

C++ wrappers over the polyplug C ABI:
- `Runtime` — Main runtime class with RAII
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- Zero-overhead ABI wrappers

### Guest Library (`guest/`)

Bootstrap layer for C++ plugins:
- `POLYPLUG_PLUGIN_INIT` macro — Entry point
- `PluginRegistrar` — Contract registration
- `PluginContext` — Bundle metadata
- Exception boundary — Plugin crashes don't take down host

### Loaders (`loaders/`)

Runtime adapters for loading C++ plugins:
- Native loader (dlopen/LoadLibrary)
- Register loader functions for other runtimes

## Performance Notes

- **Hot path**: Single indirect function call
- **Memory**: All cross-boundary data uses host allocator
- **Strings**: `std::string_view` for zero-copy string passing
- **RAII**: Automatic cleanup via destructors

## Requirements

- C++17 or later
- CMake 3.16+ for build integration

## See Also

- `../csharp/` — C# SDK
- `../python/` — Python SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
