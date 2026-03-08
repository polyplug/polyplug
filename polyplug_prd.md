# polyplug — PRD v2

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
10. Schema Files
11. Code Generation Pipeline
12. Plugin Discovery
13. Cross-Plugin Communication
14. Memory Model
15. Error Handling
16. Plugin Versioning and Compatibility
17. Extension System
18. Security Model
19. Developer Experience — App Developer
20. Developer Experience — Plugin Developer
21. The Full Runtime Flow
22. MVP Language Support
23. Future Work
24. Non-Goals

---

## 1. Vision

Build the **universal plugin system**. A schema-driven, cross-language plugin runtime platform where any language can be a host and any language can be a guest — with zero performance penalty regardless of the combination.

The north star use case is game engines: a game engine written in C++ should be able to load plugins written in Rust, C#, Python, or Lua at native speed, through a single unified system, without any language-specific special casing.

The platform is designed around one principle above all others: **performance over everything**. The hot path — calling a plugin function — must compile down to a single indirect function call. Nothing more.

---

## 2. Actors

Three distinct actors interact with this platform.

**YOU — the runtime author**
Builds and maintains the runtime core, host libs, guest libs, and the codegen CLI. Ships no schema files. Everything lives in source code.

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
App initializes runtime
Runtime scans plugin dirs
Reads manifest.toml (fast, no dlopen)
Resolves capability graph
dlopen in correct order
Calls generated init()
Vtables registered
Ready — all future calls are one indirect call
```

---

## 5. Runtime Core

The runtime core is written in **Rust**. It is the heart of the system, responsible for everything that happens between app startup and the first plugin call.

**Responsibilities:**

- Loading plugin bundles via platform dynamic loading (dlopen / LoadLibrary)
- Reading and validating manifest files before loading
- Building the capability graph and determining initialization order
- Detecting dependency cycles
- Managing the host allocator
- Storing and serving registered plugin vtables
- Dispatching cross-plugin calls
- Managing extensions
- Enforcing compatibility rules

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
- All structs use `#[repr(C)]` on the Rust side and standard C struct layout (no packing attributes) on the C side
- Primitives (u8–u64, f32, f64, bool) are returned directly by value
- All non-primitive return values use caller-provides-buffer pattern
- Pointers passed across boundary always point into host allocator

---

## 7. VTable System

The vtable system is how plugins and host exchange callable function pointers. It is the mechanism that makes the hot path a single indirect call.

**Exchange happens once at load time:**

```
Host loads bundle
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

**Hot path call — what actually happens at runtime:**

```c
// Host side generated code — developer writes:
Stats stats = image_stats.compute(image);

// Generated code does exactly this:
Stats stats;  // caller allocates on host allocator
AbiError err = vtable->functions[COMPUTE_FN_ID](&image, &stats);
```

One pointer dereference. One indirect call. Nothing else.

---

## 8. Host Libraries

Host libs are the ergonomic layer that app developers actually use. They wrap the runtime C ABI in a natural, idiomatic way for each language. They are thin by design — the heavy logic is in the runtime core.

The generated host-side code from `polyplugc` sits on top of the host lib. The host lib provides the foundation; generated code provides the contract-specific callers.

```
App Developer Code
        ↓
Generated Host Callers      (polyplugc output, contract-specific)
        ↓
Host Lib                    (your language wrapper, thin)
        ↓
Runtime C ABI
```

**Per language:**

```
Rust    → crate, thin, mostly re-exports runtime directly
C++     → header-only, RAII wrappers, zero overhead
C#      → NuGet package, P/Invoke declarations, ref structs
Python  → pip package, ctypes wrappers
Lua     → .lua file + C extension binding
```

**App developer runtime initialization (always in code, no config file):**

```rust
// Rust example
let runtime: PluginRuntime = PluginRuntime::new()
    .plugin_dirs(["./plugins", "~/.myapp/plugins"])
    .compatibility(Compatibility::Strict)
    .extension(TraceExtension::new())
    .init()?;
```

```cpp
// C++ example
auto runtime = PluginRuntime::builder()
    .plugin_dir("./plugins")
    .compatibility(Compatibility::Strict)
    .extension(std::make_unique<TraceExtension>())
    .init();
