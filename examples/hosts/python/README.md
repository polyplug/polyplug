# python host

A Python host that demonstrates the core polyplug workflow using `host-libs/python`.

It runs a full data-processing pipeline across five guest plugins:

1. **decode** — `pipeline.decoder` — parses a CSV line into a `DataRecord`
2. **transform** — `data.Transformer` — uppercases the fields
3. **encode** — `pipeline.encoder` — serialises the record back to CSV
4. **report** — `pipeline.reporter` — produces a summary string
5. **validate** — `pipeline.validator` — validates the final record

## Running

From the repository root:

```bash
PYTHONPATH=host-libs/python python3 examples/hosts/python/host.py
```

`libpolyplug.so` must be on `LD_LIBRARY_PATH`. Build it first:

```bash
cargo build --release -p polyplug
export LD_LIBRARY_PATH=$PWD/target/release:$LD_LIBRARY_PATH
```

## Building guest plugins first

The host tries to load bundles for all supported languages. Build the guests you
want to exercise before running:

```bash
# Rust guests
cargo build --release --manifest-path examples/guests/rust/decoder/Cargo.toml
cargo build --release --manifest-path examples/guests/rust/encoder/Cargo.toml

# Python guests (no build step — the .py file is the plugin)
# Lua and JS guests
./examples/guests/lua/build.sh
./examples/guests/js/build.sh
```

Load failures for missing bundles are printed as warnings and skipped.

## Expected output

```
Run output: ALICE,HELLO,3
Run summary: 1 records processed
Validation: ok
pipeline complete
```

## What this example shows

- `Runtime()` — creates the polyplug runtime via `ctypes`
- `runtime.load_bundle(path)` — loads a bundle directory
- `runtime.find_by_bundle(bundle_id, contract_id, min_version)` — lookup by bundle + contract
- `runtime.resolve_plugin(packed_handle)` — acquire a `PluginGuard` (holds vtable lifetime)
- `guard.get_vtable()` — obtain the raw vtable pointer
- Calling a function through the vtable using the `ABI_FN_TYPE` ctypes callback type
- Proper error handling — no `unwrap` equivalents, all errors raised as `RuntimeError`

## Contract IDs

| Contract              | ID                   |
|-----------------------|----------------------|
| `pipeline.decoder`    | `0x133E62ABD6E7D5BE` |
| `data.Transformer`    | `0x0E3044133E12EB05` |
| `pipeline.encoder`    | `0x12AD37F43386F752` |
| `pipeline.reporter`   | `0xD50E539CAE219A15` |
| `pipeline.validator`  | `0x027ABCEBF8020D90` |

See `examples/contract_ids.txt` and `examples/api.toml` for the full definitions.
