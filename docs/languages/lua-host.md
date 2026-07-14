# Lua — Host (app)

Embed the polyplug runtime in a Lua (LuaJIT) application, load plugins written in
any supported language, and call their contracts through generated typed callers.

See also: [Lua overview](lua.md) · [Lua — Guest (plugin)](lua-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI, the core host SDK, and a loader per guest language:

```bash
cargo install polyplugc

luarocks install polyplug                # core host SDK (polyplug, polyplug_abi)
luarocks install polyplug-loader-native  # always needed for native bundles
# add a loader per guest language you want to support:
luarocks install polyplug-loader-lua
luarocks install polyplug-loader-js
luarocks install polyplug-loader-python
luarocks install polyplug-loader-dotnet
```

A Lua host can load guests written in any supported language — register the
matching loader when you build the runtime.

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang lua --out generated
```

This writes `generated/host/` with `callers.lua` (typed caller constructors),
`contracts.lua` (contract-ID constants), `interface_factories.lua` (host-contract
factories), and `types.lua` (generated enums and struct mirrors). Never edit these
files. For the emitted symbol names, see [Generated names](../generated-names.md).

Require `polyplug` and `polyplug_abi` before the generated modules:

```lua
local polyplug = require('polyplug')
local abi      = require('polyplug_abi')
local callers  = require('generated.host.callers')
```

## 3. Build the runtime

Create the runtime, then register one loader per guest language:

```lua
local native_loader = require('polyplug.loaders.native')
local lua_loader    = require('polyplug.loaders.lua')

local rt = polyplug.Runtime.new()

native_loader.register(rt)
lua_loader.register(rt)
```

`Runtime.new` accepts an options table for configuration and callbacks. Wrap
each `register` in `pcall` so a missing optional loader doesn't abort startup:

```lua
local rt = polyplug.Runtime.new({
    config = {
        hot_reload_enabled = true,
        compatibility = 0,   -- 0 = Strict (default), 1 = Relaxed, 2 = Yolo
    },
    log_max_level = polyplug.LogLevel.Info,   -- Error / Warn / Info / Debug / Trace
})

local loaders = {
    function() native_loader.register(rt) end,
    function() lua_loader.register(rt) end,
    function() require('polyplug.loaders.js').register(rt) end,
    function() require('polyplug.loaders.python').register(rt) end,
    function() require('polyplug.loaders.dotnet').register(rt) end,
}
for _, register in ipairs(loaders) do
    pcall(register)
end
```

The full multi-loader host is `examples/hosts/lua/host.lua`.

### Hot-reload callback (optional)

Pass `on_reload` to observe reload phases. Hot-reload applies to native, Lua, and
JS bundles — see [Hot Reload](../HOT_RELOAD_DESIGN.md).

```lua
local rt = polyplug.Runtime.new({
    on_reload = function(phase)
        io.stderr:write('phase: ' .. tostring(phase.phase_type) .. '\n')
    end,
})
```

### Custom logger (optional)

A custom `log` function receives runtime diagnostics and guest log calls.
Requires `polyplug-loader-lua`. Output goes to stderr:

```lua
local rt = polyplug.Runtime.new({
    log = function(level, scope, message)
        io.stderr:write(string.format('[%d][%s] %s\n', level, scope, message))
    end,
    log_max_level = polyplug.LogLevel.Info,
})
```

### Signature policy (optional)

```lua
local rt = polyplug.Runtime.new({
    signature_policy = polyplug.SignaturePolicy.Required,  -- Off / WarnOnly / Required
    trusted_keys = { ... },   -- optional 32-byte Ed25519 public keys (key-pinning)
})
```

`Required` rejects unsigned or tampered bundles. See the
[Trust Model](../TRUST_MODEL.md).

### Inspect loaded bundles and contracts

Lua returns copied table snapshots from `rt:bundle_descriptors()` and
`rt:registered_contract_descriptors()`:

```lua
for _, bundle in ipairs(rt:bundle_descriptors()) do
    print(bundle.name, tonumber(bundle.source_kind))
end
local contracts = rt:registered_contract_descriptors()
```

`source_kind` is the payload-free SDK projection of Rust `BundleOrigin`:
`Internal`, `Path`, `Code`, or `Bytes`. Programmatically supplied source and
byte payloads are never exposed. The tables describe bundles currently loaded
and contracts currently registered; they are not an application per-plugin
enabled state and do not invoke a contract `initialize` operation. Keep that
policy in the application; see
[Runtime lifecycle is not application enablement](../ARCHITECTURE.md#runtime-lifecycle-is-not-application-enablement).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), register it through the generated factory before loading bundles.
Requires `polyplug-loader-lua`:

```lua
local interface_factories = require('generated.host.interface_factories')

local ConsoleLogger = {}
ConsoleLogger.__index = ConsoleLogger

function ConsoleLogger.new()
    return setmetatable({}, ConsoleLogger)
end

function ConsoleLogger:log(message)
    print('[plugin] ' .. message)
end

local logger_iface = interface_factories.create_host_logger_interface(
    ConsoleLogger.new, lua_loader.bridge_lib())
rt:register_host_contract(logger_iface)
```

## 5. Load bundles

```lua
-- Load a single bundle directory
rt:load_bundle('/path/to/my_plugin')
```

To scan a directory and load every bundle under it, see the loop in
`examples/hosts/lua/host.lua`.

`load_bundle` dispatches to the loader matching the bundle's `loader` field. A
native, Lua, or JS bundle can be hot-reloaded with `rt:reload_bundle(path)` — see
[Hot Reload](../HOT_RELOAD_DESIGN.md).

## 6. Call a contract

```lua
local host = rt:host()   -- HostApi* pointer for caller construction

local decoder = callers.PipelineDecoderContract_create(rt, host)
if not decoder then
    error('no pipeline.Decoder plugin loaded')
end

local result_sv = decoder:decode('name,value,42')
print(abi.to_str(result_sv))   -- DECODED:name|value|42
```

Each generated `{Ns}{Type}Contract_create` returns `nil` when no implementation
is loaded — guard before calling. A hot-reloaded plugin is picked up
automatically. Caller and method
names come from [Generated names](../generated-names.md).

## 7. Unload a bundle

```lua
rt:unload_bundle(polyplug.bundle_id('my_plugin'))
```

Quiesce in-flight calls before unloading — see [Unload](../UNLOAD_DESIGN.md).

## Full reference

`examples/hosts/lua/host.lua` registers all five loaders with graceful skip, a
host contract, scans a directory, loads every bundle, and runs the full
five-stage pipeline end to end. Generated callers live at
`examples/hosts/lua/generated/`.
