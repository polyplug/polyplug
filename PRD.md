# polyplug — PRD v4

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

**C# unsafe is confined to generated code only — never in libs or app code**

`unsafe` in C# requires `<AllowUnsafeBlocks>true</AllowUnsafeBlocks>` in the project file, or the `unsafe` keyword on a specific type or method. Neither is acceptable in `sdks/csharp/guest/`, `sdks/csharp/host/`, plugin developer projects, or host app developer projects — they must compile with zero unsafe.

`unsafe` IS used in generated `Init.cs` (produced by `polyplugc generate`), which lives in an isolated generated project that polyplugc controls. This is where `delegate*` unmanaged function pointers are used to call vtable functions via the `calli` IL instruction — a genuine ~4–6x performance advantage over `Marshal.GetDelegateForFunctionPointer` (which heap-allocates a delegate and routes through `Delegate.Invoke`). Plugin developers never edit generated files and never enable unsafe in their own project.

All hand-written C# — structs, host lib, guest lib — uses safe equivalents:
- `byte*` → `IntPtr` (pointer-sized, ABI-identical, no unsafe)
- `void*` → `IntPtr` (same)
- `nuint` as pointer → `ulong` (polyplug is 64-bit only — always 8 bytes)
- `delegate*` in struct fields → `IntPtr` fields; generated code casts at call site
- `[UnmanagedCallersOnly]` init parameters → `IntPtr` (blittable, ABI-correct)

The result: app developers and plugin developers never interact with `unsafe` in any form.

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
const PluginGuard* resolve_plugin(PluginHandle handle);

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

// Passed to init() — gives plugin access to its bundle directory.
// Valid for the duration of init() only. Do not store the pointer.
// Copy bundle_path into owned storage if needed after init returns.
typedef struct {
    StringView bundle_path;  // absolute path to bundle directory, UTF-8
    // future fields appended here — ABI-stable by addition only
} PluginContext;

// Opaque — managed by runtime. Holds an arc-swap read guard keeping the
// vtable pointer alive for exactly one call sequence.
typedef struct PluginGuard PluginGuard;
```

**Dependency enforcement — hard error on undeclared access:**

`find_by_contract`, `find_by_bundle`, and `find_all_by_contract` check that the calling plugin (identified by its `bundle_id`) declared the requested dependency in its `bundle.toml`. If it did not, the call returns a null/error immediately. Plugins cannot discover arbitrary contracts by probing — they can only access what they declared. This enables the runtime to know the complete dependency graph at load time.

**Trust model:**

polyplug assumes plugins are trusted code loaded by the app developer. Malicious in-process code is explicitly out of scope. `PluginInterface` pointers must never be cast to mutable and written to — doing so is undefined behavior. There is no runtime enforcement: `mprotect` is bypassable by in-process code and is not used (security theater). See `TRUST_MODEL.md`.

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
Host calls init(registrar, ctx) passing HostVTable ptr and PluginContext
        │
        ▼
Plugin resolves declared dependencies via find_by_contract / find_by_bundle
Plugin stores PluginGuard for each dependency (arc-swap read guard)
        │
        ▼
Plugin builds PluginInterface (its functions for host to call)
        │
        ▼
Plugin calls registrar->register() passing PluginInterface ptr
        │
        ▼
Host stores PluginInterface ptr in arc-swap slot
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
    const PluginGuard*       (*resolve_plugin)(PluginHandle handle);
    const void*              (*get_extension)(uint32_t extension_id);
} HostVTable;
```

**PluginInterface — one per contract implemented:**

```c
typedef struct {
    uint64_t contract_id;
    uint32_t contract_version;
    uint32_t function_count;
    void*    functions[];      // fixed order defined by contract schema
} PluginInterface;
```

**PluginRegistrar — bridge during init only:**

```c
typedef struct {
    void (*register_plugin)(
        PluginRegistrar*        self,
        const PluginDescriptor* descriptor,
        const PluginInterface*     vtable
    );
    const HostVTable* host;
} PluginRegistrar;
```

**Bundle entry point — single symbol exposed by every bundle:**

```c
// PluginContext valid for duration of init() only — do not store the pointer.
void init(PluginRegistrar* registrar, const PluginContext* ctx);
```

**Registry storage — arc-swap slots for hot-reload safety:**

Each registered plugin occupies one slot in the registry:

```rust
struct PluginSlot {
    vtable:     ArcSwap<VTableSlot>,  // atomically swappable, arc-swap crate
    generation: u32,                   // incremented on unload, detects stale handles
}

struct VTableSlot(pub *const PluginInterface);
// SAFETY: PluginInterface is read-only after registration. Send+Sync by trust model.
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
Rust    crates/polyplug/   →  polyplug crate (crates.io)
                               PluginRuntime builder, type-safe ABI wrappers

C++     sdks/cpp/host/     →  polyplug package (vcpkg / Conan / release archive)
                               RAII Runtime class, zero-overhead ABI wrappers
                               loaders/python.hpp, loaders/lua.hpp, etc. (one per loader)

C#      sdks/csharp/host/  →  Polyplug NuGet
                               P/Invoke declarations, Runtime class,
                               ref struct wrappers for StringView and Buffer
                               Register*Loader() methods (one per loader)

Python  sdks/python/host/  →  polyplug pip package
                               ctypes bindings, Runtime class, ctypes.Structure wrappers
                               loaders/python.py, loaders/lua.py, etc. (one per loader)

Lua     sdks/lua/host/     →  polyplug LuaRocks rock / release archive
                               LuaJIT FFI host lib, Runtime metatable, Guard metatable
                               register_*_loader() functions (one per loader)
                               Performance: JIT-inlined C calls, near-native or faster

JS/TS   sdks/js/host/      →  @polyplug/core JSR package
                               Deno.dlopen host lib, Runtime class, TypeScript types
                               register*Loader() functions (one per loader)
                               Requires --allow-ffi at runtime
                               Performance: <10ns (V8 fast call), ~150ns (BigInt/slow path)
```

Note: `polyplug_loaders_dotnet`, `polyplug_loaders_python`, `polyplug_loaders_lua`,
`polyplug_loaders_js` are **loader packages** — they teach
the runtime how to load plugins written in those languages. They are distinct from the
host lib for a given language. A C# host app uses `sdks/csharp/host/` (`Polyplug` NuGet)
to drive the runtime. It additionally installs `Polyplug.Loaders.Python` only if it
wants to load Python guest plugins. Each loader is a separate opt-in package in every
language ecosystem. See §24 for the full package listing per language.

**App developer runtime initialization — with loader registration:**

```rust
// Rust — loader crates linked directly, no FFI needed
use polyplug::PluginRuntime;
use polyplug_dotnet::DotnetLoader;
use polyplug_python::PythonLoader;
use polyplug_lua::LuaLoader;
use polyplug_js::JsLoader;

let runtime = PluginRuntime::new()
    .plugin_dirs(["./plugins"])
    .loader(DotnetLoader::new())
    .loader(PythonLoader::new())
    .loader(LuaLoader::new())
    .loader(JsLoader::new())
    .init()?;
```

```cpp
// C++ — links libpolyplug.so + libpolyplug_dotnet.so + ...
#include <polyplug/runtime.hpp>
#include <polyplug/loaders.hpp>   // NEW — loader registration

auto rt = polyplug::Runtime::build();
polyplug::register_dotnet_loader(rt, { .min_framework = "10.0" });
polyplug::register_python_loader(rt, { .min_version = "3.11" });
polyplug::register_lua_loader(rt);
polyplug::register_js_loader(rt);
rt.load_bundle("./plugins/my_plugin");
```

```csharp
// C# — P/Invoke libpolyplug.so + libpolyplug_dotnet.so + ...
var rt = new Polyplug.Runtime();
rt.RegisterDotnetLoader("10.0");
rt.RegisterPythonLoader("3.11");
rt.RegisterLuaLoader();
rt.RegisterJsLoader();
rt.LoadBundle("./plugins/my_plugin");
```

```python
# Python — ctypes libpolyplug.so + libpolyplug_dotnet.so + ...
import polyplug
from polyplug import loaders

rt = polyplug.Runtime()
loaders.register_dotnet_loader(rt, min_framework="10.0")
loaders.register_python_loader(rt, min_version="3.11")
loaders.register_lua_loader(rt)
loaders.register_js_loader(rt)
rt.load_bundle("./plugins/my_plugin")
```

```lua
-- Lua — LuaJIT FFI into libpolyplug.so + libpolyplug_*.so
local polyplug = require("polyplug")

local rt = polyplug.runtime_new()
polyplug.register_dotnet_loader(rt, { min_framework = "10.0" })
polyplug.register_python_loader(rt, { min_version = "3.11" })
polyplug.register_lua_loader(rt)
polyplug.register_js_loader(rt)
polyplug.load_bundle(rt, "./plugins/my_plugin")
```