```

```csharp
// C# example
var runtime = PluginRuntime.Builder()
    .PluginDir("./plugins")
    .Compatibility(Compatibility.Strict)
    .Extension(new TraceExtension())
    .Init();
```

---

## 9. Guest Libraries

Guest libs are the thin bootstrap layer that every plugin is built on top of. They handle everything a plugin needs to function before any business logic runs. They are distributed as language-specific packages.

**Responsibilities:**
- Plugin entry point macro / attribute
- Host allocator hookup (so all allocations go through host_alloc)
- Panic / exception boundary (so plugin crash cannot take down host)
- ABI primitive types (StringView, Buffer, AbiError, PluginError, etc.)
- Basic FFI safety helpers

**Guest lib is the foundation. Generated code from polyplugc sits on top of it:**

```
Plugin Dev Business Logic       (implements contract traits)
        ↓
Generated ABI Wrappers          (polyplugc output, contract-specific)
        ↓
Guest Lib                       (your language bootstrap, thin)
        ↓
Runtime C ABI
```

**Per language:**

```
Rust    → crate with proc macro for entry point, allocator hook
C++     → header-only, entry point macro, RAII helpers
C#      → NuGet, entry point attribute, P/Invoke helpers
Python  → pip package, entry point decorator, ctypes helpers
Lua     → .lua file, entry point registration helper
```

---

## 10. Schema Files

There are exactly **two schema files** in the entire system. Both are TOML.

---

### api.toml — owned by App Developer

Defines the complete plugin API for their ecosystem. Contains domain types and contracts. This is the single source of truth for what plugins can do in their ecosystem.

App developer writes it. App developer distributes it to plugin developers (directly or bundled inside the published guest SDK package).

```toml
# api.toml

# Domain types — used in contract function signatures
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

[[type]]
name = "AudioFrame"
fields = [
    { name = "samples",     type = "Buffer" },
    { name = "sample_rate", type = "u32" },
    { name = "channels",    type = "u8" }
]

# Contracts — the plugin API
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


[[contract]]
name    = "audio.decode"
version = "1.0"

[[contract.functions]]
name    = "decode"
params  = [{ name = "raw", type = "Buffer" }]
returns = "AudioFrame"
```

**Primitive types available in any api.toml without declaration:**

These come from the runtime and are always available:

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

Defines the contents of a plugin bundle. Declares which plugins are in the bundle, which contracts they implement, and what they depend on.

Plugin developer writes it. It is the single input codegen needs to generate the entire bundle — entry point, vtables, ABI wrappers, and discovery manifest.

```toml
# bundle.toml

[bundle]
name    = "image_bundle"
version = "1.0"

# Points to the app's api.toml
# Can be a local path or a published package name
api = "path/to/api.toml"
# or
api = "my_app_sdk"   # if installed as a package

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

The plugin developer implements only the contract traits. Everything else is generated.

---

### manifest.toml — generated, not hand-written

Auto-generated by polyplugc at build time. Placed next to the compiled bundle. The runtime reads this file for fast pre-load discovery without dlopen.

```toml
# image_bundle.manifest.toml — GENERATED, never edit by hand

name           = "image_bundle"
version        = "1.0"
file           = "image_bundle.so"
provides       = ["image.decode@1.0", "image.stats@1.0"]
requires       = ["image.decode@1.0"]
function_count = { "image.decode@1.0" = 2, "image.stats@1.0" = 2 }
```

---

## 11. Code Generation Pipeline

`polyplugc` is a standalone CLI binary written in Rust. It is the only codegen tool. It reads schema files and generates all glue code for all supported languages.

Being a standalone binary means it works with any build system: cargo, cmake, msbuild, make, or any custom pipeline.

**The side is inferred from the schema file passed — there is no `--side` flag:**

- `--api` → app developer context → generates **both** `host/` and `guest/` output
- `--bundle` → plugin developer context → generates guest bundle code only

**Internal pipeline:**

```
api.toml  OR  bundle.toml
        │
        ▼
Schema Parser
        │
        ▼
Intermediate Representation (IR)
[Rust structs, serde-based]
        │
        ▼
Language Generators
[one generator per language, implements CodeGenerator trait]
        │
        ▼
Generated Files
```

**IR structure:**

