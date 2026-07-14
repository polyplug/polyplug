# Lua — polyplug

Lua works as both a host and a guest. A Lua app embeds the runtime and calls
plugins through generated typed callers; a Lua plugin is a
plain `.lua` file shipped as-is — no build step. For measured overhead, see
[Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
cargo install polyplugc
```

**Host SDK + loaders** — install the core host SDK plus one loader per guest
language:

```bash
luarocks install polyplug                # core host SDK (polyplug, polyplug_abi)
luarocks install polyplug-loader-native  # native (.so / .dylib / .dll) bundles
luarocks install polyplug-loader-lua     # Lua (LuaJIT) bundles
luarocks install polyplug-loader-js      # JavaScript (QuickJS) bundles
luarocks install polyplug-loader-python  # Python bundles
luarocks install polyplug-loader-dotnet  # .NET / C# bundles
```

**Guest SDK** — for plugin authors:

```bash
luarocks install polyplug-guest          # guest helpers (polyplug_guest)
luarocks install polyplug-abi            # ABI type mirror (polyplug_abi)
```

## Guides

- **[Lua — Host (app)](lua-host.md)** — embed the runtime, load plugins of any
  language, call contracts.
- **[Lua — Guest (plugin)](lua-guest.md)** — write a Lua plugin, generate glue,
  implement a contract, assemble and validate the bundle.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/lua/` (`host.lua`) — registers all five loaders and runs
  the full five-stage pipeline.
- Guests: `examples/guests/lua/` — five `.lua` plugins (`decoder`, `transformer`,
  `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/lua/generated/` (host callers) and
`examples/guests/lua/<plugin>/generated/` (guest glue).

## Internal plugin profile

External plugins use the standard bundle command. An application can instead
generate one internal profile with
`polyplugc generate --bundle bundle.toml --internal --lang lua --out ./generated`.
It supplies ordinary Lua factories to generated guest provider bindings and
receives generated host caller bindings from the committed handles; registration,
calls, and unload then follow the same pipeline as an external plugin.

## Shared generated declarations

Lua keeps the default unified output. A split project uses `require` module
names such as `common.domain` and `common.guest_contracts`; see the
[split-output guide](../CODE_GENERATION.md#tested-specifier-forms-for-every-maintained-language)
for the exact emit and ImportOnly commands.