```typescript
// Deno — Deno.dlopen into libpolyplug.so + libpolyplug_*.so
import * as polyplug from "./sdks/js/host/polyplug.js";

const rt = polyplug.runtimeNew();
await polyplug.registerDotnetLoader(rt, { minFramework: "10.0" });
await polyplug.registerPythonLoader(rt, { minVersion: "3.11" });
await polyplug.registerLuaLoader(rt);
await polyplug.registerJsLoader(rt);
polyplug.loadBundle(rt, "./plugins/my_plugin");
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
C++     → sdks/cpp/guest/ header-only, entry point macro, RAII helpers
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

This is not a feature flag. It is a missing dependency. True zero cost.

Each adapter implements the `BundleLoader` trait defined in `polyplug`:

```rust
pub trait BundleLoader: Send + Sync {
    fn runtime_name(&self) -> &'static str;
    fn load(
        &self,
        path: &Path,
        registrar: &mut PluginRegistrar,
    ) -> Result<(), RuntimeError>;
}
```

App developer registers adapters at init:

```rust
PluginRuntime::new()
    .loader(DotnetLoader::new())              // from polyplug-dotnet
    .loader(PythonLoader::new())              // from polyplug-python
    .loader(LuaLoader::new())                 // from polyplug-lua
    .loader(JsLoader::new(JsConfig {}))       // from polyplug-js (QuickJS)
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

The generated `runtimeconfig.json` includes `additionalProbingPaths` pointing to the bundle directory. This allows managed assembly dependencies (`.dll` files) shipped inside the bundle directory to be found by the CLR automatically. For native interop DLLs, plugin developers use `NativeLibrary.Load(Path.Combine(bundlePath, "native.dll"))` with the `bundle_path` from `PluginContext`.

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
  - Different major → `RuntimeError::RuntimeVersionMismatch { required, found }`
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

> **unsafe policy:** `sdks/csharp/guest/` and `sdks/csharp/host/` have zero `unsafe` and require no `<AllowUnsafeBlocks>` in the plugin developer's or host app developer's project. All `unsafe` is confined to generated `Init.cs` only, in the polyplugc-controlled generated project. Plugin developers never edit or enable unsafe anywhere.

```csharp
// Generated Init.cs — polyplugc output, plugin developer never edits this.
// <AllowUnsafeBlocks>true</AllowUnsafeBlocks> is set ONLY in the generated .csproj.

// Init receives IntPtr parameters — blittable, ABI-correct, no unsafe on method.
[UnmanagedCallersOnly(EntryPoint = "init",
    CallConvs = new[] { typeof(CallConvCdecl) })]
public static AbiError Init(IntPtr registrarPtr, IntPtr ctxPtr) {
    // Called from a Rust (foreign) thread — CLR thread affinity required
    Thread.BeginThreadAffinity();
    try {
        // Generated code casts IntPtr → delegate* here (unsafe block, generated only)
        unsafe {
            var registrar = (PluginRegistrar*)registrarPtr;
            var ctx       = (PluginContext*)ctxPtr;
            // register vtables via delegate* — calli IL, zero allocation
        }
        return AbiError.Ok;
    } catch (Exception ex) {
        return AbiError.FromException(ex);  // blittable uint, no marshalling
    } finally {
        Thread.EndThreadAffinity();
    }
}

// Every ABI function generated in Init.cs must also declare CallConvCdecl
// All parameters and return types must be blittable — no managed references
// delegate* unmanaged used for vtable calls: calli IL = ~4-6x faster than
// Marshal.GetDelegateForFunctionPointer (no heap alloc, no Delegate.Invoke)
```

**C# host lib P/Invoke — `LibraryImport` + `[SuppressGCTransition]` on hot path:**

```csharp
// sdks/csharp/host/ — zero unsafe. void* → IntPtr, ABI-identical.
// LibraryImport (source-generated, AOT-safe) replaces DllImport everywhere.
[LibraryImport("polyplug"), SuppressGCTransition]
public static partial uint CallPlugin(
    PluginHandle handle, uint fnId, IntPtr args, IntPtr outPtr);

// find_plugin and get_extension also get [SuppressGCTransition]
// host_alloc / host_free do NOT — they may trigger GC
```

`LibraryImport` is the modern replacement for `DllImport`. It generates the P/Invoke
marshalling code at compile time (source generator) rather than at runtime, making it
AOT-compatible, trimming-safe, and faster than `DllImport` on the cold path.
`[SuppressGCTransition]` eliminates the GC transition overhead on short, non-blocking
native calls. Only safe when the native function is guaranteed short and does not block
or call back into managed code directly.

