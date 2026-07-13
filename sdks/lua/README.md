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
local callers = require("generated.host.callers")

local runtime = polyplug.Runtime.new()
runtime:load_bundle("./plugins/my_plugin")

local decoder = callers.PipelineDecoder_create(runtime, runtime:host())
if decoder then
    local result = decoder:decode(input)
end
```

Each generated caller creates one guest instance and retains it across method calls.
Call `caller:reset()` to replace that instance or `caller:destroy()` during teardown.

## Internal plugins

The default command emits external plugin bindings. Generate the internal
profile explicitly when the application supplies Lua implementation factories:

```bash
polyplugc generate --bundle bundle.toml --internal --lang lua --out ./generated
```

The bundle-identity-namespaced `guest/internal.lua` module exposes
`providers(values)` and `register(runtime, providers)`. Each provider is a
factory that returns a fresh implementation table for a guest instance:

```lua
local internal = dofile("generated/internal/<bundle>-<bundle-id-hex>/guest/internal.lua")

local providers = internal.providers({
    platform_plugin_platform_plugin = make_platform_plugin,
})
local registration = internal.register(runtime, providers)
local bundle_id = registration.bundle_id
```

The registrar consumes provider input on the attempt, registers every generated
guest provider binding, validates the exact manifest set, and atomically
publishes it. It returns named generated host caller bindings built from the
committed handles; their call usage is identical to callers from external plugins.
Before `runtime:unload_bundle(bundle_id)`, the application must quiesce every
caller and destroy all guest instances for the bundle. Every committed internal
bundle is marked privately in `Runtime`; while stateful instances remain,
`unload_bundle` returns `InternalPluginInUse` and leaves the bundle live. After
destroying or resetting callers and destroying those instances, retry the unload
(subject to normal dependency checks). This refusal is a guard, not a replacement
for host quiescence. External unload paths may warn and proceed with live instances,
so they cannot use the internal guard. A successful unload invalidates those callers
and releases the generated provider binding state; callers must not be used afterward.

For an internal Lua plugin, the native `host_bridge` owns `Resident` and
`ContractBridge` records. Those records retain provider factories, dispatchers, and
per-instance Lua values through Lua registry references, and release those references
when the resident is released during lifecycle cleanup.

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
