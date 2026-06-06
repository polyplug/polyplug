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

`to_str(view)` decodes an incoming `StringView` to a Python `str`.
`alloc_string(host_ptr, s)` allocates an outgoing `StringView` through the host
allocator (for data that must outlive the call), and
`alloc_string_arena(arena_alloc, s)` allocates a per-call return `StringView`
from the active arena. The loader resolves the generated `polyplug_init` symbol
directly — there is no decorator to apply.

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate Python bindings from api.toml
polyplugc generate --api api.toml --lang python --out ./generated

# Generate Python bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang python --out ./src/generated
```

## Bundle layout

Assemble the bundle directory yourself — the entry module plus any required modules:

```
dist/my-plugin/
├── manifest.toml          # emitted by `generate` (carries the precomputed bundle_id)
├── plugin.py              # the entry module (runtime = "python")
└── guest/                 # generated helper modules imported by plugin.py
```

The Python loader adds the bundle dir to `sys.path`, so in-bundle imports resolve.
Validate before shipping:

```bash
polyplugc validate --bundle-dir dist/my-plugin/
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

`polyplug_guest` — helpers for Python plugins. Python plugins are VM-dispatch
plugins: the guest never builds a `GuestContractInterface` or registers native
function pointers. The loader executes the module, calls
`polyplug_init(host_ptr: int, ctx_ptr: int) -> None`, then reads the
`_polyplug_registrations` list the guest deposited. This library provides the
helpers generated code and plugin authors rely on:
- `register_contract(globals(), contract, functions, plugin_name=None)` — deposit
  a contract's functions (ordered by `fn_id`) into the caller module's
  `_polyplug_registrations` list. Each function is called as
  `fn(args_ptr: int, out_ptr: int, arena_ptr: int)`; return for Ok, raise for error.
- `to_str(view)` — decode an incoming `StringView` to a Python `str`
- `alloc_string(host_ptr, s)` — allocate an outgoing `StringView` via the host
  allocator (for data that must outlive the call)
- `alloc_string_arena(arena_alloc, s)` — allocate a per-call return `StringView`
  from the active arena via the loader-injected `_polyplug_arena_alloc` bridge
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