**C# unsafe boundary — complete summary:**

| Location | `unsafe`? | `<AllowUnsafeBlocks>`? | Reason |
|---|---|---|---|
| `sdks/csharp/guest/` | ❌ None | ❌ Not required | IntPtr/ulong replace raw pointers |
| `sdks/csharp/host/` | ❌ None | ❌ Not required | IntPtr replaces void* in P/Invoke |
| Plugin developer project | ❌ None | ❌ Not required | Writes only business logic |
| Host app developer project | ❌ None | ❌ Not required | Uses safe Runtime class only |
| Generated `Init.cs` | ✅ Isolated block | ✅ Generated .csproj only | `delegate*` for `calli` perf gain |

**Performance:**
- CLR startup: one-time cost at first .NET plugin load (~100–500ms)
- Per-call managed/unmanaged transition with `[SuppressGCTransition]`: ~5–15ns
- Per-call without: ~50–200ns
- `DelegateLoader` cached: assembly function pointer lookup is ~0.1ms, not ~30ms
- Subsequent .NET plugins share the already-running CLR — fast load
- `delegate*` in generated code: `calli` IL — ~4–6x faster than delegate allocation path

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

Plugins are loaded via `importlib.util.spec_from_file_location`. Before loading, polyplug-python prepends the bundle directory to `sys.path`. If `bundle_dir/site-packages/` exists, it is also prepended — this allows plugin developers to ship pip dependencies inside their bundle directory. These paths are not removed after load (removing them could break already-imported modules).

The `sdks/python/host/` package loads `polyplug.so` from a co-located path configured at builder time.

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

**Dependency path setup:**

Before executing the plugin chunk, polyplug-lua prepends the bundle directory to `package.path` (for `.lua` files) and `package.cpath` (for C extension modules `.so`/`.dll`). This allows plugin developers to ship Lua module dependencies inside their bundle directory — a `require "somedep"` will find `bundle_dir/somedep.lua` or `bundle_dir/somedep.so` automatically. These paths are not removed after load.

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
Plugin developers needing Node.js APIs should write native plugins or use a different runtime.

**Performance:** JS value boxing/unboxing only — ~50-200ns per cross-plugin call.
Same performance tier as Lua. No channel, no thread hop, no IPC.

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
# image_bundle/manifest.toml — GENERATED by polyplugc pack. DO NOT EDIT.

name           = "image_bundle"
bundle_id      = 0xD4E9...          # fnv1a_64("image_bundle") — baked by polyplugc
version        = "1.0"
runtime        = "native"
provides       = ["image.decode@1", "image.stats@1"]
function_count = { "image.decode@1" = 2, "image.stats@1" = 2 }
needs_reinit_on_dep_reload = false

# native runtime: per-platform binary table
[file]
linux.x86_64   = "libimage_bundle.x86_64.so"
linux.aarch64  = "libimage_bundle.aarch64.so"
windows.x86_64 = "image_bundle.x86_64.dll"
macos.aarch64  = "libimage_bundle.aarch64.dylib"

# Resolved dependency list — consumed by graph builder and hot-reload engine
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

**Manifest field rules:**

- `name` — bundle's human-readable name. One field only. There is no `bundle_name`.
- `bundle_id` — `fnv1a_64(name)`, pre-computed at pack time so the scanner never hashes at runtime.
- `provides` — contract names use `@{major}` format: `"image.decode@1"` not `"image.decode@1.0"`. Must be consistent with `function_count` keys — identical strings.
- `function_count` — map from contract name (`@{major}` format) to function count. Used by the runtime to validate the registered vtable matches what was declared. Catches bundles compiled against an older API version before any call is made. Missing entry = hard error.
- `needs_reinit_on_dep_reload` — **always emitted**, always `true` or `false` explicitly. Default is `false`. Must appear before `[file]` table. The manifest is the only runtime artifact — `bundle.toml` never ships, so the hot-reload engine reads this from the manifest, not from `bundle.toml`.
- `[file]` — must appear **after** all scalar top-level fields (`name`, `bundle_id`, `version`, `runtime`, `provides`, `function_count`, `needs_reinit_on_dep_reload`). TOML assigns all subsequent key-value pairs to the most recently opened table header — placing any scalar field after `[file]` nests it inside the file table silently.
- For non-native runtimes: `file = "path"` flat string instead of `[file]` table.

