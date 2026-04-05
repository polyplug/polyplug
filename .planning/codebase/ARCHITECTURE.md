# Architecture

**Analysis Date:** 2026-04-05 (updated from 2026-04-02)

## System Overview

**Purpose:** polyplug is a high-performance, zero/minimal-overhead cross-language plugin runtime. It enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

**Problem Solved:** Traditional plugin systems require language-specific bindings, manual FFI management, and have significant runtime overhead. polyplug provides:
- Single API for all supported languages
- Direct function pointer dispatch (zero overhead for native, minimal for VM-based)
- Hot-reload with callback-based instance safety
- Type-safe code generation for all bindings
- Cross-platform support (Linux, macOS, Windows)

## Pattern Overview

**Overall:** Plugin Runtime with Instance-Based Model

**Key Characteristics:**
- Trait-based loader abstraction (`BundleLoader`) for multi-language support
- Simple index-based handles (no generation counter - safety via callback contract)
- Direct `Arc<GuestContractInterface>` storage (no wrapper)
- Two-phase lifecycle: initialization (single-threaded, graph-based load order) then runtime (lock-free dispatch)
- Code generation via `polyplugc` CLI for type-safe bindings

## Layers

**FFI Layer:**
- Purpose: C ABI entry points for host language bindings
- Location: `crates/polyplug/src/ffi.rs`
- Contains: `#[no_mangle]` C ABI functions (`polyplug_runtime_create`, `polyplug_runtime_load_bundle`, etc.)
- Depends on: Runtime, Registry
- Used by: SDKs in `sdks/*/host/`

**Runtime Core Layer:**
- Purpose: Plugin lifecycle management, loader coordination, hot-reload
- Location: `crates/polyplug/src/runtime.rs`, `runtime_builder.rs`, `reload.rs`
- Contains: `Runtime` struct, `RuntimeBuilder` pattern, reload orchestration
- Depends on: Registry, Loaders, Capability graph
- Used by: FFI layer, Host SDKs

**Registry Layer:**
- Purpose: Interface storage, handle validation, contract lookup
- Location: `crates/polyplug/src/registry/plugin_registry.rs`
- Contains: `PluginRegistry` with index-based slots, direct `Arc<GuestContractInterface>` storage
- Depends on: ABI types (`GuestContractInterface`, `PluginHandle`)
- Used by: Runtime, Host callbacks

**Loader Layer:**
- Purpose: Language-specific bundle loading and initialization
- Location: `crates/polyplug_native/src/loader.rs`, `crates/polyplug_python/src/lib.rs`, `crates/polyplug_js/src/loader.rs`, `crates/polyplug_lua/src/loader.rs`, `crates/polyplug_dotnet/src/lib.rs`
- Contains: `NativeLoader`, `PythonLoader`, `JsLoader`, `LuaLoader`, `DotnetLoader` implementing `BundleLoader`
- Depends on: Runtime (for registration), language-specific VMs (pyo3, rquickjs, mlua, netcorehost)
- Used by: Runtime during `load_bundle()` and `reload_bundle()`

**ABI Layer:**
- Purpose: C-compatible type definitions for host/plugin boundary
- Location: `crates/polyplug_abi/src/`
- Contains: `GuestContractInterface`, `HostContractInterface`, `RuntimeAbi`, `PluginHandle`, `StringView`, `Buffer`, `AbiError`, `DispatchType`
- Depends on: No internal dependencies (standalone)
- Used by: All layers crossing FFI boundary

**Code Generation Layer:**
- Purpose: Generate type-safe host/guest bindings from API definitions
- Location: `crates/polyplugc/src/`, `crates/polyplug_codegen/src/`
- Contains: Parser for `api.toml`/`bundle.toml`, IR validation, per-language generators
- Depends on: polyplug_abi types for contract ID hashing
- Used by: Plugin developers via `polyplugc generate` CLI

## Data Flow

### Plugin Loading Flow

1. Host calls `Runtime::builder().loader(...).build()` or FFI `polyplug_runtime_create`
2. `RuntimeBuilder::build()` scans plugin directories via `scanner::scan_dirs`
3. Parses manifests (`manifest.toml`) to get `ManifestData` for each bundle
4. Builds `CapabilityGraph` from manifests (providers + dependencies)
5. Validates version compatibility (`validate_bundle_compatibility`)
6. Computes topological load order (providers before dependents)
7. For each bundle in order: dispatches to matching `BundleLoader` via `runtime_name`
8. Loader calls plugin's `polyplug_init(rt_ctx, abi, ctx)`
9. Plugin calls `abi.register_contract` to register interfaces
10. Registry stores interfaces with index-based handles

### Plugin Dispatch Flow

1. Host calls `runtime.find_by_contract(contract_id, min_version)`
2. Registry returns `ContractHandle` (index only)
3. Host calls `runtime.resolve_contract(handle)` or FFI `polyplug_runtime_resolve_plugin`
4. Registry returns `&GuestContractInterface`
5. Host calls `interface.create_instance(rt_ctx)` → `GuestContractInstance`
6. Host dispatches through interface (native pointers or VM call)
7. Plugin executes, returns result via ABI types
8. Host calls `interface.destroy_instance(rt_ctx, instance)` when done

### Hot-Reload Flow

