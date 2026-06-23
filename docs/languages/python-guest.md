# Python — Guest (plugin)

A Python guest is a single `.py` file. There is no build step — you ship plain
Python source. The polyplug runtime discovers, loads, and calls it through a
generated ABI glue layer produced by `polyplugc`.

See the [Python overview](python.md) for install instructions.

The five Python guest examples live in `examples/guests/python/` (decoder,
encoder, transformer, reporter, validator). Each follows the steps below.

---

## 1. Install the guest SDK

```bash
pip install polyplug-guest polyplug-abi

# Install the CLI for code generation
pip install polyplugc               # or: uv tool install polyplugc
```

---

## 2. Define the contract (`api.toml`)

Contracts are shared between host and guest; the host author typically provides
`api.toml`. The pipeline example uses:

```toml
# examples/api.toml (excerpt)

[api]
name = "pipeline"
version = "1.0.0"

[[contract]]
name = "pipeline.Decoder"
version = "1.0"

[[contract.function]]
name = "decode"
args = [{ name = "input", type = "StringView" }]
returns = "StringView"
```

---

## 3. Write `bundle.toml`

`bundle.toml` is your plugin's manifest. Set `loader = "python"` and `file` to
your main `.py` file name.

```toml
# bundle.toml

[bundle]
name = "python_decoder"
version = "1.0.0"
api = "../../../api.toml"   # path to the shared api.toml
loader = "python"
file = "decoder.py"

[[plugin]]
name = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`implements` identifies the contract as `name@major_version`. The `loader` value
must be exactly `"python"`.

---

## 4. Generate guest glue code

```bash
polyplugc generate --bundle bundle.toml --lang python --out generated
```

This writes the following files into `generated/`:

```
generated/
├── manifest.toml            ship-ready manifest (never edit by hand)
└── guest/
    ├── contracts.py         generated plugin base class + factory setter + ABI dispatch
    ├── contracts.pyi        type stubs
    ├── host_contracts.py    host-contract callers (if api.toml defines host contracts)
    ├── host_contracts.pyi
    ├── init.py              polyplug_init entry point
    ├── types.py             generated enums and structs
    └── types.pyi
```

Never edit the generated files — regenerate with the same command when
`bundle.toml` or `api.toml` changes.

For the `pipeline.Decoder` contract the generator produces a base class named
`DECODERPipelineDecoderPlugin`, a `set_decoder_factory` function, and the
`polyplug_init` entry point that the loader calls.

---

## 5. Implement the contract

Create your implementation file (e.g. `decoder.py`) next to `bundle.toml`.
Import the generated base class, subclass it, and call `set_decoder_factory`
at module import time:

```python
# decoder.py

from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_factory,
    polyplug_init,  # re-export so the loader can find it
)


class DecoderImpl(DECODERPipelineDecoderPlugin):
    """The factory receives the HostApi pointer at polyplug_init time."""

    def __init__(self, host_ptr: int) -> None:
        self._host_ptr: int = host_ptr

    def decode(self, input: str) -> str:
        return f"DECODED:{input.replace(',', '|')}"


# Register the factory; the generated polyplug_init constructs the
# implementation with its owning runtime's host pointer.
set_decoder_factory(lambda host_ptr: DecoderImpl(host_ptr))
```

Key points:

- The base class method signature uses plain `str` — the generated ABI dispatch
  layer handles `StringView` ↔ `str` conversion.
- `host_ptr` is the raw `HostApi` pointer for this runtime instance. Store it
  as an instance field if you need to call host contracts or log through the
  host. Do not store it in module globals.
- `set_decoder_factory` must be called at import time (module top level), not
  lazily, because the loader imports the file once and immediately calls
  `polyplug_init`.

---

## 6. Assemble the bundle

There is no compile step. Assemble the bundle directory:

```
my_decoder/
├── manifest.toml          (copied from generated/manifest.toml)
├── decoder.py             your implementation
├── generated/             (or vendor the generated files alongside decoder.py)
│   └── guest/
│       ├── contracts.py
│       ├── init.py
│       └── types.py
└── site-packages/         vendored Python packages (polyplug_guest, polyplug_abi, …)
    ├── polyplug_guest/
    └── polyplug_abi/
```

The Python loader automatically prepends the bundle directory and
`<bundle_dir>/site-packages/` to `sys.path`, so vendored SDK packages are found
without any host-side configuration. `build_all.sh` in the examples directory
handles this assembly automatically.

---

## 7. Validate the bundle

```bash
polyplugc validate --bundle-dir my_decoder/
```

This checks `manifest.toml` for correctness, verifies the declared contracts
match `api.toml`, and confirms the required files are present.

---

## 8. (Optional) Sign the bundle

```bash
polyplugc sign --bundle-dir my_decoder/ --key signing_key.pem
```

Signing adds a signature that the host can verify when `RuntimeConfig` is
configured with trusted public keys. See `docs/TRUST_MODEL.md` for details.

---

## Known limitations

**Python guests do not support hot-reload.** Calling `reload_bundle` on a Python
bundle returns `RuntimeError::HotReloadDisabled`. This is a CPython constraint —
the interpreter initializes once per process and Python modules cannot be
cleanly unloaded. Native (cdylib), Lua, and JavaScript (QuickJS) bundles do
support hot-reload.

**Multiple runtimes share the CPython interpreter.** Python plugins from
different `Runtime` instances in the same process can see each other's modules
and state. For full isolation, use separate processes.

---

## Full examples

See `examples/guests/python/` for five complete Python guest plugins built
against the shared `examples/api.toml`.
