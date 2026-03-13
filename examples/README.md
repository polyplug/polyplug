# polyplug Examples

This directory contains the canonical examples for the **polyplug** plugin runtime. These examples demonstrate how to build hosts and guest plugins in various supported languages.

## Directory Structure

- **`hosts/`**: Host runtimes that load and execute polyplug bundles.
- **`guests/`**: Guest plugins implementing specific contracts.
- **`abi_types.md`**: Canonical reference for the `DataRecord` ABI type used by these examples.
- **`build.sh`**: Master build script for all guest plugins.
- **`contract_ids.txt`**: Registry of contract IDs used across these examples.
- **`api.toml`**: The API definition used by `polyplugc` to generate bindings.

## Supported Languages

We provide examples for 6 major languages, totaling 6 hosts and 12 guest plugins.

### Hosts
Available in `examples/hosts/`:
- **Rust**: The reference host implementation.
- **C++**: High-performance native host.
- **C#**: .NET integration.
- **Python**: Scripting integration via `ctypes`.
- **Lua**: Fast scripting via LuaJIT FFI.
- **JavaScript**: Deno and QuickJS support.

### Guests
Available in `examples/guests/`:
- **Rust**: `decoder`, `encoder`
- **C++**: `transformer`, `validator`
- **C#**: `reporter`, `logger`
- **Python**: `analyzer`, `filter`
- **Lua**: `processor`, `mapper`
- **JavaScript**: `fetcher`, `parser`

## Building the Examples

### Guest Plugins
To build all guest plugins across all languages, run the master build script from the repository root:

```bash
./examples/build.sh
```

You can also build specific languages:

```bash
./examples/build.sh rust cpp
```

Individual language build scripts are located at `examples/guests/<lang>/build.sh`.

### Host Runtimes
Host runtimes are typically built using their respective language's standard build tools (e.g., `cargo build` for Rust, `cmake` for C++). See the README within each host directory for specific instructions.

## ABI Reference
All examples in this directory use a shared `DataRecord` structure for data exchange. For detailed memory layouts and language-specific struct definitions, see [abi_types.md](./abi_types.md).
