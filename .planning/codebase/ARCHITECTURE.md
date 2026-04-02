# Architecture

**Analysis Date:** 2026-04-02

## System Overview

**Purpose:** polyplug is a high-performance, zero/minimal-overhead cross-language plugin runtime. It enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

**Problem Solved:** Traditional plugin systems require language-specific bindings, manual FFI management, and have significant runtime overhead. polyplug provides:
- Single API for all supported languages
- Direct function pointer dispatch (zero overhead for native, minimal for VM-based)
- Hot-reload with quiescence-based safety
- Type-safe code generation for all bindings
- Cross-platform support (Linux, macOS, Windows)

## Pattern Overview

**Overall:** Plugin Runtime with Generational Registry

**Key Characteristics:**
- Trait-based loader abstraction (`BundleLoader`) for multi-language support
- Generational index pattern for safe handle management (stale handle detection)
- `ArcSwap` for atomic vtable swapping during hot-reload
- Two-phase lifecycle: initialization (single-threaded, graph-based load order) then runtime (lock-free dispatch)
- Code generation via `polyplugc` CLI for type-safe bindings

## Layers

**Host Integration Layer:**
- Purpose: FFI entry points for host language bindings
- Location: `crates/polyplug/src/ffi.rs`
- Contains: `#[no_mangle]` C ABI functions (`polyplug_runtime_create`, `polyplug_runtime_load_bundle`, etc.)
- Depends on: Runtime, Registry
- Used by: SDKs in `sdks/*/host/`

**Runtime Core Layer:**
- Purpose: Plugin lifecycle management, loader coordination, hot-reload
- Location: `crates/polyplug/src/runtime.rs`, `runtime_builder.rs`, `reload.rs`
- Contains: `Runtime` struct, `RuntimeBuilder` pattern, reload orchestration
- Depends on: Registry, Loaders, Compatibility graph
- Used by: FFI layer, Host SDKs

**Registry Layer:**
- Purpose: VTable storage, handle validation, contract lookup
- Location: `crates/polyplug/src/registry/plugin_registry.rs`
- Contains: `PluginRegistry` with generational slots, `PluginGuard` for RAII vtable access
- Depends on: ABI types (`PluginInterface`, `PluginHandle`)
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
- Contains: `PluginInterface`, `HostVTable`, `PluginHandle`, `StringView`, `Buffer`, `AbiError`, `DispatchType`
- Depends on: No internal dependencies (standalone)
- Used by: All layers crossing FFI boundary

**Code Generation Layer:**
- Purpose: Generate type-safe host/guest bindings from API definitions
- Location: `crates/polyplugc/src/`, `crates/polyplug_codegen/src/`
- Contains: Parser for `api.toml`/`bundle.toml`, IR validation, per-language generators
- Depends on: polyplug_abi types for contract ID hashing
- Used by: Plugin developers via `polyplugc generate` CLI

## Data Flow

**Plugin Loading Flow:**

1. Host calls `Runtime::builder().loader(...).build()` or FFI `polyplug_runtime_create`
2. `RuntimeBuilder::build()` scans plugin directories via `scanner::scan_dirs`
3. Parses manifests (`manifest.toml`) to get `ManifestData` for each bundle
4. Builds `CapabilityGraph` from manifests (providers + dependencies)
5. Validates version compatibility (`validate_bundle_compatibility`)
6. Computes topological load order (providers before dependents)
7. For each bundle in order: dispatches to matching `BundleLoader` via `runtime_name`
8. Loader calls plugin's `polyplug_init(rt_ctx, host_vtable, ctx)`
9. Plugin calls `host_vtable.register_plugin` to register vtables
10. Registry stores vtables with generational handles

**Plugin Dispatch Flow:**

1. Host calls `runtime.find_by_contract(contract_id, min_version)`
2. Registry returns `PluginHandle` (index + generation)
3. Host calls `runtime.resolve_plugin(handle)` or FFI `polyplug_runtime_resolve_plugin`
4. Registry validates generation, returns `PluginGuard` wrapping `Arc<VTableSlot>`
5. Host calls generated contract caller method (e.g., `decoder.decode(input)`)
6. Caller dereferences vtable function pointer, invokes with args
7. Plugin executes, returns result via ABI types (`StringView`, `Buffer`, `AbiError`)
8. Guard dropped, Arc reference released

**Hot-Reload Flow:**

