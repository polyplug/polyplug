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

**Guest plugins** depend only on guest-libs (polyplug_guest).
**Host applications** depend only on host-libs (polyplug).

## See Also

- `api.toml` — API definition
- `guest-libs/` — Guest libraries
- `host-libs/` — Host libraries
