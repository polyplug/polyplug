# Lua — Guest (plugin)

Write a polyplug plugin in Lua (LuaJIT): generate the ABI glue, implement a
contract, and assemble a bundle any polyplug host can load. New to polyplug? Start
with the [Quick Start](../QUICKSTART.md).

See also: [Lua overview](lua.md) · [Lua — Host (app)](lua-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and the guest SDK:

```bash
cargo install polyplugc

luarocks install polyplug-guest    # guest helpers (polyplug_guest)
luarocks install polyplug-abi      # ABI type mirror (polyplug_abi)
```

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the entry-point `.lua`
file, and which contracts this bundle implements. The `api` field points at the
shared `api.toml` contract (see `examples/api.toml`).

```toml
# bundle.toml
[bundle]
name    = "my_decoder"
version = "1.0.0"
api     = "../api.toml"   # path to api.toml, relative to this file
loader  = "lua"
file    = "decoder.lua"   # entry-point .lua file (flat filename, no path)

[[plugin]]
name       = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`implements` names each contract as `<namespace>.<Name>@<major_version>`. Add one
`[[plugin]]` section per plugin in the bundle. To declare a runtime dependency on
another contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang lua --out generated
```

This writes `contracts.lua` (factory setter, dispatch table, `polyplug_init`
entry point), `host_contracts.lua` (host-contract callers, if `api.toml` defines
a host contract), and `types.lua` (generated enum mirrors) under `generated/guest/`,
plus a `manifest.toml` under `generated/`. Re-run whenever `bundle.toml`
or `api.toml` changes; never edit generated files. For the emitted symbol names,
see [Generated names](../generated-names.md).

## 4. Implement the plugin

Create the entry-point file named in `bundle.toml`. Require the generated
`contracts` module and register a factory that returns a per-instance table — a
Lua guest has no generated contract type. Full source:
`examples/guests/lua/decoder`.

```lua
-- decoder.lua
local polyplug     = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts    = require('generated.guest.contracts')

local function new_decoder(host)
    local self = {}

    function self:decode(input)
        polyplug.log(host, polyplug.LogLevel.Info, 'guest.my_decoder', 'decode called')
        local s = polyplug_abi.to_str(input):gsub(',', '|')
        return 'DECODED:' .. s
    end

    return self
end

contracts.set_decoder_factory(new_decoder)

return contracts
```

- Store `host` on `self`; call host services through it. Never use a module
  global.
- Return a Lua string from a contract method.
- `polyplug_abi.to_str(sv)` reads a `StringView` as a Lua string.
- `polyplug.log(host, level, scope, message)` routes through `HostApi.log`; a nil
  host pointer is a safe no-op.

To call a host contract from your plugin, resolve the caller in the generated
`host_contracts.lua` — `HostLoggerContract.from_host(host, 0)` returns an object
whose methods (`:log(message)`) dispatch into the host service. Factory setter,
caller, and method names come from [Generated names](../generated-names.md).

## 5. Assemble the bundle

Lua is interpreted — there is no build step. Copy the entry-point file and the
generated glue next to the generated `manifest.toml`:

```text
dist/my_decoder/
├── manifest.toml          # from generated/manifest.toml
├── decoder.lua            # your entry point
└── generated/
    └── guest/
        ├── contracts.lua
        ├── host_contracts.lua
        └── types.lua
```

```bash
mkdir -p dist/my_decoder/generated/guest
cp generated/manifest.toml dist/my_decoder/
cp decoder.lua             dist/my_decoder/
cp generated/guest/*.lua   dist/my_decoder/generated/guest/
```

Extra `.lua` modules shipped in the bundle are found automatically — no path
setup.

## 6. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_decoder
```

This checks the manifest is consistent, the entry-point file is present, and the
bundle conforms to the ABI rules — the same checks the loader applies at runtime.

## 7. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/my_decoder
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/lua/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/lua/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/lua/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/lua/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/lua/validator/` | `pipeline.Validator` |
