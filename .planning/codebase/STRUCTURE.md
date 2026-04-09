# Codebase Structure

**Analysis Date:** 2026-04-02

## Directory Layout

```
polyplug/
├── crates/                    # Rust crates (core runtime and language loaders)
│   ├── polyplug/              # Core runtime crate
│   ├── polyplug_abi/          # ABI type definitions (shared)
│   ├── polyplug_native/       # Native (.so/.dll/.dylib) loader
│   ├── polyplug_python/       # Python (CPython/pyo3) loader
│   ├── polyplug_js/           # JavaScript (QuickJS) loader
│   ├── polyplug_lua/          # Lua (LuaJIT) loader
│   ├── polyplug_dotnet/       # .NET (CoreCLR) loader
│   ├── polyplugc/             # Code generation CLI
│   ├── polyplug_codegen/      # Code generation library
│   ├── polyplug_utils/        # Shared utilities (BundleId, etc.)
│   └── sdk_validator/         # SDK validation tool
├── sdks/                      # Cross-language SDKs
│   ├── rust/                  # Rust host and guest SDKs
│   ├── python/                # Python host and guest SDKs
│   ├── csharp/                # C#/.NET host and guest SDKs
│   ├── cpp/                   # C++ host and guest SDKs
│   ├── lua/                   # Lua host and guest SDKs
│   └── js/                    # JavaScript host and guest SDKs
├── examples/                  # Working examples
│   ├── hosts/                 # Example host applications
│   │   └── rust/              # Rust host example
│   └── guests/                # Example plugins
│       └── rust/              # Rust plugin examples (decoder, encoder, etc.)
├── tests/                     # Integration tests
│   ├── fixtures/              # Test plugins (native, python, lua, js, csharp)
│   └── integration/           # Integration test suites
└── Cargo.toml                 # Workspace manifest
```

## Directory Purposes

**`crates/polyplug/`:**
- Purpose: Core runtime implementation
- Contains: `Runtime`, `PluginRegistry`, `BundleLoader` trait, FFI layer, hot-reload framework
- Key files: `src/runtime.rs`, `src/registry/plugin_registry.rs`, `src/ffi.rs`, `src/loader/mod.rs`

**`crates/polyplug_abi/`:**
- Purpose: C-compatible ABI type definitions
- Contains: `GuestContractInterface`, `HostInterface`, `GuestContractHandle`, `StringView`, `Buffer`, `AbiError`, dispatch types
- Key files: `src/plugin/mod.rs`, `src/host/mod.rs`, `src/types/mod.rs`, `src/dispatch/mod.rs`

**`crates/polyplug_native/`:**
- Purpose: Native shared library loader
- Contains: `NativeLoader` implementing `BundleLoader`, library handle management
- Key files: `src/loader.rs`, `src/lib.rs`, `src/config.rs`

**`crates/polyplug_python/`:**
- Purpose: CPython plugin loader
- Contains: `PythonLoader`, Python interpreter initialization via pyo3
- Key files: `src/lib.rs`, `src/bridge.rs`, `src/context.rs`

**`crates/polyplug_js/`:**
- Purpose: QuickJS JavaScript plugin loader
- Contains: `JsLoader`, QuickJS VM management via rquickjs
- Key files: `src/loader.rs`, `src/bridge.rs`, `src/ffi.rs`

**`crates/polyplug_lua/`:**
- Purpose: LuaJIT plugin loader
- Contains: `LuaLoader`, Lua VM management via mlua
- Key files: `src/loader.rs`, `src/bridge.rs`

**`crates/polyplug_dotnet/`:**
- Purpose: .NET CoreCLR plugin loader
- Contains: `DotnetLoader`, CLR hosting via netcorehost
- Key files: `src/lib.rs`, `src/ffi.rs`, `src/version.rs`

**`crates/polyplugc/`:**
- Purpose: CLI code generation tool
- Contains: `generate` and `pack` commands, parser, IR, per-language generators
- Key files: `src/lib.rs`, `src/parser/`, `src/generators/`

