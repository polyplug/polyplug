# polyplug — PRD v3

## Table of Contents

1. Vision
2. Actors
3. Core Design Principles
4. High-Level Architecture
5. Runtime Core
6. ABI Layer
7. VTable System
8. Host Libraries
9. Guest Libraries
10. Language Runtime Adapters
11. Schema Files
12. Code Generation Pipeline
13. Plugin Discovery
14. Cross-Plugin Communication
15. Memory Model
16. Error Handling
17. Plugin Versioning and Compatibility
18. Extension System
19. Security Model
20. Developer Experience — App Developer
21. Developer Experience — Plugin Developer
22. The Full Runtime Flow
23. MVP Language Support
24. Package Ecosystem
25. Future Work
26. Non-Goals

---

## 1. Vision

Build the **universal plugin system**. A schema-driven, cross-language plugin runtime platform where any language can be a host and any language can be a guest — with zero performance penalty regardless of the combination.

The north star use case is game engines: a game engine written in C++ should be able to load plugins written in Rust, C#, Python, or Lua at native speed, through a single unified system, without any language-specific special casing.

The platform is designed around one principle above all others: **performance over everything**. The hot path — calling a plugin function — must compile down to a single indirect function call. Nothing more.

---

## 2. Actors

Three distinct actors interact with this platform.

**YOU — the runtime author**
Builds and maintains the runtime core, language adapters, host libs, guest libs, and the codegen CLI. Ships no schema files. Everything lives in source code.

**APP DEVELOPER — embeds the runtime**
A developer who wants to add a plugin system to their application. They define the plugin API for their ecosystem, embed the runtime, and distribute an SDK for plugin developers to build against.

**PLUGIN DEVELOPER — writes plugins**
A developer who builds plugins for a specific app's ecosystem. They install the app's SDK, implement contracts, and ship compiled plugin bundles.

---

## 3. Core Design Principles

**Performance over everything**
The hot path is one indirect function call. All resolution, validation, and graph building happens at load time. Zero overhead at call time.

**Frozen ABI**
The core ABI is frozen at v1. It never changes. All evolution happens through the extension system. Breaking the ABI is not an option.

**Schema-driven**
The app developer's entire plugin API is derived from a single schema file. Codegen produces all glue code for all supported languages. Developers only write business logic.

**Language agnostic**
Every supported language can be a host and a guest. No language is a first-class citizen. The C ABI is the universal boundary.

**Pay only for what you use**
Language runtime adapters (dotnet, python, lua) are separate crates following the serde model. If you do not depend on `polyplug-dotnet`, .NET support does not exist in your binary. Not a feature flag — a missing dependency. True zero cost for unused languages.

**Generated code can be unsafe — tests confirm safety**
Since all glue code is generated and not hand-written, it can use maximally unsafe patterns to achieve maximum performance. The test suite confirms correctness.

**Creator-owns memory**
The caller allocates memory for return values. The callee fills it. All cross-boundary memory lives in the host allocator. No GC language puts cross-boundary data on its managed heap.

---

## 4. High-Level Architecture

```
APP DEVELOPER                        PLUGIN DEVELOPER
─────────────────                    ─────────────────
api.toml                             bundle.toml
    │                                    │
    ▼                                    ▼
polyplugc generate                   polyplugc generate
    │                                    │
    ├── generated/host/                  ├── generated init()
    │   (host callers, used by app)      ├── generated vtables
    │                                    ├── generated ABI wrappers
    └── generated/guest/                 └── generated manifest.toml
        (guest SDK, distributed               (for discovery)
         to plugin devs)
              │
              ▼
         Plugin Dev installs SDK
         implements contract traits
         builds image_bundle.so


AT RUNTIME
──────────
App initializes runtime (with only the adapters it depends on)
Runtime scans plugin dirs
Reads manifest.toml (fast, no loading)
Resolves capability graph
Dispatches each bundle to correct loader (native, dotnet, python, lua)
Loaders initialize bundles in correct order
Vtables registered
Ready — all future calls are one indirect call
```

---

## 5. Runtime Core

The runtime core is the `polyplug` crate written in **Rust**. It is the heart of the system and the only crate that is always present. It has zero knowledge of any managed runtime (CLR, CPython, Lua VM). That knowledge lives in adapter crates.

**Responsibilities:**

- Loading native plugin bundles via platform dynamic loading (dlopen / LoadLibrary)
- Reading and validating manifest files before loading
- Building the capability graph and determining initialization order
- Detecting dependency cycles
- Managing the host allocator
- Storing and serving registered plugin vtables
- Dispatching cross-plugin calls
- Managing extensions
- Enforcing compatibility rules
- Providing the `BundleLoader` trait that adapter crates implement

**The runtime exposes a minimal C ABI** so it can be embedded in any language. The C ABI is the only public contract of the runtime core. Host libs wrap this ABI for each supported language.

The runtime is distributed as a compiled dynamic library:

```
polyplug.so      (Linux)
polyplug.dll     (Windows)
polyplug.dylib   (macOS)
```

---

## 6. ABI Layer

The ABI is the frozen contract between the runtime and all plugins. It uses C calling conventions. It is minimal by design. Once v1 is released it never changes. **ABI re-frozen as of Epic 9.7.**

**Core ABI functions:**

```c
// Memory
void*  host_alloc(size_t size);
void   host_free(void* ptr);

// Dependency resolution — only valid for declared dependencies
// contract_id and bundle_id are computed by polyplugc from names in bundle.toml
// Plugin developers never write IDs — they write names. Codegen handles the rest.
PluginHandle find_by_contract(uint64_t contract_id, uint32_t min_version);
PluginHandle find_by_bundle(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
size_t       find_all_by_contract(uint64_t contract_id, uint32_t min_version,
                                   PluginHandle* out, size_t out_cap);  // caller-provides-buffer

// One-time resolution at init: PluginHandle → arc-swap Guard (opaque)
// Returned guard keeps vtable alive. Store the guard, use guard->vtable on hot path.
const PluginVTableGuard* resolve_plugin(PluginHandle handle);

// Extension lookup
const void* get_extension(uint32_t extension_id);
```

**ID computation — always done by polyplugc, never by developers:**

```
contract_id = fnv1a_64("contract.name@major")   e.g. fnv1a_64("image.decode@1") 
bundle_id   = fnv1a_64("bundle_name")           e.g. fnv1a_64("awesome_filter")
extension_id = fnv1a_32("extension_name")        e.g. fnv1a_32("trace")
```

These constants are baked into generated code as named constants. Plugin and app developers only ever write human-readable names in `.toml` files.

**Core ABI types:**

```c
typedef struct {
    uint32_t    code;     // 0 = success
    StringView  message;  // empty if success
} AbiError;

typedef struct {
    const uint8_t* ptr;   // UTF-8 bytes, not null-terminated
    size_t         len;
} StringView;

typedef struct {
    void*  ptr;
    size_t len;
    size_t cap;
} Buffer;

typedef struct {
    uint32_t index;       // slot index in registry
    uint32_t generation;  // detects use-after-unload
} PluginHandle;           // null sentinel: { index: U32_MAX, generation: 0 }

// Opaque — managed by runtime. Holds an arc-swap read guard keeping the
// vtable pointer alive for exactly one call sequence.
typedef struct PluginVTableGuard PluginVTableGuard;
```

**Dependency enforcement — hard error on undeclared access:**

`find_by_contract`, `find_by_bundle`, and `find_all_by_contract` check that the calling plugin (identified by its `bundle_id`) declared the requested dependency in its `bundle.toml`. If it did not, the call returns a null/error immediately. Plugins cannot discover arbitrary contracts by probing — they can only access what they declared. This enables the runtime to know the complete dependency graph at load time.

**Trust model:**

polyplug assumes plugins are trusted code loaded by the app developer. Malicious in-process code is explicitly out of scope. `PluginVTable` pointers must never be cast to mutable and written to — doing so is undefined behavior. There is no runtime enforcement: `mprotect` is bypassable by in-process code and is not used (security theater). See `TRUST_MODEL.md`.

