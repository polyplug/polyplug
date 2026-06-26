# polyplug Lua SDK

Build polyplug hosts and plugins in Lua. The host side wraps the native runtime
through LuaJIT FFI; guest plugins run in an embedded LuaJIT VM. Strings are
native Lua strings (UTF-8). Requires LuaJIT 2.1+ (standard Lua 5.x is not
supported).

## Install

```bash
# Host SDK + a loader per guest language you support
luarocks install polyplug                # core host SDK (polyplug, polyplug_abi)
luarocks install polyplug-loader-native  # native (.so / .dylib / .dll) bundles
# polyplug-loader-{lua,js,python,dotnet} as needed

# Plugin author
luarocks install polyplug-guest          # guest helpers (polyplug_guest)
luarocks install polyplug-abi            # ABI type mirror (polyplug_abi)
```

Install the CLI to generate bindings:

```bash
cargo install polyplugc
```

## Generate bindings

```bash
polyplugc generate --bundle bundle.toml --lang lua --out ./generated
```

## Host application

```lua
local polyplug = require("polyplug")

local runtime = polyplug.Runtime.new()
runtime:load_bundle("./plugins/my_plugin")

local decoder = PipelineDecoder.create(runtime)
if decoder then
    local result = decoder:decode(input)
end
```

## Plugin author

Provide an author factory `factory(host) -> impl` whose returned object's methods
are the contract functions, and register it with the generated
`set_<contract>_factory`. State on the returned object is per-instance; the host
pointer is threaded in, never stored in a global:

```lua
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function new_decoder(host)
    local self = {}
    function self:decode(input)
        return polyplug.alloc_string('DECODED:' .. polyplug_abi.to_str(input))
    end
    return self
end

contracts.set_decoder_factory(new_decoder)
return contracts
```

## Learn more

- [Lua — Host guide][host] — embed the runtime, hot-reload, custom logger, signing & key pinning
- [Lua — Guest guide][guest] — generate → implement → bundle
- [Lua overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/lua.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/lua-host.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/lua-guest.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
