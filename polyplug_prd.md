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

The ABI is the frozen contract between the runtime and all plugins. It uses C calling conventions. It is minimal by design. Once v1 is released it never changes.

**Core ABI functions:**

```c
// Memory
void*  host_alloc(size_t size);
void   host_free(void* ptr);

// Plugin discovery
PluginHandle find_plugin(uint64_t contract_id, uint32_t min_version);

// Cross-plugin dispatch
AbiError call_plugin(
    PluginHandle plugin,
    uint32_t     function_id,
    void*        args,
    void*        out
);

// Extension lookup
const void* get_extension(uint32_t extension_id);
```

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

typedef void* PluginHandle;  // opaque, managed by runtime
```

**Rules:**
- All strings crossing the ABI boundary are UTF-8
- All structs use `#[repr(C)]` on the Rust side and standard C struct layout on the C side
- Primitives (u8–u64, f32, f64, bool) are returned directly by value
- All non-primitive return values use caller-provides-buffer pattern
- Pointers passed across boundary always point into host allocator

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
Plugin builds PluginVTable (its functions for host to call)
        │
        ▼
Plugin calls registrar->register() passing PluginVTable ptr
        │
        ▼
Host stores PluginVTable ptr
        │
        ▼
Load complete. All future calls = one indirect call.
```

This exchange is identical regardless of what language the plugin is written in. The native loader uses dlopen. The dotnet loader uses hostfxr. The python loader uses CPython embedding. The lua loader uses the Lua VM. All arrive at the same vtable exchange. The host never knows what language loaded the plugin.

**HostVTable — given to every plugin at init:**

```c
typedef struct {
    void*        (*alloc)(size_t size);
    void         (*free)(void* ptr);
    PluginHandle (*find_plugin)(uint64_t contract_id, uint32_t min_version);
    AbiError     (*call_plugin)(PluginHandle, uint32_t fn_id, void* args, void* out);
    const void*  (*get_extension)(uint32_t extension_id);
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

**Hot path call:**

```c
// Developer writes:
Stats stats = image_stats.compute(image);

// Generated code does:
Stats stats;  // caller allocates on host allocator
AbiError err = vtable->functions[COMPUTE_FN_ID](&image, &stats);
```

One pointer dereference. One indirect call. Nothing else.

---

## 8. Host Libraries

Host libs are the ergonomic layer that app developers actually use. They wrap the runtime C ABI in a natural, idiomatic way for each language. They are thin by design.

```
App Developer Code
        ↓
Generated Host Callers      (polyplugc output, contract-specific)
        ↓
Host Lib                    (language wrapper, thin)
        ↓
Runtime C ABI               (polyplug core)
```

**Per language:**

```
Rust    → polyplug crate IS the runtime core, no separate wrapper needed
C++     → host-libs/cpp/ header-only, RAII wrappers, zero overhead
C#      → Polyplug NuGet, P/Invoke declarations, ref structs
Python  → polyplug pip package, ctypes wrappers
Lua     → polyplug.lua + C extension binding
```

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

Guest libs are the thin bootstrap layer that every plugin is built on top of.

**Responsibilities:**
- Plugin entry point macro / attribute
- Host allocator hookup
- Panic / exception boundary
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
    .loader(DotnetLoader::new())    // from polyplug-dotnet
    .loader(PythonLoader::new())    // from polyplug-python
    .loader(LuaLoader::new())       // from polyplug-lua
    .init()?;
```

If a bundle's manifest declares `runtime = "lua"` but no Lua loader is registered:

```
Error: bundle "my_lua_plugin" requires runtime "lua"
but no loader is registered for runtime "lua".
Add polyplug-lua as a dependency and register LuaLoader at init.
```

Native plugins (Rust, C++, C# NativeAOT) require no adapter. The built-in native loader in `polyplug` handles them via dlopen.

---

### polyplug-dotnet

Enables loading of standard .NET C# plugins without NativeAOT.

Uses Microsoft's `hostfxr` native API to initialize the CLR once per process, then uses `load_assembly_and_get_function_pointer` to obtain a native-callable pointer to the managed `Init` method.

```
polyplug-dotnet calls hostfxr
        ↓
hostfxr initializes CLR (once per process, shared across all .NET plugins)
        ↓
hostfxr loads managed assembly
        ↓
hostfxr returns fn ptr to [UnmanagedCallersOnly] Init method
        ↓
Init(registrar) called — identical vtable exchange from here
```

The C# plugin's Init method (generated by polyplugc):

```csharp
[UnmanagedCallersOnly(EntryPoint = "init")]
public static unsafe void Init(PluginRegistrar* registrar) {
    // register plugins, hand over vtable
}
```

`[UnmanagedCallersOnly]` works in standard .NET, not just NativeAOT.

**Performance:**
- CLR startup: one-time cost at first .NET plugin load (~100–500ms)
- Per-call: one managed/unmanaged transition (~50–200ns vs ~1ns native)
- Subsequent .NET plugins share the already-running CLR — fast load
- Full .NET framework available: reflection, EF, anything managed

**C# bundle manifest:**

```toml
runtime = "dotnet"    # standard .NET, requires polyplug-dotnet
# or absent           # NativeAOT, loaded by native loader, no adapter needed
```

---

### polyplug-python

Enables loading of Python plugins via CPython embedding.

Uses `pyo3` or direct `libpython` linking to initialize a Python interpreter (once per process), import the plugin module, and extract the `init` callable. The Python `init` function calls back through ctypes to register vtables.

**Performance:** Python interpreter is the bottleneck, not polyplug.

---

### polyplug-lua

Enables loading of Lua plugins via LuaJIT or standard Lua embedding.

Uses `mlua` to initialize a Lua VM, load the plugin script, and call `init`. LuaJIT is strongly recommended for near-native performance via JIT compilation.

---

## 11. Schema Files

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

---

### bundle.toml — owned by Plugin Developer

Defines the contents of a plugin bundle. Written by plugin developer. Never distributed.

```toml
# bundle.toml

[bundle]
name    = "image_bundle"
version = "1.0"
runtime = "native"    # native (default) | dotnet | python | lua

api = "path/to/api.toml"
# or
api = "my_app_sdk"    # if installed as a package

[[plugin]]
name       = "ImageDecoder"
version    = "1.2.0"
implements = ["image.decode@1.0"]
requires   = []
optional   = ["trace"]

[[plugin]]
name       = "ImageStats"
version    = "1.0.0"
implements = ["image.stats@1.0"]
requires   = ["image.decode@1.0"]
optional   = ["trace"]
```

**What polyplugc generates from bundle.toml:**

```
generated/
├── init.rs / init.cpp / Init.cs     bundle entry point
├── vtables.rs / vtables.cpp         vtable structs and registration
├── contracts.rs / contracts.cpp     traits / interfaces to implement
├── types.rs / types.cpp             domain types from api.toml
└── manifest.toml                    discovery manifest (auto-generated)
```

---

### manifest.toml — generated, not hand-written

Auto-generated by polyplugc. Placed next to the compiled bundle. Runtime reads this for fast pre-load discovery without loading.

```toml
# image_bundle.manifest.toml — GENERATED, never edit by hand

name           = "image_bundle"
version        = "1.0"
runtime        = "native"
file           = "image_bundle.so"
provides       = ["image.decode@1.0", "image.stats@1.0"]
requires       = ["image.decode@1.0"]
function_count = { "image.decode@1.0" = 2, "image.stats@1.0" = 2 }
```

The `runtime` field tells discovery which loader to dispatch to.

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
Scans configured directories. Recognizes `.so/.dll/.dylib` for compiled bundles and directories for script bundles. Configured at runtime init in code, no config file.

**Layer 2 — Manifest reading**
Reads companion `manifest.toml` before any loading. Extracts: name, version, runtime, provides, requires, function_count. The `runtime` field determines which loader handles this bundle.

**Layer 3 — Capability graph resolution**
1. Collects all provided capabilities
2. Validates all requires are satisfied
3. Detects cycles
4. Topological sort for initialization order

Fails with clear error before loading anything if requirements unmet.

**Layer 4 — Explicit registration**

```rust
runtime.load_bundle("./plugin.so")?;
runtime.load_bundle_with("./plugin.so", LoadOptions {
    compatibility: Compatibility::Relaxed,
})?;
```

---

## 14. Cross-Plugin Communication

Plugins never link to each other. All calls route through host dispatcher.

```
Plugin A → host->call_plugin(handle, fn_id, args, out) → Plugin B vtable
```

Handle resolved once at init via `find_plugin`. Every subsequent call is one indirect call. Works identically regardless of what languages A and B are written in.

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
Rust    host + guest    native, near zero overhead
C++     host + guest    header-only libs, near zero overhead
C#      host + guest    NativeAOT (native loader) or standard .NET (polyplug-dotnet)
Python  host + guest    ctypes, via polyplug-python
Lua     host + guest    LuaJIT recommended, via polyplug-lua
```

**C# — two modes, same generated code:**

```
NativeAOT       runtime = "native"    native loader, native performance, no adapter
standard .NET   runtime = "dotnet"    polyplug-dotnet required, CLR via hostfxr
```

Plugin developer chooses in bundle.toml. App developer ensures the right adapter is registered.

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
└── polyplugc             CLI codegen tool

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
```

---

## 25. Future Work

**Near term:**
- Hot reload
- Async plugin execution
- WASM sandbox support
- Plugin marketplace tooling (signing, verification, distribution)

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