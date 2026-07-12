# polyplug_codegen

Code-generation library for polyplug ABI SDK mirrors and contract bindings. It
owns the API and bundle parser, validated IR, all six contract generators, and
safe incremental output writing. `polyplugc` is the command-line interface over
this library.

## Consumers

1. **`polyplug_abi`'s build script** (`crates/polyplug_abi/build/generate.rs`)
   drives the `languages/` emitters at build time. It extracts ABI items
   (`data::Item`: consts, structs, enums, unions, function signatures) from the
   `polyplug_abi` sources and emits the language mirrors under `sdks/*/abi`.
   There is no Rust ABI emitter because the Rust ABI is the source of truth.
2. **Contract-binding consumers** call `generate`, `generate_ir`, and
   `write_output` to parse a manifest or render validated IR, then write the
   generated files. Rust in-process guests use `generate_rust_guest` with
   `RustGuestMode::InProcess`; their generated `InProcessFactory::new` accepts
   `Send + Sync + 'static` function items or captured closures and the runtime
   owns the resulting resident after canonical registration commits. The existing
   `generate` API and `polyplugc` CLI retain disk-bundle output. `write_output`
   accepts only relative file paths without root, prefix, or parent-directory
   components. The `polyplugc` CLI calls the same disk-generation API.

## Modules

| Module | Purpose |
|---|---|
| `data` | Language-neutral ABI item model (`Item`, `ConstInfo`, `StructInfo`, `EnumInfo`, `UnionInfo`, `FunctionInfo`, field/layout info) |
| `context` | ABI-SDK `GenerationContext` and language settings |
| `languages/` | The five ABI-SDK emitters: `cpp.rs`, `csharp.rs`, `python.rs`, `lua.rs`, `js.rs` |
| `parser` | TOML API and bundle parsing into validated contract IR |
| `ir` | Validated contract, bundle, dependency, type, enum, and version model |
| `generators/` | Contract-binding backends for Rust, C++, C#, Python, Lua, and QuickJS |
| `generate` | Public manifest/IR generation, incremental writer, Rust formatting, and language parsing |
| `error` | `PolyplugcError` and `SourceLocation` diagnostics |
| `reserved` | Reserved-word union table across all six target languages |

## Generated output rules

- Generated files carry a header marking them as generated and must never be
  edited by hand — fix the emitter and rebuild.
- The `sdks/*/abi` mirrors regenerate when `polyplug_abi` builds (its build
  script runs the emitters).
- The helper surface and enum mirrors of the emitted SDK files are validated by
  `sdk_validator` against `checks/sdk_validator.yaml`:

```bash
cargo run -p sdk-validator -- --config checks/sdk_validator.yaml --fail-on-missing
```

## Testing

```bash
cargo test --package polyplug_codegen
```

Includes ABI emitter coverage plus parser, validated-IR, six-backend, incremental
writer, and unsafe-output-path coverage for contract binding generation.

## License

Apache 2.0 — See `../../LICENSE` for details.
