# Lua — Guest (plugin)

This guide walks through writing a polyplug plugin in Lua (LuaJIT): installing the guest SDK, defining a bundle, generating glue code, implementing the contract, assembling the bundle, and validating it.

See [Lua — overview](lua.md) for installation. See [Lua — Host](lua-host.md) to embed polyplug in a Lua app.

---

## Step 1 — Install the guest SDK

```bash
luarocks install polyplug-guest    # polyplug_guest helpers
luarocks install polyplug-abi      # ABI type mirror (polyplug_abi)
```

Also install `polyplugc`:

```bash
cargo install polyplugc                             # from source
curl -fsSL https://polyplug.github.io/install.sh | bash   # prebuilt binary
```

---

## Step 2 — Write `bundle.toml`

`bundle.toml` is the plugin manifest. It declares what this bundle is, which language it uses, which file the loader loads, and which contracts it implements.

```toml
[bundle]
name    = "my_decoder"
version = "1.0.0"
api     = "../api.toml"    # path to api.toml, relative to this file
loader  = "lua"
file    = "decoder.lua"    # entry-point .lua file (flat filename, no path)

[[plugin]]
name       = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`loader = "lua"` tells the runtime to use the LuaJIT loader. `implements` references each contract as `name@major_version`.

---

## Step 3 — Generate guest glue code

```bash
polyplugc generate --bundle bundle.toml --lang lua --out generated
```

This writes into `generated/`:

```
generated/
├── manifest.toml               ship-ready manifest (computed ID, never edit by hand)
└── guest/
    ├── contracts.lua           factory-setter, dispatch table, polyplug_init entry point
    ├── host_contracts.lua      host-contract call helpers (if api.toml defines host_contract)
    └── types.lua               generated enum mirrors
```

Re-run whenever `bundle.toml` or `api.toml` changes. Never edit generated files — regenerate instead.

---

## Step 4 — Implement the contract

Create your entry-point file (the `file =` value in `bundle.toml`). You `require` the generated `contracts` module and register a factory function that returns a per-instance implementation table.

```lua
-- decoder.lua
local polyplug_abi = require('polyplug_abi')
local contracts    = require('generated.guest.contracts')

local function new_decoder(host)
    local self = {}

    function self:decode(input)
        -- to_str converts a StringView cdata to a Lua string (no copy ownership)
        local s = polyplug_abi.to_str(input)
        return 'DECODED:' .. s:gsub(',', '|')
    end

    return self
end

-- Register the factory; the loader calls it once per host-created instance.
contracts.set_decoder_factory(new_decoder)

return contracts
```

Key points:

- The factory receives `host` — a `HostApi*` integer threaded by the loader. Store it on `self` if you need to call host services (logging, host contracts) from the instance.
- Return a Lua string from contract methods; the generated dispatch wraps it in a `StringView` via the per-call arena allocator automatically.
- `polyplug_abi.to_str(sv)` converts an incoming `StringView` cdata to a Lua string for the duration of the call.
- Never store the `host` pointer in a module-level global — each instance carries its own.

### Calling back into the host (logging)

```lua
local polyplug = require('polyplug_guest')

local function new_decoder(host)
    local self = {}

    function self:decode(input)
        -- Send an Info-level log to the host's log funnel
        polyplug.log(host, polyplug.LogLevel.Info, 'guest.my_decoder', 'decode called')
        local s = polyplug_abi.to_str(input)
        return 'DECODED:' .. s:gsub(',', '|')
    end

    return self
end
```

`polyplug.log(host, level, scope, message)` routes through `HostApi.log`; a nil host pointer is a safe no-op.

### Calling a host contract

If the API defines a `[[host_contract]]`, the generated `host_contracts.lua` provides a caller:

```lua
local host_contracts = require('generated.guest.host_contracts')

local function new_reporter(host)
    local self = {}

    function self:report(input)
        local s = polyplug_abi.to_str(input)
        -- Call back into the host's logger contract
        host_contracts.HostLoggerContract_log(host, 'reporting: ' .. s)
        return 'Report: ' .. s
    end

    return self