**Rules:**
- All strings crossing the ABI boundary are UTF-8
- All structs use `#[repr(C)]` on the Rust side and standard C struct layout on the C side
- Primitives (u8–u64, f32, f64, bool) are returned directly by value
- All non-primitive return values use caller-provides-buffer pattern
- Pointers passed across boundary always point into host allocator
- `find_all_by_contract` uses caller-provides-buffer pattern — no allocation in runtime

---

## 7. VTable System

The vtable system is how plugins and host exchange callable function pointers. It is the mechanism that makes the hot path a single indirect call.

**Exchange happens once at load time:**

```
Host loads bundle (via correct loader — native, dotnet, python, or lua)
        │
        ▼
Host builds HostVTable (its functions for plugins to call)
        │
        ▼
Host calls init(registrar) passing HostVTable ptr
        │
        ▼
Plugin resolves declared dependencies via find_by_contract / find_by_bundle
Plugin stores PluginVTableGuard for each dependency (arc-swap read guard)
        │
        ▼
Plugin builds PluginVTable (its functions for host to call)
        │
        ▼
Plugin calls registrar->register() passing PluginVTable ptr
        │
        ▼
Host stores PluginVTable ptr in arc-swap slot
        │
        ▼
Load complete. All future calls = one indirect call.
```

This exchange is identical regardless of what language the plugin is written in.

**HostVTable — given to every plugin at init:**

```c
typedef struct {
    void*                    (*alloc)(size_t size);
    void                     (*free)(void* ptr);
    PluginHandle             (*find_by_contract)(uint64_t contract_id, uint32_t min_version);
    PluginHandle             (*find_by_bundle)(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
    size_t                   (*find_all_by_contract)(uint64_t contract_id, uint32_t min_version,
                                                      PluginHandle* out, size_t out_cap);
    const PluginVTableGuard* (*resolve_plugin)(PluginHandle handle);
    const void*              (*get_extension)(uint32_t extension_id);
} HostVTable;
```

**PluginVTable — one per contract implemented:**

```c
typedef struct {
    uint64_t contract_id;
    uint32_t contract_version;
    uint32_t function_count;
    void*    functions[];      // fixed order defined by contract schema
} PluginVTable;
```

**PluginRegistrar — bridge during init only:**

```c
typedef struct {
    void (*register_plugin)(
        PluginRegistrar*        self,
        const PluginDescriptor* descriptor,
        const PluginVTable*     vtable
    );
    const HostVTable* host;
} PluginRegistrar;
```

**Bundle entry point — single symbol exposed by every bundle:**

```c
void init(PluginRegistrar* registrar);
```

**Registry storage — arc-swap slots for hot-reload safety:**

Each registered plugin occupies one slot in the registry:

```rust
struct PluginSlot {
    vtable:     ArcSwap<VTableSlot>,  // atomically swappable, arc-swap crate
    generation: u32,                   // incremented on unload, detects stale handles
}

struct VTableSlot(pub *const PluginVTable);
// SAFETY: PluginVTable is read-only after registration. Send+Sync by trust model.
unsafe impl Send for VTableSlot {}
unsafe impl Sync for VTableSlot {}
```

**Cross-plugin call — hot path with arc-swap guard:**

Plugin resolves dependency once at init, stores the guard. On hot path loads vtable from guard directly — one pointer load, one indirect call.

```rust
// Generated guest code — at init (once per dependency):
let handle = host.find_by_contract(IMAGE_PROCESSOR_CONTRACT_ID, 1)?;
let guard = host.resolve_plugin(handle)?;   // arc-swap read guard
self.image_processor = guard;               // stored for lifetime of plugin

// Generated guest code — on hot path:
let vtable = self.image_processor.vtable(); // one load from guard
vtable.functions[PROCESS_FN_ID](args, out); // one indirect call
```

**Why arc-swap for hot-reload:**

`ArcSwap<VTableSlot>` allows the runtime to atomically swap the vtable pointer during hot-reload without any locking on the reader path. Readers (callers) pay one atomic load per call sequence. The `Arc` refcount in the guard keeps the old vtable alive until all in-flight calls complete — automatic quiescence, no notification needed.

Hot-reload implementation details are in Epic 17 (Hot-Reload).

**Hot path call:**

```c
// Developer writes:
Stats stats = image_stats.compute(image);

// Generated code does:
Stats stats;  // caller allocates on host allocator
AbiError err = vtable->functions[COMPUTE_FN_ID](&image, &stats);
```

One guard load. One pointer dereference. One indirect call. Nothing else.

---

## 8. Host Libraries

**Host libs are idiomatic wrappers over the polyplug C ABI, one per language.** That is all they are. They wrap the ABI functions — `host_alloc/host_free`, `find_by_contract`, `find_by_bundle`, `find_all_by_contract`, `resolve_plugin`, `get_extension` — in the natural idiom of each language. Written once, stable forever because the C ABI is frozen.

The generated host callers from `polyplugc` sit on top of the host lib. The host lib is contract-agnostic infrastructure; generated code is contract-specific.

```
App Developer Code
        ↓
Generated Host Callers      (polyplugc output — contract-specific, per app)
        ↓
Host Lib                    (C ABI wrapper — contract-agnostic, written once)
        ↓
polyplug C ABI
```

**Per language — source location, published artifact, and contents:**

```
Rust    host-libs/rust/    →  polyplug crate
                               PluginRuntime builder, type-safe ABI wrappers

C++     host-libs/cpp/     →  single-header / vcpkg / conan
                               RAII Runtime class, zero-overhead ABI wrappers

C#      host-libs/csharp/  →  Polyplug NuGet
                               P/Invoke declarations, Runtime builder class,
                               ref struct wrappers for StringView and Buffer

Python  host-libs/python/  →  polyplug pip package
                               ctypes bindings, Runtime class, ctypes.Structure
                               wrappers for StringView and Buffer

Lua     host-libs/lua/     →  polyplug.lua + .so
                               FFI declarations, Runtime table, FFI cdata
                               wrappers for StringView and Buffer
```

Note: `polyplug-dotnet`, `polyplug-python`, `polyplug-lua` are **not** host libs. They are Rust adapter crates that teach the runtime how to *load* plugins written in those languages. A C# host app needs `host-libs/csharp/` to drive the runtime. It separately needs `polyplug-dotnet` only if it wants to load `.NET plugins`.

**App developer runtime initialization:**

```rust
// Rust
use polyplug::PluginRuntime;
use polyplug_dotnet::DotnetLoader;
use polyplug_python::PythonLoader;

let runtime: PluginRuntime = PluginRuntime::new()
    .plugin_dirs(["./plugins", "~/.myapp/plugins"])
    .compatibility(Compatibility::Strict)
    .loader(DotnetLoader::new())
    .loader(PythonLoader::new())
    .extension(TraceExtension::new())
    .init()?;
```

```cpp
// C++
auto runtime = Polyplug::Runtime::builder()
    .plugin_dir("./plugins")
    .compatibility(Polyplug::Compatibility::Strict)
    .init();
```

```csharp
// C#
var runtime = Polyplug.Runtime.Builder()
    .PluginDir("./plugins")
    .Compatibility(Polyplug.Compatibility.Strict)
    .Init();
```

---

## 9. Guest Libraries

**Guest libs are the C ABI bootstrap layer that every plugin is built on top of, one per language.** They are the plugin-side mirror of the host libs — hand-written once, contract-agnostic, stable forever.

**Responsibilities:**
- Plugin entry point macro / attribute
- Host allocator hookup (all allocations go through host_alloc)
- Panic / exception boundary (plugin crash cannot take down host)
- ABI primitive types (StringView, Buffer, AbiError, PluginError)
- Basic FFI safety helpers

```
Plugin Dev Business Logic
        ↓
Generated ABI Wrappers          (polyplugc output)
        ↓
Guest Lib                       (language bootstrap, thin)
        ↓
Runtime C ABI
```

**Per language:**

```
Rust    → polyplug-guest crate, proc macro, allocator hook
C++     → guest-libs/cpp/ header-only, entry point macro, RAII helpers
C#      → Polyplug.Guest NuGet, entry point attribute, marshaling helpers
Python  → polyplug-guest pip package, entry point decorator, ctypes helpers
Lua     → polyplug-guest.lua, entry point registration helper
```

---

## 10. Language Runtime Adapters

Language runtime adapters are separate crates that teach `polyplug` how to load non-native bundles. They follow the **serde model**: separate crates, not feature flags.

