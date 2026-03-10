# Polyplug Showcase

This developer-facing showcase demonstrates the polyplug runtime's ability to coordinate plugins written in different languages using unified ABI contracts. It isn't production code, but a functional proof of concept for cross-language extensibility.

## Plugins

The showcase features 6 plugins across different runtimes:

1. **csv_decoder** (Rust, native .so): Decodes CSV lines into `DataRecord` structs. Implements `pipeline.decoder@1`.
2. **uppercase_transformer** (C++, native .so): Transforms field values to uppercase. Implements `pipeline.transformer@1`.
3. **summary_reporter** (Python): Generates a summary string from a `DataRecord`. Implements `pipeline.reporter@1` (v1.1).
4. **reverse_transformer** (Lua/LuaJIT): Reverses field values character by character. Implements `pipeline.transformer@1` using relaxed compatibility by exposing two functions.
5. **field_validator** (JavaScript/QuickJS): Validates `DataRecord` fields. Implements `pipeline.validator@1`.
6. **csv_encoder** (C#/.NET): Encodes a `DataRecord` back to a CSV string. Implements `pipeline.encoder@1` and demonstrates cross-plugin dependency lookup for the Rust decoder.

## Pipeline Flow (Run 1)

Input CSV → **csv_decoder** → **field_validator** → **uppercase_transformer** → **csv_encoder** → Output CSV → **summary_reporter**

## Key Concepts Demonstrated

- **Multi-language plugin loading**: Rust, C++, C#, Python, Lua, and JavaScript all participating in a single pipeline.
- **Contract-based dispatch**: Using computed contract IDs to route calls regardless of the implementation language.
- **Bundle-specific lookup**: Selecting between different transformer implementations using `find_by_bundle`.
- **TraceExtension integration**: Plugins emitting trace messages that the host runtime captures and displays.
- **Relaxed version compatibility**: The Lua plugin satisfies a single-function contract by exposing multiple related functions.
- **Cross-plugin dependency**: The C# encoder declares and resolves a dependency on the Rust decoder.
- **Error handling**: Proper propagation when malformed input causes a plugin to return a non-zero error code.

## Build and Run

Before running the showcase, build the plugins:

```bash
# Build all plugins (required first time or after source changes)
./showcase/build_plugins.sh
```

Execute the host application to see the pipeline in action:

```bash
cargo run --manifest-path showcase/host/Cargo.toml
```

## Verified Output

```
=== polyplug showcase ===
--- Run 1: C++ uppercase transformer ---
[trace] [csv_decoder] decode called
[trace] [uppercase_transformer] transform called
Run output: name,value,count
ALICE,HELLO,3
Run summary: Summary: name=ALICE value=HELLO count=3
--- Run 2: Lua reverse transformer ---
[trace] [csv_decoder] decode called
Run output: name,value,count
ecilA,olleh,3
Run summary: Summary: name=ecilA value=olleh count=3
--- Error scenario: malformed input ---
Error: decode failed with code 1
=== showcase complete ===
```