**The `runtime` field** tells discovery which loader to dispatch to. The `[[dependency]]` array feeds the topological sort and the hot-reload notification graph.

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
Rust    &str → StringView: zero cost (From<&str> impl)
C++     std::string_view → StringView: zero cost (implicit conversion operator)
C#      string → StringView: Marshal.PtrToStringUTF8 (no unsafe required)
Python  str → StringView: encode UTF-8, pass ptr+len
Lua     string → StringView: already bytes, just ptr+len
```

**ABI type helpers — per language:**

All guest libs expose ergonomic helpers on `StringView` and `Buffer` so plugin
developers never touch raw pointers directly.

```
Rust      StringView: as_str(), as_bytes(), is_empty(), From<&str>, From<StringView>→String,
                      Display, Debug, PartialEq<str>
          Buffer:     as_slice(), as_slice_mut(), is_empty()

C++       StringView: implicit operator std::string_view (zero-copy),
                      explicit operator std::string (allocating),
                      ctor from std::string_view, ctor from string literal,
                      empty()
          Buffer:     data<T>(), size(), empty()

C#        StringView: explicit operator string (Marshal, no unsafe),
                      ToString(), IsEmpty
                      PinnedStringView.Pin(string) → IDisposable RAII wrapper
          Buffer:     IsEmpty
          NOTE: no unsafe keyword required anywhere — no project option changes

Python    StringView: __str__, __bytes__, __bool__, __eq__ (vs str and StringView),
                      __repr__, from_str() classmethod → (view, backing_bytes)
          Buffer:     __bytes__, __bool__, __len__

Lua       StringView: __tostring, __eq (vs string and StringView), __len,
                      from_string() (caller keeps string alive)
          Buffer:     __tostring, __len, __bool (LuaJIT extension)

JS/TS     StringViewHelper: decode(sv), encode(s)→{view,bytes}, isEmpty(sv)
          BufferHelper:     toBytes(buf), isEmpty(buf)
          (no operator overloading in JS — static helper class is idiomatic)
```

**PluginContext helpers:**

```
Rust      ctx.bundle_path() → &str
C++       ctx->bundle_path (StringView with helpers above)
C#        ctx.BundlePathString → string  (property, no unsafe)
Python    ctx.bundle_path_str() → str
Lua       ctx:bundle_path_str() → string
JS/TS     bundlePath: string  (passed as plain string to JS init, not a struct)
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

**Trust model — in-process plugins:**

polyplug assumes plugins are trusted code vetted and distributed by the app developer. Malicious in-process code is explicitly out of scope. See `TRUST_MODEL.md` for the complete trust model including plugin crash isolation policy.

**Input validation at the host boundary (Epic 23):**

Even with trusted plugins, malformed or corrupted plugin binaries are a real scenario (corrupted download, wrong architecture binary, partial write). polyplug defends against these at load time:

- All strings extracted from plugin binaries (contract names, bundle names) are validated as UTF-8 via `std::str::from_utf8`. Invalid UTF-8 is a hard load error (`RuntimeError::InvalidUtf8`). `from_utf8_unchecked` is only used on host-owned data with a mandatory `// SAFETY:` comment.
- All C facade FFI functions null-check every pointer parameter at entry. Null pointers return defined errors, never UB.
- Malformed binaries (truncated, wrong magic, missing `init` symbol) return clean `Err` results. The runtime remains healthy after rejecting a bad bundle.

**Double-free detection (debug builds):**

`TrackingAllocator` maintains a `HashSet` of live allocations in debug builds (`cfg(debug_assertions)`). A double-free panics immediately with the address. Release builds have zero overhead. ASan runs in a dedicated CI job for system-level memory safety verification.

**Plugin crash isolation — non-goal:**

A plugin that segfaults kills the host process. This is intentional — see `TRUST_MODEL.md` and section 27 (Non-Goals).

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
Rust        host + guest    native, zero overhead
C++         host + guest    header-only libs, zero overhead
C#          host + guest    NativeAOT (native) or standard .NET (polyplug_dotnet)
Python      host + guest    host: ctypes; guest: polyplug_python adapter
Lua         host + guest    host: LuaJIT FFI; guest: polyplug_lua adapter
JavaScript  host: Deno only (Deno.dlopen)
            guest: QuickJS (polyplug_js)
