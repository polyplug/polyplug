# Technology Stack

**Analysis Date:** 2026-04-02

## Languages

**Primary:**
- Rust 1.85 (Edition 2024) - Core runtime, FFI layer, loaders, CLI, and Rust SDKs

**Secondary:**
- TypeScript/Deno - JavaScript SDK (`sdks/js/`)
- Python 3.10+ - Python SDK and loaders (`sdks/python/`)
- C# (.NET 10.0) - C# SDK (`sdks/csharp/`)
- Lua (LuaJIT) - Lua SDK (`sdks/lua/`)
- C++ (header-only) - C++ SDK (`sdks/cpp/`)

## Runtime

**Environment:**
- Native C ABI via shared libraries (`.so`, `.dylib`, `.dll`)
- Cargo workspace with multiple crate types (`cdylib`, `rlib`)

**Package Manager:**
- Cargo (Rust) - Workspace-based dependency management
- Lockfile: `Cargo.lock` (present)

**For Secondary Languages:**
- npm/Deno for TypeScript SDK
- pip/setuptools for Python packages
- NuGet for .NET packages
- LuaRocks (implicit) for Lua

## Frameworks

**Core:**
- polyplug (custom) - Universal cross-language plugin runtime
- Uses `#[repr(C)]` FFI for cross-language ABI boundary

**Testing:**
- Rust: Built-in `#[test]` + `criterion` for benchmarks
- External toolchains tested via CI matrix (dotnet, python, lua, js-quickjs)

**Build/Dev:**
- Just (justfile) - Task runner for build operations
- polyplugc - CLI code generator for multi-language bindings
- ast-grep - SDK consistency validation

## Key Dependencies

**Critical:**
- `libloading` 0.9 - Dynamic library loading for native plugins
- `pyo3` 0.28 - Python bindings (for Python loader)
- `mlua` 0.11 (LuaJIT vendored) - Lua bindings (for Lua loader)
- `rquickjs` 0.11 - QuickJS JavaScript engine (for JS loader)
- `netcorehost` 0.20 - .NET runtime hosting (for .NET loader)
- `arc-swap` 1.7 - Hot-reload atomic pointer swapping
- `notify` 8.2 - File system watching for hot-reload

**Infrastructure:**
- `serde` 1.0 + `toml` 0.9 - Manifest parsing and serialization
- `thiserror` 2.0 + `anyhow` 1.0 - Error handling
- `petgraph` 0.8 - Dependency graph algorithms
- `syn` 2 + `quote` 1 - Code generation (proc-macro style)
- `clap` 4.5 - CLI argument parsing (polyplugc)
- `pelite` 0.10 - PE file parsing (Windows .NET hosting)
- `tree-sitter` 0.25 + `tree-sitter-lua` 0.2 - Lua source parsing

## Configuration

**Environment:**
- Cargo workspace with unified versions via `workspace.package`
- Platform-specific Rust flags in `.cargo/config.toml` (target-cpu=native warning)
- Release profile: opt-level=3, LTO=thin, strip=symbols

**Build:**
- `Cargo.toml` - Workspace manifest
- `justfile` - Build automation (46KB comprehensive task runner)
- `sdk_validator.yaml` - SDK consistency rules
- `abi.toml` (in `crates/polyplug_abi/`) - ABI type definitions

## Platform Requirements

**Development:**
- Rust 1.85+ toolchain
- Python 3.10+ with dev headers (for Python loader)
- Lua 5.4+ dev headers (for Lua loader)
- .NET 10.0 SDK (for .NET loader and C# SDK)
- Deno 1.38.0+ (for TypeScript SDK)

**Production:**
- Platform-specific native libraries:
  - `libpolyplug.so` (Linux x64)
  - `libpolyplug.dylib` (macOS x64/arm64)
  - `polyplug.dll` (Windows x64)
- Loader cdylibs for each runtime language
- .NET runtime 10.0 for .NET plugins

---

*Stack analysis: 2026-04-02*