```rust
struct IR {
    types:     Vec<TypeDef>,
    contracts: Vec<Contract>,
    bundle:    Option<BundleManifest>,
}

struct Contract {
    name:      String,
    version:   Version,
    functions: Vec<Function>,
}

struct Function {
    name:    String,
    params:  Vec<Param>,
    returns: Option<TypeRef>,  // None = void
}
```

**Generator trait — adding a new language is implementing one trait:**

```rust
trait CodeGenerator {
    fn generate_host(&self, ir: &IR) -> GeneratedFiles;
    fn generate_guest(&self, ir: &IR) -> GeneratedFiles;
}

struct RustGenerator;
struct CppGenerator;
struct CSharpGenerator;
struct PythonGenerator;
struct LuaGenerator;
```

**CLI usage:**

```bash
# App developer — pass --api to generate BOTH host callers AND guest SDK
# Output is split into host/ and guest/ subdirectories automatically
polyplugc generate \
    --api api.toml \
    --lang cpp \
    --out ./generated
# produces:
#   ./generated/host/    ← type-safe callers for app developer
#   ./generated/guest/   ← SDK distributed to plugin developers

# Plugin developer — pass --bundle to generate full bundle glue
polyplugc generate \
    --bundle bundle.toml \
    --lang rust \
    --out ./src/generated
# produces:
#   ./src/generated/types.rs
#   ./src/generated/contracts.rs
#   ./src/generated/vtables.rs
#   ./src/generated/init.rs
#   ./src/generated/manifest.toml

# Validate schema without generating
polyplugc validate --api api.toml
polyplugc validate --bundle bundle.toml
```

**Rust build.rs integration:**

```rust
// build.rs — automatic regeneration when schema changes
fn main() {
    polyplugc::build::generate()
        .bundle("bundle.toml")
        .lang(Lang::Rust)
        .output("src/generated")
        .watch()  // reruns if bundle.toml or api.toml changes
        .run();
}
```

**Code generation rules (baked into codegen, not schema annotations):**

- Non-primitive params → always passed by reference
- Non-primitive return values → caller-provides-buffer, hidden from developer
- Primitive return values (u8–u64, f32, f64, bool) → returned directly by value
- Every ABI call wrapped in panic/exception catch on guest side
- All cross-boundary strings are UTF-8; GC language boundaries transcode automatically
- All cross-boundary structs live in host allocator; GC language wrappers are ref structs / stack-only

---

## 12. Plugin Discovery

Discovery happens in four layers. The runtime works through them in order.

**Layer 1 — Directory scanning**

Runtime scans configured directories for bundle files:

```
.so / .dll / .dylib    compiled bundles
directories            script bundles (Python, Lua)
```

Configured at runtime init in code. No config file.

**Layer 2 — Manifest reading (fast pre-load)**

For every bundle found, runtime reads the companion `manifest.toml` before calling dlopen. This is a fast file read — no dynamic loading, no code execution.

Runtime uses manifest to:
- Know what the bundle provides and requires
- Decide whether to load it at all
- Build the capability graph

**Layer 3 — Capability graph resolution**

After reading all manifests, runtime:

1. Collects all provided capabilities
2. Validates all required capabilities are satisfied
3. Detects dependency cycles
4. Determines initialization order (topological sort)

If any required capability is missing, runtime fails with a clear error before loading anything.

**Layer 4 — Explicit registration**

App developer can also load bundles explicitly:

```rust
runtime.load_bundle("./specific_plugin.so")?;
runtime.load_bundle_with("./plugin.so", LoadOptions {
    compatibility: Compatibility::Relaxed,
})?;
```

---

## 13. Cross-Plugin Communication

Plugins never link to each other directly. All cross-plugin calls route through the host runtime dispatcher.

**Call flow:**

```
Plugin A
    │ generated SDK wrapper
    ▼
host->call_plugin(handle, function_id, args, out)
    │ runtime dispatch
    ▼
Plugin B vtable->functions[function_id](args, out)
```

Plugin A resolves Plugin B's handle once at init time via `find_plugin`. After that every call is one indirect call through the stored handle.

**From plugin A's perspective (generated code):**

```rust
// At init — resolve once, store handle
let decoder_handle: PluginHandle = host.find_plugin(IMAGE_DECODE_CONTRACT_ID, VERSION_1_0);

// At call time — one indirect call
let image: Image = decoder.decode(&raw_bytes)?;
```