- If `polyplug-dotnet` is not in your `Cargo.toml`, .NET support is not compiled into your binary
- If `polyplug-python` is not in your `Cargo.toml`, Python support is not compiled into your binary
- If `polyplug-lua` is not in your `Cargo.toml`, Lua support is not compiled into your binary
- If `polyplug-js` is not in your `Cargo.toml`, QuickJS JS support is not compiled into your binary
- If `polyplug-js-deno` is not in your `Cargo.toml`, V8/Deno JS support is not compiled into your binary

This is not a feature flag. It is a missing dependency. True zero cost.

Each adapter implements the `BundleLoader` trait defined in `polyplug`:

```rust
pub trait BundleLoader: Send + Sync {
    fn runtime_name(&self) -> &'static str;
    fn load(
        &self,
        path: &Path,
        registrar: &mut PluginRegistrar,
    ) -> Result<(), PolyplugError>;
}
```

App developer registers adapters at init:

```rust
PluginRuntime::new()
    .loader(DotnetLoader::new())              // from polyplug-dotnet
    .loader(PythonLoader::new())              // from polyplug-python
    .loader(LuaLoader::new())                 // from polyplug-lua
    .loader(JsLoader::new(JsConfig {}))       // from polyplug-js (QuickJS)
    .loader(JsDenoLoader::new(JsDenoConfig {})) // from polyplug-js-deno (V8)
    .init()?;
```

If a bundle's manifest declares `runtime = "js-quickjs"` but no JS loader is registered:

```
Error: bundle "my_js_plugin" requires runtime "js-quickjs"
but no loader is registered for runtime "js-quickjs".
Add polyplug-js as a dependency and register JsLoader at init.
```

