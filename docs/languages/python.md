# Python — polyplug

Python works as both a host and a guest. As a host it embeds the runtime
through `ctypes` and loads bundles of any language; as a guest it ships a plain
`.py` file any polyplug host can load — no build step, no Rust toolchain. For
measured overhead, see [Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
uv tool install polyplugc      # recommended; or: pipx install polyplugc / pip install polyplugc
cargo install polyplugc        # requires a Rust toolchain
```

**Host runtime** — install the runtime plus a loader per guest language:

```bash
pip install polyplug polyplug-abi
pip install polyplug-loaders-native    # native (.so / .dylib / .dll) bundles
pip install polyplug-loaders-python    # Python bundles
pip install polyplug-loaders-lua       # Lua bundles
pip install polyplug-loaders-js        # JavaScript (QuickJS) bundles
pip install polyplug-loaders-dotnet    # .NET / C# bundles
```

**Guest SDK** — install for a plugin written in Python:

```bash
pip install polyplug-guest polyplug-abi
```

## Guides

- **[Python — Host (app)](python-host.md)** — embed the runtime, register
  loaders, load plugins of any language, call contracts.
- **[Python — Guest (plugin)](python-guest.md)** — write a Python plugin,
  generate glue, implement and ship a `.py` file.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/python/` (`main.py`) — registers all five loaders and runs
  the full five-stage pipeline.
- Guests: `examples/guests/python/` — five plugins (`decoder`, `transformer`,
  `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/python/generated/` (host callers) and
`examples/guests/python/<plugin>/generated/` (guest glue).
