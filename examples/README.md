# polyplug Examples — Cross-Language Plugin Platform

Complete, identical examples across all 6 supported languages.

## Quick Start

```bash
./examples/build_all.sh
./examples/hosts/rust/target/release/pipeline_host
```

## Plugin Pipeline

```
Input → Decoder → Transformer → Encoder → Reporter → Validator
"name,value,42" → "DECODED:name|value|42" → "TRANSFORMED:NAME|value (transformed)|43" → ...
```

`api.toml` uses `[[guest_contract]]` for the five plugin-provided stages and
`[[host_contract]]` for services the hosts provide. `plugin_contract` is not a
schema alias; migrate a legacy table before running the examples.

## Guest Plugins (30 total)

| Language | Decoder | Encoder | Transformer | Reporter | Validator |
|----------|---------|---------|-------------|----------|-----------|
| Rust | ✓ | ✓ | ✓ | ✓ | ✓ |
| C++ | ✓ | ✓ | ✓ | ✓ | ✓ |
| C# | ✓ | ✓ | ✓ | ✓ | ✓ |
| Python | ✓ | ✓ | ✓ | ✓ | ✓ |
| Lua | ✓ | ✓ | ✓ | ✓ | ✓ |
| JavaScript | ✓ | ✓ | ✓ | ✓ | ✓ |

## Host Applications (6 total)

| Language | Directory |
|----------|-----------|
| Rust | hosts/rust/ |
| C++ | hosts/cpp/ |
| C# | hosts/csharp/ |
| Python | hosts/python/ |
| Lua | hosts/lua/ |
| JavaScript | hosts/js/ |

## File Structure

```
examples/
├── api.toml
├── build_all.sh
├── README.md
├── guests/{rust,cpp,csharp,python,lua,js}/{decoder,encoder,transformer,reporter,validator}/
├── hosts/{rust,cpp,csharp,python,lua,js}/
└── plugins/
```

## Dependencies

**Guest plugins** depend only on sdks/*/guest (polyplug_guest).
**Host applications** depend only on sdks/*/host (polyplug).

## See Also

- `api.toml` — API definition
- `../sdks/` — SDKs for each language
- `../../docs/` — Design documentation
- `../docs/CODE_GENERATION.md` — unified default and opt-in split output for all six languages