**`crates/polyplug_codegen/`:**
- Purpose: Code generation library (shared with polyplugc)
- Contains: IR types, generator traits, error types
- Key files: `src/lib.rs`, `src/generator.rs`, `src/languages/`

**`sdks/rust/`:**
- Purpose: Rust-specific host and guest libraries
- Contains: `host/` crate for hosts, `guest/` crate for plugins, `abi/` shared ABI types
- Key files: `host/src/lib.rs`, `guest/src/lib.rs`

**`examples/hosts/rust/`:**
- Purpose: Demonstrates host usage pattern
- Contains: Complete host with all loaders, contract callers, hot-reload handling
- Key files: `src/main.rs`, `generated/host/` (generated callers)

**`examples/guests/rust/`:**
- Purpose: Demonstrates plugin implementation pattern
- Contains: Multiple plugins (decoder, encoder, transformer, reporter, validator)
- Key files: `*/src/lib.rs`, `*/generated/guest/` (generated vtables)

**`tests/fixtures/`:**
- Purpose: Test plugins for integration tests
- Contains: Minimal plugins in each language for testing load/reload/errors
- Key files: `test_plugin/src/lib.rs`, `test_plugin_python/`, `test_plugin_lua/`, etc.

## Key File Locations

**Entry Points:**
- `crates/polyplug/src/lib.rs`: Core runtime crate root
- `crates/polyplug/src/runtime.rs:99-103`: `Runtime::builder()` entry
- `crates/polyplug/src/ffi.rs:135-144`: `polyplug_runtime_create` FFI entry
- `crates/polyplugc/src/lib.rs:15-56`: `generate()` function entry

**Configuration:**
- `Cargo.toml`: Workspace manifest with all member crates
- `crates/*/Cargo.toml`: Per-crate dependencies and features
- `examples/*/Cargo.toml`: Example-specific configuration

**Core Logic:**
- `crates/polyplug/src/runtime.rs`: Runtime struct, host callbacks, bundle loading
- `crates/polyplug/src/registry/plugin_registry.rs`: VTable storage, handle validation
- `crates/polyplug/src/loader/bundle_loader.rs:8-51`: `BundleLoader` trait definition
- `crates/polyplug/src/reload.rs:77-111`: Quiescence wait implementation

**ABI Types:**
- `crates/polyplug_abi/src/plugin/plugin_interface.rs`: VTable struct
- `crates/polyplug_abi/src/host/host_vtable/host_vtable.rs`: Host callback table
- `crates/polyplug_abi/src/plugin/plugin_handle.rs`: Generational handle
- `crates/polyplug_abi/src/types/string_view.rs`: Non-owning string type

**Testing:**
- `crates/polyplug/src/ffi.rs:574-979`: FFI tests (runtime isolation, handle packing)
- `crates/polyplug/src/registry/plugin_registry.rs:601-1022`: Registry tests
- `tests/integration/tests/`: Integration test suite

## Naming Conventions

**Files:**
- `mod.rs`: Module root files
- `lib.rs`: Library crate root
- `main.rs`: Binary crate root
- `*_test.rs` or `tests/`: Test modules
- `generated/`: Code-generated files (never hand-edited)

**Directories:**
- `src/`: Source code
- `tests/`: Test code
- `examples/`: Example code
- `abi/`: ABI definitions (shared between host/guest)
- `host/`: Host-side code
- `guest/`: Plugin-side code

**Generated Code:**
- `generated/host/host_callers.rs`: Contract caller functions
- `generated/host/types.rs`: Host-side type definitions
- `generated/guest/vtables.rs`: Plugin vtable setup
- `generated/guest/init.rs`: Plugin initialization code

## Where to Add New Code

**New Loader (e.g., WebAssembly):**
- Primary crate: `crates/polyplug_wasm/` (new crate)
- Implement `BundleLoader` trait
- Add to workspace `Cargo.toml` members
- Create `src/lib.rs` with loader and config
- Create `src/loader.rs` with load/reload logic

