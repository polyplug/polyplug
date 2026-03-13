# rust_minimal

A minimal Rust host that demonstrates the core polyplug workflow:

1. Create a `Runtime` and point it at a plugin directory
2. Find a plugin by contract ID
3. Resolve the handle to a vtable
4. Call a function through the vtable
5. Print the result

## Running

From the repository root:

```bash
cargo run -p rust_minimal
```

The host scans `examples/guests/rust/` for plugins. If no guest plugins are built yet, it exits cleanly with a message explaining how to build them.

## Building guest plugins first

```bash
cargo build --release --manifest-path examples/guests/rust/decoder/Cargo.toml
```

Then run the host again:

```bash
cargo run -p rust_minimal
```

Expected output:

```
=== polyplug rust_minimal example ===
Scanning: examples/guests/rust
Found plugin for contract 0x133E62ABD6E7D5BE
Plugin result: rust:transform(hello from rust_minimal)
=== rust_minimal example complete ===
```

## What this example shows

- `Runtime::builder().plugin_dir(...).build()` — scanning and loading plugins
- `runtime.find_by_contract(contract_id, min_version)` — looking up a plugin
- `runtime.resolve_plugin(handle)` — getting the vtable pointer
- Calling a function through the vtable using `unsafe` with `// SAFETY:` justifications
- Proper error handling with a typed `HostError` enum (no `unwrap`)

## Contract

The host looks for a plugin implementing `data.Transformer` (contract ID `0x133E62ABD6E7D5BE`), which exposes a single function:

```
transform(input: StringView) -> StringView
```

See `examples/api.toml` for the full contract definition.
