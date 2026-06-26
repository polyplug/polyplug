# polyplug Python SDK

Build polyplug hosts and plugins in Python. The host side drives the native
runtime through ctypes; the guest side is a VM-dispatch plugin the Python loader
runs. Strings are native UTF-8 — no transcoding. Requires Python 3.10+ (CPython).

## Install

```bash
# Host application (+ a loader package per guest language you support)
pip install polyplug polyplug-abi
pip install polyplug-loaders-native    # native (.so / .dylib / .dll) bundles
# polyplug-loaders-{python,lua,js,dotnet} as needed

# Plugin author
pip install polyplug-guest polyplug-abi
```

Install the CLI to generate bindings:

```bash
uv tool install polyplugc      # or: pipx install polyplugc / cargo install polyplugc
```

## Generate bindings

```bash
polyplugc generate --bundle bundle.toml --lang python --out ./generated
```

## Host application

```python
from polyplug import Runtime

runtime = Runtime()
runtime.load_bundle("./plugins/my_plugin")

decoder = PipelineDecoder.create(runtime)
if decoder:
    result = decoder.decode(input)
```

## Plugin author

Subclass the generated base class and register it as the factory at module load.
The constructor receives the `HostApi` pointer — no host pointer is stored in the
SDK:

```python
from generated.guest.contracts import DECODERPipelineDecoderPlugin, set_decoder_factory

class DecoderImpl(DECODERPipelineDecoderPlugin):
    def __init__(self, host_ptr: int) -> None:
        self._host_ptr = host_ptr

    def decode(self, input):
        return f"DECODED:{input.replace(',', '|')}"

set_decoder_factory(DecoderImpl)
```

`polyplug_init` RETURNS its `(registrations, abi_error)` pair — nothing is
deposited into any module namespace.

> CPython initializes once per process: `Runtime` instances in the same process
> share one interpreter. Use separate processes for full isolation.

## Learn more

- [Python — Host guide][host] — embed the runtime, hot-reload, signing
- [Python — Guest guide][guest] — generate → implement → bundle
- [Python overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/python.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/python-host.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/python-guest.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
