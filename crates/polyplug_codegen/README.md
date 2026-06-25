# polyplug_codegen

ABI-SDK code-generation library. It emits the per-language `sdks/*/abi` mirror
files from the extracted Rust ABI types, and owns the shared data/error/config
types that the `polyplugc` CLI consumes.

This crate is **not** the contract-plugin generator. The per-contract host/guest
binding generators live in `crates/polyplugc/src/generators/` and share no
language emitters with this crate by design (see the project `CLAUDE.md`).

## Two consumers

1. **`polyplug_abi`'s build script** (`crates/polyplug_abi/build/generate.rs`)
   drives the `languages/` emitters at build time. It extracts the ABI items
   (`data::Item`: consts, structs, enums, unions, function signatures) from the
   `polyplug_abi` sources and emits the language mirrors under `sdks/*/abi`.
   There is no Rust emitter — the Rust ABI is the source of truth itself.
2. **`polyplugc`** depends on this crate only for shared types: `GenerateConfig`,
   `GenerateOutput`, `Side`, `PolyplugcError` (with `error::SourceLocation`),
   `ResolvedBundleFile`, `PlatformKey`, and `Lang`. It does **not** call the
   `languages/` emitters.

## Modules

| Module | Purpose |
|---|---|
| `data` | Language-neutral ABI item model (`Item`, `ConstInfo`, `StructInfo`, `EnumInfo`, `UnionInfo`, `FunctionInfo`, field/layout info) |
| `generator` | `CodeGenerator` trait — item-by-item generation (`generate_const`, `generate_struct`, `generate_enum`, `generate_union`, `generate_function`) |
| `context` | `GenerationContext` + `Language` (Cpp, CSharp, Python, Lua, JavaScript): type mappings and formatting settings |
| `languages/` | The five ABI-SDK emitters: `cpp.rs`, `csharp.rs`, `python.rs`, `lua.rs`, `js.rs` |
| `error` | `PolyplugcError` and `SourceLocation` — shared diagnostics used by `polyplugc` |
| `reserved` | Reserved-word union table across all six target languages; contract identifiers that collide with any language keyword are rejected at parse time |

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

Includes `tests/layout_calculations.rs` (struct size/offset math) and
`tests/typed_fn_ptr_generation.rs` (typed function-pointer emission).

## License

Apache 2.0 — See `../../LICENSE` for details.
