# Examples Gallery

All reference examples live under `examples/`. They share a single API
contract (`examples/api.toml`) and demonstrate a five-stage data-processing
pipeline. Every contract is implemented in all six supported languages, and
every host application loads all available native + VM plugins and drives the
same pipeline.

---

## The Pipeline API (`examples/api.toml`)

Five plugin contracts, one host contract:

| Contract | Function | Input | Output |
|---|---|---|---|
| `pipeline.Decoder` | `decode(StringView) → StringView` | `"name,value,42"` | `"DECODED:name\|value\|42"` |
| `data.Transformer` | `transform(StringView) → StringView` | decoded string | `"TRANSFORMED:NAME\|value (transformed)\|43"` |
| `pipeline.Encoder` | `encode(StringView) → StringView` | transformed string | `"NAME,value (transformed),43"` |
| `data.Reporter` | `report(StringView) → StringView` | transformed string | `"Report: NAME has value 'value (transformed)' with count 43"` |
| `pipeline.Validator` | `validate(StringView) → StringView` | decoded string | `"VALID:name\|value\|42"` or `"INVALID:reason"` |
| `host.logger` (host contract) | `log`, `log_with_level` | — | host-side console output |

---

## Building and Running

```
# Build polyplugc, all guest plugins, and the Rust host in one shot:
cd examples
./build_all.sh

# Run the Rust pipeline host (loads all plugins it finds under examples/plugins/):
POLYPLUG_PLUGIN_PATH=examples/plugins \
  examples/hosts/rust/target/release/pipeline_host
```

`build_all.sh` handles code generation (`polyplugc generate`), compilation, and
bundle assembly for every language. See the script for per-language build
commands if you want to build only one language at a time.

---

## Guest Plugins

### Rust (`examples/guests/rust/`)

All five Rust plugins are `cdylib` crates. They depend only on
`sdks/rust/guest` (`polyplug_guest`). Generated code lives in
`<plugin>/generated/`; implementation in `<plugin>/src/lib.rs`.

| Plugin | Path | Contract | What it does |
|---|---|---|---|
| decoder | `guests/rust/decoder/` | `pipeline.Decoder` | Replaces commas with pipes; prefixes `DECODED:` |
| transformer | `guests/rust/transformer/` | `data.Transformer` | Uppercases the name field, appends `(transformed)`, increments the counter |
| encoder | `guests/rust/encoder/` | `pipeline.Encoder` | Strips `TRANSFORMED:` prefix, converts pipes back to commas |
| reporter | `guests/rust/reporter/` | `data.Reporter` | Formats a human-readable report string; calls the `host.logger` host contract |
| validator | `guests/rust/validator/` | `pipeline.Validator` | Validates the three-part pipe-delimited format; returns `VALID:…` or `INVALID:reason` |

The reporter is the most instructive Rust guest: it demonstrates calling back
into the host via the generated `HostLoggerCaller` in
`generated/guest/host_contract_callers.rs`.

### C++ (`examples/guests/cpp/`)

Five C++ plugins compiled as shared libraries with `g++ -std=c++20 -fPIC
-shared`. Generated header/source live in `<plugin>/generated/`; implementation
in `<plugin>/<plugin>.cpp`. The build command is in `build_all.sh`.

| Plugin | Path | Contract |
|---|---|---|
| decoder | `guests/cpp/decoder/` | `pipeline.Decoder` |
| transformer | `guests/cpp/transformer/` | `data.Transformer` |
| encoder | `guests/cpp/encoder/` | `pipeline.Encoder` |
| reporter | `guests/cpp/reporter/` | `data.Reporter` |
| validator | `guests/cpp/validator/` | `pipeline.Validator` |

### C# (`examples/guests/csharp/`)

Five .NET 8 class-library plugins published with `dotnet publish`. The
`[ModuleInitializer]` attribute handles registration without an explicit entry
point. C# bundles are assembled separately under `examples/plugins-csharp/`
because the dotnet loader is optional and not registered by all example hosts.

| Plugin | Path | Contract |
|---|---|---|
| Decoder | `guests/csharp/decoder/` | `pipeline.Decoder` |
| Transformer | `guests/csharp/transformer/` | `data.Transformer` |
| Encoder | `guests/csharp/encoder/` | `pipeline.Encoder` |
| Reporter | `guests/csharp/reporter/` | `data.Reporter` |
| Validator | `guests/csharp/validator/` | `pipeline.Validator` |

### Python (`examples/guests/python/`)

Five Python plugins — a single `.py` file each. The loader prepends the bundle
directory and `bundle_dir/site-packages/` to `sys.path` so vendored SDK
packages are found automatically. No build step required; `build_all.sh` copies
the source file and vendors the Python SDK.

| Plugin | Path | Contract |
|---|---|---|
| decoder | `guests/python/decoder/` | `pipeline.Decoder` |
| transformer | `guests/python/transformer/` | `data.Transformer` |
| encoder | `guests/python/encoder/` | `pipeline.Encoder` |
| reporter | `guests/python/reporter/` | `data.Reporter` |
| validator | `guests/python/validator/` | `pipeline.Validator` |