1. Host calls `runtime.reload_bundle(path)` (triggered by file watcher or manual)
2. Runtime fires `ReloadPhase::Preparing` callback
3. Loader loads new library/VM code, calls `polyplug_init`
4. Registry atomically swaps vtables via `swap_vtable` (`ArcSwap`)
5. Runtime waits for quiescence (`wait_for_quiescence`) using Arc strong_count
6. Runtime fires `ReloadPhase::Reloaded` callback
7. Host releases cached raw pointers (CRITICAL safety step)
8. Loader drops old library (native) or lets GC clean up (VM-based)

## Key Abstractions

**`BundleLoader` Trait:**
- Purpose: Abstract loader interface for all language runtime types
- Examples: `crates/polyplug_native/src/loader.rs:166-244`, `crates/polyplug_python/src/lib.rs:62-248`
- Pattern: Trait with `runtime_name()`, `load()`, `reload()` methods

**`PluginInterface` VTable:**
- Purpose: Function dispatch table registered by plugins
- Examples: `crates/polyplug_abi/src/plugin/plugin_interface.rs`
- Pattern: `#[repr(C)]` struct with `contract_id`, `contract_version`, `function_count`, `dispatch_type`, dispatch union

**`PluginHandle` Generational Index:**
- Purpose: Safe handle to plugin vtable with stale detection
- Examples: `crates/polyplug_abi/src/plugin/plugin_handle.rs`
- Pattern: `{ index: u32, generation: u32 }` - validated against slot generation on each resolve

**`HostVTable` Callback Table:**
- Purpose: Host capabilities exposed to plugins during init
- Examples: `crates/polyplug_abi/src/host/host_vtable/host_vtable.rs`
- Pattern: Function pointers for `register_plugin`, `alloc`, `free`, `find_by_contract`, `resolve_plugin`

**`CapabilityGraph`:**
- Purpose: Dependency resolution for load ordering
- Examples: `crates/polyplug/src/compatibility/capability_graph.rs`
- Pattern: Directed graph with petgraph, cycle detection, topological sort

**`PluginGuard` RAII Wrapper:**
- Purpose: Reference-counted vtable access for hot-reload safety
- Examples: `crates/polyplug/src/registry/plugin_registry.rs` (internal)
- Pattern: Wraps `Arc<VTableSlot>`, provides `vtable()` method, dropped after call

## Entry Points

**Rust Host Entry:**
- Location: `crates/polyplug/src/runtime.rs:99-103`
- Triggers: `Runtime::builder().build()`
- Responsibilities: Creates runtime, registers loaders, loads bundles

**FFI Entry Points:**
- Location: `crates/polyplug/src/ffi.rs:135-526`
- Triggers: C ABI calls from Python/C#/Lua/JS SDKs
- Responsibilities: `polyplug_runtime_create`, `polyplug_runtime_load_bundle`, `polyplug_runtime_find_by_contract`, etc.

**Plugin Entry Point:**
- Location: Required symbol in plugin binary
- Triggers: Loader after dlopen/import
- Responsibilities: Register vtables via `host_vtable.register_plugin`

**Code Generation Entry:**
- Location: `crates/polyplugc/src/lib.rs:15-56`
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
- `RegistryError`: Stale handles, contract collisions, duplicate providers
- `GraphError`: Dependency cycles, unsatisfied capabilities
- `HostContractError`: Duplicate/missing host contracts

## Cross-Cutting Concerns

**Logging:** Host-provided warning callback via `RuntimeBuilder::on_warning`, falls back to stderr

**Validation:** 
- Manifest parsing with required fields (`id`, `name`, `runtime`, `file`)
- Version compatibility checks with `Compatibility::Strict/Relaxed/Yolo` modes
- Function count validation against manifest `function_count` entries
- Bundle ID tampering detection via `HostContext.bundle_id` verification

**Authentication/Security:** 
- Bundle ID enforcement prevents plugins from accessing undeclared dependencies
- ABI version sentinel (`POLYPLUG_ABI_VERSION`) rejects mismatched plugins
- Panic isolation via `catch_unwind` at every FFI boundary

**Memory Management:**
- Host allocator (`polyplug_host_alloc`, `polyplug_host_free`) for all cross-boundary memory
- `Buffer` type owns memory via host allocator
- `StringView` is non-owning borrow (caller responsible for lifetime)

**Concurrency:**
- Registry uses `RwLock` for registration (rare) and read guards for dispatch (common)
- `ArcSwap` for atomic vtable swaps during hot-reload
- `Mutex` for loader-internal library handles
- TLS for init-phase bundle context (dependency enforcement)

---

*Architecture analysis: 2026-04-02*