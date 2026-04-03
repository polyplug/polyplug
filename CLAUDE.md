<!-- GSD:project-start source:PROJECT.md -->
## Project

**polyplug**

A high-performance, zero/minimal-overhead cross-language plugin runtime for Rust. Enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

**Core Value:** The core runtime is loader-agnostic — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

### Constraints

- **Architecture:** Core crate must have zero loader-specific code or dependencies
- **Safety:** Hot-reload safety contract — hosts must not cache raw function pointers
- **Compatibility:** Breaking changes acceptable — not published yet
<!-- GSD:project-end -->

<!-- GSD:stack-start source:codebase/STACK.md -->
## Technology Stack

## Languages
- Rust 1.85 (Edition 2024) - Core runtime, FFI layer, loaders, CLI, and Rust SDKs
- TypeScript/Deno - JavaScript SDK (`sdks/js/`)
- Python 3.10+ - Python SDK and loaders (`sdks/python/`)
- C# (.NET 10.0) - C# SDK (`sdks/csharp/`)
- Lua (LuaJIT) - Lua SDK (`sdks/lua/`)
- C++ (header-only) - C++ SDK (`sdks/cpp/`)
## Runtime
- Native C ABI via shared libraries (`.so`, `.dylib`, `.dll`)
- Cargo workspace with multiple crate types (`cdylib`, `rlib`)
- Cargo (Rust) - Workspace-based dependency management
- Lockfile: `Cargo.lock` (present)
- npm/Deno for TypeScript SDK
- pip/setuptools for Python packages
- NuGet for .NET packages
- LuaRocks (implicit) for Lua
## Frameworks
- polyplug (custom) - Universal cross-language plugin runtime
- Uses `#[repr(C)]` FFI for cross-language ABI boundary
- Rust: Built-in `#[test]` + `criterion` for benchmarks
- External toolchains tested via CI matrix (dotnet, python, lua, js-quickjs)
- Just (justfile) - Task runner for build operations
- polyplugc - CLI code generator for multi-language bindings
- ast-grep - SDK consistency validation
## Key Dependencies
- `libloading` 0.9 - Dynamic library loading for native plugins
- `pyo3` 0.28 - Python bindings (for Python loader)
- `mlua` 0.11 (LuaJIT vendored) - Lua bindings (for Lua loader)
- `rquickjs` 0.11 - QuickJS JavaScript engine (for JS loader)
- `netcorehost` 0.20 - .NET runtime hosting (for .NET loader)
- `arc-swap` 1.7 - Hot-reload atomic pointer swapping
- `notify` 8.2 - File system watching for hot-reload
- `serde` 1.0 + `toml` 0.9 - Manifest parsing and serialization
- `thiserror` 2.0 + `anyhow` 1.0 - Error handling
- `petgraph` 0.8 - Dependency graph algorithms
- `syn` 2 + `quote` 1 - Code generation (proc-macro style)
- `clap` 4.5 - CLI argument parsing (polyplugc)
- `pelite` 0.10 - PE file parsing (Windows .NET hosting)
- `tree-sitter` 0.25 + `tree-sitter-lua` 0.2 - Lua source parsing
## Configuration
- Cargo workspace with unified versions via `workspace.package`
- Platform-specific Rust flags in `.cargo/config.toml` (target-cpu=native warning)
- Release profile: opt-level=3, LTO=thin, strip=symbols
- `Cargo.toml` - Workspace manifest
- `justfile` - Build automation (46KB comprehensive task runner)
- `sdk_validator.yaml` - SDK consistency rules
- `abi.toml` (in `crates/polyplug_abi/`) - ABI type definitions
## Platform Requirements
- Rust 1.85+ toolchain
- Python 3.10+ with dev headers (for Python loader)
- Lua 5.4+ dev headers (for Lua loader)
- .NET 10.0 SDK (for .NET loader and C# SDK)
- Deno 1.38.0+ (for TypeScript SDK)
- Platform-specific native libraries:
- Loader cdylibs for each runtime language
- .NET runtime 10.0 for .NET plugins
<!-- GSD:stack-end -->

<!-- GSD:conventions-start source:CONVENTIONS.md -->
## Conventions

## Naming Patterns
- Source files: `snake_case.rs` (e.g., `runtime.rs`, `plugin_registry.rs`)
- Test files: `snake_case.rs` matching test purpose (e.g., `integration_load.rs`, `stress_error.rs`)
- Benchmark files: `snake_case.rs` (e.g., `vtable_dispatch.rs`)
- Module directories: `snake_case` (e.g., `loader/`, `registry/`)
- Public functions: `snake_case` (e.g., `find_by_contract`, `load_bundle`)
- FFI exports: `polyplug_` prefix (e.g., `polyplug_runtime_create`, `polyplug_init`)
- Callbacks: descriptive `snake_case` with purpose (e.g., `capture_register_callback`, `bench_find_by_contract`)
- Test functions: `snake_case` with test prefix or descriptive name (e.g., `test_load_and_abi_version`, `stress_error_code_and_message_received_correctly`)
- Local variables: `snake_case` (e.g., `bundle_id`, `contract_id`, `registry`)
- Thread-local statics: `SCREAMING_SNAKE_CASE` (e.g., `CAPTURED_VTABLE_PTR`, `BENCH_REGISTRY`, `ERROR_REGISTRY`)
- Constants: `SCREAMING_SNAKE_CASE` (e.g., `TEST_PLUGIN_SO`, `POLYPLUG_ABI_VERSION`, `ABI_OK`)
- Structs: `PascalCase` (e.g., `Runtime`, `PluginRegistry`, `HostContext`)
- Enums: `PascalCase` with `PascalCase` variants (e.g., `RuntimeError`, `LoaderError`, `ReloadPhaseType`)
- Type aliases: `PascalCase` (e.g., `PolyplugError`, `WarningCb`, `ReloadCb`)
- Traits: `PascalCase` (e.g., `BundleLoader`, `TestAddPlugin`)
## Code Style
- No explicit rustfmt configuration detected (uses default Rust style)
- Max line length appears to follow standard Rust conventions
- Indentation: 4 spaces
- Use trailing commas in multi-line constructs
- Clippy via workspace lints in `Cargo.toml`
- Key lint rules enforced at workspace level:
- Tests suppress `expect_used` with `#![allow(clippy::expect_used)]` at crate root
- Rust edition 2024 (workspace-wide)
- Minimum Rust version: 1.85
## Import Organization
- Grouped imports with braces: `use polyplug_abi::{ABI_OK, AbiError, HostVTable};`
- Types explicitly imported, not glob-imported in public API
- Test files may use more glob imports for brevity
- Workspace dependencies use path aliases: `polyplug_utils`, `polyplug_abi`, `polyplug`
- External crate imports from workspace deps: `thiserror`, `anyhow`, `serde`, `petgraph`
## Error Handling
- Use `thiserror::Error` derive for error types
- Error enums organized by domain: `RuntimeError`, `LoaderError`, `RegistryError`, `GraphError`, `AllocatorError`, `HostContractError`
- Error variants use structured fields with descriptive messages:
- Type alias for top-level error: `pub type PolyplugError = RuntimeError;`
- Error chaining via `#[error(transparent)]` and `#[from]`:
- FFI boundary stores errors in `last_error: Mutex<String>` field, retrieved via `get_last_error()`
- Public API returns `Result<T, PolyplugError>` or domain-specific error
- Internal functions may use domain-specific errors
- FFI functions return error codes (`u32`), with messages in thread-local storage
- All FFI exports wrapped in `std::panic::catch_unwind` to prevent panics crossing ABI boundary
- ABI error codes defined in `polyplug_abi`: `ABI_OK = 0`, `ABI_ERROR_GENERIC = 1`, `ABI_ERROR_PANIC = 3`
- `AbiError` struct: `{ code: u32, message: StringView }`
## Async Patterns
- No async runtime used (fully synchronous design)
- Plugin dispatch is direct pointer dereference (zero overhead)
- Thread-safe via `RwLock`, `Mutex`, and `Arc`
- `#[inline(always)]` on hot-path functions like `find_by_contract`
- `Arc<PluginRegistry>` for shared registry
- `RwLock<HashMap>` for host contracts
- `Mutex<HashMap>` for bundle manifests
- `Mutex<String>` for last error storage
- Thread-local `RefCell` for test state capture
## Memory Management
- `Arc<T>` for shared ownership across threads (e.g., `Arc<PluginRegistry>`, `Arc<VTableSlot>`)
- `Box<dyn BundleLoader>` for trait object ownership
- `Box::leak()` for static vtable references in tests
- `core::mem::forget(library)` to prevent `dlclose` on loaded plugin libraries
- `Arc` for shared, thread-safe ownership
- `Mutex` and `RwLock` for interior mutability
- `RefCell` for thread-local test state
- Raw pointers for FFI boundary (e.g., `*const PluginInterface`, `*mut c_void`)
- Host allocator functions: `polyplug_host_alloc(size, align)` and `polyplug_host_free(ptr, size, align)`
- Plugin-allocated strings must be freed by caller after reading
- Static string views (`from_static`) must NOT be freed
- Vtables are `'static` (live for process lifetime after registration)
- `PluginGuard` provides scoped access with RAII pattern
- FFI callbacks receive `*const` pointers with explicit lifetime documentation
## Documentation Patterns
- Every module has `//!` doc comment describing purpose
- Example from `crates/polyplug/src/runtime.rs`:
- Public functions have `///` doc comments with purpose
- `# Safety` section required for all unsafe functions (enforced by clippy lint)
- Example safety documentation:
- `// SAFETY:` comments before every unsafe block explaining why it's safe
- `// ---` section separators for logical grouping in long files
- Comment blocks use `// ───` Unicode dashes for visual sectioning
- Test files have `//!` module doc explaining test purpose
- Test functions have `///` doc comments describing what is tested
- Step-by-step comments in complex tests (e.g., `// ── Step 1: Locate workspace root`)
## Comments
- Every unsafe operation requires a `// SAFETY:` comment
- Complex FFI interactions need step-by-step comments
- Non-obvious business logic (e.g., dependency enforcement rules)
- Build.rs script logic with path resolution explanations
- Not applicable (Rust project)
- Use `///` for rustdoc
- Use Unicode dash separators: `// ─── Section Name ───────────────────────`
- Example pattern:
## Function Design
- Functions typically under 50 lines for clarity
- Complex functions broken into helper functions
- Hot-path functions marked `#[inline(always)]` for zero overhead
- Prefer references over owned values when not transferring ownership
- Use `&Path` for file paths
- Use struct args for multi-parameter FFI calls (e.g., `AddArgs`, `ChainArgs`)
- FFI uses raw pointers: `*const T`, `*mut T`
- `Result<T, E>` for fallible operations
- `Option<T>` for nullable results
- `AbiError` for FFI boundary returns
- Handle packing for FFI: `u64` packed from `PluginHandle { index, generation }`
## Module Design
- Public API exported from `lib.rs` via `pub use`
- Internal modules marked `pub(crate)` or private
- Re-export convenience types: `pub use reload::ReloadEvent;`
- `lib.rs` acts as barrel file for crate exports
- Submodules have their own `mod.rs` or direct file structure
- Example pattern:
- `pub` for public API
- `pub(crate)` for internal shared functions
- `pub(super)` or `pub(in path)` for restricted visibility
- Private by default for implementation details
<!-- GSD:conventions-end -->

<!-- GSD:architecture-start source:ARCHITECTURE.md -->
## Architecture

## System Overview
- Single API for all supported languages
- Direct function pointer dispatch (zero overhead for native, minimal for VM-based)
- Hot-reload with quiescence-based safety
- Type-safe code generation for all bindings
- Cross-platform support (Linux, macOS, Windows)
## Pattern Overview
- Trait-based loader abstraction (`BundleLoader`) for multi-language support
- Generational index pattern for safe handle management (stale handle detection)
- `ArcSwap` for atomic vtable swapping during hot-reload
- Two-phase lifecycle: initialization (single-threaded, graph-based load order) then runtime (lock-free dispatch)
- Code generation via `polyplugc` CLI for type-safe bindings
## Layers
- Purpose: FFI entry points for host language bindings
- Location: `crates/polyplug/src/ffi.rs`
- Contains: `#[no_mangle]` C ABI functions (`polyplug_runtime_create`, `polyplug_runtime_load_bundle`, etc.)
- Depends on: Runtime, Registry
- Used by: SDKs in `sdks/*/host/`
- Purpose: Plugin lifecycle management, loader coordination, hot-reload
- Location: `crates/polyplug/src/runtime.rs`, `runtime_builder.rs`, `reload.rs`
- Contains: `Runtime` struct, `RuntimeBuilder` pattern, reload orchestration
- Depends on: Registry, Loaders, Compatibility graph
- Used by: FFI layer, Host SDKs
- Purpose: VTable storage, handle validation, contract lookup
- Location: `crates/polyplug/src/registry/plugin_registry.rs`
- Contains: `PluginRegistry` with generational slots, `PluginGuard` for RAII vtable access
- Depends on: ABI types (`PluginInterface`, `PluginHandle`)
- Used by: Runtime, Host callbacks
- Purpose: Language-specific bundle loading and initialization
- Location: `crates/polyplug_native/src/loader.rs`, `crates/polyplug_python/src/lib.rs`, `crates/polyplug_js/src/loader.rs`, `crates/polyplug_lua/src/loader.rs`, `crates/polyplug_dotnet/src/lib.rs`
- Contains: `NativeLoader`, `PythonLoader`, `JsLoader`, `LuaLoader`, `DotnetLoader` implementing `BundleLoader`
- Depends on: Runtime (for registration), language-specific VMs (pyo3, rquickjs, mlua, netcorehost)
- Used by: Runtime during `load_bundle()` and `reload_bundle()`
- Purpose: C-compatible type definitions for host/plugin boundary
- Location: `crates/polyplug_abi/src/`
- Contains: `PluginInterface`, `HostVTable`, `PluginHandle`, `StringView`, `Buffer`, `AbiError`, `DispatchType`
- Depends on: No internal dependencies (standalone)
- Used by: All layers crossing FFI boundary
- Purpose: Generate type-safe host/guest bindings from API definitions
- Location: `crates/polyplugc/src/`, `crates/polyplug_codegen/src/`
- Contains: Parser for `api.toml`/`bundle.toml`, IR validation, per-language generators
- Depends on: polyplug_abi types for contract ID hashing
- Used by: Plugin developers via `polyplugc generate` CLI
## Data Flow
## Key Abstractions
- Purpose: Abstract loader interface for all language runtime types
- Examples: `crates/polyplug_native/src/loader.rs:166-244`, `crates/polyplug_python/src/lib.rs:62-248`
- Pattern: Trait with `runtime_name()`, `load()`, `reload()` methods
- Purpose: Function dispatch table registered by plugins
- Examples: `crates/polyplug_abi/src/plugin/plugin_interface.rs`
- Pattern: `#[repr(C)]` struct with `contract_id`, `contract_version`, `function_count`, `dispatch_type`, dispatch union
- Purpose: Safe handle to plugin vtable with stale detection
- Examples: `crates/polyplug_abi/src/plugin/plugin_handle.rs`
- Pattern: `{ index: u32, generation: u32 }` - validated against slot generation on each resolve
- Purpose: Host capabilities exposed to plugins during init
- Examples: `crates/polyplug_abi/src/host/host_vtable/host_vtable.rs`
- Pattern: Function pointers for `register_plugin`, `alloc`, `free`, `find_by_contract`, `resolve_plugin`
- Purpose: Dependency resolution for load ordering
- Examples: `crates/polyplug/src/compatibility/capability_graph.rs`
- Pattern: Directed graph with petgraph, cycle detection, topological sort
- Purpose: Reference-counted vtable access for hot-reload safety
- Examples: `crates/polyplug/src/registry/plugin_registry.rs` (internal)
- Pattern: Wraps `Arc<VTableSlot>`, provides `vtable()` method, dropped after call
## Entry Points
- Location: `crates/polyplug/src/runtime.rs:99-103`
- Triggers: `Runtime::builder().build()`
- Responsibilities: Creates runtime, registers loaders, loads bundles
- Location: `crates/polyplug/src/ffi.rs:135-526`
- Triggers: C ABI calls from Python/C#/Lua/JS SDKs
- Responsibilities: `polyplug_runtime_create`, `polyplug_runtime_load_bundle`, `polyplug_runtime_find_by_contract`, etc.
- Location: Required symbol in plugin binary
- Triggers: Loader after dlopen/import
- Responsibilities: Register vtables via `host_vtable.register_plugin`
- Location: `crates/polyplugc/src/lib.rs:15-56`
- Triggers: CLI `polyplugc generate --bundle bundle.toml --lang rust --out src/generated`
- Responsibilities: Parse API, validate IR, generate host/guest bindings
## Error Handling
- `RuntimeError` as top-level enum with variants for `Loader`, `Registry`, `Graph`, `Allocator`, `HostContract`
- Each variant has detailed context (bundle name, contract ID, version mismatch)
- FFI functions store errors in per-runtime `last_error: Mutex<String>` buffer
- All FFI entry points wrapped in `catch_unwind` to prevent panics crossing ABI
- `LoaderError`: Init failures, missing symbols, version mismatches, VM-specific errors
- `RegistryError`: Stale handles, contract collisions, duplicate providers
- `GraphError`: Dependency cycles, unsatisfied capabilities
- `HostContractError`: Duplicate/missing host contracts
## Cross-Cutting Concerns
- Manifest parsing with required fields (`id`, `name`, `runtime`, `file`)
- Version compatibility checks with `Compatibility::Strict/Relaxed/Yolo` modes
- Function count validation against manifest `function_count` entries
- Bundle ID tampering detection via `HostContext.bundle_id` verification
- Bundle ID enforcement prevents plugins from accessing undeclared dependencies
- ABI version sentinel (`POLYPLUG_ABI_VERSION`) rejects mismatched plugins
- Panic isolation via `catch_unwind` at every FFI boundary
- Host allocator (`polyplug_host_alloc`, `polyplug_host_free`) for all cross-boundary memory
- `Buffer` type owns memory via host allocator
- `StringView` is non-owning borrow (caller responsible for lifetime)
- Registry uses `RwLock` for registration (rare) and read guards for dispatch (common)
- `ArcSwap` for atomic vtable swaps during hot-reload
- `Mutex` for loader-internal library handles
- TLS for init-phase bundle context (dependency enforcement)
<!-- GSD:architecture-end -->

<!-- GSD:workflow-start source:GSD defaults -->
## GSD Workflow Enforcement

Before using Edit, Write, or other file-changing tools, start work through a GSD command so planning artifacts and execution context stay in sync.

Use these entry points:
- `/gsd:quick` for small fixes, doc updates, and ad-hoc tasks
- `/gsd:debug` for investigation and bug fixing
- `/gsd:execute-phase` for planned phase work

Do not make direct repo edits outside a GSD workflow unless the user explicitly asks to bypass it.
<!-- GSD:workflow-end -->



<!-- GSD:profile-start -->
## Developer Profile

> Profile not yet configured. Run `/gsd:profile-user` to generate your developer profile.
> This section is managed by `generate-claude-profile` -- do not edit manually.
<!-- GSD:profile-end -->
