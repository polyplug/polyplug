# External Integrations

**Analysis Date:** 2026-04-02

## APIs & External Services

**None detected.** This is a standalone plugin runtime library with no external API integrations.

## Data Storage

**Databases:**
- None. The runtime is in-memory only.

**File Storage:**
- Local filesystem only - plugin bundles loaded from paths
- Manifest files (`manifest.toml`) alongside plugin binaries

**Caching:**
- None. In-memory plugin registry with atomic hot-reload.

## Language Runtime Integrations

**Python Integration:**
- SDK: `pyo3` crate (Rust-Python bindings)
- Host: Python 3.10+ as host language via `sdks/python/host/`
- Guest: Python plugins loaded via `polyplug_python` loader
- ABI: `sdks/python/abi/abi.py`

**Lua Integration:**
- SDK: `mlua` crate with LuaJIT vendored, send feature
- Host: Lua as host language via `sdks/lua/host/`
- Guest: Lua plugins loaded via `polyplug_lua` loader
- ABI: `sdks/lua/abi/abi.lua` (FFI-based)

**JavaScript Integration:**
- SDK: `rquickjs` crate (QuickJS engine embedded)
- Host: TypeScript/Deno as host language via `sdks/js/host/`
- Guest: JS plugins loaded via `polyplug_js` loader
- ABI: `sdks/js/abi/abi.ts`

**.NET Integration:**
- SDK: `netcorehost` crate (nethost, net10_0 feature)
- Host: C#/.NET as host language via `sdks/csharp/host/`
- Guest: .NET assemblies loaded via `polyplug_dotnet` loader
- ABI: `sdks/csharp/abi/Abi.cs`
- Feature: `download-nethost` optional feature for automatic nethost download

**Native (C ABI) Integration:**
- SDK: `libloading` crate
- Host: Any language with C FFI support
- Guest: `.so/.dll/.dylib` plugins loaded via `polyplug_native` loader
- ABI: `sdks/cpp/abi/polyplug/abi.hpp` (header-only)

## FFI & ABI Protocols

**Primary Protocol:**
- C ABI (`extern "C"`, `#[no_mangle]`, `#[repr(C)]`)
- All cross-language communication via FFI boundary
- Panic safety: `catch_unwind` at every FFI entry point

**Core FFI Functions (12 entry points in `ffi.rs`):**
- `polyplug_runtime_create` - Create runtime instance
- `polyplug_runtime_create_with_options` - Create with config
- `polyplug_runtime_destroy` - Destroy runtime
- `polyplug_runtime_load_bundle` - Load plugin bundle
- `polyplug_runtime_reload_bundle` - Hot-reload bundle
- `polyplug_runtime_find_by_contract` - Find plugin by contract
- `polyplug_runtime_find_by_bundle` - Find plugin by bundle
- `polyplug_runtime_find_all_by_contract` - Find all providers
- `polyplug_runtime_resolve_plugin` - Resolve handle to vtable
- `polyplug_runtime_release_plugin` - Release resolved handle
- `polyplug_runtime_last_error` - Get error message
- `polyplug_runtime_register_loader` - Register language loader
- `polyplug_runtime_register_host_contract` - Register host contract

**ABI Type Definitions (`polyplug_abi/src/`):**
- `types.rs` - StringView, AbiError, PluginHandle
- `plugin.rs` - PluginDescriptor, PluginInterface, DispatchType
- `host/host_vtable.rs` - HostVTable callbacks
- `dispatch/` - Native vs VM dispatch structures
- `tracking.rs` - Contract ID hashing (FNV-1a)

**Dispatch Types:**
- `Native` - Direct function pointer calls (C ABI)
- `VirtualMachine` - Indirect calls through VM bridge (Python, Lua, JS)

## Internal SDK Architecture

**SDK Packages per Language:**
- `abi` - Shared ABI definitions (auto-generated)
- `host` - Host-side runtime bindings
- `guest` - Guest-side plugin author bindings
- `loaders/` - Optional loader packages (Python, Lua, JS, Dotnet, Native)

**Host Libraries:**
- `sdks/rust/host/` - `polyplug_host` crate
- `sdks/rust/guest/` - `polyplug_guest` crate
- `sdks/csharp/host/Polyplug.Host.csproj` - C# host (embeds native libs)
- `sdks/python/host/pyproject.toml` - Python host package
- `sdks/js/package.json` - TypeScript SDK (Deno module)
- `sdks/lua/host/` - Lua host library
- `sdks/cpp/host/` - C++ header-only host library

## Authentication & Identity

**Not applicable.** Plugin runtime with no authentication requirements.

## Monitoring & Observability

**Error Tracking:**
- Per-runtime error storage via `Mutex<String>`
- FFI error retrieval: `polyplug_runtime_last_error`
- ABI error codes: `ABI_OK = 0`, `ABI_ERROR_GENERIC = 1`, `ABI_ERROR_PANIC = 3`

**Logs:**
- Warning callback system: `emit_warning()` 
- stderr fallback for warnings
- Hot-reload phase callbacks: `ReloadPhase::Preparing/Reloaded/Failed`

## CI/CD & Deployment

**Hosting:**
- Native shared libraries embedded in SDK packages
- Release artifacts for 4 platforms: linux-x64, macos-x64, macos-arm64, windows-x64

**CI Pipeline:**
- GitHub Actions (`ci.yml`, `release.yml`)
- Jobs: fmt, clippy, sdk_validator, test, external-toolchains, test-download-nethost
- External toolchain matrix: dotnet, python, lua, js-quickjs

**Release Workflow:**
- Triggered by `v*` tags
- Multi-platform native library builds
- SDK consistency validation via `sdk_validator` + `ast-grep`
- CLI tool (`polyplugc`) built for all platforms

## Webhooks & Callbacks

**Incoming:**
- Hot-reload notification callback: `extern "C" fn(ReloadPhaseC)`
- Warning callback: `Fn(&str) + Send + Sync`

**Outgoing:**
- None. Pure library with no network communication.

## Build-Time Code Generation

**polyplugc CLI:**
- Generates type-safe bindings from ABI definitions
- Multi-language output: Rust, C#, Python, Lua, TypeScript
- Input: `abi.toml` type definitions
- Location: `crates/polyplugc/`

**SDK Validator:**
- Uses `ast-grep` for cross-language SDK consistency
- Config: `sdk_validator.yaml`
- Ensures ABI parity across all language SDKs

---

*Integration audit: 2026-04-02*