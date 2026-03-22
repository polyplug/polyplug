# polyplug Python SDK

Complete Python support for polyplug plugin runtime.

## Structure

```
sdks/python/
├── polyplug_abi/  # ABI type definitions (auto-generated from Rust)
├── host/          # Host runtime library for Python applications
├── guest/         # Guest library for Python plugin authors
└── loaders/       # Loader implementations (Python runtime adapter)
```

## Installation

### As Host Application

```bash
pip install polyplug
```

### As Plugin Author

```bash
pip install polyplug-guest
```

## Quick Start

### Host Application

```python
from polyplug import Runtime

runtime = Runtime()
runtime.load_bundle("./plugins/my_plugin")

# Use generated host callers to interact with plugins
decoder = PipelineDecoder.create(runtime)
if decoder:
    result = decoder.decode(input)
```

### Plugin Author

```python
from polyplug_guest import plugin, PluginRegistrar

@plugin
def init(registrar: PluginRegistrar, ctx):
    registrar.register(PipelineDecoder, DecoderImpl())

class DecoderImpl:
    def decode(self, input: str) -> str:
        return f"DECODED:{input}"
```

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate Python bindings from api.toml
polyplugc generate --api api.toml --lang python --out ./generated

# Generate Python bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang python --out ./src/generated
```

## Components

### ABI (`polyplug_abi/`)

Auto-generated from Rust ABI definitions. Contains:
- `StringView` — UTF-8 string view
- `Buffer` — Byte buffer with host allocator
- `AbiError` — Error code and message
- `PluginHandle` — Opaque plugin reference
- `PluginInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

Python wrappers over the polyplug C ABI using ctypes:
- `Runtime` — Main runtime class
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- ctypes bindings for all ABI functions

### Guest Library (`guest/`)

Bootstrap layer for Python plugins:
- `@plugin` decorator — Marks plugin entry point
- `PluginRegistrar` — Contract registration
- `PluginContext` — Bundle metadata
- Exception boundary — Plugin crashes don't take down host

### Loaders (`loaders/`)

Python runtime adapter:
- `register_python_loader()` — Register Python loader with runtime
- Automatic CPython embedding
- GIL management

## Performance Notes

- **Backend**: ctypes (default) or cffi (faster, optional)
- **String handling**: Native UTF-8, no transcoding needed
- **Memory**: All cross-boundary data in ctypes structures
- **GIL**: Released during Rust-only operations

## Requirements

- Python 3.10 or later
- CPython (not PyPy for best compatibility)

## See Also

- `../csharp/` — C# SDK
- `../cpp/` — C++ SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
