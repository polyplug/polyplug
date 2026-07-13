# Python — Guest (plugin)

Write a polyplug plugin in Python: generate the ABI glue and ship a plain `.py`
file any polyplug host can load. No build step, no Rust toolchain. New to
polyplug? Start with the [Quick Start](../QUICKSTART.md).

See also: [Python overview](python.md) · [Python — Host (app)](python-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and the guest SDK packages:

```bash
uv tool install polyplugc        # or: pipx install polyplugc / pip install polyplugc
pip install polyplug-guest polyplug-abi
```

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the entry `.py` file, and
which contracts this bundle implements. The `api` field points at the shared
`api.toml` contract (see `examples/api.toml`).

```toml
# bundle.toml
[bundle]
name = "python_decoder"
version = "1.0.0"
api = "../../../api.toml"   # path to api.toml, relative to this file
loader = "python"
file = "decoder.py"

[[plugin]]
name = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`loader` must be exactly `"python"`. `implements` names each contract as
`<namespace>.<Name>@<major_version>`. Add one `[[plugin]]` section per plugin in
the bundle. `polyplug_init` returns one registration for every declared contract.
The runtime validates and publishes that complete set atomically; a rejected
registration never exposes a partial bundle. Logical unload releases the loader's
Python objects and purges the bundle's isolated module cache. To declare a runtime
dependency on another contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang python --out generated
```

This writes the contract base class(es), factory setter, `polyplug_init`,
generated types, and a `manifest.toml` under `generated/`. Re-run
whenever `bundle.toml` or `api.toml` changes; never edit generated files. For the
emitted symbol names, see [Generated names](../generated-names.md).

## 4. Implement the plugin

Create your `.py` file next to `bundle.toml`. Import the generated base class and
factory setter, subclass the base class, and register the factory at module
import time. Full source: `examples/guests/python/decoder`.

```python
# decoder.py
from generated.guest.contracts import (
    DECODERPipelineDecoderPlugin,
    set_decoder_factory,
    polyplug_init,  # re-export so the loader can find it
)


class DecoderImpl(DECODERPipelineDecoderPlugin):
    def __init__(self, host_ptr: int) -> None:
        self._host_ptr: int = host_ptr

    def decode(self, input: str) -> str:
        return f"DECODED:{input.replace(',', '|')}"


set_decoder_factory(DecoderImpl)
```

- The loader calls the factory once per instance with the runtime's `HostApi`
  pointer; store it on the instance ([instance payload](../glossary.md)), never
  in a module global.
- Method signatures use plain `str`.
- Call `set_decoder_factory` at module top level. Base-class and setter names come
  from [Generated names](../generated-names.md).

To call a host contract (such as a logging service) from your plugin, use the
caller in the generated `host_contracts.py`. See `examples/guests/python/reporter`
for the full pattern.

## 5. Assemble the bundle

There is no build step. Place the entry `.py`, the generated glue, and the
generated `manifest.toml` together, and vendor the SDK packages under
`site-packages/`:

```text
dist/python_decoder/
├── manifest.toml          # from generated/manifest.toml
├── decoder.py             # your implementation
├── generated/guest/       # generated glue (contracts.py, init.py, types.py)
└── site-packages/         # vendored polyplug_guest, polyplug_abi
```

Vendored SDK packages under `site-packages/` load automatically — no path setup.
`examples/build_all.sh` performs this assembly.

## 6. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/python_decoder
```

This checks the manifest is consistent, the declared contracts match `api.toml`,
and the entry file is present.

## 7. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/python_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/python_decoder
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/python/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/python/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/python/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/python/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/python/validator/` | `pipeline.Validator` |
