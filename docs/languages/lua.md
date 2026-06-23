# Lua — polyplug

Lua (LuaJIT) is a first-class host **and** guest in polyplug. A Lua app can embed the runtime, register loaders for any language, scan plugin directories, and dispatch through generated typed callers — all from LuaJIT via FFI. A Lua plugin is a plain `.lua` file: no build step, shipped as-is alongside generated glue modules.

---

## Install

### `polyplugc` CLI

```bash
cargo install polyplugc                             # from source (needs Rust toolchain)
curl -fsSL https://polyplug.github.io/install.sh | bash   # prebuilt binary
```

Or grab a binary from the [GitHub Releases](https://github.com/polyplug/polyplug/releases) page. Lua has no language-registry CLI package.

### Host SDK + loaders (LuaRocks)

```bash
luarocks install polyplug          # core host SDK (polyplug, polyplug_abi)
luarocks install polyplug-loader-native    # native (.so/.dylib/.dll) bundles + hot-reload
luarocks install polyplug-loader-lua       # Lua (LuaJIT) bundles + hot-reload
luarocks install polyplug-loader-js        # JavaScript (QuickJS) bundles + hot-reload
luarocks install polyplug-loader-python    # Python bundles
luarocks install polyplug-loader-dotnet    # .NET/C# bundles
```

Install only the loaders you need. `polyplug-loader-native` and `polyplug-loader-lua` are the most common pair for a Lua host.

### Guest SDK (LuaRocks)

```bash
luarocks install polyplug-guest    # polyplug_guest helpers for plugin authors
luarocks install polyplug-abi      # ABI type mirror (polyplug_abi)
```

---

## Guides

- **[Lua — Host (app)](lua-host.md)** — embed polyplug in a Lua application; generate typed callers, load bundles, dispatch contracts.
- **[Lua — Guest (plugin)](lua-guest.md)** — write a Lua plugin; generate glue, implement a contract, assemble and validate the bundle.

---

## Examples

Working reference implementations are checked in under `examples/`:

- `examples/hosts/lua/` — full pipeline host; loads native, Lua, JS, Python, and .NET bundles; drives the five-stage pipeline through generated callers; installs a custom runtime logger.
- `examples/guests/lua/` — five Lua plugins (`decoder`, `transformer`, `encoder`, `reporter`, `validator`) implementing the pipeline API.

Run them after building all plugins with `examples/build_all.sh`:

```bash
POLYPLUG_PLUGIN_PATH=examples/plugins luajit examples/hosts/lua/host.lua
```