**If caller and callee are in the same bundle**, the call still routes through host dispatch. This keeps the model consistent and simple. The overhead is one indirect call — acceptable given the simplicity gained.

---

## 14. Memory Model

All memory that crosses a plugin boundary lives in the host allocator. This is the single most important rule of the memory model.

**Rules:**

1. All cross-boundary allocations use `host_alloc` / `host_free`
2. Caller always allocates the output buffer, callee fills it
3. A plugin must never free memory it did not allocate
4. Large shared buffers (images, audio frames) are passed by reference — never copied
5. GC languages never put cross-boundary data on their managed heap

**Per language implementation:**

```
Rust    zero cost — host allocator is just the global allocator
C++     placement new into host_alloc buffer
C#      ref struct wrapping unmanaged ptr, StructLayout.Sequential
            Marshal.PtrToStructure only for reading, never for ownership
Python  ctypes.Structure — lives in C memory, Python GC never sees it
Lua     lightuserdata pointing into host allocator
```

**String model:**

All strings at the ABI level are `StringView` — a UTF-8 byte slice with pointer and length. Non-owning. No null terminator.

```
Rust    &str → StringView: zero cost, just ptr+len
C++     std::string_view → StringView: zero cost
C#      string → StringView: transcode UTF-16 → UTF-8 into host_alloc
                             ASCII fast path: near zero cost (strip high byte)
Python  str → StringView: encode to UTF-8 bytes, pass ptr+len
Lua     string → StringView: already byte array, just ptr+len
```

The ASCII fast path is significant: most strings in plugin systems (names, paths, identifiers) are ASCII. UTF-16 to UTF-8 for ASCII is just byte narrowing — near zero cost.

**Caller-provides-buffer in practice:**

```c
// What the ABI looks like:
AbiError image_stats_compute(
    const Image* image,   // in — caller owns
    Stats*       out      // out — caller allocated, callee fills
);

// What the developer sees (generated code hides the out param):
Stats stats = image_stats.compute(image);
```

---

## 15. Error Handling

**Two levels of errors:**

**Level 1 — Recoverable errors**
Business logic errors. Plugin returns an AbiError with a non-zero code and a message. Generated code translates to native error style per language.

**Level 2 — Unrecoverable errors**
Panics, exceptions, crashes inside plugin code. Generated ABI wrapper catches everything at the boundary and converts to an AbiError. A crashing plugin cannot take down the host.

**ABI error representation:**

```c
typedef struct {
    uint32_t   code;     // 0 = success, non-zero = error
    StringView message;  // empty if success
} AbiError;
```

**PluginError — the shared error type for all contracts:**

`PluginError` is a single shared type defined in the `polyplug-guest` crate. It is not generated per-contract. All generated contract traits use `Result<T, polyplug_guest::PluginError>`. This keeps error handling consistent and eliminates per-contract codegen complexity.

```rust
// Defined once in polyplug-guest crate — never generated
pub struct PluginError {
    pub code:    u32,
    pub message: String,
}
```

**Per language native error style (generated code handles translation):**

```rust
// Rust — Result using shared PluginError from polyplug-guest
fn compute(&self, image: &Image) -> Result<Stats, polyplug_guest::PluginError>

// C++ — exceptions, host callers throw PolyplugException on non-zero AbiError
// matches the exception model already used in guest-libs
Stats compute(const Image& image);  // throws PolyplugException on error

// C# — exception
Stats Compute(Image image);  // throws PluginException on error

// Python — exception
def compute(self, image: Image) -> Stats:  # raises PluginError

// Lua — multiple return
local stats, err = plugin.compute(image)
```

**Generated guest wrapper (same pattern for all languages):**

```rust
// Generated — developer never writes this
extern "C" fn compute_abi(image: *const Image, out: *mut Stats) -> AbiError {
    std::panic::catch_unwind(|| {
        let result: Result<Stats, polyplug_guest::PluginError> = IMPL.compute(unsafe { &*image });
        match result {
            Ok(stats) => { unsafe { *out = stats }; ABI_OK }
            Err(e)    => e.into_abi_error()
        }
    }).unwrap_or(ABI_ERROR_PANIC)
}
```

---

## 16. Plugin Versioning and Compatibility

**Versioning is at the contract level.** Plugin version and contract version are independent.

