# Python — polyplug

Python is a first-class citizen in polyplug — both as a **host** (an application
that embeds the runtime, loads bundles of any language, and calls their contracts)
and as a **guest** (a plugin written in Python that any polyplug host can load).
No Rust toolchain is required on either path; the prebuilt binaries ship inside
the Python packages.

---

## Install the `polyplugc` CLI

`polyplugc` generates typed glue from a `.toml` contract definition.

```bash
uv   tool install polyplugc          # recommended
pipx install polyplugc
pip  install polyplugc
cargo install polyplugc              # requires Rust toolchain
```

Or download a prebuilt binary from the
[GitHub Releases](https://github.com/polyplug/polyplug/releases) page.

---

## Host SDK

Install the runtime library and whatever loaders your host will support:

```bash
# Core (required)
pip install polyplug polyplug-abi

# Loader extras — install only what you need
pip install polyplug-loaders-native    # load native cdylib plugins
pip install polyplug-loaders-python    # load Python plugins
pip install polyplug-loaders-lua       # load Lua (LuaJIT) plugins
pip install polyplug-loaders-js        # load JavaScript (QuickJS) plugins
pip install polyplug-loaders-dotnet    # load .NET/C# plugins

# Or pull everything at once:
pip install "polyplug[all]"
```

---

## Guest SDK

```bash
pip install polyplug-guest polyplug-abi
```

---

## Guides

- **[Python — Host (app)](python-host.md)** — embed the runtime, register loaders,
  load bundles of any language, and call their contracts from Python.
- **[Python — Guest (plugin)](python-guest.md)** — write a polyplug plugin in Python:
  define a contract, generate glue, implement and ship a `.py` file.

---

## Examples

| Role | Path |
|---|---|
| Host | `examples/hosts/python/` — entry point `main.py` |
| Guests | `examples/guests/python/` — five plugins (decoder, encoder, transformer, reporter, validator) |