TypeScript  same as JS
```

**6 host languages. 6 guest runtime types.**

QuickJS cannot be a host — it is an embedded VM with no standalone executable,
no FFI mechanism to load external shared libraries. Only Deno (which has
`Deno.dlopen` and `--allow-ffi`) can be a JS host. This is a fundamental
property of the runtimes, not a limitation of polyplug.

**C# — two modes, same generated code:**

```
NativeAOT       runtime = "native"    native loader, no adapter needed
standard .NET   runtime = "dotnet"    polyplug_dotnet required, CLR via hostfxr
```

**JS/TS — one guest runtime, one host:**

```
js-quickjs    QuickJS embedded in-process    guest only   ~50-200ns/call, +~1MB
Deno          standalone executable          host only    Deno.dlopen
```

Plugin developer sets `runtime = "js-quickjs"` in `bundle.toml`.
App developer registers the adapter via loader registration FFI.

---

## 24. Package Ecosystem

### Design principle

Follows the EntityFramework/serde opt-in model. The core runtime package is always
present. Each loader is a separate optional package — app developers install only the
loaders they actually need. A C# host app that wants Python and Lua guests installs
three packages total: the core runtime, the Python loader, and the Lua loader. Nothing
else is linked, nothing else is loaded.

Every loader package is a thin write-and-forget wrapper around the C ABI. The actual
loader logic lives once in Rust. The language-specific package contains only:
- A config struct and one registration function call (~20 lines)
- The pre-built native shared library for the target platform

The native shared library extension is platform-specific:
`.so` on Linux, `.dylib` on macOS, `.dll` on Windows. Every package ships all three.

### Package count

4 loaders × 6 host languages = 24 loader packages total. This sounds large but each is
trivial to maintain — they change only when the underlying loader's config API changes,
which is rare. The precedent is strong: SQLite, libsodium, libzmq all follow the same
pattern across all language ecosystems with no complaints about package count.

### Rust (crates.io)

```
polyplug              runtime core + Rust host lib + C facade
polyplug_guest        Rust guest lib
polyplug_codegen      codegen library (programmatic API into polyplugc logic)
polyplugc             CLI codegen tool

Loader crates — each produces rlib (Rust hosts) + cdylib (non-Rust hosts):
polyplug_loaders_dotnet    .NET loader
polyplug_loaders_python    Python loader
polyplug_loaders_lua       Lua loader
polyplug_loaders_js        QuickJS loader  (runtime = "js-quickjs")
```

Rust app developer example — C# host wanting Python + Lua guests:
```toml
[dependencies]
polyplug                 = "1"
polyplug_loaders_python  = "1"
polyplug_loaders_lua     = "1"
```

### C++ (vcpkg / Conan / release archive)

```
polyplug                  core runtime headers + libpolyplug
polyplug-guest            guest lib headers
polyplug-loaders-dotnet   .NET loader header + native binary
polyplug-loaders-python   Python loader header + native binary
polyplug-loaders-lua      Lua loader header + native binary
polyplug-loaders-js       QuickJS loader header + native binary
```

C++ app developer example — wanting Python + Lua guests:
```json
// vcpkg.json
{
  "dependencies": ["polyplug", "polyplug-loaders-python", "polyplug-loaders-lua"]
}
```
```cpp
#include <polyplug/runtime.hpp>
#include <polyplug/loaders/python.hpp>
#include <polyplug/loaders/lua.hpp>

