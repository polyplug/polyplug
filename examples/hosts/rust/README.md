# polyplug Rust Host Example

The reference Rust host that loads all 12 guest plugins and runs 3 cross-language pipelines.

## What It Does

1. Creates a `Runtime` with all language loaders (native, .NET, Python, Lua, JS)
2. Loads all 12 guest plugins from `examples/guests/`
3. Resolves each plugin by bundle name and contract ID
4. Runs 3 pipelines: decode → transform → encode → report → validate

## Pipelines

| Run | Decoder | Transformer | Encoder | Reporter | Validator |
|-----|---------|-------------|---------|----------|-----------|
| 1 | Rust | C++ | Rust | C# | C++ |
| 2 | Python | Lua | C# | Python | Lua |
| 3 | Rust | C++ | C# | JS | JS |

## Requirements

- Rust toolchain (1.85+)
- .NET 10 SDK (for C# guests)
- Python 3.11+ (for Python guests)
- LuaJIT (for Lua guests)
- Guest plugins built (see `examples/build_guests.sh`)

## Building

```bash
# Build all guest plugins first
./examples/build_guests.sh

# Build and run the Rust host
cargo run -p polyplug-rust-host
```

## Expected Output

```
=== polyplug Rust host example ===
Loading 12 guest plugins...
  [OK]   1/12 csharp/encoder
  [OK]   2/12 csharp/reporter
  ...
  [OK]  12/12 js/reporter
--- Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator ---
Run output: ALICE,HELLO,3
Run summary: ...
Validation: ok (...)
--- Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator ---
...
--- Run 3: Rust decoder, C++ transformer, C# encoder, JS reporter, JS validator ---
...
pipeline complete
```

## Key APIs Used

```rust
// Build runtime with all loaders
let runtime = Runtime::builder()
    .loader(DotnetLoader::new(DotnetConfig::default()))
    .loader(PythonLoader::new(PythonConfig::default()))
    .loader(LuaLoader::new(LuaConfig::default()))
    .loader(JsLoader::new(JsConfig {}))
    .build()?;

// Load a bundle
runtime.load_bundle(Path::new("/path/to/bundle"))?;

// Find by bundle name + contract
let handle = runtime.find_by_bundle(bundle_id, contract_id, 0)?;

// Resolve to vtable guard
let guard = runtime.registry().resolve_guard(handle)?;
let vtable: *const PluginVTable = guard.vtable();

// Call a function
let func: AbiFn = mem::transmute((*vtable).functions[0]);
let err = func(args_ptr, out_ptr);
```

## See Also

- `examples/abi_types.md` — canonical ABI type reference
- `examples/contract_ids.txt` — contract ID values
- `examples/api.toml` — API definition used to generate bindings
- `host-libs/rust/` — the `polyplug_host` library source
