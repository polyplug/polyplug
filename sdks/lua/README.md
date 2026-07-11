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

## In-process guest implementations

Generated `host/callers.lua` exposes `in_process_bundle(spec, lua_bridge_lib)` for
implementations that run in the host LuaJIT VM. Supply one table or factory per
generated contract, then register the complete bundle synchronously. Factories
receive the `HostApi*` and return a fresh implementation table for each caller
instance. Obtain `lua_bridge_lib` from the Lua loader module so its scalar native
trampolines can expand the canonical adapter-context ABI.

```lua
local callers = require("generated.host.callers")
local lua_loader = require("polyplug.loaders.lua")

local bundle = callers.in_process_bundle({
    name = "my-lua-services",
    version = { major = 1, minor = 0, patch = 0 },
    implementations = {
        ["pipeline.decoder"] = function(host)
            return {
                decode = function(self, input)
                    return "decoded:" .. input
                end,
            }
        end,
    },
}, lua_loader.bridge_lib())

local bundle_id = runtime:register_in_process_bundle(bundle)
-- Unload only after generated callers have released their instances.
runtime:unload_bundle(bundle_id)
```

The Runtime becomes the sole owner of the generated resident—typed factories,
Lua callback cdata, interfaces, backing registration tables, and live
implementations—only after the complete registration succeeds. Successful
registration consumes the bundle resident exactly once; a rejected registration
leaves it available for retry. The Runtime releases the resident only after
successful logical unload; a failed unload leaves it intact for callers to drain
and retry. Lua errors are contained at callback boundaries and returned as ABI
errors.

The generated adapter integration test covers atomic registration, per-instance
state, two-Runtime isolation, resident lifetime, failed unload retention, and
unload/re-registration:

```sh
cargo build -p polyplug -p polyplug_lua -p polyplugc
POLYPLUG_LIB=$PWD/target/debug/libpolyplug.so \
POLYPLUG_LUA_LIB=$PWD/target/debug/libpolyplug_lua.so \
POLYPLUGC_BIN=$PWD/target/debug/polyplugc \
luajit sdks/lua/host/tests/test_in_process_runtime.lua
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