```toml
[[contract]]
name    = "image.decode"
version = "1.0"          # this is the CONTRACT version
```

**Semantic versioning rules:**

- Minor version bump → adds functions, backward compatible
- Major version bump → breaking change, treated as different contract
- Host requests contract by name + minimum version
- Plugin provides contract at a specific version
- Compatible if provided version >= required version (same major)

**Function count validation:**

The generated `manifest.toml` includes function count per contract. At load time the runtime validates this count matches what the schema expects. A mismatch indicates a contract version problem and is handled according to the active compatibility mode.

**Compatibility modes** — configured at runtime init:

```rust
Compatibility::Strict   // default — fail on any version mismatch
Compatibility::Relaxed  // warn on mismatch, load anyway
Compatibility::Yolo     // no version checks at all
```

Per-bundle override:

```rust
runtime.load_bundle_with("./plugin.so", LoadOptions {
    compatibility:                   Compatibility::Relaxed,
    ignore_function_count_mismatch:  false,
})?;
```

---

## 17. Extension System

Extensions are optional host capabilities. They let the runtime evolve without touching the frozen core ABI. Plugins query extensions at init time and handle their absence gracefully.

Extensions are implemented by the app developer and passed to the runtime at init. They are not schema-defined — they are Rust traits.

**Built-in extensions (provided by the runtime):**

```
trace      — structured logging / tracing
async      — async task spawning (future)
sandbox    — permission queries (future)
```

**App developer passes extensions at init:**

```rust
let runtime: PluginRuntime = PluginRuntime::new()
    .extension(TraceExtension::new())
    .extension(MyCustomExtension::new())  // app can add custom extensions
    .init()?;
```

**Plugin queries extension at init (generated code):**

```c
const TraceExtension* trace = host->get_extension(EXT_TRACE_ID);
if (trace) {
    trace->emit("ImageDecoder started");
}
// if extension absent, plugin continues without it
```

Extensions are always optional. A plugin must never require an extension to function.

---

## 18. Security Model

Three plugin execution environments are supported. App developer chooses per plugin dir or per bundle.

```
Native    — compiled .so/.dll, full trust, maximum performance
WASM      — WebAssembly sandbox, restricted access, near-native performance
Script    — Python / Lua, runtime sandbox via interpreter restrictions
```

Sandbox policies can restrict:

- File system access
- Network access
- Host API surface
- Memory limits
- CPU time limits (script plugins only)

Security model is enforced at runtime init and at load time. It is not part of the ABI — it is a runtime concern layered on top.

Native plugins are trusted by default. WASM and script plugins have configurable sandbox policies.

---

## 19. Developer Experience — App Developer

The app developer's complete workflow.

**Step 1: Add runtime as dependency**

```toml
# Cargo.toml (Rust host)
[dependencies]
polyplug-runtime = "1.0"
```

```cmake
# CMakeLists.txt (C++ host)
find_package(Polyplug REQUIRED)
target_link_libraries(myapp Polyplug::host)
```

**Step 2: Write api.toml**

```toml
[[type]]
name = "Image"
fields = [
    { name = "width",  type = "u32" },
    { name = "height", type = "u32" },
    { name = "pixels", type = "Buffer" }
]

[[contract]]
name    = "image.decode"
version = "1.0"

[[contract.functions]]
name    = "decode"
params  = [{ name = "raw", type = "Buffer" }]
returns = "Image"
```

**Step 3: Run codegen**

```bash
# --api generates BOTH host callers and guest SDK in one command
polyplugc generate --api api.toml --lang cpp --out ./generated
# produces:
#   ./generated/host/image_decode.hpp    ← app uses this
#   ./generated/guest/image_decode.hpp   ← distribute this to plugin devs
```

**Step 4: Initialize runtime and use plugins**

```cpp
// C++ app
#include "generated/host/image_decode.hpp"

auto runtime = PluginRuntime::builder()
    .plugin_dir("./plugins")
    .init();

auto decoder = runtime.get<ImageDecodeContract>();
Image img = decoder.decode(raw_bytes);
```

**Step 5: Distribute SDK to plugin developers**