Native plugins (Rust, C++, C# NativeAOT) require no adapter. The built-in native loader in `polyplug` handles them via dlopen.

---

### polyplug-dotnet

Enables loading of standard .NET C# plugins without NativeAOT.

**Cargo dependency:**

```toml
# polyplug-dotnet/Cargo.toml
netcorehost = { version = "0.20", features = ["nethost"] }

# Optional Cargo feature — app developer opts in when they want zero system .NET dep at build time
[features]
download-nethost = ["netcorehost/download-nethost"]
```

`nethost` feature: enables `nethost::load_hostfxr()` which automatically locates hostfxr via `DOTNET_ROOT`, `PATH`, and well-known system paths — no manual scanning needed.

`download-nethost` feature: downloads the `nethost` binary from NuGet at build time — zero system .NET install required to compile `polyplug-dotnet`. App developers enable this via `polyplug-dotnet/download-nethost` in their own `Cargo.toml`. Not enabled by default (supply chain / CI caching concerns).

**Configuration — app developer provides at init:**

```rust
pub enum HostfxrLocation {
    /// nethost::load_hostfxr() — searches DOTNET_ROOT, PATH, system (default)
    Auto,
    /// Explicit path to hostfxr .so/.dll
    Path(PathBuf),
}

pub struct DotnetConfig {
    pub min_framework: String,               // e.g. "net10.0"
    pub hostfxr: HostfxrLocation,            // default: Auto
}

DotnetLoader::new(DotnetConfig {
    min_framework: "net10.0".into(),
    hostfxr: HostfxrLocation::Auto,
})
```

**runtimeconfig.json — generated on the fly, never shipped by plugin developer:**

`hostfxr_initialize_for_runtime_config` requires a `.runtimeconfig.json` file path — there is no in-memory alternative. `polyplug-dotnet` generates a minimal one in a temp dir from `DotnetConfig.min_framework` **before** calling `nethost::load_hostfxr()` context init, passes it to hostfxr, then deletes it immediately after. Plugin developers ship only the `.dll`. No `.runtimeconfig.json` in the bundle.

**Assembly target framework version check — `pelite` reads PE metadata:**

Plugin `.dll` target framework is read directly from the PE/COFF CLR metadata section using the `pelite` crate — specifically the `TargetFrameworkAttribute` custom attribute on the assembly. This happens before the CLR loads the assembly. Zero CLR involvement, zero extra files from plugin developer.

```toml
pelite = "0.10"   # lightweight zero-alloc PE reader
```

**Multi-version behavior:**

A process can only host one CLR version:
- First load: CLR initialized for `min_framework`, `DelegateLoader` cached in `OnceLock`
- Subsequent bundles: `pelite` reads `TargetFrameworkAttribute` from PE metadata
  - Compatible (same major, minor >= min_framework minor) → load silently
  - Higher minor → load with warning
  - Different major → `PolyplugError::RuntimeVersionMismatch { required, found }`
- NativeAOT is the escape hatch for plugins targeting a different major version

**`DelegateLoader` caching — critical for performance:**

`load_assembly_and_get_function_pointer` takes ~30ms per call if the delegate loader is re-obtained each time. The `AssemblyDelegateLoader` from `netcorehost` must be obtained once via `hostfxr_get_runtime_delegate` and stored in the `OnceLock` context. Per-bundle load only calls `get_function_pointer` on the cached loader — not the full chain.

**Load flow:**

```
DotnetLoader::new(DotnetConfig { min_framework: "net10.0", hostfxr: Auto })
        ↓
First .NET bundle load:
  generate runtimeconfig.json in temp dir
  nethost::load_hostfxr() → locate hostfxr
  hostfxr context init with temp runtimeconfig.json → delete temp file
  cache AssemblyDelegateLoader in OnceLock
        ↓
Each .NET bundle:
  pelite reads TargetFrameworkAttribute from PE metadata → version check
  cached AssemblyDelegateLoader.get_function_pointer("Init")
  Init(registrar) called — identical vtable exchange from here
```

**Generated C# — performance requirements:**

```csharp
// Init must declare explicit calling convention — never leave it implicit
[UnmanagedCallersOnly(EntryPoint = "init",
    CallConvs = new[] { typeof(CallConvCdecl) })]
public static unsafe AbiError Init(PluginRegistrar* registrar) {
    // Called from a Rust (foreign) thread — CLR thread affinity required
    Thread.BeginThreadAffinity();
    try {
        // register vtables
        return AbiError.Ok;
    } catch (Exception ex) {
        return AbiError.FromException(ex);  // blittable uint, no marshalling
    } finally {
        Thread.EndThreadAffinity();
    }
}

// Every ABI function generated in Init.cs must also declare CallConvCdecl
// All parameters and return types must be blittable — no managed references
```

**C# host lib P/Invoke — `[SuppressGCTransition]` on hot path:**

```csharp
// host-libs/csharp/ — P/Invoke for call_plugin on the hot path
[DllImport("polyplug"), SuppressGCTransition]
public static extern AbiError call_plugin(
    PluginHandle handle, uint fn_id, void* args, void* out);

// find_plugin and get_extension also get [SuppressGCTransition]
// host_alloc / host_free do NOT — they may trigger GC
```

`[SuppressGCTransition]` eliminates the GC transition overhead on short, non-blocking native calls. Only safe when the native function is guaranteed short and does not block or call back into managed code directly.

**Performance:**
- CLR startup: one-time cost at first .NET plugin load (~100–500ms)
- Per-call managed/unmanaged transition with `[SuppressGCTransition]`: ~5–15ns
- Per-call without: ~50–200ns
- `DelegateLoader` cached: assembly function pointer lookup is ~0.1ms, not ~30ms
- Subsequent .NET plugins share the already-running CLR — fast load

**C# bundle manifest — plugin ships only `.dll`:**

```toml
runtime = "dotnet"    # standard .NET, requires polyplug-dotnet
# or absent           # NativeAOT, loaded by native loader, no adapter needed
```

---

### polyplug-python

Enables loading of Python plugins via CPython embedding.

**Configuration:**

```rust
PythonLoader::new(PythonConfig { min_version: (3, 10) })
```

Uses `pyo3` 0.28 to embed CPython (`auto-initialize` feature removed — `prepare_freethreaded_python()` called manually). Interpreter initialized once per process via `OnceLock` using `pyo3::prepare_freethreaded_python()`. Version checked once at init by reading `sys.version_info` — there is no per-plugin version, all plugins share the same interpreter.

All plugin loads run inside `Python::with_gil(|py| { ... })`. The GIL is released via `py.allow_threads()` during any Rust-only work between loads to avoid blocking other Python threads.

Plugins are loaded via `importlib.util.spec_from_file_location` — no `sys.path` mutation. Plugin format is a single `.py` file. The `init(registrar_ptr)` function receives the registrar as a `ctypes.c_void_p` integer and registers vtables back through the C ABI via `ctypes`.

The `host-libs/python/` package loads `polyplug.so` from a co-located path configured at builder time.

**Generated code performance rules:**
- All `ctypes` function objects cached at module level — no per-call lookup
- All `argtypes`/`restype` set once at import time
- All cross-boundary data in `ctypes.Structure` — never copied to Python heap

**Performance:** Python interpreter is the bottleneck, not polyplug.

---

### polyplug-lua

Enables loading of Lua plugins via LuaJIT or standard Lua embedding.

**Configuration:**

```rust
LuaLoader::new(LuaConfig { min_version: LuaVersion::Jit })
// or
LuaLoader::new(LuaConfig { min_version: LuaVersion::Lua55 })
// or
LuaLoader::new(LuaConfig { min_version: LuaVersion::Lua54 })
```

Uses `mlua` crate with `vendored` feature (compiles LuaJIT/Lua from source — no system install) and `send` feature (makes `mlua::Lua: Send + Sync` for `OnceLock`). One shared VM per process.

```toml
# Cargo.toml for polyplug-lua (LuaJIT variant)
mlua = { version = "0.11", features = ["luajit", "vendored", "send"] }
# Lua 5.5 variant:
mlua = { version = "0.11", features = ["lua55", "vendored", "send"] }
```

`LuaVersion` enum: `Jit | Lua55 | Lua54 | Lua53`

**Registrar pointer passing — FFI cdata, NOT lightuserdata:**

LuaJIT `lightuserdata` has a 47-bit pointer limit on x86_64 Linux. The registrar pointer may live anywhere in the address range. The correct pattern is to pass the pointer as a `uintptr_t` integer and cast it to a typed pointer on the Lua side via FFI:

```lua
-- Rust sets: lua.globals().set("_registrar_ptr", ptr as i64)
local reg = ffi.cast("PluginRegistrar*", ffi.cast("uintptr_t", _registrar_ptr))
```

This cast happens once at init time. All subsequent vtable function pointer calls are FFI cdata indirect calls — JIT-compiled to near-native speed (~800M ops/sec vs ~45M for lightuserdata C bindings).

`ffi.metatype` is used for all domain types, enabling LuaJIT's allocation sinking optimization — temporary struct allocations are eliminated entirely by the JIT.

**Performance:** LuaJIT FFI call overhead is within 2x of native vtable dispatch.

---

### polyplug-js

Enables loading of JavaScript and TypeScript plugins via embedded QuickJS.
Runtime value: `js-quickjs`.

**Embedding model:** QuickJS is embedded in-process via the `rquickjs` crate —
identical model to `polyplug-lua`. One shared JS VM per process, mutex-protected.
No subprocess. No IPC. No process boundary. Direct Rust function pointer calls.

**Cargo dependency:**

```toml
rquickjs = { version = "0.11.0", features = ["loader", "futures"] }
```

**Configuration:**

```rust
JsLoader::new(JsConfig {})  // no fields — QuickJS is fully embedded, no system deps
```

**Loading model:**

```
Host process
├── rquickjs::Runtime::new()          ← embedded in-process, ~300μs startup
├── ctx.globals().set("polyplug", {}) ← all HostVTable fns as direct Rust fn ptrs:
│     findByContract(lo, hi, min_ver) → {index, generation} | null
│     findByBundle(b_lo, b_hi, c_lo, c_hi, min_ver) → {index, generation} | null
│     findAllByContract(lo, hi, min_ver) → [{index, generation}]
│     resolvePlugin(index, generation) → guard_token
│     getExtension(extension_id) → {lo, hi} | null
│     registerVtable(contract_lo, contract_hi, vtable_obj) → void
│     alloc(size) → {lo, hi}
│     free(lo, hi) → void
├── ctx.eval(bundle_js)               ← plugin runs, calls polyplug.*, registers vtable
└── vtable registered — load complete
```

**u64 lo/hi split:** QuickJS uses f64 internally — cannot hold 64-bit integers without
precision loss. All u64 values (contract_id, bundle_id, pointers) are split into
`{ lo: number, hi: number }` pairs. The JS side reassembles: `hi * 0x100000000 + lo`.
Generated code handles this transparently — plugin developer never sees it.

**Bundle format:**

```
my_plugin/
├── manifest.toml    (runtime = "js-quickjs")
└── bundle.js        (single flat file — produced by rolldown at pack time)
```

No `node_modules`. No imports at runtime. `bundle.js` is entirely self-contained.

**Build step — Rolldown:**

Plugin developer writes TypeScript. `polyplugc pack --lang js-quickjs` invokes:

```
rolldown index.ts --format iife --platform neutral --file bundle.js
```

Rolldown bundles TypeScript + all npm dependencies into one flat `bundle.js`.
Plugin developer needs: `npm i -g rolldown`. No other toolchain required.

**npm ecosystem support:**

Pure-logic npm packages (lodash, zod, date-fns, etc.) work perfectly — bundled by Rolldown.
Node.js API packages (`fs`, `http`, `net`, `crypto`) do NOT work — QuickJS has no Node APIs.
`rolldown-plugin-node-polyfills` is NOT recommended — polyfills for I/O APIs throw at runtime,
producing confusing errors. Plugin developers needing Node.js APIs must use `polyplug-js-deno`.

**Performance:** JS value boxing/unboxing only — ~50-200ns per cross-plugin call.
Same performance tier as Lua. No channel, no thread hop, no IPC.

---

### polyplug-js-deno

Enables loading of JavaScript and TypeScript plugins via embedded V8 (deno_core).
Runtime value: `js-deno`.

**Embedding model:** V8 is embedded in-process via `deno_core`. Each bundle gets
one dedicated `std::thread` with a `smol::LocalExecutor` (required by V8's
thread-pinning constraint — V8 isolates are `!Send`). All calls are in-process
via deno_core's `#[op2(fast)]` op system. No subprocess. No IPC.

**Cargo dependencies:**

```toml
deno_core    = "0.311.0"
smol         = "2.0.0"       # LocalExecutor satisfies V8 thread-pinning
futures-lite = "2.3.0"
# tokio is NOT used — smol is lighter and explicitly supported by deno_core
```

**Configuration:**

```rust
JsDenoLoader::new(JsDenoConfig {})  // no fields — V8 is fully embedded, no system deps
```

**Loading model:**

```
Host process
├── std::thread::spawn (one per bundle — V8 isolate is thread-pinned)
│   ├── smol::LocalExecutor::new()    ← thread-local, never moves
│   ├── futures_lite::future::block_on(ex.run(async {
│   │     set thread_local HostVTable*
│   │     deno_core::JsRuntime::new(extensions: [polyplug_ops])
│   │     polyplug_ops registers all HostVTable fns as #[op2(fast)] ops:
│   │       op_find_by_contract(contract_id: u64, min_ver: u32) → u64
│   │       op_find_by_bundle(bundle_id: u64, contract_id: u64, min_ver: u32) → u64
│   │       op_find_all_by_contract(contract_id: u64, min_ver: u32) → Vec<u64>
│   │       op_resolve_plugin(handle: u64) → u64
│   │       op_get_extension(extension_id: u32) → u64
│   │       op_register_vtable(contract_id: u64, vtable: VtableDesc)
│   │       op_alloc(size: u32) → u64
│   │       op_free(ptr: u64)
│   │     load_main_es_module("index.ts" or "bundle.js")
│   │     run_event_loop() → plugin runs, calls Deno.core.ops.op_register_vtable
│   │     thread parks on mpsc Receiver — waiting for vtable call requests
│   └── }))
└── vtable registered via op_register_vtable → sent to loader via oneshot channel
```

**TypeScript support:** deno_core loads `.ts` natively via `TsModuleLoader`.
No separate transpilation step required. Plugin developer ships `index.ts` directly.
Rolldown is optional — use it only to bundle npm dependencies.

**Bundle format:**

```
my_plugin/
├── manifest.toml    (runtime = "js-deno")
└── index.ts         (TypeScript — loaded natively by deno_core)
  # OR
└── bundle.js        (if npm deps bundled via rolldown)
```

**npm ecosystem support:**

Broad npm ecosystem support via V8 + Node.js compatibility layer in deno_core.
`fs`, `http`, `crypto`, and most Node.js built-ins available.
Native addons (`.node` files) are still impossible — only pure-JS packages.

**Performance:**

deno_core `#[op2(fast)]` path: ~50-200ns per op call (direct in-process function call).
Channel roundtrip for vtable calls: ~1-5μs (send to JS thread + receive result).
Total cross-plugin call overhead: ~50-200ns + ~1-5μs ≈ ~1-5μs.
~5-30x slower than `js-quickjs` for cross-plugin calls. Still fully in-process,
no OS context switch, no serialization. Acceptable for plugins where npm ecosystem
access matters more than per-call latency.

V8 startup: ~50-200ms one-time cost at first `js-deno` bundle load.
Binary size: +~30MB for embedded V8.

---

### When to pick js-quickjs vs js-deno

```
js-quickjs — pick this when:
  ✓ Plugin logic is self-contained (validation, transformation, formatting)
  ✓ npm dependencies are pure JS (lodash, zod, date-fns, etc.)
  ✓ You want smallest possible binary footprint (+~1MB)
  ✓ You want fastest startup (<300μs) and lowest per-call overhead (~50-200ns)
  ✓ You want zero system dependencies — fully embedded, no installs required
  ✗ Plugin needs Node.js APIs (fs, http, crypto, net)
  ✗ Plugin needs native npm addons

js-deno — pick this when:
  ✓ Plugin needs Node.js-compatible APIs (fs, http, crypto, etc.)
  ✓ npm packages depend on Node.js built-ins
  ✓ You want first-class TypeScript without a separate compile step
  ✓ npm ecosystem breadth matters more than per-call latency
  ✗ You need the absolute lowest cross-plugin call overhead
  ✗ Binary size is a constraint (+~30MB for V8)
  ✗ You cannot afford 1 dedicated thread per js-deno bundle
```

Both adapters can be registered simultaneously. App developer registers one or both:

```rust
PluginRuntime::new()
    .loader(JsLoader::new(JsConfig {}))          // enables js-quickjs
    .loader(JsDenoLoader::new(JsDenoConfig {}))  // enables js-deno
    .init()?;
```

Plugin developer picks their runtime by setting `runtime = "js-quickjs"` or
`runtime = "js-deno"` in `bundle.toml`. The runtime dispatches automatically.

---

There are exactly **two schema files** in the entire system. Both are TOML.

---

### api.toml — owned by App Developer

Defines domain types and contracts. Single source of truth for what plugins can do in the app's ecosystem. Written by app developer. Distributed to plugin developers.

```toml
# api.toml

[[type]]
name = "Image"
fields = [
    { name = "width",  type = "u32" },
    { name = "height", type = "u32" },
    { name = "pixels", type = "Buffer" }
]

[[type]]
name = "Stats"
fields = [
    { name = "mean",   type = "f32" },
    { name = "stddev", type = "f32" },
    { name = "min",    type = "f32" },
    { name = "max",    type = "f32" }
]

[[contract]]
name    = "image.decode"
version = "1.0"

[[contract.functions]]
name    = "decode"
params  = [{ name = "raw", type = "Buffer" }]
returns = "Image"

[[contract.functions]]
name    = "supported_formats"
returns = "StringView"


[[contract]]
name    = "image.stats"
version = "1.0"

[[contract.functions]]
name    = "compute"
params  = [{ name = "image", type = "Image" }]
returns = "Stats"

[[contract.functions]]
name    = "compare"
params  = [
    { name = "a", type = "Stats" },
    { name = "b", type = "Stats" }
]
returns = "f32"
```

**Primitive types always available without declaration:**

```
u8, u16, u32, u64
i8, i16, i32, i64
f32, f64
bool
StringView    — UTF-8 string slice, non-owning
Buffer        — byte buffer with ptr, len, cap
ptr           — raw opaque pointer (advanced use)
void          — no return value
```

**Enum types — declared with `[[enum]]`:**

```toml
[[enum]]
name    = "ColorSpace"    # PascalCase, unique across all types and enums
repr    = "u32"           # required: u8 | u16 | u32 | u64 — the ABI-level type
bitflag = true            # optional, default false — emits [Flags]/IntFlag/operator| per language

[[enum.variants]]
name  = "None"            # PascalCase
value = "0"               # expression: literals, |, <<, ~, and previously-declared variant names

[[enum.variants]]
name  = "Srgb"
value = "1"

[[enum.variants]]
name  = "Linear"
value = "1 << 1"

[[enum.variants]]
name  = "SrgbLinear"
value = "Srgb | Linear"   # variant name reference — backward-only, one level deep
```

Enum names share the namespace with `[[type]]` names — collisions are a hard error.
Enums are valid as struct field types and as function parameter/return types.
At the ABI level an enum is its `repr` type. polyplugc emits idiomatic enum types
per language. Value expressions are emitted verbatim into generated code — the
target language compiler validates range and correctness.

**Value expression rules:** integer literals (decimal, hex `0xFF`, binary `0b0101`),
bit shift (`1 << N`), bitwise OR (`A | B`), bitwise NOT (`~A`), grouping (`(...)`),
and previously-declared variant name references. No arithmetic, no forward references,
no cross-enum references.

---

### bundle.toml — owned by Plugin Developer

Defines the contents of a plugin bundle. Written by plugin developer. Never distributed.

```toml
# bundle.toml

[bundle]
name    = "image_bundle"
version = "1.0"
runtime = "native"    # native (default) | dotnet | python | lua
                      # | js-quickjs | js-deno

# For native runtime — per-platform binary table (REQUIRED, flat file = error):
[bundle.file]
linux.x86_64    = "libplugin.x86_64.so"
linux.aarch64   = "libplugin.aarch64.so"
windows.x86_64  = "plugin.x86_64.dll"
windows.aarch64 = "plugin.aarch64.dll"
macos.x86_64    = "libplugin.x86_64.dylib"
macos.aarch64   = "libplugin.aarch64.dylib"

# For all other runtimes — single relative path (REQUIRED, table = error):
# file = "relative/path/within/bundle/dir"

api = "path/to/api.toml"
# or
api = "my_app_sdk"    # if installed as a package

# Dependencies — explicit declaration required.
# Runtime enforces this: undeclared contracts cannot be accessed.
# polyplugc validates all names against api.toml at codegen time.
# IDs (contract_id, bundle_id) are computed by polyplugc — never written by hand.

[[dependency]]
contract    = "image.decode"   # any bundle implementing this contract
min_version = "1.0"

[[dependency]]
bundle      = "awesome_filter"  # specific bundle by name (from its bundle.toml [bundle].name)
contract    = "image.decode"    # which contract from that bundle
min_version = "1.0"

[[plugin]]
name       = "ImageDecoder"
version    = "1.2.0"
implements = ["image.decode@1.0"]
optional   = ["trace"]

[[plugin]]
name       = "ImageStats"
version    = "1.0.0"
implements = ["image.stats@1.0"]
optional   = ["trace"]
```

**Bundle name uniqueness rule:**

Bundle names must not match any contract name defined in the referenced `api.toml`. `polyplugc validate` and `polyplugc generate` both enforce this as a hard error:

```
error: bundle name "image.decode" conflicts with contract name "image.decode" in api.toml.
Bundle names and contract names must be unique across the ecosystem.
Rename the bundle in bundle.toml or the contract in api.toml.
```

This guarantees generated accessor names are always unambiguous. No special-case syntax or disambiguation needed anywhere in the system.

**What polyplugc generates from bundle.toml:**

```
generated/
├── init.rs / init.cpp / Init.cs     bundle entry point + dependency resolution
├── vtables.rs / vtables.cpp         vtable structs and registration
├── contracts.rs / contracts.cpp     traits / interfaces to implement
├── types.rs / types.cpp             domain types from api.toml
└── manifest.toml                    discovery manifest (auto-generated)
```

**What polyplugc bakes into generated init code (hidden from developer):**

```rust
// Generated constants — developer never writes these
const IMAGE_DECODE_CONTRACT_ID: u64 = 0xA3F2...;   // fnv1a_64("image.decode@1")
const AWESOME_FILTER_BUNDLE_ID: u64 = 0xB7C1...;   // fnv1a_64("awesome_filter")
const MY_BUNDLE_ID:             u64 = 0xD4E9...;   // fnv1a_64("image_bundle")
```

---

### manifest.toml — generated, not hand-written

Auto-generated by polyplugc. Placed next to the compiled bundle. Runtime reads this for fast pre-load discovery without loading. Contains resolved dependency information for graph building.

```toml
# image_bundle.manifest.toml — GENERATED, never edit by hand

name           = "image_bundle"
bundle_id      = 0xD4E9...          # fnv1a_64("image_bundle") — baked by polyplugc
version        = "1.0"
runtime        = "native"
file           = "image_bundle.so"
provides       = ["image.decode@1.0", "image.stats@1.0"]
function_count = { "image.decode@1.0" = 2, "image.stats@1.0" = 2 }

# Resolved dependency list — consumed by Epic 12 graph builder and hot-reload
[[dependency]]
contract    = "image.decode"
contract_id = 0xA3F2...             # baked by polyplugc
min_version = "1.0"

[[dependency]]
bundle      = "awesome_filter"
bundle_id   = 0xB7C1...             # baked by polyplugc
contract    = "image.decode"
contract_id = 0xA3F2...
min_version = "1.0"
```

The `runtime` field tells discovery which loader to dispatch to. The `[[dependency]]` array feeds the Epic 12 topological sort and the Epic 17 hot-reload notification graph.

---

## 12. Code Generation Pipeline

`polyplugc` is a standalone CLI binary written in Rust. Works with any build system.

**Side is inferred from schema file — no `--side` flag:**
- `--api` → generates `host/` and `guest/` output
- `--bundle` → generates guest bundle code only

**Internal pipeline:**

```
api.toml  OR  bundle.toml
        │
        ▼
Schema Parser
        │
        ▼
Intermediate Representation (IR)
        │
        ▼
Language Generators
        │
        ▼
Generated Files
```

**Generator trait:**

```rust
trait CodeGenerator {
    fn generate_host(&self, ir: &IR) -> Result<GeneratedFiles, PolyplugcError>;
    fn generate_guest(&self, ir: &IR) -> Result<GeneratedFiles, PolyplugcError>;
}

struct RustGenerator;
struct CppGenerator;
struct CSharpGenerator;
struct PythonGenerator;
struct LuaGenerator;
```

**CLI usage:**

```bash
# App developer
polyplugc generate --api api.toml --lang cpp --out ./generated
# produces ./generated/host/ and ./generated/guest/

# Plugin developer
polyplugc generate --bundle bundle.toml --lang rust --out ./src/generated

# Validate only
polyplugc validate --api api.toml
polyplugc validate --bundle bundle.toml
```

**Code generation rules (baked in, not schema annotations):**
- Non-primitive params → always by reference
- Non-primitive returns → caller-provides-buffer, hidden from developer
- Primitive returns (u8–u64, f32, f64, bool) → returned directly by value
- Every ABI call wrapped in panic/exception catch on guest side
- All strings are UTF-8 at ABI; GC languages transcode at boundary
- All cross-boundary structs in host allocator; GC wrappers are ref structs / stack-only

---

## 13. Plugin Discovery

Four layers, worked through in order.

**Layer 1 — Directory scanning**
Scans configured directories. Every bundle is a **directory** — flat file bundles are not valid and are silently skipped during scanning. A scan path may contain non-bundle directories and unrelated files — these are silently ignored. A directory is a bundle candidate if and only if it contains `manifest.toml` at its root.

**Layer 2 — Manifest reading**
Reads `manifest.toml` from the bundle directory before any loading. A directory without `manifest.toml` is silently skipped. A directory with a malformed `manifest.toml` is a hard error. Extracts: name, bundle_id, version, runtime, provides, function_count, dependencies, and the resolved file path for the current platform.

File path resolution rules:
- `native` runtime: `[bundle.file]` table keyed by `os.arch` (e.g. `linux.x86_64`). Any non-empty subset of platform keys is valid — plugin developer declares only the platforms they support. Empty table = hard error. Current platform key resolved at discovery time — missing platform = hard error listing supported platforms.
- All other runtimes: flat `file = "..."` string. Value must be a relative path within the bundle directory — absolute paths and `../` traversal are hard errors.
- Missing `file` field for any runtime: hard error.

**Layer 3 — Capability graph resolution**
1. Collects all provided capabilities
2. Validates all requires are satisfied
3. Detects cycles
4. Topological sort for initialization order

Fails with clear error before loading anything if requirements unmet.

**Layer 4 — Explicit registration**

```rust
runtime.load_bundle("./my_plugin/")?;            // bundle directory
runtime.load_bundle_with("./my_plugin/", LoadOptions {
    compatibility: Compatibility::Relaxed,
})?;
```

---

## 14. Cross-Plugin Communication

Plugins never link to each other directly. All cross-plugin calls go through the runtime's arc-swap slots, which are the safety mechanism for hot-reload.

**Plugin declares dependency in bundle.toml:**

```toml
[[dependency]]
contract    = "image.decode"
min_version = "1.0"
```

**polyplugc generates resolution code in init (hidden from developer):**

```rust
// Generated — plugin developer never writes this
let handle = host.find_by_contract(IMAGE_DECODE_CONTRACT_ID, 1)?;
self.decoder_guard = host.resolve_plugin(handle)?;
```

**Hot path — one load, one indirect call:**

```rust
// Developer writes:
let result = self.decoder.decode(raw);

// Generated code does:
let vtable = self.decoder_guard.vtable();           // one atomic load from arc-swap
vtable.functions[DECODE_FN_ID](&raw, &mut result);  // one indirect call
```

**For specific bundle dependency:**

```toml
[[dependency]]
bundle      = "awesome_filter"
contract    = "image.decode"
min_version = "1.0"
```

```rust
// Generated — uses find_by_bundle with baked bundle_id constant
let handle = host.find_by_bundle(AWESOME_FILTER_BUNDLE_ID, IMAGE_DECODE_CONTRACT_ID, 1)?;
self.filter_guard = host.resolve_plugin(handle)?;
```

**Undeclared access — hard error:**

If a plugin calls `find_by_contract` for a contract it did not declare in `bundle.toml`, the runtime returns an error immediately. No probing. No discovery beyond declared dependencies.

**Works identically regardless of what languages plugin A and plugin B are written in.**

---

## 15. Memory Model

All cross-boundary memory lives in the host allocator.

**Rules:**
1. All cross-boundary allocations use `host_alloc` / `host_free`
2. Caller allocates output buffer, callee fills it
3. A plugin never frees memory it did not allocate
4. Large buffers passed by reference — never copied
5. GC languages never put cross-boundary data on managed heap

**Per language:**

```
Rust    zero cost — host allocator is the global allocator
C++     placement new into host_alloc buffer
C#      ref struct wrapping unmanaged ptr, StructLayout.Sequential
Python  ctypes.Structure — lives in C memory, GC never sees it
Lua     lightuserdata pointing into host allocator
```

**String model:**

All ABI strings are UTF-8 `StringView` (ptr + len, non-owning, no null terminator).

```
Rust    &str → StringView: zero cost
C++     std::string_view → StringView: zero cost
C#      string → StringView: transcode UTF-16→UTF-8, ASCII fast path
Python  str → StringView: encode UTF-8, pass ptr+len
Lua     string → StringView: already bytes, just ptr+len
```

---

## 16. Error Handling

**Level 1 — Recoverable:** plugin returns non-zero AbiError, generated code converts to native error style.

**Level 2 — Unrecoverable:** panics/exceptions caught at ABI boundary by generated wrapper, converted to AbiError. A crashing plugin cannot take down the host.

**PluginError — defined once in guest lib, never generated:**

```rust
pub struct PluginError {
    pub code:    u32,
    pub message: String,
}
```

**Per language native style:**

```
Rust    Result<T, polyplug_guest::PluginError>
C++     throws PolyplugException
C#      throws PluginException
Python  raises PluginError
Lua     returns (value, err) multiple return
```

---

## 17. Plugin Versioning and Compatibility

Versioning at the contract level. Plugin version and contract version are independent.

**Rules:**
- Minor bump → adds functions, backward compatible
- Major bump → breaking change, different contract
- Compatible: provided major == required major AND provided minor >= required minor

**Compatibility modes:**

```rust
Compatibility::Strict   // default — fail on mismatch
Compatibility::Relaxed  // warn, load anyway
Compatibility::Yolo     // no checks
```

Per-bundle override via `LoadOptions`.

---

## 18. Extension System

Optional host capabilities. Evolve the host API without touching frozen ABI. Always optional — a plugin must never require an extension.

**Built-in:**
```
trace     — structured logging
async     — async task spawning (future)
sandbox   — permission queries (future)
```

**Registration:**

```rust
PluginRuntime::new()
    .extension(TraceExtension::new())
    .extension(MyCustomExtension::new())
    .init()?;
```

**Query at plugin init (generated code):**

```c
const TraceExtension* trace = host->get_extension(EXT_TRACE_ID);
if (trace) { trace->emit("started"); }
// absent = zero overhead, null check only
```

---

## 19. Security Model

```
Native    — compiled .so/.dll, full trust, maximum performance
WASM      — sandbox, near-native (future)
Script    — Python / Lua, interpreter restrictions
```

Sandbox policies: FS access, network access, host API surface, memory limits, CPU time limits.

---

## 20. Developer Experience — App Developer

**Step 1: Add dependencies**

```toml
# Cargo.toml
[dependencies]
polyplug        = "1.0"      # always required
polyplug-dotnet = "1.0"      # only if loading .NET plugins
polyplug-python = "1.0"      # only if loading Python plugins
polyplug-lua    = "1.0"      # only if loading Lua plugins
```

**Step 2: Write api.toml** (see Section 11)

**Step 3: Run codegen**

```bash
polyplugc generate --api api.toml --lang cpp --out ./generated
```

**Step 4: Initialize runtime**

```rust
use polyplug::PluginRuntime;
use polyplug_dotnet::DotnetLoader;

let runtime: PluginRuntime = PluginRuntime::new()
    .plugin_dir("./plugins")
    .loader(DotnetLoader::new())
    .init()?;
```

**Step 5: Use plugins**

```cpp
#include "generated/host/image_decode.hpp"
auto decoder = runtime.get<ImageDecodeContract>();
Image img = decoder.decode(raw_bytes);
```

**Step 6: Distribute SDK**

```bash
polyplugc generate --api api.toml --lang rust   --out ./sdk/rust
polyplugc generate --api api.toml --lang cpp    --out ./sdk/cpp
polyplugc generate --api api.toml --lang csharp --out ./sdk/csharp
polyplugc generate --api api.toml --lang python --out ./sdk/python
polyplugc generate --api api.toml --lang lua    --out ./sdk/lua
# distribute guest/ subdir of each + api.toml
```

---

## 21. Developer Experience — Plugin Developer

**Step 1: Install app's SDK**

```bash
cargo add my_app_plugin_sdk
pip install my_app_plugin_sdk
```

**Step 2: Write bundle.toml**

```toml
[bundle]
name    = "image_bundle"
version = "1.0"
runtime = "native"         # native | dotnet | python | lua
api     = "my_app_plugin_sdk"

[[plugin]]
name       = "ImageDecoder"
version    = "1.0.0"
implements = ["image.decode@1.0"]
requires   = []
optional   = ["trace"]
```

**Step 3: Run codegen**

```bash
polyplugc generate --bundle bundle.toml --lang rust --out ./src/generated
```

**Step 4: Implement contracts — business logic only**

```rust
// Rust
struct MyDecoder;
impl ImageDecodeContract for MyDecoder {
    fn decode(&self, raw: &Buffer) -> Result<Image, polyplug_guest::PluginError> {
        // pure business logic
    }
    fn supported_formats(&self) -> StringView {
        StringView::from_static("png,jpg,webp")
    }
}
```

```csharp
// C# (NativeAOT or standard .NET — same code either way)
public class MyDecoder : IImageDecodeContract {
    public Image Decode(Buffer raw) { /* pure business logic */ }
    public StringView SupportedFormats() => StringView.FromStatic("png,jpg");
}
```

```python
# Python
class MyDecoder(ImageDecodeContract):
    def decode(self, raw: Buffer) -> Image:
        pass  # pure business logic
    def supported_formats(self) -> str:
        return "png,jpg"
```

**Step 5: Build and ship**

```bash
cargo build --release
# ships: image_bundle.so + image_bundle.manifest.toml
```

---

## 22. The Full Runtime Flow

```
1.  App initializes runtime
        PluginRuntime::new()
            .loader(DotnetLoader::new())
            .loader(PythonLoader::new())
            .plugin_dir("./plugins")
            .init()

2.  Runtime scans ./plugins/
        rust_bundle.so + rust_bundle.manifest.toml
        csharp_bundle.dll + csharp_bundle.manifest.toml
        python_plugin/ (directory with manifest.toml)

3.  Runtime reads all manifest.toml (no loading yet)
        rust_bundle:   runtime = "native"
        csharp_bundle: runtime = "dotnet"
        python_plugin: runtime = "python"

4.  Capability graph built, validated, sorted

5.  Each bundle dispatched to correct loader:
        rust_bundle   → native loader (dlopen)
        csharp_bundle → DotnetLoader (hostfxr → CLR → managed assembly)
        python_plugin → PythonLoader (CPython → import → ctypes bridge)

6.  Each loader calls init(registrar)
        vtables registered — identical path from here for all languages

7.  App calls plugin:
        decoder.decode(raw_bytes)
        → one indirect call through vtable
        → no knowledge of plugin language
```

---

## 23. MVP Language Support

```
Rust        host + guest    native, near zero overhead
C++         host + guest    header-only libs, near zero overhead
C#          host + guest    NativeAOT (native loader) or standard .NET (polyplug-dotnet)
Python      host + guest    ctypes, via polyplug-python
Lua         host + guest    LuaJIT recommended, via polyplug-lua
JavaScript  host + guest    QuickJS embedded (polyplug-js) or V8 embedded (polyplug-js-deno)
TypeScript  host + guest    same as JS — TS compiled by Rolldown (js-quickjs) or native (js-deno)
```

**C# — two modes, same generated code:**

```
NativeAOT       runtime = "native"    native loader, native performance, no adapter
standard .NET   runtime = "dotnet"    polyplug-dotnet required, CLR via hostfxr
```

**JS/TS — two runtime variants, two adapter crates:**

```
js-quickjs    QuickJS embedded in-process    ~50-200ns/call, +~1MB binary, pure-JS npm
js-deno       V8 embedded in-process         ~1-5μs/call,   +~30MB binary, Node.js npm APIs
```

Plugin developer picks variant in `bundle.toml`. App developer registers one or both adapters.
See PRD §10 (polyplug-js and polyplug-js-deno) for when to pick which.

---

## 24. Package Ecosystem

Follows the serde model. `polyplug` is always present. Adapters are optional addons.

```
RUST (crates.io)
├── polyplug              runtime core + Rust host lib
├── polyplug-guest        Rust guest lib
├── polyplug-dotnet       standard .NET adapter
├── polyplug-python       Python adapter
├── polyplug-lua          Lua adapter
├── polyplug-js           JS/TS adapter — QuickJS embedded (runtime = "js-quickjs")
├── polyplug-js-deno      JS/TS adapter — V8 embedded via deno_core (runtime = "js-deno")
└── polyplugc             CLI codegen tool (--lang js-quickjs, --lang js-deno)

C++ (headers / vcpkg / conan)
├── host-libs/cpp/        polyplug.hpp
└── guest-libs/cpp/       polyplug_guest.hpp

.NET (NuGet)
├── Polyplug              C# host lib (P/Invoke into polyplug core)
├── Polyplug.Dotnet       loads standard .NET plugins from C# host
└── Polyplug.Guest        C# guest lib (NativeAOT or standard .NET)

Python (pip)
├── polyplug              Python host lib
└── polyplug-guest        Python guest lib

Lua
├── polyplug.lua + .so    Lua host lib
└── polyplug-guest.lua    Lua guest lib

JS/TS (guest-libs/js/ — shipped as part of polyplugc SDK output)
└── polyplug-guest.ts     shared guest lib for both js-quickjs and js-deno
    ├── AbiError, StringView, Buffer (lo/hi ptr fields for js-quickjs)
    ├── DependencyNotFoundError
    ├── EXT_TRACE_ID constant
    └── TraceVTable interface
    (re-exported in generated init.ts for both variants)

NOTE: There is no JS host lib. The host is always a Rust binary.
```

---

## 25. Future Work

**Near term:**
- Async plugin execution
- WASM sandbox support
- Plugin marketplace tooling (signing, verification, distribution)

**Planned epics (already designed):**
- Hot-reload — Epic 17 (arc-swap foundation in Epic 9.7, writer path + dlclose deferral in Epic 17)

**Long term:**
- Distributed plugins
- Remote plugin execution
- Permission system
- polyplug-jvm (Java/Kotlin via JNI embedding)
- Swift support (native, no adapter needed)
- Zig support (native, no adapter needed)

---

## 26. Non-Goals

- UI frameworks
- Build systems
- Language compilers
- Asset pipelines
- Networking libraries
- Serialization formats (beyond ABI needs)
- Out-of-process plugin execution (IPC adds unacceptable overhead, violates performance goal)

---

## 27. Hot-Reload Architecture

Hot-reload allows a running application to replace a plugin bundle with a new version without restarting. All callers transparently use the new vtable after reload with zero downtime and no stale pointer risk.

**Foundation — arc-swap slots (in place since Epic 9.7):**

Every plugin slot in the registry holds an `ArcSwap<VTableSlot>`. Readers (callers) hold an arc-swap read guard for the duration of exactly one call sequence. When the guard drops, the Arc refcount decrements. The old vtable is only freed when all in-flight guards drop — automatic quiescence, no coordination, no locking.

**Hot-reload invariants:**

1. Contract functions are synchronous and bounded — when a call returns, it is done. No background threads, no stored callbacks into the old vtable. This makes quiescence detection trivial.
2. Plugins must declare all dependencies in `bundle.toml`. The runtime knows the complete dependency graph. When bundle B is reloaded, the runtime knows every bundle that depends on B.
3. Plugins load their dependency guard once at init. On hot-reload the arc-swap slot is swapped atomically — dependents automatically use the new vtable on their next call with no notification.

**Reload path (Epic 17):**

```
New version of bundle B detected (inotify / polling / explicit API)
        │
        ▼
Runtime loads new_B.so (via correct loader), runs init, gets new PluginVTable*
        │
        ▼
Runtime atomically swaps arc-swap slot: vtable_slot.store(Arc::new(VTableSlot(new_ptr)))
        │
        ▼
Callers immediately see new vtable on next call (atomic load in arc-swap guard)
        │
        ▼
Old Arc held alive until all in-flight guards drop (quiescence — automatic)
        │
        ▼
old_arc strong_count == 1 → safe to dlclose old bundle
```

**Deferred dlclose:**

```rust
let old_arc = slot.vtable.swap(Arc::new(VTableSlot(new_ptr)));
// Spin in reloader thread ONLY until all in-flight callers complete
while Arc::strong_count(&old_arc) > 1 { std::hint::spin_loop(); }
// NOW safe to dlclose
drop(old_arc);
dlclose(old_library_handle);
```

Callers never spin. Only the reloader thread waits. Caller overhead is zero.

**Caller overhead in steady state:**

| Operation | Cost on x86_64 |
|---|---|
| arc-swap guard load | 1 atomic load (acquire = free on x86, TSO) |
| vtable pointer read from guard | 1 load |
| indirect function call | 1 indirect call |
| guard drop (Arc refcount dec) | 1 atomic decrement |
| **Total vs raw pointer** | **~2-3 cycles** |

**Dependency graph and reload ordering:**

When bundle B is reloaded, bundles that depend on B (declared in their `[[dependency]]` sections and stored in the manifest dependency graph) must also be checked. If a dependent bundle cached a guard at init time, its guard remains valid — it points to the arc-swap slot, not a raw pointer. The slot update is transparent.

If a dependent bundle itself needs re-initialization (e.g. it caches derived state from B's vtable at init time), the runtime triggers a cascading reload in topological order. This is opt-in via a `needs_reinit_on_dep_reload = true` field in `bundle.toml`.

**Limitations:**
- Recursive self-calls during reload are not supported — document clearly
- Hot-reload adds ~2-3 cycles per cross-plugin call in steady state vs a raw pointer (unavoidable cost of safety)
- Host-side direct vtable pointers (from `resolve_plugin` stored by host app) must be refreshed after reload via `runtime.refresh_handle(handle)` — generated host callers do this automatically

---

## Appendix — File Map

```
polyplug/                                YOU maintain
├── AGENTS.md
├── Cargo.toml                           workspace root
├── crates/
│   ├── polyplug/                        runtime core (renamed from polyplug-runtime)
│   │   ├── build.rs
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── abi/
│   │       │   └── mod.rs
│   │       ├── allocator/
│   │       │   └── mod.rs
│   │       ├── error/
│   │       │   └── mod.rs
│   │       ├── graph/
│   │       │   └── mod.rs
│   │       ├── loader/
│   │       │   └── mod.rs              BundleLoader trait + native loader
│   │       ├── registry/
│   │       │   └── mod.rs
│   │       └── runtime/
│   │           └── mod.rs
│   ├── polyplug-guest/                  Rust guest lib
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib/
│   │           └── mod.rs
│   ├── polyplug-dotnet/                 standard .NET adapter
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib/
│   │           └── mod.rs
│   ├── polyplug-python/                 Python adapter
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib/
│   │           └── mod.rs
│   ├── polyplug-lua/                    Lua adapter
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib/
│   │           └── mod.rs
│   └── polyplugc/                       CLI codegen tool
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── error/
│           │   └── mod.rs
│           ├── ir/
│           │   └── mod.rs
│           ├── parser/
│           │   └── mod.rs
│           └── generators/
│               ├── mod.rs
│               ├── rust/
│               │   └── mod.rs
│               ├── cpp/
│               │   └── mod.rs
│               ├── csharp/
│               │   └── mod.rs
│               ├── python/
│               │   └── mod.rs
│               └── lua/
│                   └── mod.rs
├── host-libs/
│   ├── cpp/
│   │   ├── polyplug.hpp
│   │   └── polyplug/
│   │       ├── abi.hpp
│   │       ├── error.hpp
│   │       ├── handle.hpp
│   │       └── runtime.hpp
│   ├── csharp/                          Polyplug NuGet
│   ├── python/                          polyplug pip
│   └── lua/                             polyplug.lua + .so
├── guest-libs/
│   ├── cpp/
│   │   ├── polyplug_guest.hpp
│   │   └── polyplug/
│   │       ├── abi.hpp
│   │       ├── contract.hpp
│   │       └── guest.hpp
│   ├── csharp/                          Polyplug.Guest NuGet
│   ├── python/                          polyplug-guest pip
│   └── lua/                             polyplug-guest.lua
└── tests/
    ├── fixtures/
    │   ├── test_api.toml
    │   ├── test_bundle.toml
    │   └── test_plugin/
    │       ├── Cargo.toml
    │       └── src/
    │           └── lib.rs
    ├── fnv1a_compat/
    │   └── mod.rs
    ├── integration_dispatch/
    │   └── mod.rs
    ├── integration_graph/
    │   └── mod.rs
    └── integration_load/
        └── mod.rs


my-game-engine/                          APP DEVELOPER
├── api.toml
├── Cargo.toml
│   └── polyplug = "1.0"
│       polyplug-dotnet = "1.0"          only if .NET plugins needed
└── src/
    └── generated/
        ├── host/                        polyplugc --api output
        └── guest/                       distributed to plugin devs


image-plugin/                            PLUGIN DEVELOPER
├── bundle.toml                          runtime = "native" | "dotnet" | "python" | "lua"
├── src/
│   ├── decoder.rs                       business logic only
│   └── generated/                       polyplugc --bundle output
│       ├── types.rs
│       ├── contracts.rs
│       ├── vtables.rs
│       └── init.rs
└── dist/
    ├── image_bundle.so
    └── image_bundle.manifest.toml       runtime field included
```