end
```

### Calling a peer contract (guest→guest)

Generated `peer_callers.lua` (present when the transformer contract uses peer calls) provides direct zero-overhead dispatch to other loaded plugins. See `examples/guests/lua/transformer/generated/guest/peer_callers.lua`.

---

## Step 5 — Assemble the bundle

A bundle is a directory containing `manifest.toml` plus all required files. For a Lua plugin:

```
dist/my_decoder/
├── manifest.toml          (from generated/manifest.toml)
├── decoder.lua            (your implementation)
└── generated/             (generated glue modules)
    └── guest/
        ├── contracts.lua
        ├── host_contracts.lua
        └── types.lua
```

```bash
mkdir -p dist/my_decoder/generated/guest
cp generated/manifest.toml    dist/my_decoder/
cp decoder.lua                dist/my_decoder/
cp generated/guest/*.lua      dist/my_decoder/generated/guest/
```

The Lua loader prepends the bundle directory to `package.path` and `package.cpath` at load time, so `require('generated.guest.contracts')` resolves correctly without any path configuration from your plugin.

There is **no build step** — ship the `.lua` files as-is.

---

## Step 6 — Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_decoder
```

Expected output:

```
OK: dist/my_decoder
```

This runs the same checks the loader applies at runtime: the computed `id` matches the `name`, the entry-point file exists, the version is well-formed, and the declared contracts are consistent.

---

## Step 7 — (Optional) Sign the bundle

If the host enforces `SignaturePolicy::Required`, sign the bundle before distributing:

```bash
polyplugc keygen --out keys/
polyplugc sign   --bundle-dir dist/my_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/my_decoder
```

`sign` writes `dist/my_decoder/bundle.sig` — a detached Ed25519 signature over a canonical digest of every file in the bundle. Keep `signing.key` secret; the public key travels inside `bundle.sig` for TOFU tamper-detection. Full details: [`TRUST_MODEL.md`](../TRUST_MODEL.md).

---

## Full example

Five working Lua plugins are under `examples/guests/lua/`:

| Plugin | Contract | Entry point |
|---|---|---|
| `decoder` | `pipeline.Decoder` | `decoder.lua` |
| `transformer` | `data.Transformer` | `transformer.lua` |
| `encoder` | `pipeline.Encoder` | `encoder.lua` |
| `reporter` | `data.Reporter` | `reporter.lua` |
| `validator` | `pipeline.Validator` | `validator.lua` |

The decoder (`examples/guests/lua/decoder/decoder.lua`) demonstrates one-time instance logging on first dispatch. The transformer (`transformer.lua`) shows stateless per-instance factories. The reporter calls back into the host via the `polyplug_guest.log` helper.

---

## Bundle layout reference

```
dist/<bundle>/
├── manifest.toml               generated; contains precomputed bundle ID
├── <file>.lua                  your entry point (matches bundle.toml file =)
└── generated/
    └── guest/
        ├── contracts.lua       factory-setter + dispatch + polyplug_init
        ├── host_contracts.lua  host-contract callers (if defined)
        ├── peer_callers.lua    peer contract callers (if used)
        └── types.lua           enum mirrors
```

Any extra `.lua` modules your plugin requires should also ship inside the bundle directory. The loader adds the bundle root to `package.path`/`package.cpath`, so `require('my_module')` resolves to `<bundle>/my_module.lua`.

---

## Available types

| `api.toml` type | Lua representation |
|---|---|
| `StringView` | `StringView` cdata; use `polyplug_abi.to_str(sv)` to read, return a Lua string |
| `bool` | Lua boolean |
| `i32` / `u32` / `i64` / `u64` | Lua number (LuaJIT FFI cdata for 64-bit) |
| `f32` / `f64` | Lua number |
| `void` | return `nil` |
| user enum | `number` (cast from `u32` cdata via `tonumber(v)`) |