```bash
# Run once per language you want to support for plugin developers
polyplugc generate --api api.toml --lang rust   --out ./sdk/rust
polyplugc generate --api api.toml --lang cpp    --out ./sdk/cpp
polyplugc generate --api api.toml --lang csharp --out ./sdk/csharp
polyplugc generate --api api.toml --lang python --out ./sdk/python
polyplugc generate --api api.toml --lang lua    --out ./sdk/lua

# Each --out directory contains host/ and guest/ subdirs
# Distribute the guest/ subdir (or package as crate/NuGet/pip/etc)
# Also distribute api.toml directly
```

---

## 20. Developer Experience — Plugin Developer

The plugin developer's complete workflow.

**Step 1: Install app's SDK**

```bash
cargo add my_app_plugin_sdk   # Rust
# or
pip install my_app_plugin_sdk # Python
# etc.
```

**Step 2: Write bundle.toml**

```toml
[bundle]
name = "image_bundle"
version = "1.0"
api = "my_app_plugin_sdk"

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
// Rust plugin developer writes only this:
use my_app_plugin_sdk::contracts::ImageDecodeContract;
use generated::types::*;

struct MyDecoder;

impl ImageDecodeContract for MyDecoder {
    fn decode(&self, raw: &Buffer) -> Result<Image, polyplug_guest::PluginError> {
        // pure business logic
        // no ABI, no vtables, no unsafe, no entry points
    }

    fn supported_formats(&self) -> StringView {
        StringView::from_static("png,jpg,webp")
    }
}
```

```csharp
// C# plugin developer writes only this:
public class MyDecoder : IImageDecodeContract {
    public Image Decode(Buffer raw) {
        // pure business logic
    }
    public StringView SupportedFormats() => StringView.FromStatic("png,jpg");
}
```

```python
# Python plugin developer writes only this:
class MyDecoder(ImageDecodeContract):
    def decode(self, raw: Buffer) -> Image:
        # pure business logic
        pass

    def supported_formats(self) -> str:
        return "png,jpg"
```

**Step 5: Build**

```bash
cargo build --release
# produces:
# image_bundle.so
# image_bundle.manifest.toml  (auto-generated by build.rs + polyplugc)
```

**Step 6: Ship**

```
image_bundle.so
image_bundle.manifest.toml
```

Two files. Done.

---

## 21. The Full Runtime Flow

```
1.  App initializes runtime
        .plugin_dirs(["./plugins"])
        .extension(TraceExtension::new())
        .init()

2.  Runtime scans plugin directories
        finds image_bundle.so + image_bundle.manifest.toml

3.  Runtime reads manifest.toml (fast, no dlopen)
        learns: provides [image.decode@1.0, image.stats@1.0]
                requires [image.decode@1.0]

4.  Runtime builds capability graph across all found bundles
        validates all required capabilities are satisfied
        detects cycles
        determines initialization order

5.  Runtime dlopen bundles in correct order

6.  Runtime calls init(registrar) on each bundle
        passes HostVTable ptr
        bundle registers its PluginVTables
        bundle queries extensions, stores what it needs

7.  Runtime stores all PluginVTable ptrs, indexed by contract_id

8.  App requests contract:
        runtime.get::<ImageDecodeContract>()
        returns handle with stored vtable ptr

9.  App calls plugin function:
        decoder.decode(raw_bytes)
        → generated caller allocates Image on host allocator
        → vtable->functions[DECODE_FN_ID](&raw, &out_image)
        → one indirect call
        → plugin fills out_image
        → generated caller returns Image to app

10. Cross-plugin call (ImageStats needs ImageDecoder):
        at init: decoder_handle = host->find_plugin(IMAGE_DECODE_ID, 1_0)
        at call: host->call_plugin(decoder_handle, DECODE_FN_ID, &raw, &out)
        → one indirect call through runtime dispatcher
```

---

## 22. MVP Language Support

All five languages supported as both host and guest.

```
Rust    host + guest    native, near zero overhead
C++     host + guest    header-only libs, near zero overhead
C#      host + guest    P/Invoke + ref structs, minimal overhead
Python  host + guest    ctypes, Python runtime is the bottleneck
Lua     host + guest    LuaJIT recommended, near-native performance
```

**Delivery per language:**