**New Contract:**
- Define in `api.toml` or `bundle.toml`
- Run `polyplugc generate --api api.toml --lang <lang> --out generated/`
- Generated files go to `generated/host/` or `generated/guest/`

**New Plugin:**
- Create directory in `examples/guests/<lang>/<plugin_name>/`
- Add `Cargo.toml` (or equivalent) with crate-type `cdylib` for native
- Implement contract trait in `src/lib.rs`
- Add generated code via polyplugc

**New Host Application:**
- Create directory in `examples/hosts/<lang>/`
- Add runtime dependency
- Register loaders for required languages
- Use generated contract callers

**Shared Utilities:**
- Add to `crates/polyplug_utils/src/lib.rs`
- Export from `polyplug_abi` if ABI-related

**New Test Plugin:**
- Create directory in `tests/fixtures/<plugin_name>/`
- Add minimal implementation
- Add to workspace members in root `Cargo.toml`

## Special Directories

**`generated/`:**
- Purpose: Code-generated files (host callers, guest vtables, types)
- Generated: Yes (by `polyplugc`)
- Committed: Yes (for convenience, but can be regenerated)

**`tests/fixtures/`:**
- Purpose: Test plugins used by integration tests
- Contains: Rust native plugins, Python modules, Lua scripts, JS bundles, C# assemblies
- Committed: Yes

**`target/`:**
- Purpose: Build artifacts
- Generated: Yes
- Committed: No (gitignored)

**`.planning/`:**
- Purpose: Planning documents (like this one)
- Generated: Yes
- Committed: Yes

## Module Hierarchy

**Core Runtime (`crates/polyplug/src/`):**
```
lib.rs
  - compatibility/mod.rs       # Version compatibility, capability graph
  - error.rs                   # Error type hierarchy
  - ffi.rs                     # C ABI entry points
  - host_bridge/mod.rs         # Host bridge utilities
  - loader/mod.rs              # BundleLoader trait, manifest parsing
    - bundle_loader.rs         # Trait definition
    - loaded_bundle.rs         # Loaded bundle wrapper
    - manifest.rs              # Manifest parsing
    - scanner.rs               # Plugin directory scanning
  - registry/mod.rs            # Registry module
    - plugin_registry.rs       # VTable storage, handle management
  - reload.rs                  # Hot-reload framework
  - runtime.rs                 # Core runtime struct
  - runtime_builder.rs         # Builder pattern
  - runtime_config.rs          # Configuration struct
```

**ABI Types (`crates/polyplug_abi/src/`):**
```
lib.rs
  - contract_type.rs           # Contract ID computation
  - dispatch/mod.rs            # Dispatch type definitions
  - ffi.rs                     # Allocator FFI functions
  - host/mod.rs                # Host-side types
    - host_vtable/             # HostInterface definition
    - host_context.rs          # Host context for callbacks
  - plugin/mod.rs              # Plugin-side types
    - plugin_context.rs        # Context passed to init
    - plugin_descriptor.rs     # Plugin metadata
    - plugin_handle.rs         # Generational handle
    - plugin_interface.rs      # VTable definition
  - runtime_language.rs        # Language enum
  - tracking/                  # Tracking utilities
  - types/mod.rs               # Core types (StringView, Buffer, AbiError)
```

**Code Generation (`crates/polyplugc/src/`):**
```
lib.rs                         # generate() and pack() functions
  - generators/mod.rs          # Generator trait and implementations
    - rust.rs                  # Rust code generator
    - cpp.rs                   # C++ code generator
    - csharp.rs                # C# code generator
    - python.rs                # Python code generator
    - lua.rs                   # Lua code generator
    - js_quickjs.rs            # QuickJS code generator
  - ir/mod.rs                  # Intermediate representation
  - parser/mod.rs              # API/bundle.toml parsing
  - pack/mod.rs                # Bundle packing utilities
```

---

*Structure analysis: 2026-04-02*