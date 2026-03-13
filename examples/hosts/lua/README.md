# lua host

A LuaJIT host that demonstrates the full polyplug workflow using the Lua host-lib (`host-libs/lua/polyplug.lua`).

Loads all 12 guest plugins across every supported language (Rust, C++, C#, Python, Lua, JavaScript) and runs the complete pipeline: **decode → validate → transform → encode → report**.

## Prerequisites

- **LuaJIT** (version 2.0+) — standard Lua 5.x is NOT supported (requires the `ffi` module)
- **Rust toolchain** — to build the companion shared library
- Guest plugins built (see below)

## Build the companion shared library

The host requires `libpolyplug_lua_host.so`, a cdylib that provides a runtime with all language loaders pre-registered.

From the repository root:

```bash
cargo build --manifest-path examples/hosts/lua/Cargo.toml
```

This produces `examples/hosts/lua/target/debug/libpolyplug_lua_host.so`.

## Build the guest plugins

```bash
./examples/build_guests.sh
```

Or build specific languages:

```bash
./examples/build_guests.sh rust cpp lua
```

## Running

From the repository root:

```bash
luajit examples/hosts/lua/host.lua
```

Expected output:

```
=== polyplug lua host ===
[load] guest  1 OK: csharp/encoder
[load] guest  2 OK: csharp/reporter
[load] guest  3 OK: rust/decoder
...
[load] 12/12 guests loaded
[decode]    name=Alice  value=hello  count=3
[validate]  OK
[transform] name=Alice  value=HELLO  count=3
[encode]    Alice,HELLO,3
[report]    ...
--- error scenario: malformed input ---
[error]     decode failed: ... (code 1)
pipeline complete
```

## What this example shows

- `polyplug_full.lua` — wrapper around `host-libs/lua/polyplug.lua` that uses `polyplug_runtime_new_full()` to create a runtime with all language loaders registered
- `rt:load_bundle(path)` — loading a plugin bundle from disk
- `rt:find_by_contract(contract_id, min_version)` — finding a plugin by contract ID
- `rt:resolve_plugin(handle)` — obtaining a guard with the vtable pointer
- `guard:vtable()` — retrieving the raw vtable pointer
- Vtable dispatch via `ffi.cast("const PluginVTable*", vtable_ptr)` and `vt.functions[fn_index]`
- Guard lifecycle management with `guard:free()` for deterministic cleanup
- Error handling with `pcall` for non-fatal load errors and ABI error code inspection

## Contracts used

| Contract            | ID                   | Function |
|---------------------|----------------------|----------|
| `pipeline.decoder`  | `0x133E62ABD6E7D5BE` | `decode(Buffer) -> DataRecord` |
| `pipeline.validator`| `0x027ABCEBF8020D90` | `validate(DataRecord) -> ValidationResult` |
| `pipeline.transformer` | `0x0E3044133E12EB05` | `transform(DataRecord) -> DataRecord` |
| `pipeline.encoder`  | `0x12AD37F43386F752` | `encode(DataRecord) -> Buffer` |
| `pipeline.reporter` | `0xD50E539CAE219A15` | `report(DataRecord) -> StringView` |

See `examples/contract_ids.txt` and `examples/api.toml` for the full contract definitions.

## File structure

```
examples/hosts/lua/
├── host.lua              — the host example (entry point)
├── polyplug_full.lua     — polyplug module wrapper (all-loader runtime)
├── Cargo.toml            — companion cdylib manifest
└── src/
    └── lib.rs            — companion cdylib source
```

The base Lua host-lib is at `host-libs/lua/polyplug.lua`.