```
Rust
├── host-libs/rust/              thin crate wrapping runtime C ABI
└── guest-libs/rust/             crate with proc macro, allocator hook

C++
├── host-libs/cpp/               header-only host lib
└── guest-libs/cpp/              header-only guest lib

C#
├── host-libs/csharp/            NuGet, P/Invoke declarations, ref structs
└── guest-libs/csharp/           NuGet, entry point attribute, marshaling helpers

Python
├── host-libs/python/            pip package, ctypes wrappers
└── guest-libs/python/           pip package, entry point decorator

Lua
├── host-libs/lua/               .lua file + C extension binding
└── guest-libs/lua/              .lua file, entry point helpers
```

---

## 23. Future Work

**Near term:**
- Hot reload — watch plugin dirs, reload bundles without restart
- Async plugin execution — spawn plugin calls on thread pool
- WASM sandbox support — run untrusted plugins safely
- Plugin marketplace tooling — signing, verification, distribution

**Long term:**
- Distributed plugins — plugins running in separate processes or machines
- Remote plugin execution — plugins over network
- Permission system — fine-grained capability control
- Additional language support — Java, Kotlin, Swift, Zig

---

## 24. Non-Goals

The runtime does not provide:

- UI frameworks
- Build systems
- Language compilers
- Asset pipelines
- Networking libraries
- Serialization formats (beyond what the ABI needs)

These remain the responsibility of the app developer and their ecosystem.

---

## Appendix — File Map

```
polyplug/                                YOU maintain
├── AGENTS.md
├── Cargo.toml                           workspace root
├── crates/
│   ├── polyplug-runtime/                Rust runtime core
│   │   ├── build.rs
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                   C ABI exports
│   │       ├── abi/
│   │       │   └── mod.rs              ABI structs (#[repr(C)])
│   │       ├── allocator/
│   │       │   └── mod.rs              host allocator
│   │       ├── error/
│   │       │   └── mod.rs              PolyplugError enum
│   │       ├── graph/
│   │       │   └── mod.rs              capability graph + topo sort
│   │       ├── loader/
│   │       │   └── mod.rs              bundle loading (dlopen)
│   │       ├── registry/
│   │       │   └── mod.rs              vtable registry
│   │       └── runtime/
│   │           └── mod.rs              PluginRuntime public API
│   └── polyplugc/                       CLI codegen tool
│       ├── Cargo.toml
│       └── src/
│           ├── main.rs
│           ├── error/
│           │   └── mod.rs              PolyplugcError enum
│           ├── ir/
│           │   └── mod.rs              IR structs
│           ├── parser/
│           │   └── mod.rs              TOML schema parser
│           └── generators/
│               ├── mod.rs              CodeGenerator trait
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
│   ├── rust/                            Rust host crate
│   ├── cpp/                             header-only C++ host lib
│   │   ├── polyplug.hpp                 single-include entry point
│   │   └── polyplug/
│   │       ├── abi.hpp
│   │       ├── error.hpp
│   │       ├── handle.hpp
│   │       └── runtime.hpp
│   ├── csharp/                          C# NuGet host lib
│   ├── python/                          Python pip host lib
│   └── lua/                             Lua host lib (.lua + .so)
├── guest-libs/
│   ├── rust/                            Rust guest crate
│   ├── cpp/                             header-only C++ guest lib
│   │   ├── polyplug_guest.hpp           single-include entry point
│   │   └── polyplug/
│   │       ├── abi.hpp
│   │       ├── contract.hpp
│   │       └── guest.hpp
│   ├── csharp/                          C# NuGet guest lib
│   ├── python/                          Python pip guest lib
│   └── lua/                             Lua guest lib (.lua file)
└── tests/                               integration tests
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
├── api.toml                             only schema file they write
├── src/
│   └── generated/
│       ├── host/                        polyplugc output (--api)
│       │   ├── types.hpp
│       │   └── callers.hpp
│       └── guest/                       polyplugc output (--api, distributed)
│           ├── types.hpp
│           └── contracts.hpp
└── plugins/                             runtime scans here


image-plugin/                            PLUGIN DEVELOPER
├── bundle.toml                          only schema file they write
├── src/
│   ├── decoder.rs                       business logic only
│   ├── stats.rs                         business logic only
│   └── generated/                       polyplugc output (--bundle)
│       ├── types.rs
│       ├── contracts.rs
│       ├── vtables.rs
│       └── init.rs
└── dist/
    ├── image_bundle.so
    └── image_bundle.manifest.toml       auto-generated by polyplugc
```
