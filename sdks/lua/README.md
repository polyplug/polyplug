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

Each bundle runs in its own Lua VM (one VM per runtime per bundle). The generated
`guest/contracts` module owns `polyplug_init` and registration; you provide an
author factory `factory(host) -> impl` whose returned object's methods are the
contract functions, and register it with the generated `set_<contract>_factory`.

The loader owns per-instance state: it calls your factory once per
`create_instance` (and once at load for the stateless default impl), so any state
you put on the returned object (`self`) is **per-instance** — two live instances
of the same contract never share state.

```lua
local polyplug = require('polyplug_guest')
local polyplug_abi = require('polyplug_abi')
local contracts = require('generated.guest.contracts')

local function new_decoder(host)
    local self = {}
    function self:decode(input)
        local s = polyplug_abi.to_str(input)
        return polyplug.alloc_string('DECODED:' .. s)
    end
    return self
end

contracts.set_decoder_factory(new_decoder)

return contracts
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
├── init.lua               # the entry module (loader = "lua")
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

Helpers for Lua plugins (the entry point is generated; the loader calls it).
The host pointer and per-call arena allocator are threaded explicitly — nothing
is stored in any module/VM global (Rule 12):
- `alloc_string(host_ptr, s)` — allocate in HOST memory (outlives the call); the
  `host_ptr` is the one threaded into the author factory
- `alloc_string_arena(arena_alloc, arena_ptr, s)` — per-call-arena string; both
  the allocator and arena pointer arrive as explicit dispatch arguments
- `log(host_ptr, level, scope, message)` — calls `HostApi.log` directly through
  the threaded host pointer (no `_polyplug_log` bridge)
- `ok()` / `err(code, message)` — `AbiError` constructors
- Error boundary — Plugin errors don't take down host

### Loaders (`loaders/`)

Lua runtime adapter:
- `register_lua_loader()` — Register Lua loader with runtime
- LuaJIT embedding via `mlua` crate
- Automatic GIL-like state management

## Hot-Reload

To enable hot-reload, pass `config.hot_reload_enabled = true` and an
`on_reload` callback per-instance to `Runtime.new(opts)` (no module-level
state — each runtime owns its options and its callback cdata):

```lua
local polyplug = require("polyplug")
local reload_phase = require("polyplug.reload_phase")

-- Enable hot-reload with phase notifications (per-instance configuration).
-- The ABI passes the phase by const pointer; the SDK copies all fields into
-- a plain Lua table before your callback runs, so the table is safe to keep.
local runtime = polyplug.Runtime.new({
    config = { hot_reload_enabled = true },
    on_reload = function(phase)
        if reload_phase.is_preparing(phase) then
            -- Destroy all instances for phase.bundle_id here.
        elseif reload_phase.is_reloaded(phase) then
            print("reloaded: " .. phase.bundle_name)
        elseif reload_phase.is_failed(phase) then
            io.stderr:write("reload failed: " .. phase.reason .. "\n")
        end
    end,
})
```

**Key points:**
- `hot_reload_enabled` defaults to `false` — must be explicitly enabled
- Host must track and destroy instances on `TYPE_PREPARING` notification
- The callback receives a Lua table `{ type, bundle_id, bundle_name, reason }`;
  `bundle_id` is a `uint64_t` cdata (Lua numbers lose precision past 2^53) and
  `reason` is `""` except for `TYPE_FAILED`
- Errors raised inside the callback are caught and logged to stderr — they
  never unwind across the C ABI
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

## Custom Logger

Pass `log` (and optionally `log_max_level`) to `Runtime.new(opts)` to receive
every runtime diagnostic — and guest log lines routed through the
`HostApi.log` funnel — in a Lua function:

```lua
local polyplug = require("polyplug")

local runtime = polyplug.Runtime.new({
    log = function(level, scope, message)
        -- level is a number (polyplug.LogLevel.Error .. .Trace),
        -- scope/message are plain Lua strings (already copied — safe to keep).
        io.stderr:write(string.format("[%d][%s] %s\n", level, scope, message))
    end,
    log_max_level = polyplug.LogLevel.Info, -- default: polyplug.LogLevel.Warn
})
```

**How it works (and why it needs the lua loader cdylib):** the ABI callback
`RuntimeConfig.log` receives its `scope`/`message` `StringView`s **by value**
(deliberate — hot path), and LuaJIT FFI callbacks cannot receive structs by
value. The SDK therefore installs `polyplug_lua_log_trampoline` — a native
trampoline exported by the `polyplug_lua` loader cdylib — as `RuntimeConfig.log`
and carries a `PolyplugLuaLogBridge` (scalar-only LuaJIT callback + user_data)
in `log_user_data`. The trampoline decomposes the views into ptr+len scalars
and forwards them; the SDK converts them to Lua strings before your function
runs.

**Key points:**
- Requires the `polyplug_lua` loader cdylib (set `POLYPLUG_LUA_LIB` or have
  `libpolyplug_lua` on the loader path) and the lua loaders package on
  `package.path` — `Runtime.new` raises a descriptive error otherwise
- Your function runs on **whatever thread the runtime logs from**; do not
  touch thread-affine state and never re-enter the runtime from inside it
- Errors raised inside the callback are caught and logged to stderr — they
  never unwind across the C ABI
- Levels above `log_max_level` are filtered before any formatting work —
  disabled levels cost nothing
- Without `log`, the default sink writes Error/Warn to stderr and drops the
  rest
- The callback cdata and bridge are anchored per Runtime instance and released
  on `destroy()` (no module-level state)
- Measured delivery cost: ~255 ns per delivered log line (trampoline +
  LuaJIT callback + `ffi.string` copies + user function); see
  `crates/polyplug/benches/README.md`

## Bundle Signature Verification

Pass `signature_policy` to `Runtime.new(opts)` to control how the runtime treats
each bundle's `bundle.sig` (defaults to `polyplug.SignaturePolicy.Off` — unsigned
bundles load normally):

```lua
local polyplug = require("polyplug")

local runtime = polyplug.Runtime.new({
    signature_policy = polyplug.SignaturePolicy.Required, -- Off | WarnOnly | Required
})
```

### Key pinning (`trusted_keys`)

By default, signature verification is **Trust-On-First-Use**: the runtime trusts
the key embedded in each `bundle.sig`, so it proves integrity but not
authenticity. Pass `trusted_keys` — a sequence of 32-byte Ed25519 verifying keys
— to switch to **key pinning**: after the normal signature check, the runtime
also requires the bundle's embedded key to be one of the pinned keys, rejecting a
bundle re-signed with any other key:

```lua
local runtime = polyplug.Runtime.new({
    signature_policy = polyplug.SignaturePolicy.Required,
    trusted_keys = {
        my_key_bytes,          -- a 32-byte Lua string, or
        { 1, 2, 3, --[[ … ]] }, -- a sequence of 32 byte values (0-255)
    },
})
```

**Key points:**
- Each key must be exactly 32 bytes (a Lua string of length 32 or a 32-element
  byte table); a wrong length raises an error
- Only effective alongside a non-`Off` `signature_policy`; under `Off` no
  verification runs
- An empty or omitted `trusted_keys` keeps Trust-On-First-Use (the fields stay
  zero)
- The keys are copied into a cdata buffer that lives only across the
  `polyplug_runtime_create` call — the runtime copies the keys during create, so
  the buffer is reclaimed afterward (no module-level state)

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