### Lua (`examples/guests/lua/`)

Five Lua plugins — a single `.lua` file each. The loader prepends the bundle
directory to `package.path` and `package.cpath` so the generated guest glue
(`generated/guest/`) is reachable via `require`. No build step required.

| Plugin | Path | Contract |
|---|---|---|
| decoder | `guests/lua/decoder/` | `pipeline.Decoder` |
| transformer | `guests/lua/transformer/` | `data.Transformer` |
| encoder | `guests/lua/encoder/` | `pipeline.Encoder` |
| reporter | `guests/lua/reporter/` | `data.Reporter` |
| validator | `guests/lua/validator/` | `pipeline.Validator` |

### JavaScript / QuickJS (`examples/guests/js/`)

Five JS plugins bundled to a single IIFE file with `rolldown`. `polyplug_init`
is promoted to `globalThis` after bundling so the QuickJS loader can call it.

| Plugin | Path | Contract |
|---|---|---|
| decoder | `guests/js/decoder/` | `pipeline.Decoder` |
| transformer | `guests/js/transformer/` | `data.Transformer` |
| encoder | `guests/js/encoder/` | `pipeline.Encoder` |
| reporter | `guests/js/reporter/` | `data.Reporter` |
| validator | `guests/js/validator/` | `pipeline.Validator` |

---

## Host Applications

Each host registers the loaders it supports, scans a plugin directory, loads
every bundle it finds, and runs the full five-stage pipeline.

| Language | Path | Entry point | Loaders registered |
|---|---|---|---|
| Rust | `hosts/rust/` | `src/main.rs` | native, JS (QuickJS), Lua, Python, dotnet |
| C++ | `hosts/cpp/` | `host.cpp` (`main.cpp` builds the hot-reload host) | native, JS (QuickJS), Lua, Python, dotnet |
| C# | `hosts/csharp/` | `Program.cs` | native, JS (QuickJS), Lua, Python, dotnet |
| Python | `hosts/python/` | `main.py` | native, Python, JS, Lua, dotnet |
| Lua | `hosts/lua/` | `host.lua` | native, Lua, JS, Python, dotnet |
| JavaScript (Deno) | `hosts/js/` | `host.js` | native, Lua, JS, Python, dotnet |

The Rust host (`hosts/rust/`) is the primary reference: it is the most complete
and most closely tracks internal API changes. Read it alongside the generated
code in `hosts/rust/generated/` to understand the full host-side flow.

The Lua host additionally installs a **custom runtime logger**
(`Runtime.new{ log = ..., log_max_level = ... }`, routed through the
`polyplug_lua` loader cdylib's log trampoline because LuaJIT callbacks cannot
receive the ABI's by-value `StringView`s). Its output goes to **stderr** as
`[host-log][<level>][<scope>] <message>` lines so the pipeline stdout stays
byte-identical across hosts (`verify_hosts.sh`). The lua decoder guest emits a
one-time `guest.lua_decoder` Info line through the funnel on its first
dispatch (visible from any host whose logger delivers Info). See
`sdks/lua/README.md` § Custom Logger.

### Cross-language parity host (`hosts/parity/`)

A dedicated Rust host that loads each contract from every language and asserts
the outputs are byte-for-byte identical. It is the CI correctness gate for
cross-language parity. Run it with `hosts/parity/run.sh` after `build_all.sh`.

---

## Generated code locations

Each guest and host has a `generated/` subdirectory produced by `polyplugc`.
These files are checked in so the examples build without requiring `polyplugc`
to be run first. They are regenerated by `build_all.sh`.

```
examples/
├── api.toml                          shared contract definition
├── build_all.sh                      build + assemble all plugins and hosts
├── verify_hosts.sh                   run all six hosts and compare output
├── guests/
│   ├── rust/{decoder,encoder,transformer,reporter,validator}/
│   ├── cpp/{decoder,encoder,transformer,reporter,validator}/
│   ├── csharp/{decoder,encoder,transformer,reporter,validator}/
│   ├── python/{decoder,encoder,transformer,reporter,validator}/
│   ├── lua/{decoder,encoder,transformer,reporter,validator}/
│   └── js/{decoder,encoder,transformer,reporter,validator}/
├── hosts/
│   ├── rust/        Rust host (primary reference)
│   ├── cpp/         C++ host
│   ├── csharp/      C# host
│   ├── python/      Python host
│   ├── lua/         Lua host
│   ├── js/          JavaScript (Deno) host
│   └── parity/      cross-language parity harness
└── plugins/         assembled bundles written by build_all.sh (gitignored)
```

---

## See Also

- `docs/QUICKSTART.md` — step-by-step guide: write a plugin from scratch in 10 minutes
- `docs/WORKFLOW.md` — full host and guest pipelines for all languages
- `examples/build_all.sh` — real build and assemble commands for every language
