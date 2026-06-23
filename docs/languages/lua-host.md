# Lua — Host (app)

This guide walks through embedding polyplug in a Lua (LuaJIT) application: installing the SDK, generating typed callers, building the runtime, loading bundles, and dispatching contracts.

See [Lua — overview](lua.md) for installation. See [Lua — Guest](lua-guest.md) to write a Lua plugin.

---

## Step 1 — Install the host SDK, loaders, and CLI

```bash
luarocks install polyplug
luarocks install polyplug-loader-native
luarocks install polyplug-loader-lua
# add more loaders as needed:
# luarocks install polyplug-loader-js polyplug-loader-python polyplug-loader-dotnet

cargo install polyplugc    # or: curl -fsSL https://polyplug.github.io/install.sh | bash
```

---

## Step 2 — Generate host callers

```bash
polyplugc generate --api api.toml --lang lua --out generated
```

This writes four files into `generated/host/`:

```
generated/host/
├── callers.lua             typed caller objects for each plugin contract
├── contracts.lua           contract ID constants
├── interface_factories.lua host-contract interface factories (if api.toml defines host_contract)
└── types.lua               generated enums and struct mirrors
```

Re-run whenever `api.toml` changes. Never edit the generated files — regenerate instead.

Caller struct names follow `NamespaceTypeContract_create` for a contract `namespace.Type`:

| Contract | Generated creator | Method |
|---|---|---|
| `pipeline.Decoder` | `callers.PipelineDecoderContract_create(rt, host)` | `:decode(input)` |
| `data.Transformer` | `callers.DataTransformerContract_create(rt, host)` | `:transform(input)` |

---

## Step 3 — Build the runtime and register loaders

```lua
local polyplug          = require('polyplug')
local native_loader     = require('polyplug.loaders.native')
local lua_loader        = require('polyplug.loaders.lua')
-- require other loaders as needed

local rt = polyplug.Runtime.new()

native_loader.register(rt)
lua_loader.register(rt)
```

`Runtime.new` accepts an optional options table:

```lua
local rt = polyplug.Runtime.new({
    -- Hot-reload and compatibility
    config = {
        hot_reload_enabled = true,
        compatibility = 0,   -- 0 = Strict (default), 1 = Relaxed, 2 = Yolo
    },

    -- Reload-phase callback (called on each load/reload/unload phase)
    on_reload = function(phase)
        io.stderr:write('phase: ' .. tostring(phase.phase_type) .. '\n')
    end,

    -- Custom runtime logger (receives runtime diagnostics + guest log calls)
    -- Requires polyplug-loader-lua installed (routes through the lua cdylib
    -- log trampoline — LuaJIT callbacks cannot receive by-value StringViews).
    -- Output goes to stderr so pipeline stdout stays byte-identical across hosts.
    log = function(level, scope, message)
        io.stderr:write(string.format('[%d][%s] %s\n', level, scope, message))
    end,
    log_max_level = polyplug.LogLevel.Info,   -- Error/Warn/Info/Debug/Trace

    -- Bundle signature enforcement
    signature_policy = polyplug.SignaturePolicy.Off,  -- Off (default) / WarnOnly / Required

    -- Key-pinning allowlist (optional; pairs with signature_policy ~= Off)
    -- Each entry is a 32-byte Ed25519 public key (Lua string or byte table).
    trusted_keys = { ... },
})
```

---

## Step 4 — Register a host contract (optional)

If `api.toml` declares a `[[host_contract]]`, implement it in Lua and register it through the generated factory before loading any bundles:

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

function ConsoleLogger:log_with_level(level, message)
    local names = { [0]='DEBUG',[1]='INFO',[2]='WARN',[3]='ERROR' }
    print(string.format('[plugin][%s] %s', names[tonumber(level)] or 'INFO', message))
end