1. Host calls `runtime.reload_bundle(path)` (triggered by file watcher or manual)
2. Runtime fires `ReloadPhase::Preparing` callback
3. **Host MUST destroy all instances in this callback** (safety contract)
4. Loader loads new library/VM code, calls `polyplug_init`
5. Registry atomically swaps interfaces
6. Runtime fires `ReloadPhase::Reloaded` callback
7. Host can create new instances from new interfaces

## Key Abstractions

### `BundleLoader` Trait
- Purpose: Abstract loader interface for all language runtime types
- Location: `crates/polyplug/src/loader/bundle_loader.rs`
- Pattern: Trait with `runtime_name()`, `load()`, `reload()` methods

### `GuestContractInterface`
- Purpose: Function dispatch table registered by plugins
- Location: `crates/polyplug_abi/src/guest/guest_contract_interface.rs`
- Pattern: `#[repr(C)]` struct with:
  - `contract_id`, `contract_version`
  - `dispatch_type`, `dispatch` (Native or VM)
  - `create_instance`, `destroy_instance` factory functions

### `HostContractInterface`
- Purpose: Host-provided services to plugins
- Location: `crates/polyplug_abi/src/host/host_contract_interface.rs`
- Pattern: `#[repr(C)]` struct with:
  - `contract_id`, `contract_version`, `singleton` flag
  - `dispatch_type`, `dispatch`
  - `create_instance`, `destroy_instance`

### `PluginHandle`
- Purpose: Simple handle to registry slot
- Location: `crates/polyplug_abi/src/plugin/plugin_handle.rs`
- Pattern: `{ index: u32 }` - no generation counter (safety via callback contract)

### `RuntimeAbi`
- Purpose: Host capabilities exposed to plugins during init
- Location: `crates/polyplug_abi/src/host/runtime_abi.rs`
- Pattern: Function pointers for `register_contract`, `alloc`, `free`, `find_contract`, `resolve_contract`, `get_host_contract`, `call_method`

### `CapabilityGraph`
- Purpose: Dependency resolution for load ordering
- Location: `crates/polyplug/src/compatibility/capability_graph.rs`
- Pattern: Directed graph with petgraph, cycle detection, topological sort

## Instance Model

### Guest Contracts (Plugins)

```rust
// Created by host
let instance = interface.create_instance(rt_ctx);

// Passed as first arg to all dispatch calls
dispatch(instance, fn_id, args, out);

// Destroyed by host
interface.destroy_instance(rt_ctx, instance);
```

### Host Contracts (Host Services)

```rust
// Singleton: same instance every time
let instance = runtime.get_host_contract(contract_id, 0);

// Multi-instance: new instance each call
// Caller owns and must destroy
```

### Safety Contract

- **Before hot-reload**: Host MUST destroy all instances in `ReloadPhase::Preparing` callback
- **After hot-reload**: Host can create new instances from new interfaces
- **Leaked instances**: Undefined behavior if used after reload

## Entry Points

**Rust Host Entry:**
- Location: `crates/polyplug/src/runtime.rs`
- Triggers: `Runtime::builder().build()`
- Responsibilities: Creates runtime, registers loaders, loads bundles

**FFI Entry Points:**
- Location: `crates/polyplug/src/ffi.rs`
- Triggers: C ABI calls from Python/C#/Lua/JS SDKs
- Responsibilities: `polyplug_runtime_create`, `polyplug_runtime_load_bundle`, `polyplug_runtime_find_by_contract`, etc.

**Plugin Entry Point:**
- Location: Required symbol in plugin binary
- Triggers: Loader after dlopen/import
- Responsibilities: Register interfaces via `abi.register_contract`

**Code Generation Entry:**
- Location: `crates/polyplugc/src/lib.rs`
- Triggers: CLI `polyplugc generate --bundle bundle.toml --lang rust --out src/generated`
- Responsibilities: Parse API, validate IR, generate host/guest bindings

## Error Handling

**Strategy:** Typed error hierarchy with thiserror

**Patterns:**
- `RuntimeError` as top-level enum with variants for `Loader`, `Registry`, `Graph`, `Allocator`, `HostContract`
- Each variant has detailed context (bundle name, contract ID, version mismatch)
- FFI functions store errors in per-runtime `last_error: Mutex<String>` buffer
- All FFI entry points wrapped in `catch_unwind` to prevent panics crossing ABI

**Error Categories:**
- `LoaderError`: Init failures, missing symbols, version mismatches, VM-specific errors
- `RegistryError`: Invalid handles, contract collisions, duplicate providers
- `GraphError`: Dependency cycles, unsatisfied capabilities
- `HostContractError`: Duplicate/missing host contracts

## Cross-Cutting Concerns

**Validation:**
- Manifest parsing with required fields (`id`, `name`, `runtime`, `file`)
- Version compatibility checks with `Compatibility::Strict/Relaxed/Yolo` modes
- Function count validation against manifest `function_count` entries
- Bundle ID enforcement prevents plugins from accessing undeclared dependencies
- ABI version sentinel (`POLYPLUG_ABI_VERSION`) rejects mismatched plugins

**Memory Management:**
- Host allocator (`polyplug_host_alloc`, `polyplug_host_free`) for all cross-boundary memory
- `Buffer` type owns memory via host allocator
- `StringView` is non-owning borrow (caller responsible for lifetime)
- Interfaces are `'static` (live for process lifetime after registration)

**Concurrency:**
- Registry uses `RwLock` for registration (rare) and read guards for dispatch (common)
- `Mutex` for loader-internal library handles
- TLS for init-phase bundle context (dependency enforcement)

---
*Architecture analysis: 2026-04-05 (updated)*