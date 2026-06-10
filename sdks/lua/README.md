# polyplug Lua SDK

Complete Lua support for polyplug plugin runtime.

## Structure

```
sdks/lua/
├── abi/           # ABI type definitions (auto-generated from Rust)
├── host/          # Host runtime library for Lua applications
├── guest/         # Guest library for Lua plugin authors
└── loaders/       # Loader implementations (Lua runtime adapter)
```

## Installation

### Via LuaRocks

```bash
luarocks install polyplug
```

### Manual

Copy `polyplug.lua` and native library to your project.

## Quick Start

### Host Application

```lua
local polyplug = require("polyplug")
local reload_phase = require("polyplug.reload_phase")

local runtime = polyplug.Runtime.new()
runtime:load_bundle("./plugins/my_plugin")

-- Use generated host callers to interact with plugins
local decoder = PipelineDecoder.create(runtime)
if decoder then
    local result = decoder:decode(input)
end
```

### Plugin Author

```lua
local polyplug_guest = require("polyplug_guest")

polyplug_guest.plugin(function(host, ctx)
    host.register_guest_contract(host, descriptor, contractInterface)
end)

DecoderImpl = {}

function DecoderImpl:decode(input)
    return "DECODED:" .. input
end
```

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate Lua bindings from api.toml
polyplugc generate --api api.toml --lang lua --out ./generated

# Generate Lua bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang lua --out ./src/generated
```

## Bundle layout

Assemble the bundle directory yourself — the entry file plus any required modules:

```
dist/my-plugin/
├── manifest.toml          # emitted by `generate` (carries the precomputed bundle_id)
├── init.lua               # the entry module (runtime = "lua")
└── guest/                 # generated helper modules required by init.lua
```

The Lua loader adds the bundle dir to `package.path`, so `require("guest.contracts")`
and other in-bundle modules resolve. Validate before shipping:

```bash
polyplugc validate --bundle-dir dist/my-plugin/
```

## Components

### ABI (`abi/`)

Auto-generated from Rust ABI definitions using LuaJIT FFI:
- `StringView` — UTF-8 string view (ffi.cdata)
- `Buffer` — Byte buffer with host allocator
- `AbiError` — Error code and message
- `GuestContractHandle` — Opaque plugin reference
- `GuestContractInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

LuaJIT FFI wrappers over the polyplug C ABI:
- `Runtime` — Main runtime class (metatable)
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- Near-native performance via JIT inlining

### Guest Library (`guest/`)

Bootstrap layer for Lua plugins:
- `polyplug_guest.plugin()` — Marks plugin entry point
- `HostApi` — Contract registration
- `BundleInitContext` — Bundle metadata
- Error boundary — Plugin errors don't take down host

### Loaders (`loaders/`)

Lua runtime adapter:
- `register_lua_loader()` — Register Lua loader with runtime
- LuaJIT embedding via `mlua` crate
- Automatic GIL-like state management

## Hot-Reload

To enable hot-reload, pass `config.hot_reload_enabled = true` per-instance to
`Runtime.new(opts)` (no module-level state — each runtime owns its options):

```lua
local polyplug = require("polyplug")

-- Enable hot-reload (per-instance configuration)
local runtime = polyplug.Runtime.new({
    config = { hot_reload_enabled = true },
})
```

**Key points:**
- `hot_reload_enabled` defaults to `false` — must be explicitly enabled
- Host must track and destroy instances on `TYPE_PREPARING` notification
- **Known limitation:** `opts.on_reload` raises an error on LuaJIT — the ABI
  passes `ReloadPhase` to the callback **by value**, and LuaJIT FFI cannot
  create callbacks with struct-by-value parameters. Lua hosts currently cannot
  receive reload-phase notifications.
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

## Performance Notes

- **Backend**: LuaJIT FFI (required)
- **Hot path**: JIT-compiled FFI calls (~800M ops/sec)
- **Memory**: ffi.metatype for allocation sinking
- **Strings**: Native Lua strings (UTF-8)

## Requirements

- LuaJIT 2.1+ (standard Lua 5.x not supported for performance reasons)
- `mlua` crate for Rust embedding

## See Also

- `../csharp/` — C# SDK
- `../python/` — Python SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