-- lua_loader.bridge_lib() hands the generated factory the polyplug_lua cdylib
-- it needs for the host-contract dispatch trampoline.
local logger_iface = interface_factories.create_host_logger_interface(
    ConsoleLogger.new, lua_loader.bridge_lib())
rt:register_host_contract(logger_iface)
```

---

## Step 5 — Load bundles

```lua
-- Load a single bundle directory
rt:load_bundle('/path/to/my_plugin')

-- Or scan and load all bundles under a directory
local function load_dir(plugin_path)
    local pipe = io.popen('ls -d "' .. plugin_path .. '"/*/ 2>/dev/null')
    if not pipe then return end
    for dir in pipe:lines() do
        local path = dir:gsub('/$', '')
        local ok, err = pcall(function() rt:load_bundle(path) end)
        if not ok then
            io.stderr:write('skipped ' .. path .. ': ' .. tostring(err) .. '\n')
        end
    end
    pipe:close()
end

load_dir(os.getenv('POLYPLUG_PLUGIN_PATH') or 'plugins')
```

A Lua host can load guests written in **any** language — just register the corresponding loader before calling `load_bundle`. Lua and native bundles support **hot-reload**:

```lua
rt:reload_bundle('/path/to/my_plugin')
```

---

## Step 6 — Resolve and call a contract

```lua
local abi     = require('polyplug_abi')
local callers = require('generated.host.callers')

local host = rt:host()   -- HostApi* pointer for caller initialisation

-- Resolve the first available implementation of pipeline.Decoder
local decoder = callers.PipelineDecoderContract_create(rt, host)
if not decoder then
    error('no pipeline.Decoder plugin loaded')
end

-- Call the contract method; result is a StringView cdata
local result_sv = decoder:decode('name,value,42')
print(abi.to_str(result_sv))   -- "DECODED:name|value|42"
```

`PipelineDecoderContract_create` (and all generated `*_create` functions) returns `nil` when no matching contract is loaded, so guard the result before calling.

---

## Step 7 — Unload a bundle

```lua
local bundle_id = polyplug.bundle_id('my_decoder')
rt:unload_bundle(bundle_id)
```

---

## Full example

The reference Lua host is at `examples/hosts/lua/host.lua`. It demonstrates:

- All five loaders registered with graceful skip on unavailable cdylibs
- A custom runtime logger routed through the lua loader trampoline
- The `host.logger` host-contract registered via a generated interface factory
- Full five-stage pipeline dispatch: decode → transform → encode → report → validate
- Bundle-ID and contract-ID discovery printed on load
- Optional round-trip micro-benchmark (env `POLYPLUG_BENCH_ITERS`)

---

## Runtime API reference

| Method | Description |
|---|---|
| `polyplug.Runtime.new(opts?)` | Create a new runtime instance |
| `rt:load_bundle(path)` | Load a plugin bundle from a directory path |
| `rt:reload_bundle(path)` | Hot-reload a Lua or native bundle |
| `rt:unload_bundle(bundle_id)` | Unload a bundle by its ID |
| `rt:find_guest_contract(cid, min_ver)` | Find one contract handle (nil index = not found) |
| `rt:find_all_guest_contracts(cid, min_ver, cap?)` | Find all matching handles |
| `rt:resolve_guest_contract(handle)` | Resolve a handle → `GuestContractInterface*` |
| `rt:register_host_contract(iface)` | Register a host-side contract implementation |
| `rt:register_loader(loader_ptr)` | Register an opaque loader (used by loader modules) |
| `rt:host()` | Return the `HostApi*` pointer for FFI callers |
| `rt:destroy()` | Destroy the runtime and free all resources |
| `polyplug.bundle_id(name)` | Compute `fnv1a_64(name)` bundle ID |
| `polyplug.guest_contract_id(name, major)` | Compute guest contract ID |
| `polyplug.LogLevel` | `{ Error, Warn, Info, Debug, Trace }` |
| `polyplug.SignaturePolicy` | `{ Off, WarnOnly, Required }` |
