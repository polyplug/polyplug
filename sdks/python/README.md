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

First generate the guest bindings for your contract with `polyplugc` (see
[Code Generation](#code-generation) below). This produces a
`generated/guest/contracts.py` module containing, for each contract:

- a base class (e.g. `DECODERPipelineDecoderPlugin`) with one method per
  contract function,
- a `set_<contract>_impl()` registration function,
- the `polyplug_init` entry point and `polyplug_abi_version` the loader calls.

Write your plugin by subclassing the generated base class and registering an
instance at module load time:

```python
from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_impl,
    polyplug_abi_version,
    polyplug_init,
)
from polyplug_guest import to_str, alloc_string


class DecoderImpl(DECODERPipelineDecoderPlugin):
    def decode(self, input):
        s = to_str(input).replace(",", "|")
        return alloc_string(f"DECODED:{s}")


set_decoder_impl(DecoderImpl())
```

`to_str(view)` decodes an incoming `StringView` to a Python `str`, and
`alloc_string(s)` allocates an outgoing `StringView` through the host allocator.
The loader resolves the generated `polyplug_init` / `polyplug_abi_version`
symbols directly — there is no decorator to apply.

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
- `GuestContractHandle` — Opaque plugin reference
- `GuestContractInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

Python wrappers over the polyplug C ABI using ctypes:
- `Runtime` — Main runtime class
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- ctypes bindings for all ABI functions

### Guest Library (`guest/`)

`polyplug_guest` — bootstrap helpers for Python plugins. The contract registration
and `polyplug_init` entry point are produced by `polyplugc` in the generated
`contracts.py`; this library provides the runtime helpers that generated code and
plugin authors rely on:
- `to_str(view)` — decode an incoming `StringView` to a Python `str`
- `alloc_string(s)` — allocate an outgoing `StringView` via the host allocator
- `store_host_interface(ptr)` / `get_host_interface()` — stash the host interface
  pointer for the allocator
- Re-exported ABI types — `HostApi`, `BundleInitContext`, `StringView`,
  `AbiError`, `AbiErrorCode`, and the other types generated code imports

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
