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

## Hot-Reload

To enable hot-reload, set `hot_reload_enabled=True` and register an `on_reload` callback:

```python
from polyplug import Runtime, RuntimeConfig, ReloadPhase

# Enable hot-reload
config = RuntimeConfig(hot_reload_enabled=True)
Runtime.set_config(config)

# Register callback before creating runtime
def on_reload(phase):
    if phase.type == ReloadPhase.PREPARING:
        # Destroy instances for this bundle
        instances.pop(phase.bundle_id, None)
    elif phase.type == ReloadPhase.RELOADED:
        print(f"Reloaded: {phase.bundle_name}")
    elif phase.type == ReloadPhase.FAILED:
        print(f"Failed: {phase.reason}")

Runtime.on_reload(on_reload)

runtime = Runtime()
```

**Key points:**
- `hot_reload_enabled` defaults to `False` — must be explicitly enabled
- Callback must be registered **before** creating the runtime
- Host must track and destroy instances on `PREPARING` notification
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

## Performance Notes

- **Backend**: ctypes (default) or cffi (faster, optional)
- **String handling**: Native UTF-8, no transcoding needed
- **Memory**: All cross-boundary data in ctypes structures
- **GIL**: Released during Rust-only operations

## Requirements

- Python 3.10 or later
- CPython (not PyPy for best compatibility)

## Runtime Isolation Note

The Python loader uses a **process-wide CPython interpreter**. This means:
- Multiple `Runtime` instances in the same process share the same Python interpreter
- Python plugins from different runtimes can see each other's modules and state
- **For full isolation between Python runtimes, use separate processes**

Other loaders (Lua, JavaScript, Native) provide per-runtime isolation.

## See Also

- `../csharp/` — C# SDK
- `../cpp/` — C++ SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