auto rt = polyplug::Runtime::build();
polyplug::register_python_loader(rt);
polyplug::register_lua_loader(rt);
```

### .NET (NuGet)

```
Polyplug                  core runtime + C# host lib
Polyplug.Guest            C# guest lib
Polyplug.Loaders.Dotnet   .NET loader + native binary (runtimes/*/native/)
Polyplug.Loaders.Python   Python loader + native binary
Polyplug.Loaders.Lua      Lua loader + native binary
Polyplug.Loaders.Js       QuickJS loader + native binary
```

NuGet bundles native binaries via the `runtimes/<rid>/native/` convention.
MSBuild copies the correct platform binary to the output directory automatically.

C# app developer example — wanting Python + Lua guests:
```
dotnet add package Polyplug
dotnet add package Polyplug.Loaders.Python
dotnet add package Polyplug.Loaders.Lua
```
```csharp
var rt = new Polyplug.Runtime();
rt.RegisterPythonLoader();
rt.RegisterLuaLoader();
```

### Python (pip)

```
polyplug                  core runtime + Python host lib
polyplug-guest            Python guest lib
polyplug-loaders-dotnet   .NET loader + native binary (wheel)
polyplug-loaders-python   Python loader + native binary
polyplug-loaders-lua      Lua loader + native binary
polyplug-loaders-js       QuickJS loader + native binary
```

Python wheels bundle native binaries directly. `ctypes.CDLL` resolves them
from the wheel-installed path at runtime.

Python app developer example — wanting Python + Lua guests:
```
pip install polyplug polyplug-loaders-python polyplug-loaders-lua
```
```python
import polyplug
from polyplug.loaders.python import register_python_loader
from polyplug.loaders.lua import register_lua_loader

rt = polyplug.Runtime()
register_python_loader(rt)
register_lua_loader(rt)
```

### Lua (LuaRocks / release archive)

```
polyplug                  core runtime + Lua host lib (LuaJIT FFI)
polyplug-guest            Lua guest lib
polyplug-loaders-dotnet   .NET loader + native binary
polyplug-loaders-python   Python loader + native binary
polyplug-loaders-lua      Lua loader + native binary
polyplug-loaders-js       QuickJS loader + native binary
```

Lua app developer example — wanting Python + Lua guests:
```
luarocks install polyplug
luarocks install polyplug-loaders-python
luarocks install polyplug-loaders-lua
```
```lua
local polyplug = require("polyplug")
local rt = polyplug.runtime_new()
polyplug.register_python_loader(rt)
polyplug.register_lua_loader(rt)
```

### JS/TS — Deno (JSR / release archive)

```
@polyplug/core            Deno.dlopen host lib (polyplug.ts)
@polyplug/guest           shared guest lib (js-quickjs)
@polyplug/loaders-dotnet  .NET loader registration + native binary setup
@polyplug/loaders-python  Python loader registration + native binary setup
@polyplug/loaders-lua     Lua loader registration + native binary setup
@polyplug/loaders-js      QuickJS loader registration + native binary setup
```

Deno packages (JSR) contain the TypeScript wrapper. Native binaries are downloaded
for the current platform via `deno task setup` from the GitHub release — the standard
Deno pattern for packages with native dependencies.

Deno app developer example — wanting Python + Lua guests:
```json
// deno.json
{
  "imports": {
    "@polyplug/core": "jsr:@polyplug/core@1",
    "@polyplug/loaders-python": "jsr:@polyplug/loaders-python@1",
    "@polyplug/loaders-lua": "jsr:@polyplug/loaders-lua@1"
  },
  "tasks": {
    "setup": "deno run -A jsr:@polyplug/core/setup python lua"
  }
}
```
```typescript
import { runtimeNew, loadBundle } from "@polyplug/core";
import { registerPythonLoader } from "@polyplug/loaders-python";
import { registerLuaLoader } from "@polyplug/loaders-lua";

const rt = runtimeNew();
registerPythonLoader(rt);
registerLuaLoader(rt);
```

### Loader FFI internals (relevant to contributors, not app developers)

Each loader crate produces both `rlib` (used by Rust hosts via crate API) and
`cdylib` (used by all non-Rust hosts via the C ABI). The cdylib exports:

```
polyplug_*_loader_create(config)  → *mut c_void   (opaque Box<dyn BundleLoader>)
polyplug_*_loader_free(ptr)       → void           (discard without registering)
```

The runtime core (`libpolyplug`) exports:

```
polyplug_runtime_register_loader(rt, loader_ptr) → u32  (0 = ok)
```

Non-Rust host libs call create → register in two steps. The language-specific
wrapper in each loader package reduces this to a single idiomatic function call.

NOTE: `sdks/js/host/` targets Deno as the host runtime (Deno.dlopen into libpolyplug).
QuickJS cannot be a standalone host — it is an embedded VM that runs inside a Rust process.

---

## 25. C Facade — Stable Host API for FFI Consumers

`sdks/lua/host/` and `sdks/js/host/` both call into `libpolyplug.so` via FFI (LuaJIT FFI and `Deno.dlopen` respectively). They cannot use the Rust-native API surface. A thin stable `extern "C"` facade is therefore added to `crates/polyplug/src/ffi/mod.rs` and exported from `lib.rs`.

**Design rules:**
- All symbols prefixed `polyplug_`
- No Rust types cross the boundary — only primitives, pointers, and ABI structs
- `PluginHandle` packed as `u64`: `(generation as u64) << 32 | index as u64`
- Errors reported via `polyplug_last_error()` thread-local string — never panics across FFI
- Runtime pointer opaque (`*mut OpaqueRuntime`) — consumers never dereference it

**Exported symbols:**

```c
// Lifecycle
OpaqueRuntime* polyplug_runtime_create(void);
void           polyplug_runtime_destroy(OpaqueRuntime* rt);

// Loader registration — used by loader cdylibs (libpolyplug_loaders_dotnet.so etc.)
// loader_ptr is a Box<dyn BundleLoader> as *mut c_void produced by
// polyplug_*_loader_create(). Calling this transfers ownership of loader_ptr.
uint32_t polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);

// Bundle loading
uint32_t polyplug_runtime_load_bundle(OpaqueRuntime* rt,
                                       const uint8_t* path, size_t path_len);
uint32_t polyplug_runtime_reload_bundle(OpaqueRuntime* rt,
                                         const uint8_t* path, size_t path_len);

// Discovery
uint64_t polyplug_runtime_find_by_contract(OpaqueRuntime* rt,
                                            uint64_t contract_id,
                                            uint32_t min_version);
uint64_t polyplug_runtime_find_by_bundle(OpaqueRuntime* rt,
                                          uint64_t bundle_id,
                                          uint64_t contract_id,
                                          uint32_t min_version);
size_t   polyplug_runtime_find_all_by_contract(OpaqueRuntime* rt,
                                                uint64_t contract_id,
                                                uint32_t min_version,
                                                uint64_t* out,
                                                size_t out_cap);

// Vtable access
OpaquePluginGuard* polyplug_runtime_resolve_plugin(OpaqueRuntime* rt,
                                                    uint64_t packed_handle);
void               polyplug_runtime_plugin_release(OpaquePluginGuard* guard);
const void*        polyplug_runtime_plugin_vtable(OpaquePluginGuard* guard);

// Error retrieval — UTF-8, caller-provides-buffer
size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
size_t polyplug_runtime_error_message_len(void);
```

**Plugin ABI (guest exports — called by runtime on dlopen):**

```c
// Version sentinel — called first, must return 1
uint32_t polyplug_abi_version(void);

// Plugin constructor — registers vtables with the runtime
AbiError polyplug_init(PluginRegistrar* registrar, const PluginContext* ctx);
```

**Performance:** The C facade is a zero-overhead shim — each function is a direct call into the existing Rust runtime with no additional allocation or logic. LuaJIT JIT-compiles these calls to direct indirect calls (bypassing PLT). Deno uses V8 Fast API calls for non-BigInt parameters (<10ns) and the standard call path for BigInt u64 parameters (~150ns).

---

## 26. Future Work

**Near term:**
- Async plugin execution
- WASM sandbox support
- Plugin marketplace tooling (signing, verification, distribution)

**Planned epics (already designed):**
- Hot-reload — Epic 17 ✅ done
- PluginContext + ABI type helpers — Epic 20 ✅ done
- Lua host lib + Deno host lib + C facade — Epic 21 ✅ done

**Long term:**
- Distributed plugins
- Remote plugin execution
- Permission system
- polyplug-jvm (Java/Kotlin via JNI embedding)
- Swift support (native, no adapter needed)
- Zig support (native, no adapter needed)

---

## 27. Non-Goals

- UI frameworks
- Build systems
- Language compilers
- Asset pipelines
- Networking libraries
- Serialization formats (beyond ABI needs)
- Out-of-process plugin execution (IPC adds unacceptable overhead, violates performance goal)
- Plugin crash isolation (a plugin segfault kills the host process — by design; app developers needing crash isolation must run plugins in a separate process with their own IPC layer; polyplug does not provide this)

---

## 28. Hot-Reload Architecture

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
Runtime loads new_B.so (via correct loader), runs init, gets new PluginInterface*
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
├── sdks/
│   ├── cpp/
│   │   ├── host/
│   │   │   ├── polyplug.hpp
│   │   │   └── polyplug/
│   │   │       ├── abi.hpp
│   │   │       ├── error.hpp
│   │   │       ├── handle.hpp
│   │   │       └── runtime.hpp
│   │   └── guest/
│   │       ├── polyplug_guest.hpp
│   │       └── polyplug/
│   │           ├── abi.hpp
│   │           ├── contract.hpp
│   │           └── guest.hpp
│   ├── csharp/
│   │   ├── host/                          Polyplug NuGet
│   │   └── guest/                         Polyplug.Guest NuGet
│   ├── python/
│   │   ├── host/                          polyplug pip
│   │   └── guest/                         polyplug-guest pip
│   ├── lua/
│   │   ├── host/                          polyplug.lua + .so
│   │   └── guest/                         polyplug-guest.lua
│   └── js/
│       ├── host/                          @polyplug/core JSR
│       └── guest/                         @polyplug/guest JSR
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
