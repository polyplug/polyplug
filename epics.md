# polyplug — Epic Prompts

All prompts are addressed to the **PLANNER agent**.
The planner interviews the developer, resolves all ambiguity, then produces a
step-by-step plan for the **EXECUTER agent** to follow.

Read `AGENTS.md` and `polyplug_prd.md` before planning any epic.

---

## Actual Project State (as of Epic 6 completion)

```
.
├── AGENTS.md
├── BENCHMARKS.md
├── Cargo.lock
├── Cargo.toml
├── crates
│   ├── polyplugc
│   │   ├── Cargo.toml
│   │   └── src
│   │       ├── error/mod.rs
│   │       ├── generators/
│   │       │   ├── cpp/mod.rs        (stub or partial)
│   │       │   ├── mod.rs
│   │       │   └── rust/mod.rs       (stub or partial)
│   │       ├── ir/mod.rs
│   │       ├── main.rs
│   │       └── parser/mod.rs
│   └── polyplug-runtime              (NOT yet renamed to polyplug)
│       ├── benches/vtable_dispatch.rs
│       ├── build.rs
│       ├── Cargo.toml
│       └── src
│           ├── abi/mod.rs
│           ├── allocator/
│           │   ├── mod.rs
│           │   └── tracking/mod.rs   (debug tracking allocator from Epic 6)
│           ├── error/mod.rs
│           ├── graph/mod.rs
│           ├── lib.rs
│           ├── loader/mod.rs
│           ├── registry/mod.rs
│           └── runtime/mod.rs
├── guest-libs
│   ├── cpp/                          implemented
│   │   ├── polyplug/
│   │   │   ├── abi.hpp
│   │   │   ├── contract.hpp
│   │   │   └── guest.hpp
│   │   └── polyplug_guest.hpp
│   └── rust/                         scaffolded
│       ├── Cargo.toml
│       └── src/lib/mod.rs
├── host-libs
│   ├── cpp/                          implemented
│   │   ├── polyplug/
│   │   │   ├── abi.hpp
│   │   │   ├── error.hpp
│   │   │   ├── handle.hpp
│   │   │   └── runtime.hpp
│   │   └── polyplug.hpp
│   └── rust/                         scaffolded
│       ├── Cargo.toml
│       └── src/lib/mod.rs
└── tests
    ├── fixtures/
    │   ├── error_plugin/             Rust fixture plugin
    │   ├── liberror_plugin.so        pre-compiled
    │   ├── libmemory_plugin.so       pre-compiled
    │   ├── libtest_plugin_cpp.so     pre-compiled C++ plugin
    │   ├── libtest_plugin.so         pre-compiled Rust plugin
    │   ├── memory_plugin/            Rust fixture plugin
    │   ├── test_api.toml
    │   ├── test_bundle.toml
    │   ├── test_panic_api.toml
    │   └── test_plugin/              Rust fixture plugin
    ├── fnv1a_compat/mod.rs
    ├── integration_codegen_cpp/mod.rs    codegen tests already wired
    ├── integration_codegen_rust/mod.rs   codegen tests already wired
    ├── integration_dispatch/mod.rs
    ├── integration_graph/mod.rs
    ├── integration_load/mod.rs
    ├── integration_panic/mod.rs
    ├── smoke/mod.rs
    ├── stress_error/mod.rs
    └── stress_memory/mod.rs
```

**Completed:**
- Epic 1: project skeleton
- Epic 2: IR and schema parser
- Epic 3: core ABI structs
- Epic 4: runtime core (loader, registry, graph, allocator, runtime API)
- Epic 5: codegen pipeline scaffolding (IR, parser, generator stubs, CLI skeleton)
- Epic 6: memory + error hardening, tracking allocator, criterion benchmarks,
  stress tests, BENCHMARKS.md

**Important facts for planning:**
- Runtime crate is named `polyplug-runtime` — NOT yet renamed to `polyplug`
- Rust and C++ generator stubs exist — unknown if partial or empty
- `integration_codegen_cpp` and `integration_codegen_rust` test files exist —
  unknown if they pass or are empty stubs
- `libtest_plugin_cpp.so` exists as a fixture — C++ plugin already compiled
- Rust host-lib and guest-lib scaffolded but likely empty
- `test_panic_api.toml` exists — panic isolation already tested
- Tracking allocator in `allocator/tracking/mod.rs` is production-quality debug tool

**Not yet done:**
- Rust generator fully producing compilable output
- C++ generator fully producing compilable output
- Crate rename from `polyplug-runtime` to `polyplug`
- BundleLoader trait for adapter crates
- polyplug-dotnet, polyplug-python, polyplug-lua adapter crates
- Full plugin discovery system
- Extension system
- Version negotiation
- C#, Python, Lua host/guest libs
- Showcase app

---

## Epic 7 — Complete Rust and C++ Code Generators + Crate Rename

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies to every file touched
- polyplug_prd.md — sections 11 (Schema Files), 12 (Code Generation Pipeline),
  15 (Memory Model), 16 (Error Handling)

---

PROJECT CONTEXT

polyplug is a universal cross-language plugin runtime platform.
The codegen CLI is polyplugc.

Current state of generators:
  crates/polyplugc/src/generators/rust/mod.rs  — stub or partial, unknown state
  crates/polyplugc/src/generators/cpp/mod.rs   — stub or partial, unknown state

These test files already exist and may be empty stubs or partially passing:
  tests/integration_codegen_rust/mod.rs
  tests/integration_codegen_cpp/mod.rs

This pre-compiled C++ plugin fixture already exists:
  tests/fixtures/libtest_plugin_cpp.so

The runtime crate is currently named polyplug-runtime.
It must be renamed to polyplug as part of this epic.

The Rust host-lib (host-libs/rust/) and guest-lib (guest-libs/rust/) are
scaffolded but likely empty. They need filling in this epic.

C++ host-libs and guest-libs are fully implemented at:
  host-libs/cpp/
  guest-libs/cpp/

---

EPIC GOAL

PART A — Crate rename (do this first, before any generator work):
  Rename crates/polyplug-runtime/ to crates/polyplug/
  Update all Cargo.toml references across the workspace
  Update all use/extern crate statements in all source files
  Confirm cargo build --workspace passes after rename before proceeding

PART B — Rust host-lib (host-libs/rust/src/lib/mod.rs):
  Thin ergonomic wrapper around polyplug C ABI
  PluginRuntime builder pattern matching PRD section 8
  Re-exports or wraps polyplug types for app developer convenience

PART C — Rust guest-lib (guest-libs/rust/src/lib/mod.rs):
  Plugin entry point proc macro or macro_rules
  Host allocator hookup (global allocator backed by host_alloc/host_free)
  Panic boundary helpers
  PluginError type
  ABI primitive re-exports (StringView, Buffer, AbiError)

PART D — RustGenerator fully implemented:

  From --api api.toml produces generated/host/ and generated/guest/:
  - host/types.rs       domain types as #[repr(C)] structs
  - host/callers.rs     type-safe contract caller structs wrapping PluginHandle
  - guest/types.rs      same domain types
  - guest/contracts.rs  traits plugin developer implements (pure Rust, no unsafe)

  From --bundle bundle.toml produces generated/:
  - types.rs            domain types
  - contracts.rs        traits to implement
  - vtables.rs          PluginVTable construction per contract
  - init.rs             bundle entry point, panic::catch_unwind per ABI function
  - manifest.toml       discovery manifest with runtime = "native" + function_count

PART E — CppGenerator fully implemented:

  From --api api.toml produces generated/host/ and generated/guest/:
  - host/types.hpp      domain types with static_assert layout checks vs Rust
  - host/callers.hpp    contract caller classes
  - guest/types.hpp     same domain types
  - guest/contracts.hpp abstract base classes per contract

  From --bundle bundle.toml produces generated/:
  - types.hpp
  - contracts.hpp
  - vtables.hpp         vtable construction
  - init.cpp            bundle entry point, try/catch per ABI function
  - manifest.toml       runtime = "native" + function_count

PART F — CLI wiring:
  polyplugc generate --lang rust --api / --bundle fully working
  polyplugc generate --lang cpp  --api / --bundle fully working
  polyplugc validate --api and --bundle fully working

---

CODE GENERATION RULES (non-negotiable, baked into generators)

- Non-primitive params: always by reference (Rust: &T, C++: const T*)
- Non-primitive returns: caller-provides-buffer, hidden from developer-facing API
- Primitive returns (u8-u64, f32, f64, bool): returned directly by value
- Guest ABI functions: wrapped in panic::catch_unwind (Rust) or try/catch (C++)
- All generated files: "// THIS FILE IS AUTO-GENERATED BY polyplugc" header
- No .unwrap() in generator production code
- Generated Rust: explicit type annotation on every binding

---

INTEGRATION TESTS

The planner must assess current state of integration_codegen_rust and
integration_codegen_cpp before writing the plan — ask me what they contain.

Required passing tests after this epic:
1. End-to-end Rust: api.toml → generate → compile plugin → load → call → assert
2. End-to-end C++: same flow for C++ output
3. C++ static_assert layout checks compile without error
4. Cross-language: Rust host loads C++ plugin
5. Cross-language: C++ host loads Rust plugin
6. Panic isolation: Rust plugin panics → host survives → ABI_ERROR_PANIC returned
7. Exception isolation: C++ plugin throws → host survives → error returned

---

VERIFICATION CHECKLIST

- cargo build --workspace passes after crate rename
- All integration tests pass after rename + generator completion
- Generated Rust compiles with clippy -- -D warnings zero warnings
- Generated C++ compiles with -Wall -Wextra -Werror
- static_assert layout checks confirm Rust and C++ struct layouts match
- No .unwrap() anywhere — grep confirms zero hits outside tests
- cargo test --workspace passes
- polyplugc generate --lang rust produces compilable output from test_api.toml
- polyplugc generate --lang cpp produces compilable output from test_api.toml
- manifest.toml generated with runtime = "native" and function_count fields

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- What currently exists in integration_codegen_rust and integration_codegen_cpp
  (empty stubs? partial tests? fully passing tests?)
- What currently exists in the Rust and C++ generator stubs
  (empty trait impls? partial codegen? close to complete?)
- Exact naming conventions for generated types, traits, functions
  (e.g. ImageDecodeContract vs IImageDecode vs ImageDecoder)
- Whether Rust guest traits use Result<T, PluginError> and whether
  PluginError is already defined in guest-libs/rust/ or needs defining here
- C++ error model in generated callers: exceptions or std::expected
- Whether the CLI validate subcommand is already wired
- Whether build.rs helper crate (polyplugc-build) is in scope for this epic
- Any constraints from existing C++ host-libs/guest-libs the generator must match
- Whether the crate rename should happen as a separate commit before generator work

Do not write the plan until I have answered all questions.
```

---

## Epic 8 — BundleLoader Trait and Adapter Crate Infrastructure

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (Language Runtime Adapters), section 5 (Runtime Core)

---

PROJECT CONTEXT

polyplug follows the serde model for language support:
- polyplug crate: always present, runtime core
- polyplug-dotnet, polyplug-python, polyplug-lua: optional separate crates
- Not feature flags. If the crate is not in Cargo.toml it does not exist in binary.

After Epic 7 the crate is named polyplug (renamed from polyplug-runtime).

Currently the loader (crates/polyplug/src/loader/mod.rs) only handles native
bundles via dlopen. It has no concept of multiple loader types.

This epic introduces the BundleLoader trait so adapter crates can register
themselves with the runtime. The three adapter crates are scaffolded here.
They are fully implemented in Epics 9, 10, 11.

---

EPIC GOAL

1. BundleLoader trait in crates/polyplug/src/loader/mod.rs:

   pub trait BundleLoader: Send + Sync {
       fn runtime_name(&self) -> &'static str;
       fn load(
           &self,
           path: &Path,
           registrar: &mut PluginRegistrar,
       ) -> Result<(), PolyplugError>;
   }

2. NativeBundleLoader: existing dlopen logic wrapped in a struct implementing
   BundleLoader. runtime_name returns "native". Registered by default — app
   developer does not call .loader() for native plugins.

3. PluginRuntime builder gets .loader(impl BundleLoader) method:
   - App developer registers additional loaders at init
   - Runtime dispatches by matching manifest runtime field to loader runtime_name
   - If no loader registered for a runtime field value, clear error:
     "bundle X requires runtime Y but no loader is registered for Y.
      Add polyplug-Y as a dependency and register YLoader at init."

4. manifest.toml runtime field:
   - Parser reads runtime field, defaults to "native" if absent
   - ManifestData struct gains runtime: String field
   - Discovery passes runtime field to loader dispatch
   - Tests: native bundle loads correctly via NativeBundleLoader
   - Tests: unknown runtime field produces clear error message

5. Three adapter crate scaffolds added to workspace:
   crates/polyplug-dotnet/
     Cargo.toml (depends on polyplug)
     src/lib/mod.rs (DotnetLoader stub: returns PolyplugError::NotImplemented)

   crates/polyplug-python/
     Cargo.toml (depends on polyplug)
     src/lib/mod.rs (PythonLoader stub: returns PolyplugError::NotImplemented)

   crates/polyplug-lua/
     Cargo.toml (depends on polyplug)
     src/lib/mod.rs (LuaLoader stub: returns PolyplugError::NotImplemented)

   All three added to workspace Cargo.toml.
   All three compile cleanly.

6. Tests:
   - All existing integration tests still pass (native loader unchanged behavior)
   - Unknown runtime: bundle with runtime = "unknown_xyz" fails with clear error
   - Correct dispatch: manifest runtime = "native" uses NativeBundleLoader
   - Stub loaders: DotnetLoader/PythonLoader/LuaLoader registered, but bundle
     with their runtime name returns NotImplemented error (expected behavior)

---

VERIFICATION CHECKLIST

- All existing integration tests pass
- New loader dispatch tests pass
- Unknown runtime error message is human-readable and actionable
- Three adapter crate scaffolds compile cleanly
- cargo build --workspace succeeds
- No .unwrap() in production code
- clippy passes with zero warnings

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Should NativeBundleLoader be pub or pub(crate)
- What happens if .loader() is called twice with the same runtime_name:
  overwrite, error, or last-wins
- Should adapter crates re-export polyplug types or require app to
  depend on polyplug directly in their Cargo.toml
- Whether the manifest parser already reads the runtime field or
  it needs adding in this epic
- Whether PolyplugError needs a new NotImplemented variant or
  another variant is more appropriate for the stub loaders

Do not write the plan until I have answered all questions.
```

---

## Epic 9 — polyplug-dotnet: Standard .NET Adapter

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-dotnet subsection),
  section 15 (Memory Model, C# entries), section 16 (Error Handling, C# entry)

---

PROJECT CONTEXT

C# plugins come in two modes:
- NativeAOT: compile to native .so/.dll, loaded by NativeBundleLoader, no adapter
- Standard .NET: managed IL assembly, requires polyplug-dotnet adapter

The crate scaffold exists from Epic 8 at crates/polyplug-dotnet/.
This epic fills it in completely, plus C# guest lib, C# host lib,
and CSharpGenerator in polyplugc.

---

PRE-ANSWERED DECISIONS

.NET target version: net10.0

DotnetConfig:
  pub struct DotnetConfig {
      pub min_framework: String,   // e.g. "net10.0"
  }
  DotnetLoader::new(DotnetConfig { min_framework: "net10.0".into() })
  DotnetConfig is mandatory — no default constructor.

runtimeconfig.json strategy:
  polyplug-dotnet generates a minimal runtimeconfig.json in a temp dir
  from DotnetConfig.min_framework at first CLR init. Never shipped by
  plugin developer. Deleted immediately after hostfxr_initialize_for_runtime_config.
  Plugin bundle ships only: image_bundle.dll + image_bundle.manifest.toml

Multi-version strategy:
  CLR initialized once via OnceLock<Arc<HostfxrContext>> in DotnetLoader.
  Subsequent plugins: read target framework from assembly metadata.
  - Compatible (same major, minor >= min_framework minor) → load silently
  - Higher minor → load with warning
  - Different major → PolyplugError::RuntimeVersionMismatch { required, found }
  NativeAOT is the escape hatch for plugins targeting a different major version.

hostfxr location: DOTNET_ROOT env var → PATH scan → well-known paths
  (/usr/lib/dotnet, /usr/share/dotnet, ~/.dotnet)
  Clear error if not found.

C# error model: exceptions. Plugin methods throw PluginException.
  Generated Init.cs wraps each ABI function in try/catch, converts to AbiError.

Async: out of scope. Sync only.

C# plugin .dll compilation in tests: dotnet CLI via build.rs.
  build.rs runs `dotnet build` on the C# fixture project before tests run.

NuGet publishing: out of scope. Local .csproj class libraries only.

Dependency order: Rust crate first → C# libs → CSharpGenerator.

---

EPIC GOAL

1. polyplug-dotnet crate (crates/polyplug-dotnet/):
   DotnetConfig struct (as above).
   DotnetLoader implementing BundleLoader:
   - runtime_name returns "dotnet"
   - Locates hostfxr: DOTNET_ROOT → PATH → well-known paths
   - On first .NET bundle load:
     generates minimal runtimeconfig.json in temp dir from min_framework
     calls hostfxr_initialize_for_runtime_config with temp path
     deletes temp file immediately
     stores HostfxrContext in OnceLock
   - Per bundle:
     reads target framework from assembly metadata
     version check per multi-version strategy above
     calls load_assembly_and_get_function_pointer
     obtains fn ptr to [UnmanagedCallersOnly] Init method
     calls Init(registrar) — identical vtable exchange from here
   - PolyplugError variants for every failure mode:
     HostfxrNotFound, ClrInitFailed, AssemblyNotFound, InitSymbolMissing,
     RuntimeVersionMismatch { required: String, found: String }

2. C# guest lib (guest-libs/csharp/) — .csproj class library, net10.0:
   - ABI types as [StructLayout(LayoutKind.Sequential)] structs
   - StringView and Buffer as ref structs (stack-only, no GC pressure)
   - PluginRegistrar P/Invoke wrappers
   - PluginException type (code: uint, message: string)
   - [UnmanagedCallersOnly] entry point infrastructure

3. C# host lib (host-libs/csharp/) — .csproj class library, net10.0:
   - P/Invoke declarations for all polyplug C ABI functions
   - Polyplug.Runtime class with builder pattern (PluginDir, Loader, Extension, Build)
   - ref struct wrappers for StringView and Buffer
   - P/Invoke declarations for StringView and Buffer interop

4. CSharpGenerator (crates/polyplugc/src/generators/csharp/mod.rs — new):

   From --api api.toml:
   - generated/host/Types.cs         domain types as [StructLayout] structs
   - generated/host/Callers.cs       contract caller classes (P/Invoke into vtable)
   - generated/guest/Types.cs        same domain types
   - generated/guest/Contracts.cs    interfaces plugin developer implements

   From --bundle bundle.toml:
   - generated/Types.cs
   - generated/Contracts.cs
   - generated/Vtables.cs            vtable construction, function ptr delegates
   - generated/Init.cs               [UnmanagedCallersOnly] Init,
                                     try/catch per ABI function → AbiError
   - generated/manifest.toml         runtime = "dotnet"

5. UTF-16 to UTF-8 transcoding in generated C# code:
   - ASCII fast path: strip high byte, near-zero cost
   - Full transcoding for non-ASCII via Encoding.UTF8
   - Transcoding buffer allocated via host_alloc, not managed heap

6. polyplugc generate --lang csharp wired into CLI

7. C# fixture plugin for integration tests:
   - tests/fixtures/csharp_plugin/ — .csproj targeting net10.0
   - Implements the test contract from test_api.toml
   - build.rs compiles it via `dotnet build` before tests run
   - Output: tests/fixtures/libtest_plugin_csharp.dll

8. Cross-language integration tests:
   - Rust host loads standard .NET C# plugin → call two functions → assert results
   - C# host (using host-libs/csharp/) loads Rust plugin → call → assert
   - C# host loads C# plugin
   - UTF-16/UTF-8 round-trip: ASCII and non-ASCII strings
   - C# exception in plugin does not crash Rust host → AbiError returned
   - GC stress: trigger GC during plugin call, assert no memory corruption
   - Multi-version: second .NET plugin with higher minor → warning emitted, loads
   - Multi-version: .NET plugin targeting different major → RuntimeVersionMismatch error

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- GC stress test passes with no corruption
- UTF-16/UTF-8 round-trip passes for ASCII and non-ASCII
- C# exception does not crash Rust host
- CLR initialized exactly once (OnceLock guarantees, verified in multi-plugin test)
- runtimeconfig.json temp file does not persist after CLR init
- hostfxr not found produces clear actionable error message
- Higher minor version: warning emitted, plugin loads
- Different major version: RuntimeVersionMismatch error, clear message
- polyplugc generate --lang csharp produces compilable net10.0 C# output
- No .unwrap() in Rust production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 9.5 — polyplug-dotnet: Performance Hardening

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-dotnet subsection)

---

PROJECT CONTEXT

Epic 9 produced a working polyplug-dotnet implementation. This epic hardens it
for maximum performance and correctness. Eight specific problems were identified
in the Epic 9 design — all are fixed here. No new features, no scope creep.
Every change is a targeted fix to an existing file.

---

PRE-ANSWERED DECISIONS

Problem 1 — Switch from raw hostfxr FFI to netcorehost crate:
  Replace any manual hostfxr bindings with the netcorehost crate.
  Cargo.toml:
    netcorehost = { version = "0.20", features = ["nethost"] }
  nethost::load_hostfxr() replaces the manual DOTNET_ROOT → PATH → well-known
  path scan. The crate handles this automatically.

Problem 2 — Add download-nethost Cargo feature (opt-in):
  polyplug-dotnet/Cargo.toml gains:
    [features]
    download-nethost = ["netcorehost/download-nethost"]
  NOT enabled by default. App developer enables via:
    polyplug-dotnet = { ..., features = ["download-nethost"] }
  Allows building polyplug-dotnet with zero system .NET install.
  DotnetConfig gains HostfxrLocation enum:
    pub enum HostfxrLocation { Auto, Path(PathBuf) }
    pub struct DotnetConfig {
        pub min_framework: String,
        pub hostfxr: HostfxrLocation,   // default: Auto
    }

Problem 3 — Cache AssemblyDelegateLoader in OnceLock (critical ~30ms savings):
  The AssemblyDelegateLoader obtained via hostfxr_get_runtime_delegate must be
  stored in the OnceLock context alongside the hostfxr context.
  Per-bundle load calls get_function_pointer on the CACHED loader — never
  re-obtains the delegate loader. This saves ~30ms per bundle load.
  OnceLock value changes from OnceLock<Arc<HostfxrContext>> to:
    OnceLock<Arc<DotnetContext>>
    pub struct DotnetContext {
        hostfxr_context: HostfxrContext,
        delegate_loader: AssemblyDelegateLoader,
    }

Problem 4 — Use pelite to read TargetFrameworkAttribute from PE metadata:
  Replace any assembly metadata reading approach with pelite:
    pelite = "0.10"
  pelite reads TargetFrameworkAttribute from the CLR metadata section of the
  .dll without loading the CLR and without requiring any extra file from the
  plugin developer. This is the ONLY correct approach — no runtimeconfig.json
  from plugin, no CLR needed for version check.
  Version check runs on the raw .dll bytes before any load call.

Problem 5 — runtimeconfig.json generated BEFORE nethost::load_hostfxr() init:
  The temp runtimeconfig.json must exist on disk when
  hostfxr_initialize_for_runtime_config is called. It is deleted immediately
  after that call returns (hostfxr reads it synchronously, does not hold it).
  Sequence:
    1. write temp runtimeconfig.json from min_framework
    2. nethost::load_hostfxr() → locate hostfxr
    3. hostfxr context init with temp path
    4. DELETE temp file
    5. obtain + cache AssemblyDelegateLoader
    6. store DotnetContext in OnceLock

Problem 6 — [UnmanagedCallersOnly] must declare CallConvCdecl explicitly:
  Every [UnmanagedCallersOnly] method in generated C# and in guest-libs/csharp/
  must have CallConvs = new[] { typeof(CallConvCdecl) } explicitly.
  Implicit calling convention is platform-dependent and wrong.
  This applies to Init and every generated ABI wrapper function.
  Update CSharpGenerator to emit this on every generated function.
  Update guest-libs/csharp/ manually.

Problem 7 — Thread.BeginThreadAffinity / EndThreadAffinity in generated Init:
  Init is called from a Rust thread (foreign thread, not CLR-created).
  Generated Init.cs must wrap its entire body:
    Thread.BeginThreadAffinity();
    try { ... } finally { Thread.EndThreadAffinity(); }
  This ensures correct managed code execution on the foreign thread.
  Update CSharpGenerator to always emit this pattern in Init.
  Update guest-libs/csharp/ entry point infrastructure to document this requirement.

Problem 8 — [SuppressGCTransition] on hot-path P/Invokes in host-libs/csharp/:
  The following P/Invoke declarations in host-libs/csharp/ must add
  [SuppressGCTransition]:
    call_plugin    (hot path — vtable dispatch)
    find_plugin    (hot path — plugin lookup)
    get_extension  (hot path — extension query)
  The following must NOT have [SuppressGCTransition]:
    host_alloc     (may trigger GC)
    host_free      (may interact with GC)
  [SuppressGCTransition] is only safe for short, non-blocking native calls
  that never call back into managed code. call_plugin, find_plugin,
  get_extension satisfy this contract.

---

EPIC GOAL

1. polyplug-dotnet/Cargo.toml:
   - Replace raw hostfxr dependencies with netcorehost = { features = ["nethost"] }
   - Add pelite = "0.10"
   - Add [features] download-nethost = ["netcorehost/download-nethost"]

2. DotnetConfig in crates/polyplug-dotnet/src/config/mod.rs (new submodule):
   - HostfxrLocation enum (Auto, Path(PathBuf))
   - DotnetConfig struct (min_framework: String, hostfxr: HostfxrLocation)
   - impl Default for HostfxrLocation → Auto

3. DotnetContext in crates/polyplug-dotnet/src/context/mod.rs (new submodule):
   - DotnetContext struct (hostfxr_context, delegate_loader: AssemblyDelegateLoader)
   - OnceLock<Arc<DotnetContext>> as the shared state in DotnetLoader
   - init_context(config: &DotnetConfig) -> Result<Arc<DotnetContext>, PolyplugError>
     implements the 6-step sequence from Problem 5 above

4. pelite version reader in crates/polyplug-dotnet/src/version/mod.rs (new submodule):
   - read_target_framework(dll_path: &Path) -> Result<String, PolyplugError>
   - Uses pelite to open the PE file, walk CLR metadata, find TargetFrameworkAttribute
   - Parses "net10.0" style string, returns it
   - Called before any load attempt, purely on bytes — no CLR involvement

5. DotnetLoader::load() rewrite:
   - Calls read_target_framework() first → version check → warn or error
   - Gets or initializes DotnetContext via OnceLock
   - Calls cached delegate_loader.get_function_pointer("Init") only
   - Calls Init(registrar) with correct calling convention

6. CSharpGenerator updates (crates/polyplugc/src/generators/csharp/mod.rs):
   - Init function template: add CallConvCdecl, Thread.BeginThreadAffinity/EndThreadAffinity
   - Every generated ABI wrapper function: add CallConvCdecl
   - All [UnmanagedCallersOnly] methods: verify return type is blittable AbiError (uint)
     not void — exception path must return AbiError, cannot throw through ABI boundary

7. guest-libs/csharp/ updates:
   - Every [UnmanagedCallersOnly] method: add CallConvs = new[] { typeof(CallConvCdecl) }
   - Entry point infrastructure: add Thread.BeginThreadAffinity/EndThreadAffinity pattern
   - Document in code comments: all parameters must be blittable

8. host-libs/csharp/ updates:
   - call_plugin P/Invoke: add [SuppressGCTransition]
   - find_plugin P/Invoke: add [SuppressGCTransition]
   - get_extension P/Invoke: add [SuppressGCTransition]
   - host_alloc / host_free: no [SuppressGCTransition] — leave as is

9. Tests:
   - Existing Epic 9 integration tests must all still pass unchanged
   - New test: verify DelegateLoader cached — load two .NET plugins, assert
     get_function_pointer called only once per bundle not twice
   - New test: verify pelite reads TargetFrameworkAttribute correctly from test fixture .dll
   - New test: version mismatch via pelite — wrong framework dll → RuntimeVersionMismatch
   - New test: [SuppressGCTransition] hot path benchmark — assert call_plugin overhead
     is within 3x of native vtable dispatch (from BENCHMARKS.md baseline)
   - New test: download-nethost feature compiles when enabled (CI build matrix)

---

VERIFICATION CHECKLIST

- All existing Epic 9 integration tests pass unchanged
- netcorehost crate used — no raw hostfxr FFI anywhere in polyplug-dotnet
- pelite reads TargetFrameworkAttribute correctly (unit test)
- Version mismatch detected by pelite before CLR loads assembly
- DelegateLoader cached in OnceLock — get_function_pointer not re-obtained per bundle
- runtimeconfig.json temp file sequence correct: written → init → deleted → loader cached
- Every [UnmanagedCallersOnly] in generated code has CallConvCdecl explicitly
- Init.cs has Thread.BeginThreadAffinity / EndThreadAffinity
- call_plugin / find_plugin / get_extension have [SuppressGCTransition]
- host_alloc / host_free do NOT have [SuppressGCTransition]
- download-nethost feature builds cleanly when enabled
- Hot path benchmark within 3x of native baseline
- No .unwrap() in Rust production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 9.6 — NativeBundleLoader: libloading Audit and Library Handle Lifetime

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 6 (ABI), section 7 (VTable System)

---

PROJECT CONTEXT

Epic 6 implemented NativeBundleLoader using libloading (or equivalent).
This epic audits the implementation for one critical correctness risk and
ensures the crate version is current. No new features — targeted correctness
and hardening only.

The critical risk: libloading's Library handle must be kept alive as long as
any function pointers derived from it are alive. If Library is dropped,
dlclose() unmaps the plugin's code pages. Any subsequent vtable call into
that plugin is a use-after-free — silent memory corruption or SIGBUS.

polyplug stores raw function pointers in the Registry's PluginVTable entries.
The Library handle that owns those pages must live exactly as long as the
PluginRuntime that holds the Registry.

---

PRE-ANSWERED DECISIONS

libloading version: 0.9
  Cargo.toml: libloading = "0.9"
  Update if Epic 6 used an older version.

Library handle storage:
  The Library handle must be stored inside PluginRuntime (or Registry)
  in a Vec<libloading::Library> (or equivalent owned collection).
  It must NOT be stored as a local variable that drops at end of load().
  It must NOT be stored in NativeBundleLoader — the loader may be
  dropped before PluginRuntime.
  Correct owner: Registry or PluginRuntime, lifetime tied to the runtime.

  Concrete change if not already correct:
    Registry gains: loaded_libraries: Vec<libloading::Library>
    NativeBundleLoader::load() moves the Library into registry after
    extracting the Init fn ptr.

RTLD flags: libloading uses RTLD_NOW | RTLD_LOCAL by default.
  RTLD_LOCAL is correct — plugins must not pollute the global symbol namespace.
  RTLD_NOW is correct — fail fast if symbols are missing at load time.
  No changes needed to dlopen flags.

Symbol lookup: the only symbol looked up per native bundle is "init"
  (the [no_mangle] extern "C" fn init(registrar: *mut PluginRegistrar)).
  All other dispatch goes through the vtable — no further dlsym calls.

---

EPIC GOAL

1. Audit crates/polyplug/src/loader/mod.rs (NativeBundleLoader):
   - Confirm libloading = "0.9" in Cargo.toml — update if not
   - Confirm Library handle is NOT dropped at end of load()
   - Confirm Library handle is stored in Registry or PluginRuntime
     with lifetime tied to the runtime
   - If Library handle is incorrectly dropped: move it into
     Registry::loaded_libraries: Vec<libloading::Library>
   - Add // SAFETY: comment on every unsafe block involving Library
     and Symbol explaining the lifetime guarantee

2. If Registry gains loaded_libraries field:
   - Add field to Registry struct in crates/polyplug/src/registry/mod.rs
   - Ensure Registry::clear() or drop also drops all Library handles
   - No public API change needed — internal field only

3. Add explicit use-after-unload test:
   - Load a native plugin, call it successfully
   - Verify Library handle is still alive (not yet dropped)
   - Drop PluginRuntime
   - Verify no SIGBUS / use-after-free (address sanitizer or miri)
   - This test must run under `cargo test` with ASAN or miri enabled

4. Add doc comment to NativeBundleLoader::load() explaining:
   - Why Library handle must be moved into Registry
   - What happens if it is dropped early (dlclose, code unmap)
   - That RTLD_LOCAL prevents symbol pollution

---

VERIFICATION CHECKLIST

- libloading = "0.9" in Cargo.toml
- Library handle stored in Registry or PluginRuntime — never drops before runtime
- Every unsafe block in NativeBundleLoader has // SAFETY: comment
- Use-after-unload test passes under miri or ASAN
- All existing native plugin integration tests still pass
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 9.7 — Full ABI Redesign: Dependencies, Multi-Impl, arc-swap, Hot-Reload Foundation

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 6 (ABI Layer), section 7 (VTable System),
  section 8 (Host Libraries), section 9 (Guest Libraries),
  section 11 (Schema Files — bundle.toml and manifest.toml),
  section 14 (Cross-Plugin Communication), section 27 (Hot-Reload Architecture)

---

PROJECT CONTEXT

The current ABI has:
  - find_plugin(contract_id, min_version) → PluginHandle (generational index)
  - call_plugin(handle, fn_id, args, out) → AbiError
  - PluginHandle { index: u32, generation: u32 }
  - Registry: HashMap<u64, u32> — one slot per contract, first-registered wins
  - PluginSlot: { vtable: *const PluginVTable, generation: u32 }

Problems with the current design:
  1. call_plugin adds an extra indirection on every cross-plugin call
  2. No multi-implementation support (one contract → one provider)
  3. No plugin identity in ABI (cannot find a specific bundle's implementation)
  4. No declared dependency enforcement (plugins can probe any contract)
  5. Raw *const PluginVTable stored in slots — unsafe for hot-reload
  6. bundle.toml has no [[dependency]] section — runtime has no dependency graph
  7. manifest.toml has no dependency information — Epic 12 graph builder blind

This epic redesigns the entire cross-plugin interaction surface to fix all of this.
It is a BREAKING change to the frozen stable ABI. Safe to make now — no external
consumers exist. After this epic the ABI is re-frozen.

Epic 9.5 (polyplug-dotnet hardening) is already implemented. Do not touch it.

---

PRE-ANSWERED DECISIONS

NEW ABI FUNCTIONS (replace find_plugin and call_plugin):

  // Any implementation of a contract
  PluginHandle find_by_contract(uint64_t contract_id, uint32_t min_version);

  // Specific bundle's implementation of a contract
  PluginHandle find_by_bundle(uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);

  // All implementations of a contract — caller-provides-buffer
  size_t find_all_by_contract(uint64_t contract_id, uint32_t min_version,
                               PluginHandle* out, size_t out_cap);

  // One-time resolution: PluginHandle → opaque arc-swap guard
  const PluginVTableGuard* resolve_plugin(PluginHandle handle);

REMOVED FROM ABI:
  call_plugin — removed entirely. No longer needed.
  find_plugin — replaced by find_by_contract, find_by_bundle, find_all_by_contract.

KEPT WITH SAME SEMANTICS:
  host_alloc, host_free, get_extension — unchanged.
  PluginHandle { index: u32, generation: u32 } — kept, generational check still valuable.
  PluginHandle null sentinel: { index: u32::MAX, generation: 0 }.

NEW ABI TYPE — PluginVTableGuard:
  Opaque type. Returned by resolve_plugin. Holds an arc-swap read guard.
  Keeps the vtable pointer alive for the duration it is held.
  Plugin stores one guard per dependency at init time.
  On hot path: guard->vtable() returns *const PluginVTable for that call.
  Guard is NOT Send — must be resolved and used on the same thread as init,
  or resolved per-call if plugin is called from multiple threads.
  Lifetime: valid until the plugin is unloaded. Runtime manages guard validity.

ID COMPUTATION (already implemented in abi/mod.rs, no changes needed):
  contract_id = fnv1a_64("contract.name@major")  — existing function
  bundle_id   = fnv1a_64("bundle_name")           — NEW, same algorithm
  extension_id = fnv1a_32("extension_name")       — existing function

DEPENDENCY ENFORCEMENT:
  find_by_contract, find_by_bundle, find_all_by_contract ALL check that the
  calling plugin's bundle_id declared the requested dependency.
  If not declared → hard error: PolyplugError::UndeclaredDependency { bundle_id, contract_id }.
  The calling bundle_id is passed implicitly — runtime tracks which plugin is
  currently in its init() via a thread-local or init-context parameter.
  Enforcement only applies during init(). Hot path calls go directly via guard.

REGISTRY CHANGES:
  Old: contract_index: HashMap<u64, u32>          — one slot per contract
  New: contract_index: HashMap<u64, Vec<u32>>     — multiple slots per contract
  find_by_contract returns first registered (index 0 of the Vec).
  find_all_by_contract fills caller's buffer with all slots for that contract.
  find_by_bundle looks up by bundle_id first, then filters by contract_id.
  New index: bundle_index: HashMap<u64, u32>      — bundle_id → slot index.
  PluginSlot gains: bundle_id: u64 field (set at registration time).

REGISTRY SLOT CHANGE — arc-swap:
  Old: vtable: *const PluginVTable
  New: vtable: ArcSwap<VTableSlot>
  where VTableSlot(pub *const PluginVTable) is Send + Sync by trust model.
  arc-swap = "1.7" added to Cargo.toml of polyplug crate.
  Generation check remains in resolve_plugin (not on hot path).
  resolve_plugin(handle):
    1. Validate generation (stale handle → PolyplugError::StaleHandle)
    2. Return an opaque PluginVTableGuard wrapping an arc-swap Guard<Arc<VTableSlot>>
    3. Guard keeps old vtable alive until dropped — automatic quiescence for hot-reload

BUNDLE.TOML [[DEPENDENCY]] SCHEMA:
  Plugin developer writes:
    [[dependency]]
    contract    = "image.decode"    # contract name, validated against api.toml
    min_version = "1.0"

    [[dependency]]
    bundle      = "awesome_filter"  # bundle name, validated against api.toml ecosystem
    contract    = "image.decode"
    min_version = "1.0"

  polyplugc validates both names at codegen time.
  Hard error if contract name not in api.toml.
  Hard error if bundle name not known (from api.toml or explicit bundle registry).
  polyplugc computes and bakes:
    const IMAGE_DECODE_CONTRACT_ID: u64 = fnv1a_64("image.decode@1");
    const AWESOME_FILTER_BUNDLE_ID: u64 = fnv1a_64("awesome_filter");
    const MY_BUNDLE_ID:             u64 = fnv1a_64("image_bundle");

  The MY_BUNDLE_ID constant is passed to the runtime at init time so the runtime
  knows which bundle is currently initializing (for dependency enforcement).

GENERATED INIT CODE PATTERN (all 5 languages):
  For each [[dependency]] in bundle.toml:
    1. Call find_by_contract(CONTRACT_ID, MIN_VERSION)
       or find_by_bundle(BUNDLE_ID, CONTRACT_ID, MIN_VERSION) for bundle deps
    2. Hard null/error check → DependencyNotFound error with contract name in message
    3. Call resolve_plugin(handle) → store guard as module-level / struct field
  All dependency resolution MUST complete before any vtable registration.
  Generated pattern per language:
    Rust:   let handle = host.find_by_contract(ID, VER).ok_or(DependencyNotFound)?;
            let guard = host.resolve_plugin(handle)?;
            self.decoder = guard;
    C++:    auto handle = host->find_by_contract(ID, VER);
            if (!handle) throw DependencyNotFound("image.decode");
            decoder_guard_ = host->resolve_plugin(handle);
    C#:     var handle = host.FindByContract(ID, VER);
            if (handle.IsNull) throw new DependencyNotFoundException("image.decode");
            _decoderGuard = host.ResolvePlugin(handle);
    Python: handle = host.find_by_contract(ID, VER)
            if handle is None: raise DependencyNotFoundError("image.decode")
            self._decoder_guard = host.resolve_plugin(handle)
    Lua:    local handle = host.find_by_contract(ID, VER)
            if handle == nil then error("dependency not found: image.decode") end
            decoder_guard = host.resolve_plugin(handle)

MANIFEST.TOML CHANGES:
  Add bundle_id field (fnv1a_64 of bundle name).
  Replace requires = ["..."] array with [[dependency]] table array:
    [[dependency]]
    contract    = "image.decode"
    contract_id = 0xA3F2...
    min_version = "1.0"

    [[dependency]]
    bundle      = "awesome_filter"
    bundle_id   = 0xB7C1...
    contract    = "image.decode"
    contract_id = 0xA3F2...
    min_version = "1.0"
  The old requires = [...] field is removed from manifest.toml.
  Epic 12 graph builder reads [[dependency]] from manifest.toml.

NEW ERROR VARIANTS (crates/polyplug/src/error/mod.rs):
  PolyplugError::UndeclaredDependency { bundle_id: u64, contract_id: u64 }
  PolyplugError::DependencyNotFound { contract_name: String, min_version: u32 }
  PolyplugError::BundleNotFound { bundle_name: String, contract_name: String }
  PolyplugError::StaleHandle { index: u32 }  (rename from existing if needed)

TRUST MODEL — no changes from design:
  Plugins are trusted code. *const PluginVTable is never mutable.
  arc-swap ensures hot-reload safety without locks.
  mprotect rejected — security theater for in-process trusted code.

---

EPIC GOAL

1. Add arc-swap dependency to polyplug crate:
   - arc-swap = "1.7" in crates/polyplug/Cargo.toml
   - VTableSlot(pub *const PluginVTable) newtype with Send+Sync impls + SAFETY comments
   - PluginVTableGuard as a public opaque wrapper around arc_swap::Guard<Arc<VTableSlot>>

2. Registry changes in crates/polyplug/src/registry/mod.rs:
   - contract_index: HashMap<u64, Vec<u32>> (multi-impl support)
   - bundle_index: HashMap<u64, u32> (bundle_id → slot)
   - PluginSlot gains: bundle_id: u64, vtable: ArcSwap<VTableSlot>
   - New methods:
       find_by_contract(contract_id, min_version) → Option<PluginHandle>
       find_by_bundle(bundle_id, contract_id, min_version) → Option<PluginHandle>
       find_all_by_contract(contract_id, min_version, out: &mut Vec<PluginHandle>) → usize
       resolve(handle) → Result<PluginVTableGuard, PolyplugError>
   - Remove old: find(contract_id) method

3. ABI changes in crates/polyplug/src/abi/mod.rs:
   - Remove: call_plugin extern fn, PluginHandle typedef (if was typedef, keep struct)
   - Add: find_by_contract, find_by_bundle, find_all_by_contract, resolve_plugin
   - Add: bundle_id() function: fnv1a_64(bundle_name: &str) → u64
   - Add opaque PluginVTableGuard type to ABI types
   - Add // ABI FROZEN AS OF EPIC 9.7 comment block listing all stable items
   - Every unsafe block has // SAFETY: comment

4. Runtime changes in crates/polyplug/src/runtime/mod.rs:
   - Add init_context: thread-local tracking which bundle_id is in init()
   - Implement find_by_contract with dependency enforcement check
   - Implement find_by_bundle with dependency enforcement check
   - Implement find_all_by_contract with dependency enforcement check
   - Implement resolve_plugin delegating to registry
   - Remove call_plugin implementation
   - Update HostVTable construction: remove call_plugin ptr, add new fn ptrs
   - HostVTable now has: alloc, free, find_by_contract, find_by_bundle,
     find_all_by_contract, resolve_plugin, get_extension

5. New error variants in crates/polyplug/src/error/mod.rs:
   - UndeclaredDependency { bundle_id: u64, contract_id: u64 }
   - DependencyNotFound { contract_name: String, min_version: u32 }
   - BundleNotFound { bundle_name: String, contract_name: String }

6. polyplugc — bundle.toml parser changes (crates/polyplugc/src/parser/mod.rs):
   - Parse [[dependency]] table array:
       Dependency::ByContract { contract: String, min_version: String }
       Dependency::ByBundle { bundle: String, contract: String, min_version: String }
   - Validate contract names against api.toml schema — hard error if unknown
   - Validate bundle name (from [bundle].name) does not match any contract name
     in the referenced api.toml — hard error with clear message if it does
   - This check runs in both `polyplugc validate` and `polyplugc generate`
   - Remove parsing of requires = [...] from [[plugin]] entries
   - Store parsed dependencies in ResolvedBundle IR node

7. polyplugc — IR changes (crates/polyplugc/src/ir/mod.rs):
   - ResolvedBundle gains: dependencies: Vec<ResolvedDependency>
   - ResolvedDependency::ByContract { contract_id: u64, contract_name: String, min_version: u32 }
   - ResolvedDependency::ByBundle { bundle_id: u64, bundle_name: String,
                                     contract_id: u64, contract_name: String, min_version: u32 }
   - bundle_id computed via new abi::bundle_id() function
   - MY_BUNDLE_ID constant added to IR for the bundle being compiled

8. polyplugc — all 5 generators updated:
   For each generator (Rust, C++, C#, Python, Lua):
   a. Emit MY_BUNDLE_ID constant (fnv1a_64 of bundle name)
   b. Emit per-dependency contract_id / bundle_id constants
   c. Emit dependency resolution code in init():
      - find_by_contract or find_by_bundle call with hard error on null
      - resolve_plugin call, guard stored as module-level / struct field
      - All dependency resolution before any vtable registration
   d. Emit hot-path call pattern using guard->vtable() instead of stored raw ptr
   e. Update cross-plugin callers to use guard-based dispatch

9. polyplugc — manifest.toml generation updated (all 5 generators):
   - Add bundle_id field to manifest output
   - Replace requires = [...] with [[dependency]] table array
   - Each dependency entry includes resolved IDs alongside human-readable names

10. All 5 host libs updated (host-libs/):
    - Remove find_plugin wrapper
    - Remove call_plugin wrapper
    - Add find_by_contract wrapper
    - Add find_by_bundle wrapper
    - Add find_all_by_contract wrapper (caller-provides-buffer pattern in each language)
    - Add resolve_plugin wrapper returning opaque guard type
    Each language wraps in its natural idiom.

11. All 5 guest libs updated (guest-libs/):
    - Remove call_plugin usage
    - Update cross-plugin call helpers to use guard-based dispatch
    - Add DependencyNotFound error type where not already present

12. TRUST_MODEL.md at repo root:
    - polyplug trust model: plugins are trusted, loaded by app developer
    - PluginVTable is never mutable — const always, UB to cast to mutable
    - arc-swap provides hot-reload safety without locks
    - mprotect rejected — bypassable by in-process code, security theater
    - Malicious in-process code is explicitly out of scope
    - Undeclared dependency access is a hard error — not a security boundary
      but a correctness guarantee enabling the dependency graph
    Add reference to TRUST_MODEL.md in AGENTS.md

13. Integration tests:
    a. Multi-impl: two bundles register for same contract_id
       find_by_contract returns first registered
       find_all_by_contract returns both
       find_by_bundle returns the specific one requested
    b. Undeclared dependency: bundle calls find_by_contract for undeclared contract
       → UndeclaredDependency error, does not proceed
    c. Declared dependency not loaded: bundle declares dep, nothing provides it
       → DependencyNotFound from generated init code
    d. Cross-plugin call via guard: plugin A calls plugin B through resolved guard
       → correct result, overhead measured vs host-to-plugin baseline
    e. arc-swap read path: verify guard keeps vtable alive during call
       (inspect Arc refcount in test — drops to 1 after guard drops)
    f. find_all_by_contract: caller-provides-buffer returns correct count and handles
    g. All existing integration tests still pass

14. Benchmarks:
    - Cross-plugin call via guard vs host-to-plugin direct: must be within ~3 cycles
    - find_by_contract uncontended: confirm O(1) HashMap lookup

---

VERIFICATION CHECKLIST

- polyplugc hard-errors if bundle name matches any contract name in api.toml
- call_plugin does not exist anywhere in codebase
- find_plugin does not exist anywhere in codebase  
- find_by_contract, find_by_bundle, find_all_by_contract implemented and tested
- resolve_plugin returns PluginVTableGuard wrapping arc-swap guard
- ArcSwap<VTableSlot> in every PluginSlot — no raw *const PluginVTable in slots
- bundle_index: HashMap<u64, u32> in Registry
- contract_index: HashMap<u64, Vec<u32>> in Registry (multi-impl)
- MY_BUNDLE_ID constant emitted by all 5 generators
- Dependency resolution code emitted in init() by all 5 generators
- Hard error on undeclared dependency access (not warn, not skip — error)
- Hard error in generated init when declared dependency not found
- [[dependency]] in bundle.toml parsed and validated by polyplugc
- manifest.toml [[dependency]] array with resolved IDs emitted by all 5 generators
- All 5 host libs updated — no find_plugin, no call_plugin
- All 5 guest libs updated — guard-based cross-plugin dispatch
- TRUST_MODEL.md exists, referenced from AGENTS.md
- Every unsafe block has // SAFETY: comment
- UndeclaredDependency, DependencyNotFound, BundleNotFound error variants exist
- Multi-impl test: find_all_by_contract returns all providers
- arc-swap read path test passes
- Cross-plugin benchmark within ~3 cycles of host-to-plugin
- All existing integration tests pass
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 10 — polyplug-python: Python Adapter

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-python subsection),
  section 15 (Memory Model, Python entries), section 16 (Error Handling, Python entry)

---

PROJECT CONTEXT

Python plugins are interpreted scripts. polyplug-python embeds CPython,
imports the plugin module, and calls the init function. The Python init
function uses ctypes to register vtables back through the C ABI.

The Python runtime is the performance bottleneck — not polyplug.
ctypes.Structure keeps cross-boundary data in C memory, outside Python GC.

The crate scaffold exists from Epic 8 at crates/polyplug-python/.
This epic fills it in completely, plus Python guest lib, Python host lib,
and PythonGenerator in polyplugc.

---

PRE-ANSWERED DECISIONS

PythonConfig:
  pub struct PythonConfig {
      pub min_version: (u32, u32),   // (major, minor) e.g. (3, 10)
  }
  PythonLoader::new(PythonConfig { min_version: (3, 10) })
  PythonConfig is mandatory.

Version check strategy — ONE interpreter, checked ONCE:
  Python has one interpreter per process. There is no per-plugin version.
  All plugins share the same interpreter. Version is checked once at init
  by reading sys.version_info via pyo3. If too old: hard error, no loading.
  - interpreter minor >= min_version.minor (same major) → proceed
  - interpreter minor < min_version.minor → InterpreterVersionTooOld error
  - interpreter major != min_version.major → InterpreterVersionTooOld error

Interpreter location: PYTHONHOME env var → PATH scan → well-known paths
  (/usr/bin/python3, /usr/local/bin/python3, system default).
  pyo3 respects PYO3_PYTHON env var for build-time selection.

Embedding: pyo3 0.28 — auto-initialize feature removed, use manually:
  Init: pyo3::prepare_freethreaded_python() once via OnceLock.
  All plugin loads: Python::with_gil(|py| { ... }).
  GIL released via py.allow_threads(|| { ... }) during any Rust-only work
  between plugin calls — never hold the GIL when not interacting with Python.
  pyo3 creates thread state automatically for each Rust thread.
  Cargo.toml: pyo3 = { version = "0.28", features = [] }

ctypes: standard library, no extra dependency.

host-libs/python/ polyplug.so resolution:
  ctypes.CDLL(os.path.join(os.path.dirname(__file__), "polyplug.so"))
  Co-located with the Python package. In tests, build.rs copies the
  built polyplug .so next to the Python package dir.
  Path override: Polyplug.Runtime.Builder.lib_path(path).

Plugin format: single .py file (no sys.path mutation).
  Loaded via importlib.util.spec_from_file_location.

Interpreter sharing: one shared interpreter, one GIL per process. OnceLock.

pip publishing: out of scope. Local package structure only.

Plugin packaging in tests: .py file in tests/fixtures/. No build step.

Dependency order: Rust crate first → Python libs → PythonGenerator.

---

EPIC GOAL

1. polyplug-python crate (crates/polyplug-python/):
   PythonConfig struct (as above).
   PythonLoader implementing BundleLoader:
   - runtime_name() returns "python"
   - On first load: pyo3::prepare_freethreaded_python() via OnceLock
     then Python::with_gil checks sys.version_info vs min_version → error or proceed
   - Per bundle: Python::with_gil(|py| {
       load via importlib.util.spec_from_file_location (no sys.path mutation)
       call init(registrar_ptr as ctypes c_void_p integer)
       plugin init calls ctypes back into C ABI to register vtables
     })
   - GIL released via py.allow_threads() during Rust-only work between loads
   - PolyplugError variants: InterpreterNotFound, InterpreterInitFailed,
     InterpreterVersionTooOld { required: String, found: String },
     ModuleImportFailed, InitFunctionMissing, InitRaisedException

2. Python guest lib (guest-libs/python/) — local package structure:
   - ctypes bindings for PluginRegistrar, HostVTable, PluginVTable
   - StringView and Buffer as ctypes.Structure (C memory, not Python heap)
     All string data stays in host_alloc memory — never copied to Python str
   - register_plugin(registrar_ptr, vtable) helper
   - Exception boundary: bare try/except per ABI fn in generated init → AbiError
   - PluginError(code: int, message: str) — encodes into ABI_ERROR_PLUGIN

3. Python host lib (host-libs/python/) — local package structure:
   - ctypes.CDLL loaded from co-located polyplug.so (path configurable)
   - ctypes bindings for all polyplug C ABI functions with explicit
     argtypes and restype on every function — no untyped calls
   - PluginRuntime class with builder pattern (plugin_dir, lib_path, build)
   - StringView and Buffer as ctypes.Structure — all data in C memory

4. PythonGenerator (crates/polyplugc/src/generators/python/mod.rs — new):

   From --api api.toml:
   - generated/host/types.py        domain types as ctypes.Structure
   - generated/host/callers.py      contract caller classes
   - generated/host/types.pyi       type stubs for IDE support
   - generated/host/callers.pyi
   - generated/guest/types.py
   - generated/guest/contracts.py   ABC per contract

   From --bundle bundle.toml:
   - generated/types.py
   - generated/contracts.py
   - generated/vtables.py           vtable construction via ctypes
   - generated/init.py              init(registrar_ptr), try/except per ABI fn → AbiError
   - generated/manifest.toml        runtime = "python"

   Performance rules for generated code:
   - All ctypes function objects cached at module level (not re-looked-up per call)
   - All argtypes/restype set once at import time
   - No Python object allocation on the hot path — ctypes.Structure only

5. polyplugc generate --lang python wired into CLI

6. .pyi stubs generated alongside every .py file

7. Python fixture plugin for integration tests:
   - tests/fixtures/test_plugin.py — single .py file
   - Implements test contract from test_api.toml
   - No build step

8. build.rs copies polyplug .so next to host-libs/python/ package dir for tests

9. Cross-language integration tests:
   - Rust host loads Python plugin → call two functions → assert results
   - Python host (host-libs/python/) loads Rust plugin → call → assert
   - Python host loads Python plugin
   - Python exception in plugin does not crash Rust host → AbiError returned
   - UTF-8 string round-trip: ASCII and non-ASCII
   - Interpreter too old → InterpreterVersionTooOld error, clear message
   - GIL released during Rust-only work (verified via allow_threads coverage)
   - ctypes function objects cached at module level (code inspection check)

10. Generated Python passes mypy --strict with zero errors

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- Python exception does not crash Rust host
- ctypes.Structure used for all cross-boundary types — no Python heap objects crossing ABI
- All ctypes function objects cached at module level — no per-call lookup
- argtypes/restype explicit on every ctypes function binding
- Generated Python passes mypy --strict
- .pyi stubs generated alongside all .py files
- polyplugc generate --lang python produces runnable output
- Interpreter version too old → InterpreterVersionTooOld error, clear message
- GIL not held during Rust-only work between plugin loads
- polyplug.so co-location and path resolution works in tests
- No .unwrap() in Rust production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 11 — polyplug-lua: Lua Adapter

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-lua subsection),
  section 15 (Memory Model, Lua entries), section 16 (Error Handling, Lua entry)

---

PROJECT CONTEXT

LuaJIT provides near-native performance via JIT compilation and its FFI library
which enables zero-copy struct passing — no intermediate copies at the ABI boundary.

The crate scaffold exists from Epic 8 at crates/polyplug-lua/.
This epic fills it in completely, plus Lua guest lib, Lua host lib,
and LuaGenerator in polyplugc.

---

PRE-ANSWERED DECISIONS

LuaConfig:
  pub enum LuaVersion { Jit, Lua55, Lua54, Lua53 }
  pub struct LuaConfig {
      pub min_version: LuaVersion,
  }
  LuaLoader::new(LuaConfig { min_version: LuaVersion::Jit })
  LuaConfig is mandatory.

Cargo dependency:
  LuaVersion::Jit:
    mlua = { version = "0.11", features = ["luajit", "vendored", "send"] }
  LuaVersion::Lua55:
    mlua = { version = "0.11", features = ["lua55", "vendored", "send"] }
  LuaVersion::Lua54:
    mlua = { version = "0.11", features = ["lua54", "vendored", "send"] }
  LuaVersion::Lua53:
    mlua = { version = "0.11", features = ["lua53", "vendored", "send"] }
  The "vendored" feature compiles LuaJIT/Lua from source — no system install needed.
  The "send" feature makes mlua::Lua: Send + Sync, required for OnceLock<Lua>.
  Internally mlua uses a reentrant mutex to serialize VM access when "send" enabled.

Version strategy:
  LuaVersion::Jit: requires LuaJIT specifically. If LuaJIT not available at
    compile time (vendored handles this), fail at build. No standard Lua fallback.
  LuaVersion::Lua54 / Lua53: standard Lua minimum version.
  This is a compile-time choice (feature flag), not a runtime check.
  The "version mismatch" error only applies when app requests Jit but binary
  was compiled with lua54 features — caught at Rust compile time via features.

VM: one shared Lua VM per process.
  OnceLock<Lua> in LuaLoader (requires "send" feature).
  Internally mlua uses a reentrant mutex — safe for concurrent access.

Registrar pointer passing — FFI cdata pointer, NOT lightuserdata:
  CRITICAL: LuaJIT lightuserdata has a 47-bit pointer limit on x86_64 Linux
  (only 47 bits of address space). Registrar pointer may be on the stack or
  in any memory range. Use FFI cdata void pointer instead:
    Rust side: lua.globals().set("_registrar_ptr",
      lua.create_integer(registrar_ptr as usize as i64)?)
    Lua side: local reg = ffi.cast("PluginRegistrar*",
      ffi.cast("uintptr_t", _registrar_ptr))
  The casts are done once at init time only — no overhead on hot path.
  FFI cdata function pointer calls are JIT-compiled to indirect calls (~native speed).

Zero-copy struct passing via LuaJIT FFI metatype:
  All ABI structs declared via ffi.cdef in guest_lib.lua once at load.
  Function pointers in vtable stored as FFI cdata — calls are JIT-compiled.
  Performance: ffi metatype calls ~2B ops/sec vs lightuserdata ~45M ops/sec.
  This is the correct pattern for near-zero overhead.

Plugin format: single .lua file.
  Bundle path in manifest file field points to the .lua file.
  Bundle dir contains the .lua file + manifest.toml.

Lua host lib C extension: built via cc crate in build.rs.
  Produces polyplug_lua.so alongside polyplug.lua.

Lua publishing: out of scope. Local files only.

Dependency order: Rust crate first → Lua libs → LuaGenerator.

---

EPIC GOAL

1. polyplug-lua crate (crates/polyplug-lua/):
   LuaConfig + LuaVersion (as above).
   Cargo.toml: mlua with features per LuaVersion (see PRE-ANSWERED DECISIONS).
   LuaLoader implementing BundleLoader:
   - runtime_name() returns "lua"
   - VM initialized once via OnceLock<Lua> using "send" feature
   - Per bundle:
     lua.load(chunk).exec() to load plugin script
     set _registrar_ptr global as i64 (uintptr_t of registrar pointer)
     call init() Lua function — Lua FFI casts integer back to PluginRegistrar*
     Lua init builds vtable via FFI and calls registrar->register()
   - PolyplugError variants: VmInitFailed, ScriptLoadFailed,
     InitFunctionMissing, InitRaisedError

2. Lua guest lib (guest-libs/lua/polyplug_guest.lua):
   - ffi.cdef declarations for PluginRegistrar, HostVTable, PluginVTable,
     StringView, Buffer — declared ONCE at load time
   - Registrar accessed via:
     local reg = ffi.cast("PluginRegistrar*", ffi.cast("uintptr_t", _registrar_ptr))
   - All vtable function pointers stored as FFI cdata — JIT-compiled indirect calls
   - ffi.metatype used for domain types — enables JIT allocation sinking
   - Error boundary: lua_pcall wraps each ABI function in generated init
   - NO lightuserdata used for pointers — all pointers are FFI cdata void*/typed*

3. Lua host lib (host-libs/lua/):
   - polyplug.lua: LuaJIT FFI declarations for all polyplug C ABI functions
     loaded via ffi.load(polyplug_lib_path)
   - polyplug_lua.so: C extension built via cc crate in build.rs
     Exports one function: polyplug_lib_path() → path to polyplug .so
   - PluginRuntime table with builder pattern (plugin_dir, loader, extension, build)
   - All function pointer calls via FFI cdata — JIT-compiled

4. LuaGenerator (crates/polyplugc/src/generators/lua/mod.rs — new):

   From --api api.toml:
   - generated/host/types.lua       domain types via ffi.metatype cdata
   - generated/host/callers.lua     contract caller tables (FFI function ptrs)
   - generated/guest/types.lua
   - generated/guest/contracts.lua  contract interface tables (metatables)

   From --bundle bundle.toml:
   - generated/types.lua
   - generated/contracts.lua
   - generated/vtables.lua          vtable construction via ffi.new + ffi.cast
   - generated/init.lua             init(registrar_ptr_int), pcall per ABI fn
   - generated/manifest.toml        runtime = "lua"

   Performance rules for generated code:
   - All ffi.cdef calls at module top level, never in hot path
   - All function pointer casts done once at init, stored in locals
   - ffi.metatype for all domain types — enables allocation sinking by JIT
   - No Lua table lookups on hot path for vtable dispatch

5. polyplugc generate --lang lua wired into CLI

6. Lua fixture plugin for integration tests:
   - tests/fixtures/test_plugin.lua — single .lua file
   - Implements test contract from test_api.toml
   - No build step

7. Cross-language integration tests:
   - Rust host loads Lua plugin → call two functions → assert results
   - Lua host (host-libs/lua/) loads Rust plugin → call → assert
   - Lua host loads Lua plugin
   - Lua error() in plugin does not crash Rust host → AbiError returned
   - FFI cdata pointer test: registrar pointer correctly cast via uintptr_t
   - LuaJIT performance test: call overhead within 2x of native baseline
     (from BENCHMARKS.md baseline)

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- Lua error does not crash Rust host
- No lightuserdata used for any pointer — all pointers are FFI cdata (code inspection)
- Registrar pointer passed as uintptr_t integer, cast to typed* via FFI on Lua side
- ffi.metatype used for domain types (code inspection)
- All ffi.cdef calls at module load time, never on hot path
- LuaJIT performance test passes within 2x of native baseline
- polyplugc generate --lang lua produces runnable output
- polyplug_lua.so C extension built correctly via cc crate
- No .unwrap() in Rust production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 11.5 — polyplug-js and polyplug-js-deno: JavaScript and TypeScript Adapters

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-js and polyplug-js-deno subsections),
  section 23 (MVP Language Support, JS/TS rows),
  section 24 (Package Ecosystem, JS/TS packages)

---

PROJECT CONTEXT

Two separate adapter crates handle JS/TS plugins. App developer picks one or both:

  polyplug-js        → QuickJS embedded via rquickjs, runtime = "js-quickjs"
  polyplug-js-deno   → V8 embedded via deno_core, runtime = "js-deno"

Both are fully IN-PROCESS. No subprocess. No IPC. No process boundary.
Both use the same architecture as polyplug-lua: embed a JS VM in the host process,
register HostVTable functions as callable JS globals, load the plugin script,
vtable exchange happens entirely in-process via direct Rust function pointers.

Plugin bundles are single flat JS files produced by Rolldown at pack time.
Rolldown (invoked by polyplugc pack) bundles TypeScript + npm dependencies
into one self-contained bundle.js. The loader sees only bundle.js — no imports,
no npm at runtime, no node_modules.

The crate scaffolds exist from Epic 8:
  crates/polyplug-js/
  crates/polyplug-js-deno/
This epic fills both in completely, plus shared JS/TS guest lib and generators.

---

PRE-ANSWERED DECISIONS

RUNTIME NAMES (manifest.toml runtime field):
  js-quickjs   → polyplug-js (QuickJS via rquickjs)
  js-deno      → polyplug-js-deno (V8 via deno_core)
  No other JS runtime variants exist. ts-node, ts-bun, ts-deno, js-node,
  js-bun are NOT part of this system — they were rejected due to subprocess
  requirements violating the zero-overhead constraint.

CARGO DEPENDENCIES:
  crates/polyplug-js/Cargo.toml:
    rquickjs = { version = "0.11.0", features = ["loader", "futures"] }

  crates/polyplug-js-deno/Cargo.toml:
    deno_core    = "0.311.0"
    smol         = "2.0.0"
    futures-lite = "2.3.0"
  NOTE: tokio is NOT used. smol LocalExecutor satisfies V8 thread-pinning
  requirement identically to tokio current_thread, at 1/10th the weight.
  deno_core explicitly supports smol as an executor.

BUNDLE FORMAT — BOTH VARIANTS:
  Bundle is a directory containing:
    manifest.toml          (runtime = "js-quickjs" or "js-deno")
    bundle.js              (single flat file, produced by rolldown at pack time)
  No index.ts, no node_modules, no polyplug.node, no .node addon.
  The loader loads bundle.js directly — that's it.

  bundle.js is produced by plugin developer running:
    rolldown index.ts --format iife --platform neutral --file bundle.js
  polyplugc pack --lang js-quickjs shells out to rolldown automatically.
  Plugin developer needs rolldown installed: npm i -g rolldown
  polyplugc documents this requirement clearly in error messages.

polyplug-js LOADING MODEL — QuickJS in-process:
  1. rquickjs::Runtime::new()        ← one shared JS VM, mutex-protected (like mlua)
  2. ctx.globals().set("polyplug", host_object)
     host_object exposes ALL HostVTable functions as direct Rust fn ptrs:
       polyplug.findByContract(contract_id_lo, contract_id_hi, min_version)
       polyplug.findByBundle(bundle_id_lo, bundle_id_hi, contract_id_lo, contract_id_hi, min_version)
       polyplug.findAllByContract(contract_id_lo, contract_id_hi, min_version)
       polyplug.resolvePlugin(handle_index, handle_generation)
       polyplug.getExtension(extension_id)
       polyplug.registerVtable(contract_id_lo, contract_id_hi, vtable_obj)
       polyplug.alloc(size)
       polyplug.free(ptr_lo, ptr_hi)
     NOTE: u64 values split into lo/hi u32 pairs because QuickJS numbers
     are f64 — cannot hold 64-bit integers without precision loss.
     JS side reassembles: const id = (hi * 0x100000000) + lo
     This is safer than BigInt in QuickJS (BigInt support varies by version).
  3. ctx.eval(bundle_js_string)      ← plugin runs, calls polyplug.*, registers vtable
  4. Extract registered vtable from ctx globals → register in PluginRegistry
  5. Load complete. VM mutex released.

  Hot-path vtable call: host calls registered Rust fn ptr → rquickjs
  calls the JS function → direct Rust function pointer return.
  Overhead: JS value boxing/unboxing only (~50-200ns). Same tier as Lua.

polyplug-js-deno LOADING MODEL — V8 in-process, one thread per bundle:
  1. std::thread::spawn for this bundle
  2. Inside thread: smol::LocalExecutor::new()
     futures_lite::future::block_on(ex.run(async move { ... }))
  3. deno_core::JsRuntime::new(RuntimeOptions {
         extensions: vec![polyplug_extension()],
         ...
     })
     polyplug_extension() registers ALL HostVTable ops via deno_core::extension! macro:
       #[op2(fast)] op_find_by_contract(contract_id: u64, min_version: u32) → u64
       #[op2(fast)] op_find_by_bundle(bundle_id: u64, contract_id: u64, min_version: u32) → u64
       #[op2(fast)] op_find_all_by_contract(contract_id: u64, min_version: u32) → Vec<u64>
       #[op2(fast)] op_resolve_plugin(handle: u64) → u64   (returns guard token)
       #[op2(fast)] op_get_extension(extension_id: u32) → u64  (returns ptr as u64)
       #[op2(fast)] op_register_vtable(contract_id: u64, #[serde] vtable: VtableDesc)
       #[op2(fast)] op_alloc(size: u32) → u64
       #[op2(fast)] op_free(ptr: u64)
     deno_core ops are direct in-process function calls — NOT IPC.
     #[op2(fast)] uses V8 fast calls — ~50-200ns overhead.
     u64 values pass natively in deno_core ops (V8 BigInt handled automatically).
  4. TypeScript is loaded natively — deno_core TsModuleLoader handles .ts directly.
     No separate transpilation step needed for js-deno bundles.
     Plugin developer can ship index.ts directly (no rolldown required for js-deno).
     Rolldown is optional for js-deno — use it only to bundle npm dependencies.
  5. runtime.load_main_es_module(&module_url).await
  6. runtime.run_event_loop(Default::default()).await
     → plugin's top-level code runs, calls Deno.core.ops.op_register_vtable(...)
     → vtable registered back into host via op
  7. Thread parks on std::sync::mpsc::Receiver, waiting for vtable call requests.
  8. Each vtable fn ptr (stored in PluginRegistry) sends call request + args
     over channel → thread wakes → deno_core executes JS fn → result sent back.

  Hot-path vtable call overhead: deno_core #[op2(fast)] (~50-200ns) +
  channel roundtrip (~1-5μs). Still fully in-process, no OS context switch.
  ~5-30x slower than QuickJS per cross-plugin call. Documented in BENCHMARKS.md.

HostVTable pointer access in ops:
  HostVTable* stored in a thread-local static inside polyplug-js-deno,
  set before the JsRuntime is created on that thread.
  ops read it via thread_local! — safe because ops always execute on the
  same thread as the JsRuntime (V8 isolate constraint guarantees this).
  No mutex needed — single-threaded access by design.

  For QuickJS (polyplug-js):
  HostVTable* stored in a OnceLock<*const HostVTable> set once during
  JsLoader::new() from the HostVTable passed via the registrar.
  SAFETY: HostVTable* is valid for the lifetime of PluginRuntime.
  All QuickJS Rust callbacks read this OnceLock — no mutex needed on reads.

JsConfig and JsDenoConfig:
  pub struct JsConfig {
      // No fields — QuickJS is always available, no system deps
  }
  pub struct JsDenoConfig {
      // No fields — V8 is embedded, no system deps
  }
  JsLoader::new(JsConfig {})        // always works, no configuration needed
  JsDenoLoader::new(JsDenoConfig {}) // always works, no configuration needed

  App developer registers one or both:
    runtime_builder
        .loader(JsLoader::new(JsConfig {}))         // enables js-quickjs
        .loader(JsDenoLoader::new(JsDenoConfig {})) // enables js-deno

BundleLoader::runtime_name():
  JsLoader returns    &["js-quickjs"]
  JsDenoLoader returns &["js-deno"]

GENERATED FILES — polyplugc --lang js-quickjs:
  contracts.ts   — TypeScript interfaces for each contract
  types.ts        — domain types as TypeScript interfaces
  vtable.ts       — vtable registration helpers, calls polyplug.registerVtable()
  init.ts         — entry point: dependency resolution, extension queries,
                     vtable registration
  manifest.toml   — runtime = "js-quickjs"
  README.md       — build instructions: how to run rolldown, what bundle.js is

  Dependency resolution in generated init.ts (js-quickjs):
    // u64 split into lo/hi because QuickJS uses f64
    const handle = polyplug.findByContract(
        CONTRACT_ID_LO, CONTRACT_ID_HI, 1);
    if (!handle) throw new Error("dependency not found: image.decode");
    const guard = polyplug.resolvePlugin(handle.index, handle.generation);

  Extension query in generated init.ts (when optional includes "trace"):
    const tracePtr = polyplug.getExtension(EXT_TRACE_ID);
    const trace = tracePtr ? new TraceVTable(tracePtr) : null;

GENERATED FILES — polyplugc --lang js-deno:
  Same structure as js-quickjs EXCEPT:
    init.ts uses Deno.core.ops.op_find_by_contract(contract_id_bigint, min_ver)
    u64 values passed as BigInt natively (V8 supports BigInt properly)
    manifest.toml runtime = "js-deno"
  No rolldown required at build time (deno_core handles .ts natively).
  README.md documents optional rolldown usage for npm dependencies.

ABI TYPE MAPPING — TypeScript (both variants):
  u8/u16/u32  → number
  u64         → { lo: number, hi: number } for js-quickjs
                BigInt for js-deno (V8 BigInt is safe)
  i8/i16/i32  → number
  i64         → { lo: number, hi: number } for js-quickjs
                BigInt for js-deno
  f32/f64     → number
  bool        → boolean
  StringView  → { ptr_lo: number, ptr_hi: number, len: number }
  Buffer      → { ptr_lo: number, ptr_hi: number, len: number, cap: number }
  void        → void

JS/TS GUEST LIB — guest-libs/js/:
  polyplug-guest.ts:
    AbiError enum
    StringView, Buffer interfaces (with lo/hi ptr fields)
    DependencyNotFoundError class
    TraceVTable interface (emitted when trace in optional[])
    EXT_TRACE_ID constant = 0xC4EB9AEE
  Shared between js-quickjs and js-deno generators.
  Re-exported in generated init.ts.

JS/TS HOST LIB — host-libs/js/:
  Not in scope for this epic. Host-side JS is not a supported use case.
  The host is always a Rust binary. Skip this entirely.

ROLLDOWN REQUIREMENT:
  polyplugc pack --lang js-quickjs:
    Shells out to: rolldown index.ts --format iife --platform neutral --file bundle.js
    If rolldown not found on PATH: hard error with message:
      "rolldown is required for js-quickjs pack. Install with: npm i -g rolldown"
  polyplugc pack --lang js-deno:
    Rolldown optional. If index.ts present and no bundle.js:
      Warn: "Tip: run rolldown to bundle npm dependencies into bundle.js"
      Skip rolldown, ship index.ts directly.
    If bundle.js present: ship bundle.js (npm deps bundled).
  polyplugc generate --lang js-quickjs and --lang js-deno:
    Generates source files only. Does NOT invoke rolldown.
    pack is the command that invokes rolldown.

node-polyfills:
  NOT included. rolldown-plugin-node-polyfills is NOT used.
  Plugins needing Node.js APIs (fs, http, net) must use js-deno.
  This boundary is documented clearly in generated README.md and PRD.

---

EPIC GOAL

1. crates/polyplug-js/src/config/mod.rs:
   pub struct JsConfig {}  // no fields
   JsLoader::new(JsConfig {})

2. crates/polyplug-js/src/loader/mod.rs:
   JsLoader implementing BundleLoader
   runtime_name() → &["js-quickjs"]
   load(path, registrar):
     a. Read bundle.js from bundle directory
     b. Create rquickjs::Runtime + Context (or reuse shared Runtime from OnceLock)
     c. Set HostVTable* in OnceLock (first load only)
     d. Register polyplug global object with all HostVTable wrapper functions
     e. ctx.eval(bundle_js) — plugin runs, calls polyplug.registerVtable(...)
     f. Extract registered vtable from ctx, store in PluginRegistry

3. rquickjs HostVTable wrappers — all 8 functions registered on polyplug global:
   findByContract(lo, hi, min_version) → {index, generation} | null
   findByBundle(bundle_lo, bundle_hi, contract_lo, contract_hi, min_version) → {index,gen}|null
   findAllByContract(lo, hi, min_version) → [{index,generation}]
   resolvePlugin(index, generation) → guard_token (u32, opaque)
   getExtension(extension_id) → {lo, hi} | null   (ptr as lo/hi pair)
   registerVtable(contract_lo, contract_hi, vtable_obj) → void
   alloc(size) → {lo, hi}
   free(lo, hi) → void
   SAFETY comment required on every wrapper: HostVTable* lifetime = Runtime lifetime.

4. crates/polyplug-js-deno/src/config/mod.rs:
   pub struct JsDenoConfig {}  // no fields
   JsDenoLoader::new(JsDenoConfig {})

5. crates/polyplug-js-deno/src/loader/mod.rs:
   JsDenoLoader implementing BundleLoader
   runtime_name() → &["js-deno"]
   load(path, registrar):
     a. Capture HostVTable* in thread-local before spawning thread
     b. std::thread::spawn
     c. Inside thread: smol::LocalExecutor + futures_lite::future::block_on
     d. Set thread-local HostVTable* (now on correct thread)
     e. deno_core::JsRuntime::new with polyplug_ops extension
     f. load_main_es_module (index.ts or bundle.js — whichever exists)
     g. run_event_loop → plugin runs, calls Deno.core.ops.op_register_vtable
     h. Extract vtable from op call result, send back to loader via oneshot channel
     i. Thread parks on mpsc Receiver for vtable call requests
     j. Loader receives vtable, stores in PluginRegistry

6. deno_core ops — polyplug_extension() via deno_core::extension! macro:
   All 8 HostVTable functions as #[op2(fast)] ops.
   Thread-local HostVTable* read inside each op — safe (same thread as V8 isolate).
   op_register_vtable: receives vtable description, sends to loader via oneshot.

7. Channel architecture for js-deno vtable calls:
   Each js-deno bundle slot has:
     call_tx: std::sync::mpsc::SyncSender<JsCallRequest>
     result_rx: (managed per-call via oneshot)
   JsCallRequest { fn_index: u32, args: Vec<JsValue>, result_tx: oneshot::Sender<JsValue> }
   Generated vtable fn ptrs: pack args → send to call_tx → block on result_rx.
   JS thread: loop on call_rx → execute JS fn → send result.
   oneshot channel: smol::channel::oneshot or std::sync::mpsc one-shot pattern.

8. PolyplugError new variants (crates/polyplug/src/error/mod.rs):
   RolldownNotFound { hint: String }   — rolldown not on PATH during pack
   JsRuntimePanic { runtime: String, message: String }  — JS exception during load

9. polyplugc — JsQuickjsGenerator:
   crates/polyplugc/src/generators/js_quickjs/mod.rs
   Generates from bundle.toml + api.toml:
     contracts.ts  — TypeScript interfaces
     types.ts      — domain types with lo/hi ptr representation
     vtable.ts     — registerVtable() helper
     init.ts       — dependency resolution (lo/hi), extension query, vtable reg
     manifest.toml — runtime = "js-quickjs"
     README.md     — rolldown build instructions, js-quickjs vs js-deno guidance
   --lang js-quickjs routes to this generator.

10. polyplugc — JsDenoGenerator:
    crates/polyplugc/src/generators/js_deno/mod.rs
    Same structure as JsQuickjsGenerator EXCEPT:
      u64 values use BigInt (not lo/hi)
      init.ts uses Deno.core.ops.op_*() for all polyplug calls
      manifest.toml runtime = "js-deno"
      README.md — deno guidance, optional rolldown for npm deps
    --lang js-deno routes to this generator.

11. polyplugc pack command updates:
    --lang js-quickjs: shell out to rolldown. Error if not found.
    --lang js-deno:    rolldown optional. Warn if index.ts present and no bundle.js.

12. polyplugc CLI: wire --lang js-quickjs and --lang js-deno into generator dispatch.

13. guest-libs/js/polyplug-guest.ts:
    AbiError, StringView, Buffer (lo/hi ptr fields), DependencyNotFoundError,
    EXT_TRACE_ID = 0xC4EB9AEE
    TraceVTable interface with emit(ptr_lo, ptr_hi, len) signature.
    Re-exported in generated init.ts for both variants.

14. Integration tests — tests/integration_js/mod.rs (new):
    a. js-quickjs: load plugin, call two contract functions, assert results
    b. js-deno: load plugin, call two contract functions, assert results
    c. js-quickjs: plugin calls polyplug.findByContract → gets valid handle
    d. js-deno: plugin calls op_find_by_contract → gets valid handle
    e. js-quickjs: plugin calls polyplug.getExtension → null when absent, no crash
    f. js-deno: plugin calls op_get_extension → null when absent, no crash
    g. js-quickjs: full dependency chain — JS plugin depends on Rust plugin,
       resolves via findByContract, calls dependency vtable fn, asserts result
    h. js-deno: same dependency chain test
    i. js-quickjs: alloc/free — no leak (ASAN)
    j. js-deno: alloc/free — no leak (ASAN)
    k. rolldown not found: polyplugc pack --lang js-quickjs → RolldownNotFound error
    l. js-deno thread isolation: two js-deno bundles loaded → each has own thread,
       concurrent calls don't interfere

15. Benchmarks (BENCHMARKS.md):
    - js-quickjs cross-plugin call vs Rust-to-Rust baseline
    - js-deno cross-plugin call vs Rust-to-Rust baseline
    - js-deno channel roundtrip latency documented explicitly
    - "When to pick which" section in BENCHMARKS.md referencing PRD guidance

---

VERIFICATION CHECKLIST

- JsLoader runtime_name() = ["js-quickjs"] — verified
- JsDenoLoader runtime_name() = ["js-deno"] — verified
- js-quickjs load: in-process, no subprocess, no IPC — verified by test
- js-deno load: in-process, dedicated thread, smol LocalExecutor — verified
- No tokio dependency anywhere in polyplug-js-deno — clippy check
- All 8 HostVTable functions accessible from JS in both variants — each tested
- u64 lo/hi split correct in js-quickjs — no precision loss test
- BigInt correct in js-deno — no precision loss test
- js-quickjs dependency resolution works end-to-end (JS calls Rust plugin)
- js-deno dependency resolution works end-to-end (JS calls Rust plugin)
- getExtension returns null for unregistered ID — both variants, no crash
- alloc/free no leak — ASAN, both variants
- rolldown invoked by polyplugc pack --lang js-quickjs — verified
- rolldown not found → RolldownNotFound error with install hint
- js-deno: TS loaded natively without rolldown — verified
- js-deno: two bundles → two threads, concurrent calls correct — verified
- EXT_TRACE_ID = 0xC4EB9AEE in guest-libs/js/polyplug-guest.ts
- generated init.ts: extension query code only when optional includes "trace"
- BENCHMARKS.md updated with js-quickjs and js-deno numbers
- All existing integration tests pass
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 11.5 — Patch: Fix JS Architecture (polyplug-js and polyplug-js-deno)

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this patch are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

This is a PATCH prompt — it corrects what was implemented in Epic 11.5.
The original implementation was fundamentally wrong. Read the WHAT IS WRONG
section carefully before planning anything.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-js and polyplug-js-deno subsections)
- Epic 11.5 in epic_prompts.md — the full corrected spec is there
- Epic 12 context in epic_prompts.md — it is already implemented and must keep working

---

WHAT IS WRONG — THE ORIGINAL EPIC 11.5 IMPLEMENTATION

The original implementation used:
  - Node.js subprocess spawning
  - N-API / polyplug.node bridge
  - ts-node, ts-bun, ts-deno, js-node, js-bun, js-deno runtime variants
  - Env-var or dlsym to pass HostVTable* across a process boundary

ALL OF THIS IS WRONG. It violates the core polyplug design principle:
zero overhead, maximum performance, in-process only.

Subprocesses create a process boundary. Raw pointers cannot cross process
boundaries. Any pointer-passing scheme across processes (env-var, IPC,
shared memory) is fundamentally broken and was never acceptable.

The CORRECT architecture has been fully designed. See below.

---

WHAT THE CORRECT ARCHITECTURE IS

TWO separate adapter crates. App developer picks one or both:

  polyplug-js        QuickJS via rquickjs, runtime = "js-quickjs"
  polyplug-js-deno   V8 via deno_core + smol, runtime = "js-deno"

Both are IN-PROCESS. No subprocess. No IPC. No process boundary.
Same model as polyplug-lua: embed a VM, register HostVTable fns as JS globals,
eval the plugin script, vtable exchange in-process via direct fn ptrs.

Plugin bundles are single flat JS files (bundle.js) produced by Rolldown.
Rolldown is invoked by polyplugc pack at build time — not at runtime.
At runtime the loader sees only bundle.js. No imports. No npm. No node_modules.

---

PRE-ANSWERED DECISIONS

See Epic 11.5 full spec in epic_prompts.md for complete details.
Key decisions summarised here:

CRATE STRUCTURE:
  crates/polyplug-js/          QuickJS adapter (rquickjs 0.11.0)
  crates/polyplug-js-deno/     V8 adapter (deno_core 0.311.0 + smol 2.0.0)
  DELETE or gut: any polyplug-js-napi crate, any polyplug.node artifact,
  any subprocess spawning code, any env-var pointer passing code.

RUNTIME NAMES:
  "js-quickjs"   handled by JsLoader (polyplug-js)
  "js-deno"      handled by JsDenoLoader (polyplug-js-deno)
  ALL OLD NAMES REMOVED: ts-node, ts-bun, ts-deno, js-node, js-bun
  Epic 12 manifest runtime field parsing must be updated to recognise
  "js-quickjs" and "js-deno" and reject the old names.

POLYPLUG-JS (QuickJS) LOADING:
  rquickjs::Runtime::new() — one shared VM, mutex-protected (like mlua)
  HostVTable* stored in OnceLock<*const HostVTable> set at JsLoader::new()
  SAFETY: HostVTable* is valid for lifetime of PluginRuntime.
  Register polyplug global on ctx with 8 wrapper functions:
    findByContract, findByBundle, findAllByContract, resolvePlugin,
    getExtension, registerVtable, alloc, free
  u64 values split into lo/hi u32 pairs (QuickJS uses f64 internally,
  cannot hold 64-bit integers — split is the correct solution).
  ctx.eval(bundle_js) → plugin runs → registerVtable called → vtable extracted.

POLYPLUG-JS-DENO (V8) LOADING:
  std::thread::spawn per bundle (V8 isolate is thread-pinned, !Send)
  smol::LocalExecutor — NOT tokio. smol is explicitly supported by deno_core.
  HostVTable* stored in thread_local! set before JsRuntime creation.
  deno_core::extension! macro registers 8 #[op2(fast)] ops.
  deno_core TsModuleLoader handles .ts natively — rolldown optional for js-deno.
  vtable received from plugin via op_register_vtable → sent to loader via oneshot.
  Thread parks on mpsc Receiver after load — woken for each vtable call.
  Call channel: JsCallRequest { fn_index, args, result_tx } per call.

CONFIGS (both empty — no system deps):
  pub struct JsConfig {}
  pub struct JsDenoConfig {}
  JsLoader::new(JsConfig {})
  JsDenoLoader::new(JsDenoConfig {})

ROLLDOWN:
  polyplugc pack --lang js-quickjs: REQUIRED, shells out to rolldown CLI.
    rolldown index.ts --format iife --platform neutral --file bundle.js
    If not found: PolyplugError::RolldownNotFound with install hint.
  polyplugc pack --lang js-deno: OPTIONAL. deno_core loads .ts natively.
    If index.ts present and no bundle.js: warn about rolldown for npm deps.
  node-polyfills: NOT used. Documented clearly.

NEW ERROR VARIANTS (crates/polyplug/src/error/mod.rs):
  RolldownNotFound { hint: String }
  JsRuntimePanic { runtime: String, message: String }
  REMOVE: RuntimeNotYetImplemented, JsLoaderNoRuntimeConfigured

GENERATORS (polyplugc):
  REMOVE: JsNodeGenerator, JsBunGenerator, JsDenoGenerator (old stubs)
  ADD: JsQuickjsGenerator   → --lang js-quickjs
  ADD: JsDenoGenerator (new, correct) → --lang js-deno
  See Epic 11.5 spec for exact generated file structure.

EPIC 12 COMPATIBILITY:
  Epic 12 is already implemented. It reads manifest.toml runtime field
  and dispatches to registered loaders by runtime_name().
  After this patch:
    - JsLoader.runtime_name() = ["js-quickjs"]
    - JsDenoLoader.runtime_name() = ["js-deno"]
  Epic 12 dispatch requires no changes — it matches runtime field to
  runtime_name() dynamically. As long as loader is registered and
  runtime field matches, it works. Verify this still holds after patch.
  Any existing test fixtures with old runtime names (ts-node etc.) must
  be regenerated with js-quickjs or js-deno runtime.

---

PATCH SCOPE

This patch replaces the entire JS/TS adapter implementation.
The scope is:
  1. Delete/gut old polyplug-js implementation (subprocess, N-API, old configs)
  2. Implement polyplug-js correctly (QuickJS, in-process)
  3. Implement polyplug-js-deno correctly (V8, in-process, smol, dedicated thread)
  4. Update polyplugc generators (remove old, add correct two)
  5. Update guest-libs/js/ (remove N-API types, add correct lo/hi types)
  6. Remove host-libs/js/ entirely (host is always Rust, not needed)
  7. Update error variants
  8. Update Epic 12 test fixtures that use old runtime names
  9. All integration tests pass

The EXECUTER must NOT attempt to salvage the old implementation.
Start fresh on polyplug-js and polyplug-js-deno. Keep all other crates untouched.

---

VERIFICATION CHECKLIST

- No subprocess code anywhere in polyplug-js or polyplug-js-deno
- No N-API, no polyplug.node, no .node files anywhere
- No ts-node, ts-bun, ts-deno, js-node, js-bun references in any source file
- No tokio dependency in polyplug-js-deno
- No env-var pointer passing anywhere
- JsLoader.runtime_name() = ["js-quickjs"]
- JsDenoLoader.runtime_name() = ["js-deno"]
- js-quickjs: load bundle.js in-process via rquickjs, vtable registered
- js-deno: load index.ts or bundle.js in-process via deno_core + smol thread
- All 8 HostVTable functions accessible from JS — both variants
- u64 lo/hi split in js-quickjs — no precision loss
- BigInt in js-deno — no precision loss
- js-quickjs: getExtension null for unregistered ID — no crash
- js-deno: getExtension null for unregistered ID — no crash
- Full dependency chain test: JS plugin depends on Rust plugin — both variants
- rolldown invoked by polyplugc pack --lang js-quickjs
- rolldown not found → RolldownNotFound error with install hint
- js-deno: .ts loaded natively without rolldown
- Epic 12 discovery still works: js-quickjs and js-deno bundles discovered
  and dispatched correctly
- All Epic 12 existing tests still pass
- EXT_TRACE_ID = 0xC4EB9AEE in guest-libs/js/polyplug-guest.ts
- All existing integration tests (non-JS) pass
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 12 — Plugin Discovery and Manifest System

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 13 (Plugin Discovery)

---

PROJECT CONTEXT

Previous epics load plugins with explicit paths passed directly to the loader.
This epic makes discovery automatic: the runtime scans directories, reads
manifests without loading anything, resolves the full capability graph across
all discovered bundles, and dispatches each bundle to the correct loader.

The BundleLoader trait and all loader types (native + four adapters) exist.
The manifest.toml runtime field is read by the parser (from Epic 8).
Epic 9.7 redesigned manifest.toml — it now has:
  - bundle_id: u64 (fnv1a_64 of bundle name)
  - [[dependency]] table array (replaces the old requires = [...] string array)
    each entry has: contract, contract_id, min_version
    or: bundle, bundle_id, contract, contract_id, min_version
Epic 11.5 added polyplug-js and polyplug-js-deno. Valid runtime values are now:
  native, dotnet, python, lua, js-quickjs, js-deno
  js-quickjs routes to JsLoader (polyplug-js, QuickJS embedded in-process).
  js-deno routes to JsDenoLoader (polyplug-js-deno, V8 embedded in-process).
  The manifest reader must recognise all six as valid — not treat them as unknown.
This epic wires everything together into a complete discovery pipeline.
The capability graph builder must consume [[dependency]] from manifest.toml,
not the old requires = [...] field (which no longer exists).

---

EPIC GOAL

1. Directory scanner in crates/polyplug/src/loader/ (new submodule or extend existing):
   - Scans configured directories for bundle files
   - Recognizes .so, .dll, .dylib for compiled native bundles
   - Recognizes directories containing manifest.toml for script bundles
     (Python, Lua, and JS/TS — all use directory format for script bundles)
   - Finds companion manifest.toml for each compiled bundle
   - NEVER calls any loader or dlopen during scanning phase
   - Returns Vec<(bundle_path, ManifestData)>

2. Manifest reader:
   - Reads manifest.toml from disk
   - Parses into ManifestData with explicit types on all fields:
     name: String, bundle_id: u64, version: String, runtime: String, file: String,
     provides: Vec<String>,
     function_count: HashMap<String, u32>,
     dependencies: Vec<ManifestDependency>
   - ManifestDependency has two variants:
       ByContract { contract: String, contract_id: u64, min_version: String }
       ByBundle { bundle: String, bundle_id: u64, contract: String,
                  contract_id: u64, min_version: String }
   - The old requires = [...] string array NO LONGER EXISTS in manifest.toml.
     Do not attempt to parse it. Use [[dependency]] exclusively.
   - Skips malformed manifests with warning, does not abort entire scan
   - Logs which manifests were skipped and why

3. Full capability graph resolution across multiple discovered bundles:
   - Extends or replaces graph module to work across multiple bundle manifests
   - Collects all provides from all ManifestData
   - For each bundle, resolves all dependencies from ManifestData.dependencies:
       ByContract: validates some bundle in discovered set provides that contract
       ByBundle: validates the specific named bundle is present AND provides that contract
   - Validates all dependencies are satisfied before loading anything
   - Detects dependency cycles across bundle boundaries
   - Topological sort produces ordered Vec<bundle_path> for loading

4. Loader dispatch after graph resolution:
   - Iterates ordered bundle list
   - For each bundle: manifest.runtime → find registered loader by runtime_name
   - Calls loader.load(path, registrar)
   - If no loader for runtime: clear error, behavior discussed with me

5. Explicit registration API alongside directory scanning:
   runtime.load_bundle(path: &Path) -> Result<(), PolyplugError>
   runtime.load_bundle_with(path: &Path, opts: LoadOptions) -> Result<(), PolyplugError>

6. Multi-bundle integration tests:
   - Three bundles A→B→C: A provides X, B requires X provides Y, C requires Y
     → verified load order is A then B then C
   - Missing dependency: B requires X, nothing provides X
     → clear error before any loader.load called
   - Cycle A↔B: A requires B, B requires A
     → detected, clear error naming both
   - All language bundles in same directory: all discovered, correct loaders used
   - Malformed manifest in one bundle: that bundle skipped, others load normally
   - Unknown runtime: behavior per decision made with me

---

VERIFICATION CHECKLIST

- All multi-bundle tests pass
- Load order verified correct in dependency chain test
- Cycle detection passes with clear human-readable error
- Malformed manifest test: other bundles unaffected
- No loader.load or dlopen called before full graph resolution
  (verify with a test that checks call order)
- No .unwrap() in production code
- clippy passes with zero warnings
- All existing integration tests still pass

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Manifest file naming: bundle_name.manifest.toml or fixed name manifest.toml
- Recursive directory scanning or single-level only
- Bundle with no companion manifest file: skip silently, warn, or error
- Platform extension handling: .so / .dll / .dylib — all platforms or current only
- Symlinks: follow or skip
- When a bundle's runtime has no registered loader:
  fail entire runtime init, or skip that bundle and continue
- Two bundles provide same contract: first-registered wins, error, or configurable
- Does the graph module from Epic 4 need extending or replacing for multi-bundle

Do not write the plan until I have answered all questions.
```

---

## Epic 13 — Extension System

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 18 (Extension System)

---

PROJECT CONTEXT

The HostVTable already has get_extension(extension_id: u32) → *const ().
This function exists in the ABI but returns null for everything currently
because no extension registry has been implemented.

This epic implements the extension registry, the Extension trait,
and the trace extension as the reference implementation.

All six language generators (Rust, C++, C#, Python, Lua, js-quickjs/js-deno)
must be updated to emit extension query code in generated guest init()
when the bundle.toml optional list includes extensions.

Note: js-quickjs and js-deno are both full implementations.
js-quickjs: polyplug.getExtension(EXT_TRACE_ID) — lo/hi pair result.
js-deno: Deno.core.ops.op_get_extension(EXT_TRACE_ID) — BigInt result.

---

PRE-ANSWERED DECISIONS

Extension ID assignment:
  fnv1a_32("extension_name") — consistent with contract_id and bundle_id.
  The fnv1a_32 function already exists in crates/polyplug/src/abi/mod.rs.
  EXT_TRACE_ID: u32 = fnv1a_32("trace") = 0xC4EB9AEE
  This constant is generated by polyplugc and emitted into generated guest code
  for all six languages — never hand-written by plugin developers.

Versioning:
  No versioning in this epic. Extensions are unversioned.
  Vtable evolution is handled by adding new fields (not new extension IDs).
  The frozen ABI (ExtensionEntry = extension_id + vtable ptr) is unchanged.

Thread safety:
  Extension trait requires Send + Sync.
  Extension vtable function pointers may be called concurrently from any thread.
  TraceExtension callback must be: impl Fn(&str) + Send + Sync + 'static
  Extension registry reads use RwLock (consistent with registry pattern).

TraceExtension callback:
  Raw callback — zero new dependencies.
  TraceExtension::new(callback: impl Fn(&str) + Send + Sync + 'static)
  App developer wires it to tracing/log/eprintln as they prefer.
  No tracing crate dependency added to polyplug.

Extension query code emission:
  ONLY when the extension name is listed in the plugin's optional[] in bundle.toml.
  Empty optional list → no extension query code emitted.
  "trace" in optional list → emit getExtension(EXT_TRACE_ID) query in generated init().
  This matches the PRD pattern exactly.

Language generators to update:
  ALL SIX: Rust, C++, C#, Python, Lua, js-quickjs, js-deno.
  js-quickjs: polyplug.getExtension(EXT_TRACE_ID) returns {lo,hi}|null.
  js-deno:    Deno.core.ops.op_get_extension(EXT_TRACE_ID) returns BigInt|null.
  Full query code emitted for both JS variants — not just a constant.

Built-in extensions in scope:
  Trace only. async and sandbox are future (PRD §18).
  CounterExtension is TEST-ONLY — demonstrates custom extension pattern.

TraceVTable:
  #[repr(C)]
  pub struct TraceVTable {
      pub emit: unsafe extern "C" fn(msg: StringView),
  }
  StringView — consistent with all other ABI string passing.

Extension registry storage:
  RuntimeBuilder collects Vec<Box<dyn Extension>>.
  build() converts to HashMap<u32, Box<dyn Extension>> stored in Runtime.
  Ownership kept alive in Runtime for the process lifetime.

host_get_extension access pattern:
  GLOBAL_EXTENSION_MAP: OnceLock<HashMap<u32, *const ()>>
  Set during RuntimeBuilder::build() after all extensions are registered.
  host_get_extension reads from this global — matches GLOBAL_REGISTRY pattern.
  The raw *const () pointers are stable for the lifetime of the Runtime
  (Box<dyn Extension> in Runtime keeps vtable memory alive).

---

EPIC GOAL

1. Extension trait — crates/polyplug/src/extensions/mod.rs (new module):
   pub trait Extension: Send + Sync {
       fn extension_id(&self) -> u32;
       fn as_vtable_ptr(&self) -> *const ();
   }

2. Extension registry:
   - RuntimeBuilder::extension(impl Extension + 'static) registration method
   - RuntimeBuilder stores Vec<Box<dyn Extension>>
   - build() converts to HashMap<u32, Box<dyn Extension>> in Runtime
   - GLOBAL_EXTENSION_MAP: OnceLock<HashMap<u32, *const ()>> set during build()
   - host_get_extension(id: u32) → *const (): reads GLOBAL_EXTENSION_MAP
     returns stored ptr if found, null ptr if absent
   - RwLock for concurrent read safety on GLOBAL_EXTENSION_MAP reads
     (or OnceLock is sufficient if map is immutable after build — prefer OnceLock)

3. Trace extension — crates/polyplug/src/extensions/trace/mod.rs:
   pub const EXT_TRACE_ID: u32 = 0xC4EB9AEE;  // fnv1a_32("trace")
   #[repr(C)]
   pub struct TraceVTable {
       pub emit: unsafe extern "C" fn(msg: StringView),
   }
   pub struct TraceExtension {
       vtable: TraceVTable,
       // callback kept alive here
   }
   impl TraceExtension {
       pub fn new(callback: impl Fn(&str) + Send + Sync + 'static) -> Self
   }
   impl Extension for TraceExtension {
       fn extension_id(&self) -> u32 { EXT_TRACE_ID }
       fn as_vtable_ptr(&self) -> *const () { &self.vtable as *const _ as *const () }
   }

4. Custom extension pattern — test-only CounterExtension:
   CounterExtension with vtable: get_count() → u64, increment()
   Lives in integration test file, NOT in production code.
   Demonstrates that app developers can define arbitrary extensions.

5. Generator updates — six generators emit extension query code:
   Condition: bundle.toml [[plugin]] optional list includes "trace"
   Null check pattern per language (idiomatic):
     Rust:       let trace_vtable: Option<&TraceVTable> = ...
     C++:        const TraceVTable* trace = ...; if (trace != nullptr)
     C#:         IntPtr tracePtr = ...; if (tracePtr != IntPtr.Zero)
     Python:     trace_ptr = ...; if trace_ptr:
     Lua:        local trace = ...; if trace ~= nil then
     js-quickjs: const tracePtr = polyplug.getExtension(EXT_TRACE_ID); if (tracePtr)
     js-deno:    const tracePtr = Deno.core.ops.op_get_extension(EXT_TRACE_ID); if (tracePtr !== null)
   EXT_TRACE_ID constant emitted as generated code in all languages —
   never hand-written by plugin developers.
   polyplugc emits: Rust const, C++ constexpr, C# const, Python constant,
   Lua local, TypeScript const — all equal to 0xC4EB9AEE.

6. Integration tests — tests/integration_extension/mod.rs (new):
   a. Trace present: plugin emits messages via trace extension,
      host callback receives them in correct order — tested for all six languages
      (Rust, C++, C#, Python, Lua, js-quickjs — js-deno also if time permits)
   b. Absent trace: plugin declares optional = ["trace"] but host registers
      no TraceExtension → plugin loads and runs correctly, zero crash — all six
   c. Custom: CounterExtension — host registers it, plugin increments,
      host reads correct count
   d. Unregistered ID: host calls get_extension with unknown ID → null ptr,
      no panic, no crash
   e. TraceExtension + concurrent calls: spawn 4 threads all calling emit()
      simultaneously → no crash, no data race (ASAN + TSAN)

7. Criterion benchmark — crates/polyplug/benches/vtable_dispatch.rs:
   Add benchmark: absent extension null check overhead.
   Scenario A: plugin has trace in optional[], extension NOT registered.
     Generated null check runs, takes the absent branch.
   Scenario B: baseline, no optional[] at all.
   Delta must be statistically indistinguishable from noise.
   Document result in BENCHMARKS.md.

---

VERIFICATION CHECKLIST

- EXT_TRACE_ID = 0xC4EB9AEE (fnv1a_32("trace")) — verified in test
- Extension trait: Send + Sync — verified by compiler
- TraceExtension callback: Fn(&str) + Send + Sync + 'static — verified by compiler
- GLOBAL_EXTENSION_MAP set during build(), stable for Runtime lifetime
- host_get_extension returns null for unregistered ID — not panic, not crash
- Trace test passes for all six languages (Rust, C++, C#, Python, Lua, js-quickjs)
- Absent trace test passes for all six languages — no crash
- CounterExtension custom test passes
- Concurrent trace emit test passes under TSAN — no data race
- Criterion benchmark: absent extension overhead statistically zero
- Extension query code emitted ONLY when "trace" in optional[] — confirmed
  by generating bundle without optional and verifying no query code present
- js-quickjs: polyplug.getExtension(EXT_TRACE_ID) emitted in init.ts when optional
- js-deno: Deno.core.ops.op_get_extension(EXT_TRACE_ID) emitted in init.ts when optional
- All existing integration tests still pass
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```

---

## Epic 14 — Versioning and Compatibility

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 17 (Plugin Versioning and Compatibility)

---

PROJECT CONTEXT

At this point the full system works end to end with all five languages.
Previous epics assumed contract versions always match.
This epic enforces and tests every version mismatch scenario.

The manifest.toml already has:
- version field per bundle (set in Epic 7)
- function_count per contract (set in Epic 7)

Version negotiation happens at load time, after manifest reading,
before any loader.load() is called.

---

EPIC GOAL

1. Version struct and comparison:
   pub struct Version { pub major: u32, pub minor: u32 }
   fn is_compatible_with(&self, required: &Version) -> bool
   — compatible if self.major == required.major AND self.minor >= required.minor
   Parsing from "major.minor" string format, proper error on malformed

2. Contract version negotiation at load time:
   - Host side knows required version from api.toml schema
   - Plugin side provides version from manifest.toml
   - Strict: incompatible → PolyplugError::VersionMismatch { contract, required, found }
   - Relaxed: incompatible → emit warning, load anyway
   - Yolo: load unconditionally, no checks, no warnings

3. Function count validation:
   - manifest.toml function_count per contract vs count from api.toml
   - Strict: mismatch → PolyplugError::FunctionCountMismatch { contract, expected, found }
   - Relaxed: mismatch → warn, use minimum of the two counts
   - Yolo: ignore

4. Compatibility modes:
   pub enum Compatibility { Strict, Relaxed, Yolo }
   Global default set at runtime init via builder.
   Per-bundle override via LoadOptions.

5. LoadOptions:
   pub struct LoadOptions {
       pub compatibility: Compatibility,
       pub ignore_function_count_mismatch: bool,
   }

6. Warning mechanism for Relaxed mode (decided with me before planning):
   Candidates: trace extension callback, log crate, stderr, dedicated warning callback

7. PolyplugError new variants:
   VersionMismatch { contract: String, required: Version, found: Version }
   FunctionCountMismatch { contract: String, expected: u32, found: u32 }

8. Compatibility tests — one test function per scenario:
   v1.0 required, v1.0 provided → compatible all modes
   v1.0 required, v1.2 provided → compatible all modes (superset)
   v1.2 required, v1.0 provided → incompatible (too old)
     Strict: VersionMismatch error
     Relaxed: warning emitted, loads
     Yolo: loads silently
   v1.0 required, v2.0 provided → incompatible (major break)
     same three behaviors
   Function count mismatch:
     Strict: FunctionCountMismatch error
     Relaxed: warning, use minimum count
     Yolo: ignored
   Per-bundle LoadOptions overrides global Compatibility in all cases

---

VERIFICATION CHECKLIST

- All compatibility scenario tests pass
- VersionMismatch error message names contract, required version, found version
- FunctionCountMismatch error message names contract, expected, found
- Per-bundle override overrides global in every test scenario
- function_count field present in generated manifest.toml for all five languages
- Relaxed mode warnings surface to app developer via chosen mechanism
- No .unwrap() in production code
- clippy passes with zero warnings
- All existing integration tests still pass

---

PRE-ANSWERED DECISIONS

Version struct:
  Version is new in this epic — not previously defined in the IR or runtime.
  pub struct Version { pub major: u32, pub minor: u32 }
  major.minor only — no patch. Matches PRD §17 exactly.
  Parsing from "major.minor" string. Error on malformed: "1" or "1.2.3" both error.

Error variants:
  Separate variants — already specified in EPIC GOAL above:
    PolyplugError::VersionMismatch { contract: String, required: Version, found: Version }
    PolyplugError::FunctionCountMismatch { contract: String, expected: u32, found: u32 }

Warning mechanism for Relaxed mode:
  The trace extension callback is available after Epic 13.
  However, version warnings are a RUNTIME concern — not a plugin concern.
  Warning goes to a dedicated warning callback on RuntimeBuilder:
    builder.on_warning(impl Fn(&str) + Send + Sync + 'static)
  If no warning callback is registered: warnings go to eprintln! as fallback.
  This avoids a tracing/log dependency and matches the raw callback pattern
  established by TraceExtension.

Host-side api.toml version availability:
  Contract versions are defined in api.toml and parsed into the IR during
  polyplugc code generation. They are NOT available at runtime load time
  unless threaded through explicitly.
  Solution: polyplugc embeds the required contract versions into the generated
  host-side Rust code as constants, e.g.:
    pub const IMAGE_DECODE_REQUIRED_VERSION: Version = Version { major: 1, minor: 0 };
  These constants are read at load time by the runtime for version negotiation.
  The planner must confirm this approach is consistent with the existing
  generated host-side code structure from prior epics.

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Any versioning edge cases specific to your use cases
  (e.g. do you need per-bundle LoadOptions overrides in practice,
  or is the global Compatibility mode sufficient for now?)

Do not write the plan until I have answered remaining questions.
```

---

## Epic 15 — Complete polyplugc for All Five Languages

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 12 (Code Generation Pipeline)

---

PROJECT CONTEXT

At this point generators for all five languages exist from their respective epics.
Each was tested in isolation during its epic. This epic:

1. Audits and completes any generator gaps found across all five
2. Ensures all five are consistent in their output conventions
3. Adds incremental generation
4. Adds the polyplugc pack command
5. Runs the full 25 cross-language combination tests

This is the final codegen hardening epic before the showcase.

---

EPIC GOAL

1. Generator audit:
   For each of the six generators (Rust, C++, C#, Python, Lua, js-quickjs, js-deno):
   - Verify host-side output: types, callers
   - Verify guest SDK output: types, contracts
   - Verify guest bundle output: types, contracts, vtables, init, manifest.toml
   - Fix any gaps or inconsistencies found
   The planner must ask me what gaps were identified in previous epics.

2. Consistent output conventions across all generators:
   - All generated files have "THIS FILE IS AUTO-GENERATED BY polyplugc" header
   - manifest.toml always has: name, version, runtime, file, provides,
     requires, function_count, needs_reinit_on_dep_reload
   - Naming conventions consistent per language idioms
   The planner documents any convention decisions in the plan.

3. Incremental generation:
   - Hash the IR for each output file
   - Skip regenerating files whose IR hash is unchanged
   - Always regenerate manifest.toml regardless
   - Print output: "regenerated N files, skipped M unchanged"

4. polyplugc pack command:
   polyplugc pack --api api.toml --lang <lang> --out <dir>
   Produces properly structured package ready for distribution:
   - Rust:       crate directory (Cargo.toml, src/, ready for cargo publish)
   - C++:        header directory with single-include entry point
   - C#:         NuGet package directory structure
   - Python:     pip package directory (pyproject.toml, package dir)
   - Lua:        module directory (.lua files + C ext if needed)
   - js-quickjs: npm package directory + rolldown invocation → bundle.js
   - js-deno:    directory with index.ts + optional rolldown for npm deps

5. The cross-language combination tests:
   Matrix: {Rust, C++, C#, Python, Lua, js-quickjs}² = 36 combinations (6×6).
   js-deno tested separately — dedicated thread model needs separate scaffolding.
   For every pair (host_lang, guest_lang) in the 6×6 matrix:
   - Generate host callers for host_lang from test api.toml
   - Generate guest bundle for guest_lang from test bundle.toml
   - Build both (pre-built fixtures from tests/fixtures/ for non-Rust)
   - Rust host loads plugin: runtime.load_bundle(path)
   - Call at least two contract functions
   - Assert correct return values
   All 36 combinations must pass.

---

VERIFICATION CHECKLIST

- All 36 cross-language combination tests pass (js-quickjs as JS representative)
- js-deno separate combination tests pass (at least Rust↔js-deno pair)
- Generated code for all six generators compiles without warnings
- All generated files have auto-generated header comment
- manifest.toml always has all required fields for all six generators
- Incremental: schema change → regeneration; no change → skip
- polyplugc pack produces valid package structure for all six generators
- js-quickjs pack: rolldown invoked, bundle.js produced
- js-deno pack: index.ts shipped, rolldown optional
- No .unwrap() in polyplugc production code
- clippy passes with zero warnings
- cargo test --workspace passes

---

PRE-ANSWERED DECISIONS

Generator count:
  Six generators: Rust, C++, C#, Python, Lua, js-quickjs, js-deno.
  js-quickjs is full implementation. js-deno is full implementation.
  No stubs. Audit checks both for correctness.

Known gaps to audit (planner must verify each before writing plan):
  - All generators: confirm EXT_TRACE_ID constant emitted correctly after Epic 13
  - js-quickjs: confirm polyplug.getExtension emitted when optional includes "trace"
  - js-deno: confirm Deno.core.ops.op_get_extension emitted when optional includes "trace"
  - All generators: confirm manifest.toml has function_count field after Epic 14
  - All generators: confirm needs_reinit_on_dep_reload in manifest.toml
  - C#: confirm [SuppressGCTransition] on hot-path generated callers
  - Python: confirm ctypes function objects cached at module level (not per-call)
  - Lua: confirm ffi.metatype used, no lightuserdata
  - js-quickjs: confirm u64 lo/hi split in all generated types — no BigInt
  - js-deno: confirm BigInt used for u64 in all generated types — no lo/hi

Cross-language combination test organisation:
  One parameterized test file: tests/cross_language/mod.rs
  Matrix: {Rust, C++, C#, Python, Lua, js-quickjs} × {Rust, C++, C#, Python, Lua, js-quickjs}
  = 36 combinations. Each combination is one test function, named:
    test_host_<lang>_guest_<lang>()
  js-deno tested separately — not included in 36-combination matrix
  (dedicated thread model requires separate test scaffolding).
  Pre-built fixtures approach for non-Rust plugins:
    Non-Rust plugins are pre-built and committed to tests/fixtures/
    as compiled .so/.dll/bundle.js files.
    The test suite does NOT trigger non-Rust builds at test time.
    CI builds fixtures separately before running cargo test.
    A build script (tests/fixtures/build_all.sh) documents how to rebuild fixtures.
    js-quickjs fixtures: rolldown-bundled bundle.js files committed to fixtures/.

polyplugc pack command:
  In scope for this epic — final codegen hardening before showcase.
  Pack output structures per language:
    Rust:       crate directory (Cargo.toml, src/, ready for cargo publish)
    C++:        header directory with single-include entry point
    C#:         NuGet package directory structure
    Python:     pip package directory (pyproject.toml, package dir)
    Lua:        module directory (.lua files + C ext if needed)
    js-quickjs: npm package directory + rolldown invocation → bundle.js
    js-deno:    directory with index.ts + optional rolldown for npm deps

---

REMAINING QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Any generator output inconsistencies you noticed while running Epics 9-13
  not covered by the known gap list above
- Whether js-deno combination tests should be in a separate test file
  or integrated into the main cross-language matrix with special scaffolding

Do not write the plan until I have answered remaining questions.
```

---

## Epic 16 — Integration Showcase

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — all sections, this showcase exercises every feature

---

PROJECT CONTEXT

The full system is complete:
- polyplug runtime core (renamed from polyplug-runtime)
- All five language generators in polyplugc
- All three adapter crates: polyplug-dotnet, polyplug-python, polyplug-lua
- Full plugin discovery system
- Extension system with trace extension
- Version negotiation and compatibility modes
- All 25 cross-language combinations tested and passing

This epic builds the showcase. It lives at showcase/ in the project root.
It is not a toy — it is a real, working, complete application that any
developer could read to understand how to use polyplug.

---

THE SHOWCASE: multi-language data processing pipeline

Host runs: load input → decode → transform → encode → report

Four contracts:
  Decoder:     decode(Buffer) → DataRecord
  Transformer: transform(DataRecord) → DataRecord
  Encoder:     encode(DataRecord) → Buffer
  Reporter:    report(DataRecord) → StringView

Five plugins, one per language (plus one JS/TS):
  Rust:    Decoder    — parses CSV bytes into DataRecord
  C++:     Transformer — uppercases all string fields
  C#:      Encoder    — serializes DataRecord back to CSV bytes
  Python:  Reporter   — formats a human-readable summary string
  Lua:     Transformer — reverses all string fields (alternative transformer)
  TS/JS:   Validator  — validates DataRecord fields, returns AbiError on invalid input
           (runtime = "js-quickjs" — lightest footprint, pure-logic validation)

Host application (language chosen with me):
  - Initializes runtime with all adapters
  - Scans showcase/plugins/
  - Runs pipeline: decode → C++ transform → encode → report
  - Runs again: decode → Lua transform → encode → report
  - Prints results and all trace output
  - Handles all errors gracefully

---

EPIC GOAL

1. showcase/api.toml with four contracts and DataRecord type

2. Five plugin implementations, each in showcase/plugins/<name>/:
   bundle.toml + source + builds to .so/.dll + manifest.toml

3. Host application (language chosen with me):
   All features demonstrated (see feature list below)

4. Every polyplug feature demonstrated:
   - Cross-language calls: all five languages in one pipeline run
   - Plugin discovery: directory scanning only, no explicit paths
   - Cross-plugin communication: Encoder calls Decoder for round-trip validation
   - Trace extension: all five plugins emit trace messages, host prints them
   - Versioning: Python Reporter at contract v1.1, host requests v1.0 (compatible)
   - Compatibility Relaxed: Lua Transformer has one extra function, loads with warning
   - Error handling: Decoder returns error for malformed input, host continues

5. showcase/README.md:
   - How to build each plugin (per language)
   - How to run the host
   - Expected output (exact, copy-pasteable)
   - What each plugin does and what language it uses

6. Automated test:
   - cargo test includes a showcase test
   - Runs the full pipeline
   - Asserts correct final output
   - cargo test --workspace passes

---

VERIFICATION CHECKLIST

- Showcase builds for all five language plugins
- Host runs end to end, output matches README exactly
- Both transformer variants (C++ and Lua) produce correct output
- Trace output visible, includes messages from all five plugins
- Error scenario: malformed input handled gracefully, pipeline continues
- Versioning scenario: Python v1.1 loads when v1.0 requested
- Compatibility scenario: Lua extra function loads with Relaxed warning
- Cross-plugin: Encoder→Decoder round-trip works correctly
- Automated showcase test passes in cargo test --workspace
- README is complete and accurate with exact expected output
- No .unwrap() in any production code including showcase host application

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Which language for the host application
- DataRecord: fixed fields (name: StringView, value: StringView, count: u32)
  or dynamic key-value structure
- Showcase run mode: CLI with arguments, or hardcoded demo input
- Whether all five plugin builds are triggered by a single script/Makefile
  or documented as separate steps per language
- How the automated test handles building non-Rust plugins
  (pre-built fixtures in showcase/fixtures/? build script? CI only?)
- Any additional scenarios beyond the list above
- Whether showcase has its own README.md or content goes into root README.md

Do not write the plan until I have answered all questions.

---

PRE-ANSWERED DECISIONS

Host application language:
  Rust. The showcase host is a Rust binary in showcase/host/.
  Reasons: no additional runtime dependency, most complete API surface,
  demonstrates the primary use case cleanly, cargo run just works.

System context:
  The full system at showcase time includes polyplug-js (QuickJS) and
  polyplug-js-deno (V8). The showcase uses js-quickjs for the TS/JS Validator
  plugin — lightest footprint, sufficient for pure-logic validation.
  Six plugins total (Rust, C++, C#, Python, Lua, js-quickjs) — already
  reflected in THE SHOWCASE above.

Automated test / non-Rust plugin build:
  Pre-built fixtures: all non-Rust plugins are pre-built and committed to
  showcase/fixtures/ as compiled artifacts (.so/.dll/.node files).
  The cargo test showcase test loads from showcase/fixtures/.
  A showcase/build_plugins.sh script documents how to rebuild all plugins.
  CI runs build_plugins.sh before cargo test.
  Matches the fixtures approach established in Epic 15.

showcase/README.md:
  Separate showcase/README.md — not folded into root README.md.
  Root README.md links to it.

---

REMAINING QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- DataRecord: fixed fields (name: StringView, value: StringView, count: u32)
  or dynamic key-value structure?
  (Recommendation: fixed 3-field struct — simpler ABI, demonstrates all
  primitive and StringView types without requiring dynamic allocation)
- Showcase run mode: CLI with arguments selecting transformer, or two
  hardcoded sequential runs (C++ transform then Lua transform)?
- Any additional scenarios beyond those listed above

Do not write the plan until I have answered remaining questions.
```

---

## Epic 17 — Hot-Reload

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 7 (VTable System — arc-swap slots),
  section 14 (Cross-Plugin Communication),
  section 27 (Hot-Reload Architecture)

---

PROJECT CONTEXT

The arc-swap foundation was laid in Epic 9.7:
  - Every PluginSlot holds ArcSwap<VTableSlot>
  - resolve_plugin returns a PluginVTableGuard (arc-swap read guard)
  - Guards keep old vtables alive until dropped — automatic quiescence
  - manifest.toml [[dependency]] array gives the runtime the full dependency graph

This epic implements the writer path — the actual hot-reload mechanism:
  - Detecting when a new bundle version is available
  - Loading the new version
  - Atomically swapping the arc-swap slot
  - Waiting for quiescence (all in-flight calls complete)
  - Safely calling dlclose on the old bundle
  - Cascading reload for dependents that need re-initialization

The arc-swap reader path already costs ~2-3 cycles per call. This epic
adds ZERO overhead to the reader path. All cost is in the reloader.

---

PRE-ANSWERED DECISIONS

DETECTION MODES — two supported:
  1. inotify / FSEvents / ReadDirectoryChangesW (OS file watcher, per-platform)
     runtime.watch_plugin_dir("./plugins") — background thread watches for changes
  2. Explicit API — app calls runtime.reload_bundle(path) directly
     Useful for tests and for apps that manage file watching themselves.
  Both modes trigger the same internal reload path.
  File watcher is opt-in via Cargo feature: hot-reload = ["notify"]
  notify = "6" crate used for cross-platform file watching.

RELOAD PATH (atomic, no locks on reader path):
  Step 1: Load new bundle via correct loader (same as initial load).
          New bundle runs its own init() — registers its new vtable.
          New vtable ptr extracted from registrar before slot swap.
  Step 2: Atomically swap arc-swap slot:
          slot.vtable.store(Arc::new(VTableSlot(new_vtable_ptr)))
          Readers immediately see new vtable on next call.
  Step 3: Wait for quiescence in reloader thread ONLY:
          while Arc::strong_count(&old_arc) > 1 { std::hint::spin_loop(); }
          This spin only happens in the reloader thread. Callers never spin.
  Step 4: dlclose old bundle (now safe — no caller holds old vtable).
          drop(old_arc) triggers deallocation.
  Step 5: Check dependency graph for cascade.
          Any bundle that declared [[dependency]] bundle = "reloaded_bundle"
          and set needs_reinit_on_dep_reload = true is re-initialized.
          Cascade order follows topological sort from Epic 12 graph.

CASCADING RELOAD:
  Default: dependents automatically see new vtable via arc-swap — no action needed.
  Optional: if bundle needs_reinit_on_dep_reload = true (in bundle.toml):
    Runtime calls init() on the dependent bundle again after dep reload.
    Dependent bundle re-resolves its guards and re-registers its vtable.
    Cascade is topological — dependencies reload before dependents.
  needs_reinit_on_dep_reload = false is the default. Most bundles do not need it.

HOST DIRECT PATH — refresh after reload:
  Generated host callers go through resolve_plugin on every use — not cached raw ptr.
  App developers who cache raw pointers manually must call runtime.refresh_handle(handle)
  after a reload event to get a fresh guard.
  ReloadEvent is delivered via callback registered at runtime init.

RELOAD CALLBACK — app developer opt-in:
  runtime.on_reload(|event: ReloadEvent| {
      // event.bundle_name: &str
      // event.old_version: &str
      // event.new_version: &str
  });
  Callback fires AFTER swap, BEFORE dlclose.
  All new calls already use new vtable when callback fires.

RECURSIVE SELF-CALL LIMITATION:
  A bundle cannot call itself recursively during its own reload.
  This is UB and explicitly documented as unsupported in TRUST_MODEL.md.
  No runtime detection needed.

NEW CARGO FEATURE in polyplug crate:
  [features]
  hot-reload = ["dep:notify"]
  default = []
  notify = { version = "6", optional = true }
  All file-watcher code gated behind #[cfg(feature = "hot-reload")].
  The arc-swap slot is ALWAYS present (foundational, not gated).
  Only the file watcher and background thread are gated by the feature.

NEEDS_REINIT_ON_DEP_RELOAD in bundle.toml:
  [bundle]
  name = "my_bundle"
  needs_reinit_on_dep_reload = false   # default — omit if false

---

EPIC GOAL

1. Add notify dependency to polyplug crate (optional):
   notify = { version = "6", optional = true }
   [features] hot-reload = ["dep:notify"]

2. New module: crates/polyplug/src/reload/mod.rs
   pub struct ReloadEvent {
       pub bundle_name: String,
       pub old_version: String,
       pub new_version: String,
   }
   All reload logic lives here.

3. ReloadCallback registration in PluginRuntime builder:
   builder.on_reload(impl Fn(ReloadEvent) + Send + Sync + 'static)
   Stored as Option<Arc<dyn Fn(ReloadEvent) + Send + Sync>> in runtime.

4. Core reload function (always compiled, no feature gate):
   runtime.reload_bundle(path: &Path) → Result<(), PolyplugError>
   Implements the full 5-step reload path:
   a. Load new bundle via correct loader, capture new vtable ptr from registrar
   b. Swap arc-swap slot: slot.vtable.store(Arc::new(VTableSlot(new_ptr)))
   c. Hold old_arc. Spin until Arc::strong_count(&old_arc) == 1 (quiescence).
      Spin only in this function — callers never block.
   d. dlclose: drop(old_arc), then drop old library handle.
   e. Walk dependency graph. For each bundle with needs_reinit_on_dep_reload = true
      that declared a dependency on the reloaded bundle:
        call reload_bundle(dependent_path) recursively in topological order.
   f. Fire on_reload callback if registered.

5. File watcher (hot-reload feature only):
   runtime.watch_plugin_dir(dir: &Path) → Result<(), PolyplugError>
   Uses notify crate to watch directory recursively.
   On file-change event for a known bundle file (.so/.dll/.dylib):
     Debounce: 100ms — ignore events within 100ms of last event for same file
     Call reload_bundle(changed_path) in background thread.
   Watcher background thread stops when PluginRuntime drops (join on drop).

6. needs_reinit_on_dep_reload in bundle.toml parser:
   polyplugc parser: read optional needs_reinit_on_dep_reload = bool, default false.
   Add to ResolvedBundle IR.
   Add to manifest.toml generation: needs_reinit_on_dep_reload = bool field.
   ManifestData gains: needs_reinit_on_dep_reload: bool field.
   Epic 12 discovery reads this field. reload_bundle reads it from ManifestData.

7. TRUST_MODEL.md — add section: Hot-Reload Safety Guarantees:
   - arc-swap ensures readers never see a freed vtable
   - dlclose only after quiescence (strong_count == 1)
   - Recursive self-call during reload is UB — unsupported, not detected
   - Host cached raw vtable pointers must be refreshed via runtime.refresh_handle()
   - needs_reinit_on_dep_reload = false is safe for most bundles (arc-swap handles it)

8. Integration tests (tests/integration_reload/mod.rs — new file):
   a. Basic reload: load bundle V1, call it, replace file with V2, call reload_bundle,
      call again — verify V2 behavior observed.
   b. In-flight safety: spawn thread making calls into bundle in a tight loop,
      call reload_bundle from main thread concurrently,
      verify no crash, no use-after-free (ASAN), all calls return valid results.
   c. Quiescence: after reload_bundle returns, Arc::strong_count of old_arc == 1.
      Test by inspecting via test-only hook.
   d. dlclose timing: verify old library handle NOT closed while call in flight.
      Use ASAN or a test-only flag in VTableSlot drop impl.
   e. Cascade: bundle A (needs_reinit = true) depends on B.
      Reload B → A is automatically re-initialized.
      Verify A uses new B vtable after cascade.
   f. Callback: on_reload fires once per reload, correct bundle_name and versions.
      Callback fires after swap (new vtable visible) and before dlclose.
   g. File watcher (hot-reload feature): copy new .so into watched dir,
      wait 200ms, verify reload_bundle was triggered automatically.
   h. Multiple reloads: reload same bundle 50 times.
      No memory leak (check RSS or use valgrind/ASAN).
      No leaked library handles (check /proc/self/maps or equivalent).
   i. All languages: reload native, dotnet, python, lua, js-quickjs bundles —
      each passes basic reload test. js-deno reload: dedicated thread re-spawned,
      new JsRuntime initialized with new bundle.js.

9. Benchmarks (BENCHMARKS.md update):
   - Steady-state call overhead: hot-reload feature enabled vs disabled.
     Must be identical — arc-swap always present.
   - Reload latency: time from reload_bundle() call to completion.
     Include in BENCHMARKS.md.

---

VERIFICATION CHECKLIST

- reload_bundle(path) swaps vtable, waits for quiescence, dlcloses old — passes
- In-flight safety test passes under ASAN — no use-after-free
- Arc strong_count == 1 before dlclose — verified via test hook
- dlclose timing test passes — old handle not freed while call active
- Cascade reload test passes — dependent re-initialized after dep reload
- Callback fires after swap, before dlclose — timing verified in test
- File watcher test passes (hot-reload feature)
- Multiple reload test: no memory leak, no leaked handles
- All language bundles hot-reloadable (native, dotnet, python, lua, js-quickjs, js-deno)
- hot-reload Cargo feature gates file watcher only — arc-swap always compiled
- Steady-state benchmark: feature on/off = identical overhead
- needs_reinit_on_dep_reload in bundle.toml, manifest.toml, ManifestData
- TRUST_MODEL.md hot-reload section exists and is accurate
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
- cargo test --workspace --features hot-reload passes
```

---

## Epic 18 — Bundle-as-Folder Enforcement + Per-Platform Native Binary Paths

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 11 (bundle.toml schema), section 13 (Plugin Discovery)
- crates/polyplug/src/loader/manifest/mod.rs — current manifest parser
- crates/polyplug/src/loader/scanner/mod.rs — current discovery scanner

---

PROJECT CONTEXT

Currently manifest.toml has a single `file = "..."` field and bundles can be
either flat files or directories. This epic enforces:

1. Every bundle MUST be a directory. Flat file bundles are a hard error at discovery.
2. Every runtime MUST have an explicit `file` field (or `[bundle.file]` table for
   native). No defaults, no scanning, no inference. Missing file = hard error.
3. `file` values are RELATIVE paths within the bundle directory. Absolute paths
   and path traversal (../) are hard errors.
4. Native runtime uses a `[bundle.file]` TOML table keyed by "os.arch".
   All other runtimes use a flat `file = "..."` string.
5. polyplugc generate and polyplugc pack are updated to emit the new manifest format.
6. All existing test fixtures are updated to the new format.

---

PRE-ANSWERED DECISIONS

BUNDLE DIRECTORY ENFORCEMENT:
  Scanner (crates/polyplug/src/loader/scanner/mod.rs) must:
  - Only recognise entries that are DIRECTORIES as potential bundles.
  - Any non-directory entry in a scan path is silently skipped (not an error —
    scan paths may contain unrelated files).
  - A directory is a bundle candidate if and only if it contains manifest.toml
    at its root. No manifest.toml = silently skipped.
  - A directory with manifest.toml that fails to parse = hard error with path.

FILE FIELD RULES:
  All runtimes: `file` is REQUIRED. Missing = hard error at manifest parse time.
  Error message format:
    error: bundle "NAME" manifest.toml missing required `file` field.
    All runtimes require an explicit file path relative to the bundle directory.

  Relative path enforcement:
  - Value must not start with `/` — absolute path = hard error.
  - Value must not contain `..` anywhere — path traversal = hard error.
  - Value is joined to the bundle directory at load time:
      bundle_dir.join(file_value)
  - This produces the final path passed to the loader.
  Error message format:
    error: bundle "NAME" file path "VALUE" is invalid.
    File paths must be relative to the bundle directory and must not contain `..`.

NATIVE RUNTIME — [bundle.file] TABLE:
  Native runtime requires a TOML table, not a string:

    [bundle.file]
    linux.x86_64   = "libplugin.x86_64.so"
    linux.aarch64  = "libplugin.aarch64.so"
    windows.x86_64 = "plugin.x86_64.dll"
    windows.aarch64 = "plugin.aarch64.dll"
    macos.x86_64   = "libplugin.x86_64.dylib"
    macos.aarch64  = "libplugin.aarch64.dylib"

  If native runtime has flat `file = "..."` string: hard error.
  Error message:
    error: bundle "NAME" uses runtime "native" but has a flat `file` string.
    Native bundles require a [bundle.file] table with per-platform entries.
    Example: [bundle.file] / linux.x86_64 = "libplugin.x86_64.so"

  Platform key format: "os.arch" — both components required, dot-separated.
  Valid OS values: linux, windows, macos (matching std::env::consts::OS exactly).
  Valid arch values: x86_64, aarch64 (matching std::env::consts::ARCH exactly).
  Plugin developer declares ONLY the platforms they support.
  Any non-empty subset of platform keys is valid.
  An empty [bundle.file] table is a hard error:
    error: bundle "NAME" has an empty [bundle.file] table.
    At least one platform entry is required.
  At runtime: if current OS.ARCH key is absent from the table: hard error.
  Error message:
    error: bundle "NAME" does not support linux.aarch64.
    Supported platforms: linux.x86_64, windows.x86_64

  All other runtimes (dotnet, python, lua, js-quickjs, js-deno):
  If they have a [bundle.file] TABLE instead of flat string: hard error.
  Error message:
    error: bundle "NAME" uses runtime "RUNTIME" but has a [bundle.file] table.
    Only native runtime uses per-platform file tables.
    Use: file = "relative/path/to/plugin_file"

MANIFEST PARSING CHANGES:
  ManifestData struct gains:
    pub file: BundleFile   // replaces existing file: String

  pub enum BundleFile {
      Single(String),                          // all non-native runtimes
      PerPlatform(HashMap<String, String>),    // native runtime only
  }

  impl BundleFile {
      pub fn resolve(&self, runtime: &str) -> Result<&str, PolyplugError>
      // For Single: returns the path string directly.
      // For PerPlatform: looks up OS.ARCH key, errors if not found.
  }

  Validation at parse time (before any loading):
  - native + Single → hard error
  - non-native + PerPlatform → hard error
  - Any variant + absolute path → hard error
  - Any variant + path containing ".." → hard error

NEW ERROR VARIANTS (crates/polyplug/src/error/mod.rs):
  BundleNotADirectory { path: PathBuf }
    — a flat file was passed directly as a bundle path (explicit load, not scan)
  ManifestMissingFile { bundle: String }
    — manifest parsed but file field absent
  ManifestInvalidFilePath { bundle: String, path: String, reason: String }
    — absolute path, path traversal, or other invalid relative path
  ManifestWrongFileFormat { bundle: String, runtime: String }
    — native with flat string, or non-native with table
  PlatformNotSupported { bundle: String, platform: String, supported: Vec<String> }
    — current OS.ARCH not in [bundle.file] table (includes empty table case)

POLYPLUGC GENERATE CHANGES:
  All generators updated to emit new manifest.toml format.
  Native generator emits [bundle.file] table with a comment instructing the
  plugin developer to add entries for the platforms they support:
    # Add one entry per supported platform. You do not need all platforms.
    # Valid OS values: linux, windows, macos
    # Valid arch values: x86_64, aarch64
    [bundle.file]
    linux.x86_64 = "lib{bundle_name}.x86_64.so"
    # linux.aarch64  = "lib{bundle_name}.aarch64.so"
    # windows.x86_64 = "{bundle_name}.x86_64.dll"
    # macos.aarch64  = "lib{bundle_name}.aarch64.dylib"
  Uncommented entry is linux.x86_64 only as the most common starting point.
  Plugin developer uncomments and fills in the platforms they actually build for.
  All other generators emit: file = "plugin_file.ext" (placeholder, runtime-appropriate)
  polyplugc generate README.md updated with explicit instructions:
    "Edit the file field in manifest.toml to point to your compiled plugin file."

POLYPLUGC PACK CHANGES:
  polyplugc pack always produces a directory, never a flat file.
  If output directory already exists and contains a manifest.toml:
    hard error — do not overwrite existing bundle silently.
    Error: "output directory already contains a bundle. Delete it first."
  Pack validates that the manifest.toml it generates is valid under new rules
  before writing it to disk.

EXISTING TEST FIXTURES:
  All fixtures in tests/fixtures/ that are currently flat .so files must be
  moved into their own subdirectory with a manifest.toml.
  Specifically:
    tests/fixtures/libtest_plugin.so →
      tests/fixtures/test_plugin/libtest_plugin.so (or arch-suffixed name)
      tests/fixtures/test_plugin/manifest.toml (with [bundle.file] table)
    tests/fixtures/libtest_plugin_cpp.so →
      tests/fixtures/test_plugin_cpp/  (same pattern)
    tests/fixtures/liberror_plugin.so →
      tests/fixtures/error_plugin_bundle/  (same pattern)
    tests/fixtures/libmemory_plugin.so →
      tests/fixtures/memory_plugin_bundle/  (same pattern)
    tests/fixtures/test_plugin.lua →
      tests/fixtures/lua_plugin/test_plugin.lua
      tests/fixtures/lua_plugin/manifest.toml (file = "test_plugin.lua")
    tests/fixtures/test_plugin.py →
      tests/fixtures/python_plugin/test_plugin.py
      tests/fixtures/python_plugin/manifest.toml (file = "test_plugin.py")
    tests/fixtures/test_plugin_js/bundle.js →
      already a directory — add manifest.toml with file = "bundle.js" if missing
    tests/fixtures/csharp_plugin/ →
      already a directory — verify manifest.toml has correct file field
  NOTE: existing test_plugin/ and memory_plugin/ and error_plugin/ Cargo crates
  in fixtures/ are source trees, not bundle directories — they must NOT be
  confused with bundle directories. The Cargo crates build artifacts go into
  the bundle directories above.
  All test code that hardcodes fixture paths must be updated to new paths.

BUNDLE.TOML CHANGES:
  bundle.toml (plugin developer's source file) also updated:
  For native runtime:
    [bundle.file] table replaces file = "..."
  For all other runtimes:
    file = "relative/path" replaces file = "..." (same syntax, now enforced relative)
  polyplugc generate emits the new bundle.toml format as well.
  NOTE: bundle.toml is distinct from manifest.toml.
    bundle.toml = plugin developer's source (input to polyplugc)
    manifest.toml = generated output (shipped with bundle)
  Both are updated in this epic.

PRD UPDATE:
  Section 11 (bundle.toml schema): update runtime field and file field docs.
  Section 13 (Plugin Discovery): add bundle-as-directory enforcement rule.
  Planner should note PRD update as a task but executer writes code first,
  PRD update last.

---

EPIC GOAL

1. ManifestData: replace file: String with file: BundleFile enum.
   crates/polyplug/src/loader/manifest/mod.rs

2. Manifest parser: validate file field per rules above.
   Hard errors for all invalid combinations.
   Platform resolution in BundleFile::resolve().

3. Scanner: enforce bundle-as-directory.
   Non-directory entries silently skipped.
   Directory without manifest.toml silently skipped.
   Directory with unparseable manifest.toml = hard error.

4. All loaders: use BundleFile::resolve() to get final path.
   Join resolved path to bundle_dir.
   NativeBundleLoader, DotnetLoader, PythonLoader, LuaLoader,
   JsLoader, JsDenoLoader — all updated.

5. New error variants: five new variants listed above.
   crates/polyplug/src/error/mod.rs

6. polyplugc generate: all seven generators emit new manifest.toml format.
   Native: [bundle.file] table with six platform placeholders.
   Others: file = "placeholder" string.
   Also update generated bundle.toml for native to use [bundle.file].

7. polyplugc pack: always produces directory.
   Hard error if output already contains bundle.
   Validates manifest before writing.

8. Test fixtures: restructure all flat-file fixtures into bundle directories.
   Update all test code that references old fixture paths.

9. Integration tests — tests/integration_discovery/mod.rs (add cases):
   a. Flat file in scan path → silently skipped (not an error)
   b. Directory without manifest.toml → silently skipped
   c. Bundle with missing file field → ManifestMissingFile error
   d. Bundle with absolute path in file field → ManifestInvalidFilePath error
   e. Bundle with ../ in file field → ManifestInvalidFilePath error
   f. Native bundle with flat file string → ManifestWrongFileFormat error
   g. Non-native bundle with [bundle.file] table → ManifestWrongFileFormat error
   h. Native bundle on current platform — correct .so/.dll/.dylib loaded
   i. Native bundle missing current platform → PlatformNotSupported error
      with supported platforms listed (not current platform)
   i2. Native bundle with empty [bundle.file] table → PlatformNotSupported
       with empty supported list, clear message about empty table
   j. Existing full integration tests still pass with restructured fixtures

10. PRD update: sections 11 and 13.

---

VERIFICATION CHECKLIST

- Flat file bundle → hard error at explicit load, silently skipped in scan — verified
- Directory without manifest.toml → silently skipped — verified
- Missing file field → ManifestMissingFile — verified
- Absolute path → ManifestInvalidFilePath — verified
- Path traversal (..) → ManifestInvalidFilePath — verified
- Native + flat string → ManifestWrongFileFormat — verified
- Non-native + table → ManifestWrongFileFormat — verified
- Native + no current platform entry → PlatformNotSupported with available list — verified
- Native + current platform entry → correct file loaded — verified
- All existing integration tests pass with restructured fixtures — verified
- polyplugc generate emits [bundle.file] table for native — verified
- polyplugc generate emits file = "..." for all other runtimes — verified
- polyplugc pack always produces directory — verified
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```


---

## Epic 19 — Enum Types in api.toml Schema

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 11 (api.toml schema, type system)
- crates/polyplugc/src/ir/mod.rs — IR types
- crates/polyplugc/src/parser/mod.rs — api.toml parser
- crates/polyplugc/src/generators/ — all six generators

---

PROJECT CONTEXT

api.toml currently supports only [[type]] (flat C structs with primitive fields)
and [[contract]] (functions). This epic adds [[enum]] — C-style enums with
explicit discriminant values and an optional bitflag annotation.

Enums are ABI types — they appear in contract function signatures exactly like
[[type]] structs. At the ABI level an enum is its repr type (u8/u16/u32/u64).
polyplugc generates idiomatic enum types per language. The target language
compiler validates the discriminant value expressions — polyplugc does NOT
evaluate or range-check them beyond a syntax check.

---

PRE-ANSWERED DECISIONS

API.TOML SYNTAX:

  [[enum]]
  name    = "ColorSpace"        # required, PascalCase, unique across all types+enums
  repr    = "u32"               # required: u8 | u16 | u32 | u64
  bitflag = true                # optional, default false

  [[enum.variants]]
  name  = "None"                # required, PascalCase
  value = "0"                   # required, expression string — see below

  [[enum.variants]]
  name  = "Srgb"
  value = "1"

  [[enum.variants]]
  name  = "Linear"
  value = "1 << 1"

  [[enum.variants]]
  name  = "SrgbLinear"
  value = "Srgb | Linear"       # reference to previously-declared variant by name

  [[enum.variants]]
  name  = "All"
  value = "0xFF"

VALUE EXPRESSION RULES:
  Allowed tokens:
    - Integer literals: decimal (0, 1, 255), hex (0xFF, 0xDEAD), binary (0b0101)
    - Bit shift: 1 << N  (N must be a decimal integer literal, 0-63)
    - Bitwise OR: A | B
    - Bitwise NOT/complement: ~A  (generates bitwise complement in all languages)
    - Variant name references: name of a previously declared variant in the
      SAME enum (forward references are NOT allowed)
    - Parentheses for grouping: (A | B)

  NOT allowed: arithmetic (+, -, *, /), comparison, string literals, function calls,
  cross-enum references.

  polyplugc validation: tokenize the value string and verify only allowed tokens
  appear. If invalid token found: hard error at parse time.
  Error message:
    error: enum "ColorSpace" variant "Bad" has invalid value expression "1 + 2".
    Allowed operators: | << ~
    Allowed operands: integer literals, previously declared variant names.

  polyplugc does NOT evaluate the expression numerically.
  The expression is emitted verbatim (with variant name substitution) into
  each target language. Target language compiler validates range and type.

  Variant name substitution in output:
    In the value expression, variant name references are substituted with the
    language-appropriate constant reference per generator.
    Example: value = "Srgb | Linear"
      Rust:   SrgbLinear = Srgb | Linear   (as u32 consts)
      C++:    SrgbLinear = Srgb | Linear   (as constexpr)
      C#:     SrgbLinear = Srgb | Linear   (as const uint)
      Python: SRGB_LINEAR = SRGB | LINEAR  (SCREAMING_SNAKE_CASE)
      Lua:    SrgbLinear = ColorSpace.Srgb | ColorSpace.Linear
      JS/TS:  SrgbLinear = ColorSpace.Srgb | ColorSpace.Linear

ENUM AS ABI TYPE:
  Enums are valid field types in [[type]] structs:
    [[type]]
    name = "Image"
    fields = [
        { name = "color_space", type = "ColorSpace" },
        { name = "width",       type = "u32" },
    ]
  Enums are valid function parameter and return types in [[contract]]:
    [[contract.functions]]
    name    = "decode"
    params  = [{ name = "color_space", type = "ColorSpace" }]
    returns = "ColorSpace"
  At ABI level: enum is its repr type. Generated function signatures use
  the repr type in the C ABI, with a cast to/from the enum type in generated code.

NAME UNIQUENESS:
  Enum names share the same namespace as [[type]] names.
  Hard error if an enum name collides with a type name or another enum name.
  Error:
    error: "ColorSpace" is defined as both a [[type]] and an [[enum]] in api.toml.
    Type and enum names must be unique.

IR CHANGES (crates/polyplugc/src/ir/mod.rs):
  Add to IR:
    pub struct EnumDef {
        pub name: String,
        pub repr: ReprType,
        pub bitflag: bool,
        pub variants: Vec<EnumVariant>,
    }

    pub struct EnumVariant {
        pub name: String,
        pub value: String,   // validated expression string, stored verbatim
    }

    pub enum ReprType {
        U8, U16, U32, U64,
    }

  ApiIr gains:
    pub enums: Vec<EnumDef>

  Type resolution: when resolving a field/param/return type name,
  check enums vec in addition to types vec.

CODE GENERATION — PER LANGUAGE:

  RUST (crates/polyplugc/src/generators/rust/mod.rs):
    Non-bitflag:
      #[repr(u32)]  // or u8/u16/u64 per repr
      #[derive(Debug, Clone, Copy, PartialEq, Eq)]
      pub enum ColorSpace {
          None    = 0,
          Srgb    = 1,
          Linear  = 1 << 1,
          SrgbLinear = Self::Srgb as u32 | Self::Linear as u32,
          All     = 0xFF,
      }
    Bitflag (bitflag = true):
      Use raw const pattern (do NOT add bitflags crate dependency):
        pub mod color_space {
            pub type ColorSpace = u32;
            pub const NONE:        ColorSpace = 0;
            pub const SRGB:        ColorSpace = 1;
            pub const LINEAR:      ColorSpace = 1 << 1;
            pub const SRGB_LINEAR: ColorSpace = SRGB | LINEAR;
            pub const ALL:         ColorSpace = 0xFF;
        }
        pub use color_space::ColorSpace;
      Reason: bitflags crate is not a polyplug dependency. Raw consts are
      ABI-equivalent and work without any added dep.

  C++ (crates/polyplugc/src/generators/cpp/mod.rs):
    Non-bitflag:
      enum class ColorSpace : uint32_t {
          None       = 0,
          Srgb       = 1,
          Linear     = 1 << 1,
          SrgbLinear = static_cast<uint32_t>(ColorSpace::Srgb) |
                       static_cast<uint32_t>(ColorSpace::Linear),
          All        = 0xFF,
      };
    Bitflag (bitflag = true):
      Same enum class but add bitwise operator overloads:
        inline ColorSpace operator|(ColorSpace a, ColorSpace b) {
            return static_cast<ColorSpace>(
                static_cast<uint32_t>(a) | static_cast<uint32_t>(b));
        }
        inline ColorSpace operator&(ColorSpace a, ColorSpace b) { ... }
        inline ColorSpace operator~(ColorSpace a) { ... }

  C# (crates/polyplugc/src/generators/csharp/mod.rs):
    Non-bitflag:
      public enum ColorSpace : uint
      {
          None       = 0,
          Srgb       = 1,
          Linear     = 1 << 1,
          SrgbLinear = Srgb | Linear,
          All        = 0xFF,
      }
    Bitflag (bitflag = true):
      [Flags]
      public enum ColorSpace : uint
      { ... same variants ... }

  Python (crates/polyplugc/src/generators/python/mod.rs):
    Non-bitflag:
      class ColorSpace(enum.IntEnum):
          NONE        = 0
          SRGB        = 1
          LINEAR      = 1 << 1
          SRGB_LINEAR = 1 | (1 << 1)   # variant refs substituted with literal exprs
          ALL         = 0xFF            # Python IntEnum does not allow forward refs
                                        # so variant refs are substituted at codegen time
                                        # by re-emitting the expression with names replaced
                                        # by their own value expressions recursively
                                        # (one level only — no chained refs)
    Bitflag (bitflag = true):
      class ColorSpace(enum.IntFlag):
          { same variants }
    NOTE for Python: variant name references in value expressions must be
    substituted at codegen time (not emitted as-is) because Python class body
    evaluation is sequential and IntEnum does not support forward-refs or
    self-referential expressions after the class is defined.
    One-level substitution is sufficient — enforce in validation that variant
    references only refer to previously-declared variants (already required).

  Lua (crates/polyplugc/src/generators/lua/mod.rs):
    local ColorSpace = {
        None       = 0,
        Srgb       = 1,
        Linear     = 1 << 1,
        SrgbLinear = 1 | (1 << 1),   -- variant refs substituted (same as Python)
        All        = 0xFF,
    }
    NOTE: Lua table literal body does not support forward-self-refs.
    Same one-level substitution as Python.
    bitflag = true: no change in Lua — bitwise ops already work on integers.
    Add comment: -- bitflag enum

  js-quickjs (crates/polyplugc/src/generators/js_quickjs/mod.rs):
    Non-bitflag:
      const ColorSpace = Object.freeze({
          None:       0,
          Srgb:       1,
          Linear:     1 << 1,
          SrgbLinear: 1 | (1 << 1),   // variant refs substituted
          All:        0xFF,
      });
    Bitflag (bitflag = true):
      Same Object.freeze — add JSDoc comment: /** @bitflag */
    NOTE: JS object literal also requires substitution for variant refs.

  js-deno (crates/polyplugc/src/generators/js_deno/mod.rs):
    Identical output to js-quickjs for enum generation.
    Same substitution rules apply.

SUBSTITUTION ALGORITHM (used by Python, Lua, js-quickjs, js-deno):
  For each variant, scan its value expression for variant name tokens.
  Replace each variant name token with the value expression of the referenced
  variant, wrapped in parentheses: (original_expression).
  One level only — the referenced variant's value must itself be
  a pure literal expression (no further variant refs after substitution).
  If a referenced variant itself contains a variant ref: hard error at codegen.
  Error:
    error: enum "ColorSpace" variant "SrgbLinear" references "Srgb" which
    itself references another variant. Only one level of variant reference
    is supported.
  This rule is enforced at IR validation time (before codegen), not per-language.

REPR TYPE → LANGUAGE MAPPING:
  u8:  Rust u8,  C++ uint8_t,  C# byte,   Python no change, Lua integer, JS number
  u16: Rust u16, C++ uint16_t, C# ushort, Python no change, Lua integer, JS number
  u32: Rust u32, C++ uint32_t, C# uint,   Python no change, Lua integer, JS number
  u64: Rust u64, C++ uint64_t, C# ulong,  Python no change, Lua integer, JS BigInt
       NOTE: u64 enums in js-quickjs use lo/hi split (consistent with other u64 handling).
       u64 enums in js-deno use BigInt.
       u64 enums in Python: use int (Python int is arbitrary precision).

NAMING CONVENTIONS:
  Rust:   PascalCase enum name, PascalCase variant names (Self::Variant)
  C++:    PascalCase enum class name, PascalCase variant names
  C#:     PascalCase enum name, PascalCase variant names
  Python: PascalCase class name, SCREAMING_SNAKE_CASE variant names
  Lua:    PascalCase table name, PascalCase keys
  JS/TS:  PascalCase const name, PascalCase keys

---

EPIC GOAL

1. api.toml parser (crates/polyplugc/src/parser/mod.rs):
   Parse [[enum]] sections into EnumDef IR nodes.
   Validate value expressions — tokenize, check allowed operators.
   Validate name uniqueness across types and enums.
   Validate variant name references are backward-only (no forward refs).
   Hard errors for all violations.

2. IR (crates/polyplugc/src/ir/mod.rs):
   Add EnumDef, EnumVariant, ReprType.
   Add enums: Vec<EnumDef> to ApiIr.
   Type resolution updated to include enum names.
   IR validation: one-level variant ref check (no chained refs).

3. All six generators updated:
   Emit enum type definitions per language spec above.
   Emit enums in the types output file (same file as [[type]] structs).
   Substitution algorithm implemented for Python, Lua, js-quickjs, js-deno.
   bitflag = true handled per language spec above.

4. Codegen tests (tests/integration_codegen_rust/mod.rs etc.):
   Add enum codegen test for each generator:
   a. Non-bitflag enum with literals, shift, OR, variant ref
   b. Bitflag enum — verify [Flags] / operator overloads / IntFlag emitted
   c. Enum used as function param type — correct ABI repr in generated code
   d. Enum used as struct field type — correct field type in generated code
   e. Invalid value expression (+ operator) — hard error at parse
   f. Forward variant reference — hard error at parse
   g. Chained variant reference — hard error at IR validation
   h. Enum name collision with type name — hard error at parse
   i. Missing repr field — hard error at parse
   j. Invalid repr value (e.g. repr = "f32") — hard error at parse

5. Test fixture api.toml (tests/fixtures/test_api.toml):
   Add a sample enum (non-bitflag) and a sample bitflag enum.
   Use them in at least one contract function parameter and one struct field.
   This exercises enum codegen in all integration tests automatically.

6. PRD update: section 11 (api.toml schema) — document [[enum]] syntax,
   value expression rules, bitflag annotation, repr types.

---

VERIFICATION CHECKLIST

- [[enum]] parsed correctly from api.toml — verified
- repr required, must be u8/u16/u32/u64 — hard error otherwise — verified
- value expression tokenized, invalid operators → hard error — verified
- forward variant reference → hard error — verified
- chained variant reference → hard error at IR validation — verified
- enum name collision with type name → hard error — verified
- Rust: non-bitflag → #[repr(uN)] enum, bitflag → raw consts module — verified
- C++: non-bitflag → enum class, bitflag → enum class + operator overloads — verified
- C#: non-bitflag → enum : uN, bitflag → [Flags] enum : uN — verified
- Python: non-bitflag → IntEnum, bitflag → IntFlag, variant refs substituted — verified
- Lua: table literal, variant refs substituted — verified
- js-quickjs: Object.freeze, variant refs substituted — verified
- js-deno: identical to js-quickjs — verified
- u64 enum: lo/hi in js-quickjs, BigInt in js-deno — verified
- Enum as struct field type — compiles in all languages — verified
- Enum as function param/return type — compiles in all languages — verified
- test_api.toml updated with enum examples — verified
- All existing integration tests pass — verified
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```


---

## Epic 20 — PluginContext, ABI Type Helpers, and Runtime Dependency Path Setup

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 6 (ABI Layer), section 7 (VTable System),
  section 10 (all adapter subsections), section 15 (String Model)
- crates/polyplug/src/abi/mod.rs — current ABI types
- guest-libs/ — all six guest libs
- crates/polyplugc/src/generators/ — all six generators

---

PROJECT CONTEXT

This epic adds three related things:

1. PluginContext — a new ABI struct passed to init() alongside the registrar.
   Gives plugins access to their bundle directory path at init time.
   Plugin developers use this to construct absolute paths to dependencies
   or data files inside their bundle directory.

2. Helper methods and operators on StringView and Buffer in all six guest libs.
   Makes ABI types feel native to each language rather than bare C structs.

3. Automatic dependency path setup per non-native runtime.
   polyplug transparently prepends bundle directory (and sub-paths) to each
   runtime's module search path before loading the plugin.
   Native plugins use PluginContext.bundle_path for this themselves.

---

PRE-ANSWERED DECISIONS

─────────────────────────────────────────────────────────────
PART 1 — PluginContext
─────────────────────────────────────────────────────────────

C ABI STRUCT (crates/polyplug/src/abi/mod.rs):
  #[repr(C)]
  pub struct PluginContext {
      pub bundle_path: StringView,  // absolute path to bundle directory, UTF-8
  }
  // Future fields appended here — ABI-stable by addition only.
  // Plugins must not assume sizeof(PluginContext) — always accessed via pointer.

INIT SIGNATURE CHANGE:
  OLD: void init(PluginRegistrar* registrar)
  NEW: void init(PluginRegistrar* registrar, const PluginContext* ctx)

  This is a BREAKING ABI CHANGE. All existing plugins must be recompiled.
  Acceptable pre-v1. After v1 this signature is frozen forever.

  The context pointer is valid for the entire duration of init() only.
  Plugin must not store the pointer — only copy values (e.g. copy bundle_path
  string into owned storage if needed after init returns).
  bundle_path StringView: ptr points into runtime-owned memory, valid for
  the lifetime of the PluginRuntime.

RUNTIME CONSTRUCTION:
  NativeBundleLoader constructs PluginContext { bundle_path } from the
  bundle directory path resolved at load time (Epic 18 guarantees this is
  always an absolute directory path).
  All other loaders (dotnet, python, lua, js-quickjs, js-deno) do the same.
  PluginContext is stack-allocated in the loader, passed by pointer to init().

POLYPLUGC GENERATOR CHANGES — init() signature per language:

  Rust (guest-libs/rust/):
    #[no_mangle]
    pub unsafe extern "C" fn init(
        registrar: *mut PluginRegistrar,
        ctx: *const PluginContext,
    ) {
        let ctx = unsafe { &*ctx };
        // bundle_path: ctx.bundle_path.as_str()
        plugin_init(registrar, ctx);
    }
    Generated plugin_init signature:
      fn plugin_init(registrar: &mut PluginRegistrar, ctx: &PluginContext)

  C++:
    extern "C" void init(PluginRegistrar* registrar, const PluginContext* ctx) {
        // ctx->bundle_path is a StringView with helpers
    }

  C#:
    [UnmanagedCallersOnly(EntryPoint = "init")]
    public static void Init(IntPtr registrarPtr, IntPtr ctxPtr) {
        var ctx = Marshal.PtrToStructure<PluginContext>(ctxPtr);
        // ctx.BundlePath is a StringView with helpers
    }

  Python:
    def init(registrar_ptr: int, ctx_ptr: int) -> None:
        ctx = PluginContext.from_address(ctx_ptr)
        # ctx.bundle_path is a StringView with helpers

  Lua:
    local function init(registrar_ptr, ctx_ptr)
        local ctx = ffi.cast("PluginContext*", ctx_ptr)
        -- ctx.bundle_path is a StringView with metamethods
    end

  js-quickjs:
    // polyplug.init() JS wrapper receives bundle_path as string directly
    // (QuickJS runtime converts StringView to JS string before calling JS init)
    // JS init signature: function init(bundlePath: string): void

  js-deno:
    // Deno op passes bundle_path as string directly to JS init
    // JS init signature: function init(bundlePath: string): void

  NOTE for JS variants: since JS cannot hold raw pointers safely, the loader
  extracts bundle_path from PluginContext and passes it as a plain JS string
  to the plugin's init function. The PluginContext struct is never exposed to JS.

─────────────────────────────────────────────────────────────
PART 2 — ABI Type Helpers
─────────────────────────────────────────────────────────────

RUST (guest-libs/rust/src/lib/mod.rs):
  impl StringView {
      pub fn as_str(&self) -> &str { /* from_utf8_unchecked SAFETY: ABI guarantees UTF-8 */ }
      pub fn as_bytes(&self) -> &[u8] { ... }
      pub fn is_empty(&self) -> bool { self.len == 0 }
      pub fn from_static(s: &'static str) -> Self { ... }
      // From trait impls:
  }
  impl<'a> From<&'a str> for StringView { ... }      // zero-copy borrow
  impl From<StringView> for String { ... }            // owned copy
  impl fmt::Display for StringView { ... }
  impl fmt::Debug for StringView { ... }
  impl PartialEq<str> for StringView { ... }
  impl PartialEq for StringView { ... }

  impl Buffer {
      pub fn as_slice(&self) -> &[u8] { ... }
      pub fn as_slice_mut(&mut self) -> &mut [u8] { ... }
      pub fn is_empty(&self) -> bool { self.len == 0 }
  }

  impl PluginContext {
      pub fn bundle_path(&self) -> &str { self.bundle_path.as_str() }
  }

C++ (guest-libs/cpp/polyplug/abi.hpp):
  struct StringView {
      const uint8_t* ptr;
      size_t len;

      // implicit zero-copy conversion to std::string_view
      operator std::string_view() const {
          return {reinterpret_cast<const char*>(ptr), len};
      }
      // explicit allocating conversion to std::string
      explicit operator std::string() const {
          return {reinterpret_cast<const char*>(ptr), len};
      }
      // construct from std::string_view (zero-copy, caller keeps alive)
      explicit StringView(std::string_view sv)
          : ptr{reinterpret_cast<const uint8_t*>(sv.data())}, len{sv.size()} {}
      // construct from string literal (zero-copy)
      template<size_t N>
      explicit StringView(const char (&lit)[N])
          : ptr{reinterpret_cast<const uint8_t*>(lit)}, len{N - 1} {}

      bool empty() const { return len == 0; }
  };

  struct Buffer {
      void* ptr;
      size_t len;
      size_t cap;

      template<typename T = uint8_t>
      T* data() const { return static_cast<T*>(ptr); }
      size_t size() const { return len; }
      bool empty() const { return len == 0; }
  };

  // host-libs/cpp/polyplug/abi.hpp gets same helpers (host side uses same types)

C# (guest-libs/csharp/src/Abi.cs):
  NO unsafe keyword anywhere. No project option changes required.

  [StructLayout(LayoutKind.Sequential)]
  public readonly struct StringView {
      public readonly IntPtr Ptr;
      public readonly ulong Len;   // ulong matches size_t on 64-bit — polyplug is 64-bit only

      // explicit cast TO string (allocates — developer is aware via explicit keyword)
      public static explicit operator string(StringView sv) =>
          Marshal.PtrToStringUTF8(sv.Ptr, (int)sv.Len) ?? string.Empty;

      // ToString() for convenience and debugger display
      public override string ToString() =>
          Marshal.PtrToStringUTF8(Ptr, (int)Len) ?? string.Empty;

      public bool IsEmpty => Len == 0;

      // No FROM string operator — lifetime is ambiguous, document use of
      // StringViewHelper.Pin(string) for the pinned case (see below)
  }

  // Helper for pinning managed strings into StringView lifetime
  // IDisposable RAII — pin released on Dispose
  public sealed class PinnedStringView : IDisposable {
      public StringView View { get; }
      private GCHandle _handle;
      public static PinnedStringView Pin(string s) { ... }
      public void Dispose() { _handle.Free(); }
  }

  [StructLayout(LayoutKind.Sequential)]
  public readonly struct Buffer {
      public readonly IntPtr Ptr;
      public readonly ulong Len;
      public readonly ulong Cap;

      public bool IsEmpty => Len == 0;
      // No AsSpan — requires unsafe. Access via Ptr + Len with Marshal if needed.
  }

  [StructLayout(LayoutKind.Sequential)]
  public readonly struct PluginContext {
      public readonly StringView BundlePath;
      public string BundlePathString => (string)BundlePath;
  }

  // Also update host-libs/csharp/src/Abi.cs with same helpers

PYTHON (guest-libs/python/polyplug_guest/abi.py):
  class StringView(ctypes.Structure):
      _fields_ = [("ptr", ctypes.c_void_p), ("len", ctypes.c_size_t)]

      def __str__(self) -> str:
          return ctypes.string_at(self.ptr, self.len).decode("utf-8")
      def __bytes__(self) -> bytes:
          return ctypes.string_at(self.ptr, self.len)
      def __bool__(self) -> bool:
          return self.len > 0
      def __eq__(self, other) -> bool:
          if isinstance(other, str): return str(self) == other
          if isinstance(other, StringView):
              return self.len == other.len and bytes(self) == bytes(other)
          return NotImplemented
      def __repr__(self) -> str:
          return f"StringView({str(self)!r})"

      @classmethod
      def from_str(cls, s: str) -> tuple["StringView", bytes]:
          # returns (view, backing_bytes) — caller MUST keep bytes alive
          b = s.encode("utf-8")
          return cls(ctypes.cast(b, ctypes.c_void_p), len(b)), b

  class Buffer(ctypes.Structure):
      _fields_ = [
          ("ptr", ctypes.c_void_p),
          ("len", ctypes.c_size_t),
          ("cap", ctypes.c_size_t),
      ]
      def __bytes__(self) -> bytes:
          return ctypes.string_at(self.ptr, self.len)
      def __bool__(self) -> bool:
          return self.len > 0
      def __len__(self) -> int:
          return self.len

  class PluginContext(ctypes.Structure):
      _fields_ = [("bundle_path", StringView)]

      def bundle_path_str(self) -> str:
          return str(self.bundle_path)

  # Update abi.pyi stub with all new methods and types

LUA (guest-libs/lua/polyplug_guest.lua):
  -- StringView metamethods
  StringView.__index    = StringView
  StringView.__tostring = function(self) return ffi.string(self.ptr, self.len) end
  StringView.__eq       = function(a, b)
      if type(b) == "string" then
          return ffi.string(a.ptr, a.len) == b
      end
      return a.len == b.len and
             ffi.C.memcmp(a.ptr, b.ptr, a.len) == 0
  end
  StringView.__len = function(self) return tonumber(self.len) end
  function StringView.from_string(s)
      -- s must stay alive while view is used
      return ffi.new("StringView",
          ffi.cast("const uint8_t*", s), #s)
  end

  -- Buffer metamethods
  Buffer.__index  = Buffer
  Buffer.__tostring = function(self)
      return ffi.string(self.ptr, self.len)
  end
  Buffer.__len = function(self) return tonumber(self.len) end
  Buffer.__bool = function(self) return self.len > 0 end  -- LuaJIT extension

  -- PluginContext
  PluginContext.__index = PluginContext
  function PluginContext:bundle_path_str()
      return tostring(self.bundle_path)
  end

JS GUEST LIB (guest-libs/js/polyplug-guest.ts):
  // No operator overloading in JS/TS — static helper class is the idiom.
  // Both js-quickjs and js-deno variants receive bundle_path as a plain string
  // from the loader — no StringView manipulation needed in JS.
  // StringViewHelper kept for manual FFI cases only.

  export class StringViewHelper {
      static decode(sv: StringView): string { /* TextDecoder on memory view */ }
      static encode(s: string): { view: StringView; bytes: Uint8Array } {
          // TextEncoder → Uint8Array, caller keeps bytes alive
      }
      static isEmpty(sv: StringView): boolean { return sv.len === 0; }
  }

  export class BufferHelper {
      static toBytes(buf: Buffer): Uint8Array { /* view into memory */ }
      static isEmpty(buf: Buffer): boolean { return buf.len === 0; }
  }

─────────────────────────────────────────────────────────────
PART 3 — Automatic Dependency Path Setup Per Runtime
─────────────────────────────────────────────────────────────

PYTHON (crates/polyplug-python/src/lib/loader/mod.rs):
  Before loading plugin module via importlib:
  1. Prepend bundle_dir to sys.path
  2. If bundle_dir/site-packages/ exists: also prepend bundle_dir/site-packages
  3. After plugin load: do NOT restore sys.path — prepended paths stay for the
     process lifetime (removing them could break already-imported modules).
  Use pyo3: py.run_bound("import sys; sys.path.insert(0, path)", ...) pattern.
  No performance impact on hot path — done once at load time only.

LUA (crates/polyplug-lua/src/lib/loader/mod.rs):
  Before executing plugin chunk:
  1. Prepend bundle_dir to package.path  (for .lua files)
     pattern: bundle_dir .. "/?.lua;" .. bundle_dir .. "/?/init.lua;"
  2. Prepend bundle_dir to package.cpath (for .so/.dll C extension modules)
     pattern: bundle_dir .. "/?.so;" (Linux) / bundle_dir .. "/?.dll;" (Windows)
  Use mlua: lua.load("package.path = ... ").exec() before plugin load.
  Do NOT restore after load — same reasoning as Python.

DOTNET (crates/polyplug-dotnet/src/lib/mod.rs):
  The generated runtimeconfig.json already controlled by polyplug.
  Add bundle_dir to additionalProbingPaths in generated runtimeconfig.json.
  This allows managed assembly dependencies shipped in bundle dir to be found
  by the CLR automatically.
  runtimeconfig.json generation: add "additionalProbingPaths": ["BUNDLE_DIR_ABS"]
  where BUNDLE_DIR_ABS is the absolute bundle directory path at load time.
  NOTE: additionalProbingPaths is for managed assemblies only.
  For native interop DLLs: plugin developer uses PluginContext.bundle_path
  with NativeLibrary.Load() — documented in generated C# README.md.

JS-DENO (crates/polyplug-js-deno/src/lib/loader/mod.rs):
  deno_core module resolution: load plugin as file:///bundle_dir/index.ts
  (or file:///bundle_dir/bundle.js if present).
  Relative imports within the plugin (e.g. import "./utils.ts") resolve
  correctly relative to the module URL — no extra configuration needed.
  This is already correct if the module URL uses the absolute bundle path.
  Verify and document — no code change likely needed, just confirm and test.

JS-QUICKJS (crates/polyplug-js/src/lib/loader/mod.rs):
  Rolldown pre-bundles everything into bundle.js — no imports at runtime.
  No path setup needed. Document in README.md: "all dependencies must be
  bundled via rolldown — runtime imports are not supported."

NATIVE:
  No automatic setup. Plugin developer uses PluginContext.bundle_path to
  construct absolute paths. Document clearly in generated native README.md.

─────────────────────────────────────────────────────────────
EPIC GOAL
─────────────────────────────────────────────────────────────

1. ABI: add PluginContext struct.
   crates/polyplug/src/abi/mod.rs
   Update init() signature in all documentation and ABI comments.

2. Runtime: all loaders construct PluginContext and pass to init().
   NativeBundleLoader, DotnetLoader, PythonLoader, LuaLoader,
   JsLoader, JsDenoLoader — all updated.
   JS loaders: extract bundle_path string, pass to JS init as plain string.

3. Guest libs: PluginContext + all StringView/Buffer helpers.
   guest-libs/rust/    — impl blocks + trait impls
   guest-libs/cpp/     — struct methods + conversion operators
   guest-libs/csharp/  — explicit operator + PinnedStringView + no unsafe
   guest-libs/python/  — dunder methods + abi.pyi updated
   guest-libs/lua/     — metamethods
   guest-libs/js/      — StringViewHelper + BufferHelper static classes
   Also update host-libs/cpp/ and host-libs/csharp/ with same type helpers
   (host side uses same ABI types).

4. Generators: update init() signature in generated code for all six languages.
   crates/polyplugc/src/generators/ — all six generators.
   Generated plugin_init receives ctx parameter per language spec above.

5. Automatic path setup per runtime:
   polyplug-python: prepend bundle_dir (+ site-packages if exists) to sys.path
   polyplug-lua:    prepend bundle_dir to package.path and package.cpath
   polyplug-dotnet: add bundle_dir to additionalProbingPaths in runtimeconfig.json
   polyplug-js-deno: verify module URL uses absolute bundle path (likely no change)
   polyplug-js:     document rolldown requirement, no runtime change

6. Update all existing test fixtures:
   All fixture init() functions gain ctx parameter.
   Tests that call init() directly gain a PluginContext argument.

7. Integration tests — tests/integration_context/mod.rs (new):
   a. bundle_path is correct absolute path to bundle directory
   b. bundle_path StringView: as_str()/ToString()/tostring() returns correct value
   c. Python: sys.path contains bundle_dir after load
   d. Python: sys.path contains bundle_dir/site-packages if it exists
   e. Lua: package.path contains bundle_dir pattern after load
   f. Lua: package.cpath contains bundle_dir pattern after load
   g. .NET: additionalProbingPaths contains bundle_dir in runtimeconfig.json
   h. All six languages: plugin uses ctx bundle_path to construct a path
      to a data file in bundle dir, opens and reads it — correct content

8. All existing integration tests updated for new init() signature.

9. PRD update: section 6 (PluginContext struct, init signature),
   section 7 (init signature in vtable exchange diagram),
   section 10 (Python sys.path, Lua package.path, .NET probingPaths),
   section 15 (StringView/Buffer helpers per language).

---

VERIFICATION CHECKLIST

- PluginContext struct in ABI — #[repr(C)], StringView bundle_path — verified
- init(registrar, ctx) signature correct in all six languages — verified
- bundle_path is absolute path to bundle directory — verified in test
- PluginContext pointer valid for duration of init() only — documented
- Rust StringView: as_str(), From<&str>, From<StringView>→String, Display, PartialEq — verified
- C++ StringView: implicit→string_view, explicit→string, ctor from string_view and literal — verified
- C# StringView: explicit operator string, no unsafe keyword anywhere — verified
- C# PinnedStringView: IDisposable, GCHandle released on Dispose — verified
- Python StringView: __str__, __bytes__, __bool__, __eq__, from_str — verified
- Lua StringView: __tostring, __eq, __len, from_string — verified
- JS: StringViewHelper.decode/encode/isEmpty, BufferHelper.toBytes/isEmpty — verified
- Python: sys.path prepended with bundle_dir at load time — verified
- Python: sys.path prepended with bundle_dir/site-packages if exists — verified
- Lua: package.path prepended with bundle_dir pattern — verified
- Lua: package.cpath prepended with bundle_dir pattern — verified
- .NET: additionalProbingPaths in runtimeconfig.json contains bundle_dir — verified
- js-deno: module URL is absolute file:/// path — verified
- All existing integration tests pass with updated init() signature — verified
- No unsafe in C# guest lib — verified by grep
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```


---

## Epic 21 — C Facade + Lua Host Lib + Deno Host Lib

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.

All architectural questions for this epic are pre-answered below.
Write the plan directly — do not ask further questions unless something is
genuinely contradictory or missing.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 8 (host libs overview), section 24 (package ecosystem),
  section 25 (C facade spec), section 10 (Lua and JS adapters for context)
- host-libs/python/ — reference implementation pattern to follow
- host-libs/lua/ — does not exist yet, create it
- host-libs/js/  — does not exist yet, create it
- crates/polyplug/src/ffi/ — does not exist yet, create it

---

PROJECT CONTEXT

polyplug supports any language as both host and guest. This epic adds the two missing
host libs — Lua and JS/Deno — plus the stable C facade in libpolyplug.so that both
depend on. After this epic all six supported languages can be a runtime host.

---

PRE-ANSWERED DECISIONS

─────────────────────────────────────────────────────────────
PART 1 — C Facade (crates/polyplug/src/ffi/mod.rs)
─────────────────────────────────────────────────────────────

New module: crates/polyplug/src/ffi/mod.rs
Exported from crates/polyplug/src/lib.rs as: pub mod ffi;

All symbols use #[no_mangle] and extern "C".
All symbols prefixed polyplug_.
No Rust types cross the boundary — only primitives, raw pointers, ABI structs.

PluginHandle packing:
  packed u64 = (generation as u64) << 32 | (index as u64)
  null handle = u64::MAX  (both fields U32_MAX)
  Unpack: index = (packed & 0xFFFF_FFFF) as u32
          generation = (packed >> 32) as u32

Opaque types:
  pub struct OpaqueRuntime(PluginRuntime);  // PluginRuntime = the main runtime type
  pub struct OpaqueGuard(PluginVTableGuard); // the arc-swap guard wrapper

Error handling:
  thread_local! { static LAST_ERROR: RefCell<String> = RefCell::new(String::new()); }
  fn set_last_error(msg: impl Into<String>)
  On any Err result: set_last_error, return error sentinel (0 for ptr, u32::MAX for codes)
  Never panic across FFI boundary — all panics caught with std::panic::catch_unwind

EXPORTED SYMBOLS (implement all):

  // Lifecycle
  #[no_mangle] pub unsafe extern "C"
  fn polyplug_runtime_new() -> *mut OpaqueRuntime
    // Constructs a default PluginRuntime with no adapters
    // Returns null on failure (set_last_error)

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_runtime_free(rt: *mut OpaqueRuntime)
    // Drops the OpaqueRuntime. No-op if null.

  // Bundle loading
  #[no_mangle] pub unsafe extern "C"
  fn polyplug_load_bundle(rt: *mut OpaqueRuntime,
                           path: *const u8, path_len: usize) -> u32
    // Returns 0 on success, non-zero on error (set_last_error)

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_reload_bundle(rt: *mut OpaqueRuntime,
                              path: *const u8, path_len: usize) -> u32
    // Returns 0 on success, non-zero on error

  // Discovery
  #[no_mangle] pub unsafe extern "C"
  fn polyplug_find_by_contract(rt: *mut OpaqueRuntime,
                                contract_id: u64, min_version: u32) -> u64
    // Returns packed handle, or null handle (u64::MAX) if not found

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_find_by_bundle(rt: *mut OpaqueRuntime,
                               bundle_id: u64, contract_id: u64,
                               min_version: u32) -> u64

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_find_all_by_contract(rt: *mut OpaqueRuntime,
                                    contract_id: u64, min_version: u32,
                                    out: *mut u64, out_cap: usize) -> usize
    // caller-provides-buffer pattern, returns count written

  // Vtable access
  #[no_mangle] pub unsafe extern "C"
  fn polyplug_resolve_plugin(rt: *mut OpaqueRuntime,
                               packed_handle: u64) -> *const OpaqueGuard
    // Returns null if handle invalid

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_guard_free(guard: *const OpaqueGuard)
    // Drops the guard (releases arc-swap read guard). No-op if null.

  #[no_mangle] pub unsafe extern "C"
  fn polyplug_get_vtable(guard: *const OpaqueGuard) -> *const ()
    // Returns raw pointer to PluginVTable. Valid while guard is alive.

  // Error retrieval
  #[no_mangle] pub unsafe extern "C"
  fn polyplug_last_error(out: *mut u8, out_cap: usize) -> usize
    // Copies last error string (UTF-8) into caller-provided buffer.
    // Returns number of bytes written (not including null terminator).
    // Returns 0 if no error or buffer too small.
    // Clears the stored error after reading.

NOTE: The C facade does NOT include adapter registration (polyplug-dotnet, polyplug-python, etc.)
because those adapters are Rust-only — the Lua and Deno host apps load bundles of any
runtime type transparently. The runtime determines which adapter to invoke at load time.
The FFI consumer never needs to know which adapter handles a given bundle.

─────────────────────────────────────────────────────────────
PART 2 — Lua Host Lib (host-libs/lua/)
─────────────────────────────────────────────────────────────

DIRECTORY LAYOUT:
  host-libs/lua/
    polyplug.lua           — main host lib module
    polyplug.d.lua         — EmmyLua/LuaLS type annotations (documentation only)
    README.md              — usage guide

MECHANISM: LuaJIT FFI via ffi.load("polyplug") — loads libpolyplug.so from the path
configured by the app. Performance: LuaJIT JIT-compiles C calls to direct indirect
calls; hot-path vtable dispatch approaches native speed.

IMPLEMENTATION (polyplug.lua):

  -- polyplug.lua
  local ffi = require("ffi")

  ffi.cdef[[
    typedef struct OpaqueRuntime OpaqueRuntime;
    typedef struct OpaqueGuard   OpaqueGuard;

    OpaqueRuntime* polyplug_runtime_new(void);
    void           polyplug_runtime_free(OpaqueRuntime* rt);
    uint32_t       polyplug_load_bundle(OpaqueRuntime* rt,
                                         const uint8_t* path, size_t path_len);
    uint32_t       polyplug_reload_bundle(OpaqueRuntime* rt,
                                           const uint8_t* path, size_t path_len);
    uint64_t       polyplug_find_by_contract(OpaqueRuntime* rt,
                                              uint64_t contract_id,
                                              uint32_t min_version);
    uint64_t       polyplug_find_by_bundle(OpaqueRuntime* rt,
                                            uint64_t bundle_id,
                                            uint64_t contract_id,
                                            uint32_t min_version);
    size_t         polyplug_find_all_by_contract(OpaqueRuntime* rt,
                                                  uint64_t contract_id,
                                                  uint32_t min_version,
                                                  uint64_t* out, size_t out_cap);
    OpaqueGuard*   polyplug_resolve_plugin(OpaqueRuntime* rt,
                                            uint64_t packed_handle);
    void           polyplug_guard_free(const OpaqueGuard* guard);
    const void*    polyplug_get_vtable(const OpaqueGuard* guard);
    size_t         polyplug_last_error(uint8_t* out, size_t out_cap);
  ]]

  -- Load from path provided to polyplug.load_lib(path) or default "polyplug"
  local lib = nil

  local M = {}

  -- Must be called before M.Runtime.new()
  -- path: absolute or relative path to libpolyplug.so/.dll/.dylib
  function M.load_lib(path)
      lib = ffi.load(path)
  end

  -- Guard metatable
  local Guard = {}
  Guard.__index = Guard
  Guard.__gc = function(self)
      if self._ptr ~= nil then
          lib.polyplug_guard_free(self._ptr)
          self._ptr = nil
      end
  end
  function Guard:vtable()
      -- returns raw cdata pointer — caller casts to their vtable type via ffi.cast
      return lib.polyplug_get_vtable(self._ptr)
  end
  function Guard:free()
      lib.polyplug_guard_free(self._ptr)
      self._ptr = nil
  end

  -- Runtime metatable
  local Runtime = {}
  Runtime.__index = Runtime
  Runtime.__gc = function(self)
      if self._ptr ~= nil then
          lib.polyplug_runtime_free(self._ptr)
          self._ptr = nil
      end
  end

  function M.Runtime.new()
      local ptr = lib.polyplug_runtime_new()
      if ptr == nil then
          error("polyplug_runtime_new failed: " .. M.last_error())
      end
      return setmetatable({ _ptr = ptr }, Runtime)
  end

  function Runtime:load_bundle(path)
      local code = lib.polyplug_load_bundle(self._ptr, path, #path)
      if code ~= 0 then
          error("load_bundle failed: " .. M.last_error())
      end
  end

  function Runtime:reload_bundle(path)
      local code = lib.polyplug_reload_bundle(self._ptr, path, #path)
      if code ~= 0 then
          error("reload_bundle failed: " .. M.last_error())
      end
  end

  function Runtime:find_by_contract(contract_id, min_version)
      -- contract_id: uint64 cdata (ffi.new("uint64_t", ...))
      return lib.polyplug_find_by_contract(self._ptr, contract_id, min_version)
  end

  function Runtime:find_by_bundle(bundle_id, contract_id, min_version)
      return lib.polyplug_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)
  end

  function Runtime:find_all_by_contract(contract_id, min_version, cap)
      cap = cap or 16
      local out = ffi.new("uint64_t[?]", cap)
      local n = lib.polyplug_find_all_by_contract(
          self._ptr, contract_id, min_version, out, cap)
      local results = {}
      for i = 0, tonumber(n) - 1 do
          results[i+1] = out[i]
      end
      return results
  end

  function Runtime:resolve_plugin(packed_handle)
      local ptr = lib.polyplug_resolve_plugin(self._ptr, packed_handle)
      if ptr == nil then return nil end
      return setmetatable({ _ptr = ptr }, Guard)
  end

  function Runtime:free()
      lib.polyplug_runtime_free(self._ptr)
      self._ptr = nil
  end

  function M.last_error()
      local buf = ffi.new("uint8_t[512]")
      local n = lib.polyplug_last_error(buf, 512)
      if n == 0 then return "(no error)" end
      return ffi.string(buf, n)
  end

  return M

IMPORTANT DETAILS:
- u64 IDs passed as ffi.new("uint64_t", value) — LuaJIT cdata uint64_t
- NULL_HANDLE = ffi.cast("uint64_t", ffi.new("uint64_t", 0xFFFFFFFFFFFFFFFFULL))
- Guard has __gc metamethod — automatic free when Lua GC collects
- Runtime has __gc metamethod — automatic free
- polyplug.load_lib(path) must be called before Runtime.new() — document clearly

─────────────────────────────────────────────────────────────
PART 3 — Deno Host Lib (host-libs/js/)
─────────────────────────────────────────────────────────────

DIRECTORY LAYOUT:
  host-libs/js/
    polyplug.ts            — main host lib module (Deno.dlopen)
    polyplug_test.ts       — Deno test suite
    README.md              — usage guide (note: requires --allow-ffi)
    deno.json              — Deno config

MECHANISM: Deno.dlopen into libpolyplug.so.
Performance: V8 Fast API calls for non-BigInt params (<10ns); ~150ns for BigInt u64 params.
u64 IDs are BigInt in TypeScript — they take the ~150ns slow path. This is acceptable
because find_by_contract etc. are load-time operations. Hot-path vtable dispatch is direct
memory access — no FFI call involved.

IMPLEMENTATION (polyplug.ts):

  // polyplug.ts — Deno host lib for polyplug

  export const NULL_HANDLE = 0xFFFFFFFFFFFFFFFFn;  // BigInt

  function openLib(path: string) {
      return Deno.dlopen(path, {
          polyplug_runtime_new:         { parameters: [],                                    result: "pointer"  },
          polyplug_runtime_free:        { parameters: ["pointer"],                           result: "void"     },
          polyplug_load_bundle:         { parameters: ["pointer", "buffer", "usize"],        result: "u32"      },
          polyplug_reload_bundle:       { parameters: ["pointer", "buffer", "usize"],        result: "u32"      },
          polyplug_find_by_contract:    { parameters: ["pointer", "u64", "u32"],             result: "u64"      },
          polyplug_find_by_bundle:      { parameters: ["pointer", "u64", "u64", "u32"],      result: "u64"      },
          polyplug_find_all_by_contract:{ parameters: ["pointer", "u64", "u32", "buffer", "usize"], result: "usize" },
          polyplug_resolve_plugin:      { parameters: ["pointer", "u64"],                    result: "pointer"  },
          polyplug_guard_free:          { parameters: ["pointer"],                           result: "void"     },
          polyplug_get_vtable:          { parameters: ["pointer"],                           result: "pointer"  },
          polyplug_last_error:          { parameters: ["buffer", "usize"],                   result: "usize"    },
      } as const);
  }

  function readLastError(lib: ReturnType<typeof openLib>): string {
      const buf = new Uint8Array(512);
      const n = lib.symbols.polyplug_last_error(buf, 512n);
      if (n === 0n) return "(no error)";
      return new TextDecoder().decode(buf.subarray(0, Number(n)));
  }

  function encodeStr(s: string): Uint8Array {
      return new TextEncoder().encode(s);
  }

  export class Guard {
      #lib: ReturnType<typeof openLib>;
      #ptr: Deno.PointerValue;

      constructor(lib: ReturnType<typeof openLib>, ptr: Deno.PointerValue) {
          this.#lib = lib;
          this.#ptr = ptr;
      }

      vtable(): Deno.PointerValue {
          return this.#lib.symbols.polyplug_get_vtable(this.#ptr);
      }

      free(): void {
          this.#lib.symbols.polyplug_guard_free(this.#ptr);
          this.#ptr = null;
      }

      [Symbol.dispose](): void { this.free(); }
  }

  export class Runtime {
      #lib: ReturnType<typeof openLib>;
      #ptr: Deno.PointerValue;

      private constructor(lib: ReturnType<typeof openLib>, ptr: Deno.PointerValue) {
          this.#lib = lib;
          this.#ptr = ptr;
      }

      static open(libPath: string): Runtime {
          const lib = openLib(libPath);
          const ptr = lib.symbols.polyplug_runtime_new();
          if (ptr === null) {
              throw new Error("polyplug_runtime_new failed: " + readLastError(lib));
          }
          return new Runtime(lib, ptr);
      }

      loadBundle(path: string): void {
          const encoded = encodeStr(path);
          const code = this.#lib.symbols.polyplug_load_bundle(
              this.#ptr, encoded, BigInt(encoded.length));
          if (code !== 0) {
              throw new Error("loadBundle failed: " + readLastError(this.#lib));
          }
      }

      reloadBundle(path: string): void {
          const encoded = encodeStr(path);
          const code = this.#lib.symbols.polyplug_reload_bundle(
              this.#ptr, encoded, BigInt(encoded.length));
          if (code !== 0) {
              throw new Error("reloadBundle failed: " + readLastError(this.#lib));
          }
      }

      findByContract(contractId: bigint, minVersion: number): bigint {
          return this.#lib.symbols.polyplug_find_by_contract(
              this.#ptr, contractId, minVersion);
      }

      findByBundle(bundleId: bigint, contractId: bigint, minVersion: number): bigint {
          return this.#lib.symbols.polyplug_find_by_bundle(
              this.#ptr, bundleId, contractId, minVersion);
      }

      findAllByContract(contractId: bigint, minVersion: number, cap = 16): bigint[] {
          const buf = new BigUint64Array(cap);
          const n = this.#lib.symbols.polyplug_find_all_by_contract(
              this.#ptr, contractId, minVersion,
              new Uint8Array(buf.buffer), BigInt(cap));
          return Array.from(buf.subarray(0, Number(n)));
      }

      resolvePlugin(packedHandle: bigint): Guard | null {
          const ptr = this.#lib.symbols.polyplug_resolve_plugin(
              this.#ptr, packedHandle);
          if (ptr === null) return null;
          return new Guard(this.#lib, ptr);
      }

      free(): void {
          this.#lib.symbols.polyplug_runtime_free(this.#ptr);
          this.#ptr = null;
      }

      [Symbol.dispose](): void { this.free(); }
  }

IMPORTANT DETAILS:
- Requires --allow-ffi at runtime — document in README.md
- u64 parameters passed as BigInt — takes V8 slow call path (~150ns)
  This is acceptable: find_by_contract etc. are load-time only
  Hot-path vtable dispatch does NOT go through FFI — it is direct pointer call
- path_len passed as BigInt (usize = u64 on 64-bit) for polyplug_load_bundle
- Guard and Runtime implement Symbol.dispose for "using" syntax (Deno 2.x)
- NULL_HANDLE = 0xFFFFFFFFFFFFFFFFn — check against this for not-found

─────────────────────────────────────────────────────────────
EPIC GOAL
─────────────────────────────────────────────────────────────

1. C facade — crates/polyplug/src/ffi/mod.rs
   All symbols from spec above.
   pub mod ffi; exported from crates/polyplug/src/lib.rs.
   thread_local LAST_ERROR, set_last_error, catch_unwind on every call.
   Unit tests in ffi/mod.rs: round-trip load+find+resolve on a test fixture.

2. Lua host lib — host-libs/lua/
   polyplug.lua per spec above.
   polyplug.d.lua type annotations (EmmyLua style).
   README.md: how to use, how to pass u64 IDs (ffi.new("uint64_t",...)), --
   how to cast vtable pointers, how to load_lib.

3. Deno host lib — host-libs/js/
   polyplug.ts per spec above.
   polyplug_test.ts: Deno.test suite — load bundle, find, resolve, call vtable.
   README.md: how to use, --allow-ffi requirement, BigInt for IDs, vtable usage.
   deno.json: minimal config (no extra deps needed).

4. Integration tests — tests/integration_host_lua/mod.rs (new):
   a. Runtime.new() constructs successfully
   b. load_bundle() loads a test fixture bundle
   c. find_by_contract() returns valid handle
   d. resolve_plugin() returns guard
   e. guard:vtable() returns non-null pointer
   f. calling a vtable function via FFI returns correct result
   g. NULL_HANDLE returned for missing contract
   h. last_error() returns message after failed operation

5. Integration tests — tests/integration_host_deno/mod.rs (new):
   Same surface as Lua tests but run via Deno subprocess:
   deno run --allow-ffi tests/fixtures/deno_host_test.ts
   a–h same as above
   Uses deno::run_path or std::process::Command to invoke Deno

6. PRD update: section 25 (C facade — already updated), section 8 (host-libs table).

---

VERIFICATION CHECKLIST

- C facade compiles with no warnings — verified
- All polyplug_ symbols visible in libpolyplug.so via `nm -D` — verified
- catch_unwind on every extern "C" fn — verified by grep
- LAST_ERROR thread-local cleared after polyplug_last_error read — verified
- Lua: __gc on Runtime and Guard — no memory leaks — verified
- Lua: u64 IDs as LuaJIT cdata uint64_t — not Lua numbers — verified
- Deno: BigInt for all u64 params — no number/bigint confusion — verified
- Deno: Symbol.dispose on Runtime and Guard — verified
- Deno: --allow-ffi documented in README.md — verified
- NULL_HANDLE correctly detected (u64::MAX / 0xFFFFFFFFFFFFFFFFn) — verified
- All integration tests pass — verified
- No .unwrap() in production code
- clippy passes with zero warnings
- cargo test --workspace passes
```


---

## Epic 21 — Lua Host Lib, Deno Host Lib, and C Facade

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
All architectural questions for this epic are pre-answered below.

---

READ FIRST
- AGENTS.md
- polyplug_prd.md — section 6 (ABI Layer), section 8 (Host Libraries),
  section 25 (C Facade)
- host-libs/python/runtime.py — reference implementation to mirror in Lua
- host-libs/csharp/src/Runtime.cs — reference for host API surface
- crates/polyplug/src/ffi/mod.rs — C facade (must be written first)

---

EPIC GOAL

Enable Lua apps and Deno apps to be polyplug hosts — loading plugins, calling
vtable functions, and driving the full runtime — with near-native or native
performance and no compilation step beyond building libpolyplug.so.

Three deliverables:

1. C facade — crates/polyplug/src/ffi/mod.rs
   Stable extern "C" symbols exported from libpolyplug.so.
   Foundation that both Lua and Deno host libs depend on.

2. host-libs/lua/ — LuaJIT FFI host lib
   polyplug.lua + polyplug.d.lua + README.md

3. host-libs/js/ — Deno.dlopen host lib
   polyplug.ts + polyplug_test.ts + deno.json + README.md

---

PRE-ANSWERED DECISIONS

─────────────────────────────────────────────────────────────
PART 1 — C FACADE (crates/polyplug/src/ffi/mod.rs)
─────────────────────────────────────────────────────────────

Exported symbols — all #[no_mangle] pub unsafe extern "C":

  OpaqueRuntime* polyplug_runtime_new()
  void           polyplug_runtime_free(OpaqueRuntime* rt)
  uint32_t       polyplug_load_bundle(OpaqueRuntime* rt,
                     const uint8_t* path, size_t path_len)
  uint32_t       polyplug_load_bundle_opts(OpaqueRuntime* rt,
                     const uint8_t* path, size_t path_len,
                     uint8_t compatibility)
  uint64_t       polyplug_find_by_contract(OpaqueRuntime* rt,
                     uint64_t contract_id, uint32_t min_version)
  uint64_t       polyplug_find_by_bundle(OpaqueRuntime* rt,
                     uint64_t bundle_id, uint64_t contract_id,
                     uint32_t min_version)
  size_t         polyplug_find_all_by_contract(OpaqueRuntime* rt,
                     uint64_t contract_id, uint32_t min_version,
                     uint64_t* out, size_t out_cap)
  const void*    polyplug_resolve_plugin(OpaqueRuntime* rt, uint64_t handle)
  void           polyplug_guard_free(const void* guard)
  const void*    polyplug_get_vtable(const void* guard)
  uint32_t       polyplug_reload_bundle(OpaqueRuntime* rt,
                     const uint8_t* path, size_t path_len)
  size_t         polyplug_last_error(OpaqueRuntime* rt,
                     uint8_t* out, size_t out_cap)

PluginHandle packing: u64 = (generation as u64) << 32 | index as u64
Null handle = u64::MAX (maps to PluginHandle { index: U32_MAX, generation: 0 })

OpaqueRuntime: opaque Rust struct — only ever used as a pointer by FFI callers.
polyplug_last_error: writes UTF-8 error string into caller-provided buffer,
returns bytes written. Thread-local last error, set on any non-zero return code.
Returns 0 bytes if no error pending.

All functions are zero-overhead shims — direct calls into existing Rust runtime,
no extra allocation, no extra logic.

─────────────────────────────────────────────────────────────
PART 2 — host-libs/lua/polyplug.lua
─────────────────────────────────────────────────────────────

Pure LuaJIT FFI module. No Lua/C API bindings. No compilation step.
Requires libpolyplug.so on LD_LIBRARY_PATH (or explicit path).

ffi.cdef declares all C facade symbols.
ffi.load("polyplug") loads the shared library.

Exposed Lua API:

  polyplug.Runtime.new() → Runtime userdata
  runtime:load_bundle(path: string) → nil or error()
  runtime:load_bundle_opts(path: string, compatibility: number) → nil or error()
  runtime:find_by_contract(contract_id: uint64_cdata, min_version: number) → uint64_cdata
  runtime:find_by_bundle(bundle_id: uint64_cdata, contract_id: uint64_cdata,
                          min_version: number) → uint64_cdata
  runtime:find_all_by_contract(contract_id: uint64_cdata, min_version: number,
                                cap: number?) → table of uint64_cdata
  runtime:resolve_plugin(handle: uint64_cdata) → Guard userdata
  runtime:reload_bundle(path: string) → nil or error()
  runtime:free() → nil   -- explicit free, also called by __gc

  guard:vtable() → cdata pointer (const void*)
  guard:free()   → nil   -- explicit free, also called by __gc

  polyplug.NULL_HANDLE = ffi.cast("uint64_t", 2^64 - 1)  -- u64::MAX

contract_id and bundle_id are uint64 cdata — LuaJIT native 64-bit integers.
Returned from ffi calls as cdata uint64_t directly.
Null handle detection: handle == polyplug.NULL_HANDLE

Error handling: all non-zero return codes call polyplug_last_error, then error()
with the message. Lua stack unwinding is fine — OpaqueRuntime is GC'd via __gc.

__gc metamethod on Runtime calls polyplug_runtime_free.
__gc metamethod on Guard calls polyplug_guard_free.
Both also have explicit :free() methods for deterministic cleanup.

ffi.metatype used for Runtime and Guard ctypes.
LuaJIT JIT-compiles all FFI calls to direct indirect calls — near-native speed.

host-libs/lua/polyplug.d.lua:
  EmmyLua/LuaLS annotation file. Documents all types and methods.
  ---@class Runtime
  ---@class Guard
  etc.

host-libs/lua/README.md:
  Installation (copy polyplug.lua, set LD_LIBRARY_PATH).
  Basic usage example.
  Note on --allow-ffi equivalent (none needed — LuaJIT FFI is always available).

─────────────────────────────────────────────────────────────
PART 3 — host-libs/js/polyplug.ts
─────────────────────────────────────────────────────────────

Deno.dlopen based. TypeScript. Requires --allow-ffi at runtime.
No compilation step — import polyplug.ts directly.

Deno.dlopen("libpolyplug.so", { ... }) with full symbol table matching C facade.

All u64 parameters/returns use Deno "u64" type → BigInt in TypeScript.
All pointer parameters use Deno "pointer" type → Deno.PointerValue in TypeScript.
All size_t parameters use Deno "usize" type → bigint in TypeScript (Deno maps usize to bigint).

Exposed TypeScript API:

  class Runtime {
    static build(): Runtime
    loadBundle(path: string): void
    loadBundleOpts(path: string, compatibility: number): void
    findByContract(contractId: bigint, minVersion: number): bigint
    findByBundle(bundleId: bigint, contractId: bigint, minVersion: number): bigint
    findAllByContract(contractId: bigint, minVersion: number): bigint[]
    resolvePlugin(handle: bigint): Guard
    reloadBundle(path: string): void
    free(): void
    [Symbol.dispose](): void   // using declaration support
  }

  class Guard {
    vtable(): Deno.PointerValue
    free(): void
    [Symbol.dispose](): void
  }

  export const NULL_HANDLE: bigint = 0xFFFFFFFFFFFFFFFFn

Error handling: non-zero return codes → polyplug_last_error → throw new Error(msg).
[Symbol.dispose] calls free() — supports TypeScript 5.2 "using" declarations.

host-libs/js/deno.json:
  { "name": "polyplug", "version": "0.1.0" }

host-libs/js/polyplug_test.ts:
  Deno test file. Loads a test plugin, calls find_by_contract, resolve_plugin.
  Uses test fixtures from tests/fixtures/.
  Graceful skip if libpolyplug.so not found.

host-libs/js/README.md:
  Usage: deno run --allow-ffi my_host_app.ts
  Import: import { Runtime } from "./polyplug.ts"
  Note on BigInt for contract IDs.
  Note on vtable usage (raw pointer — cast via Deno.UnsafePointer).

─────────────────────────────────────────────────────────────
TESTS
─────────────────────────────────────────────────────────────

tests/integration_host_lua/mod.rs (new):
  a. Runtime::new() returns non-null
  b. load_bundle() loads test_plugin_dir fixture
  c. find_by_contract() returns valid handle (not NULL_HANDLE)
  d. resolve_plugin() returns non-null guard
  e. guard:vtable() returns non-null pointer
  f. reload_bundle() works without crash
  g. free() / __gc called — no memory errors under valgrind
  Graceful skip if luajit not found (same skip pattern as other Lua tests).

tests/integration_host_deno/mod.rs (new):
  Rust test that shells out to: deno run --allow-ffi tests/fixtures/deno_host_test.ts
  deno_host_test.ts does the same checks as integration_host_lua above but in TS.
  Graceful skip if deno not found.

tests/fixtures/deno_host_test.ts (new):
  Standalone Deno test script exercising the full host lib surface.

---

VERIFICATION CHECKLIST

- C facade: all symbols present in libpolyplug.so (verified via nm -D) — verified
- C facade: PluginHandle pack/unpack round-trips correctly — verified
- C facade: polyplug_last_error returns correct message on error — verified
- C facade: __gc / free double-free protection — verified
- Lua: require("polyplug") works with luajit — verified
- Lua: all Runtime methods work against test fixture — verified
- Lua: __gc called on GC — verified (no leak under valgrind)
- Deno: import polyplug.ts works with --allow-ffi — verified
- Deno: all Runtime methods work against test fixture — verified
- Deno: [Symbol.dispose] calls free() — verified
- Deno: BigInt u64 round-trips correctly — verified
- No .unwrap() in ffi/mod.rs
- clippy passes with zero warnings
- cargo test --workspace passes
```


---

## Epic 22 — Eliminate `unsafe` from C# Guest Lib, Host Lib, and Showcase

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
All architectural decisions for this epic are fully pre-answered below.
Do not investigate or ask questions — implement the plan exactly as specified.

---

READ FIRST
- AGENTS.md
- polyplug_prd.md — section 3 (C# unsafe policy), section 10 (C# adapter,
  generated C# performance requirements, C# unsafe boundary table)
- guest-libs/csharp/src/Abi.cs
- guest-libs/csharp/src/PinnedStringView.cs
- guest-libs/csharp/src/StringViewHelper.cs
- host-libs/csharp/src/Abi.cs
- host-libs/csharp/src/Runtime.cs
- showcase/plugins/csv_encoder/Plugin.cs
- tests/fixtures/csharp_plugin/Plugin.cs
- crates/polyplugc/src/generators/csharp/mod.rs

---

GOAL

Remove every `unsafe` keyword and raw pointer type from:
- guest-libs/csharp/  (zero unsafe, no <AllowUnsafeBlocks> required)
- host-libs/csharp/   (zero unsafe, no <AllowUnsafeBlocks> required)
- showcase/           (zero unsafe)
- tests/fixtures/csharp_plugin/ (zero unsafe)

Confine ALL unsafe to generated Init.cs only, inside an isolated unsafe { }
block. The generated .csproj (polyplugc-controlled) keeps
<AllowUnsafeBlocks>true</AllowUnsafeBlocks>. No other .csproj has it.

---

PRE-ANSWERED DECISIONS — implement exactly as specified

─────────────────────────────────────────────────────────────
REPLACEMENT 1 — StringView struct (guest-libs and host-libs Abi.cs)
─────────────────────────────────────────────────────────────

BEFORE:
  public unsafe struct StringView {
      public byte* Ptr;
      public nuint  Len;
  }

AFTER:
  [StructLayout(LayoutKind.Sequential)]
  public struct StringView {
      public IntPtr Ptr;    // byte* → IntPtr, pointer-sized, ABI-identical
      public ulong  Len;    // nuint → ulong, polyplug is 64-bit only

      public static readonly StringView Empty = default;
      public bool IsEmpty => Len == 0;

      public override string ToString() =>
          Ptr == IntPtr.Zero ? string.Empty
          : Marshal.PtrToStringUTF8(Ptr, (int)Len) ?? string.Empty;

      public static explicit operator string(StringView sv) => sv.ToString();
  }

nuint → ulong rationale: polyplug explicitly targets 64-bit only.
ulong is always 8 bytes. nuint is 4 bytes on 32-bit — not a concern.
Add a test assertion: Marshal.SizeOf<StringView>() == 16

─────────────────────────────────────────────────────────────
REPLACEMENT 2 — Buffer struct (guest-libs and host-libs Abi.cs)
─────────────────────────────────────────────────────────────

BEFORE:
  public unsafe struct Buffer {
      public void*  Ptr;
      public nuint  Len;
      public nuint  Cap;
  }

AFTER:
  [StructLayout(LayoutKind.Sequential)]
  public struct Buffer {
      public IntPtr Ptr;
      public ulong  Len;
      public ulong  Cap;

      public bool IsEmpty => Len == 0;
  }

Add test assertion: Marshal.SizeOf<Buffer>() == 24

─────────────────────────────────────────────────────────────
REPLACEMENT 3 — PluginHandle (if unsafe, guest-libs and host-libs Abi.cs)
─────────────────────────────────────────────────────────────

  [StructLayout(LayoutKind.Sequential)]
  public struct PluginHandle {
      public uint Index;
      public uint Generation;
      public static readonly PluginHandle Null =
          new PluginHandle { Index = uint.MaxValue, Generation = 0 };
      public bool IsNull => Index == uint.MaxValue;
  }

No unsafe. Pure uint fields. Marshal.SizeOf<PluginHandle>() == 8

─────────────────────────────────────────────────────────────
REPLACEMENT 4 — AbiError (if unsafe, guest-libs and host-libs Abi.cs)
─────────────────────────────────────────────────────────────

  [StructLayout(LayoutKind.Sequential)]
  public struct AbiError {
      public uint       Code;
      public StringView Message;   // safe StringView from Replacement 1
      public static readonly AbiError Ok = default;
      public bool IsOk => Code == 0;
      public static AbiError FromException(Exception ex) { ... }
  }

─────────────────────────────────────────────────────────────
REPLACEMENT 5 — PluginContext (guest-libs Abi.cs)
─────────────────────────────────────────────────────────────

  [StructLayout(LayoutKind.Sequential)]
  public struct PluginContext {
      public StringView BundlePath;
      public string BundlePathString => BundlePath.ToString();
  }

─────────────────────────────────────────────────────────────
REPLACEMENT 6 — DataRecord and all other domain structs with padding
─────────────────────────────────────────────────────────────

Remove unsafe keyword from any domain struct that was unsafe only because
it contained StringView or Buffer fields. With safe StringView/Buffer,
these structs need only [StructLayout(LayoutKind.Sequential)].

Example:
  BEFORE: public unsafe struct DataRecord { ... }
  AFTER:  [StructLayout(LayoutKind.Sequential)]
          public struct DataRecord {
              public StringView Name;
              public StringView Value;
              public uint       Count;
              private uint      _pad;   // explicit padding — keep as-is
          }

Add test assertion: Marshal.SizeOf<DataRecord>() == 40

─────────────────────────────────────────────────────────────
REPLACEMENT 7 — PluginRegistrar and HostVTable structs (guest-libs Abi.cs)
─────────────────────────────────────────────────────────────

Function pointer fields become IntPtr. Generated code casts to delegate* at
call site inside an unsafe block (generated file only).

BEFORE:
  public unsafe struct PluginRegistrar {
      public delegate* unmanaged[Cdecl]<...> RegisterPlugin;
      public HostVTable* Host;
  }

AFTER:
  [StructLayout(LayoutKind.Sequential)]
  public struct PluginRegistrar {
      public IntPtr RegisterPluginPtr;   // delegate* → IntPtr
      public IntPtr HostPtr;             // HostVTable* → IntPtr
  }

Same pattern for HostVTable — all function pointer fields become IntPtr.
Names: append "Ptr" suffix to each field to make intent clear.

─────────────────────────────────────────────────────────────
REPLACEMENT 8 — [UnmanagedCallersOnly] Init method (generator output)
─────────────────────────────────────────────────────────────

Update crates/polyplugc/src/generators/csharp/mod.rs to generate:

  // No unsafe on the method itself — IntPtr parameters are blittable
  [UnmanagedCallersOnly(EntryPoint = "init",
      CallConvs = new[] { typeof(CallConvCdecl) })]
  public static AbiError Init(IntPtr registrarPtr, IntPtr ctxPtr) {
      Thread.BeginThreadAffinity();
      try {
          unsafe {
              // isolated unsafe block — only in generated file
              var registrar = (PluginRegistrar*)registrarPtr.ToPointer();
              var ctx       = (PluginContext*)ctxPtr.ToPointer();
              // delegate* casts and vtable registration here
              var registerFn =
                  (delegate* unmanaged[Cdecl]<PluginRegistrar*, PluginDescriptor*,
                      PluginVTable*, void>)registrar->RegisterPluginPtr;
              // ... register each contract vtable
          }
          return AbiError.Ok;
      } catch (Exception ex) {
          return AbiError.FromException(ex);
      } finally {
          Thread.EndThreadAffinity();
      }
  }

The generated .csproj must have <AllowUnsafeBlocks>true</AllowUnsafeBlocks>.
No other .csproj in the repo has this setting.

─────────────────────────────────────────────────────────────
REPLACEMENT 9 — P/Invoke in host-libs/csharp/src/Runtime.cs
─────────────────────────────────────────────────────────────

Replace all void* parameters with IntPtr. Replace nuint with ulong.
Keep [SuppressGCTransition] on hot-path calls exactly as before.

  BEFORE:
    [DllImport("polyplug"), SuppressGCTransition]
    public static extern AbiError call_plugin(
        PluginHandle handle, uint fn_id, void* args, void* out);

  AFTER:
    [DllImport("polyplug"), SuppressGCTransition]
    public static extern AbiError call_plugin(
        PluginHandle handle, uint fn_id, IntPtr args, IntPtr outPtr);

Audit ALL P/Invoke declarations in Runtime.cs — apply this pattern to every
one that uses void*, byte*, nuint, or nint as pointer types.

─────────────────────────────────────────────────────────────
REPLACEMENT 10 — showcase and test fixtures
─────────────────────────────────────────────────────────────

showcase/plugins/csv_encoder/Plugin.cs and tests/fixtures/csharp_plugin/Plugin.cs
are plugin developer code. They must compile with zero unsafe and no
<AllowUnsafeBlocks>. Apply the same struct replacements — these files use
StringView, Buffer, and generated Init. After Replacements 1–9 are applied
to the libs and generator, these files should compile clean automatically.
Verify and fix any remaining issues.

Remove <AllowUnsafeBlocks>true</AllowUnsafeBlocks> from:
- showcase/plugins/csv_encoder/CsvEncoder.csproj
- tests/fixtures/csharp_plugin/CsharpPlugin.csproj
- guest-libs/csharp/Polyplug.Guest.csproj
- host-libs/csharp/Polyplug.csproj
Only the polyplugc-generated plugin project keeps it.

---

VERIFICATION CHECKLIST

- Marshal.SizeOf<StringView>() == 16 — verified in new test
- Marshal.SizeOf<Buffer>() == 24 — verified in new test
- Marshal.SizeOf<PluginHandle>() == 8 — verified in new test
- Marshal.SizeOf<DataRecord>() == 40 — verified in new test
- grep -r "unsafe" guest-libs/csharp/ → zero results
- grep -r "unsafe" host-libs/csharp/ → zero results
- grep -r "unsafe" showcase/ → zero results
- grep -r "unsafe" tests/fixtures/csharp_plugin/ → zero results
- grep "AllowUnsafeBlocks" guest-libs/csharp/Polyplug.Guest.csproj → not present
- grep "AllowUnsafeBlocks" host-libs/csharp/Polyplug.csproj → not present
- grep "AllowUnsafeBlocks" showcase/plugins/csv_encoder/CsvEncoder.csproj → not present
- Generated Init.cs contains exactly one unsafe { } block — verified
- Generated .csproj has <AllowUnsafeBlocks>true</AllowUnsafeBlocks> — verified
- All existing C# integration tests pass — verified
- cargo test --workspace passes
- dotnet build succeeds on guest-libs/csharp/ with zero warnings
- dotnet build succeeds on host-libs/csharp/ with zero warnings
```

---

## Epic 23 — Hardening and Safety

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
All architectural decisions for this epic are fully pre-answered below.
Do not ask questions — implement exactly as specified.

---

READ FIRST
- AGENTS.md
- polyplug_prd.md — section 5 (Runtime Core), section 6 (ABI Layer),
  section 19 (Security Model), section 27 (Non-Goals)
- TRUST_MODEL.md
- crates/polyplug/src/allocator/tracking/mod.rs
- crates/polyplug/src/ffi/mod.rs
- crates/polyplug/src/loader/mod.rs  (contract name UTF-8 extraction)
- crates/polyplug/src/registry/mod.rs
- tests/stress_memory/mod.rs
- tests/stress_error/mod.rs

---

GOAL

Fix three production code bugs (items 1–3 below) and add missing test coverage
for five correctness gaps (items 4–7). One item (plugin segfault isolation)
is explicitly a non-goal — document it only.

This epic touches no public API and no ABI. Pure internal hardening.

---

PRE-ANSWERED DECISIONS

─────────────────────────────────────────────────────────────
FIX 1 — from_utf8_unchecked → from_utf8 for contract names
─────────────────────────────────────────────────────────────

PRIORITY: Highest. This is active UB in production code today.

Location: crates/polyplug/src/loader/mod.rs (and any other site that calls
from_utf8_unchecked on data originating from a plugin binary).

Find every call to std::str::from_utf8_unchecked (or as_str_unchecked, or
any other unchecked UTF-8 conversion) where the input comes from:
- A plugin's exported symbol name
- A contract name or bundle name extracted from a plugin's manifest or binary
- Any StringView passed from a plugin (i.e. originating from untrusted code)

Replace with std::str::from_utf8(...).map_err(|_| PolyplugError::InvalidUtf8 {
    context: "contract name",  // descriptive context string
})?

PolyplugError::InvalidUtf8 { context: String } variant must be added if not
already present. It is a hard load-time error — the bundle fails to load.

IMPORTANT DISTINCTION:
- StringView passed FROM HOST TO PLUGIN: host-owned, trusted, from_utf8_unchecked
  is acceptable (SAFETY comment required explaining why).
- StringView passed FROM PLUGIN TO HOST: untrusted, must use from_utf8.
- Contract names extracted from plugin binary at load time: untrusted, must
  use from_utf8.

Add SAFETY comments to every remaining from_utf8_unchecked explaining why
it is safe (i.e. it is host-owned data, not plugin-provided).

─────────────────────────────────────────────────────────────
FIX 2 — Null pointer checks in all C facade FFI functions
─────────────────────────────────────────────────────────────

Location: crates/polyplug/src/ffi/mod.rs

Every FFI function that takes a pointer parameter must null-check it at entry.
A null pointer must return a defined error, never cause UB.

Rules per function:

polyplug_runtime_new: returns null on allocation failure — already correct if
  Box::new is used (panics on OOM in Rust, acceptable). No inbound pointers.

polyplug_runtime_free(rt: *mut OpaqueRuntime):
  if rt.is_null() { return; }  // free(null) is a no-op by C convention

polyplug_load_bundle(rt, path, path_len):
  if rt.is_null() || path.is_null() { write last_error; return non-zero; }

polyplug_load_bundle_opts: same as above

polyplug_find_by_contract(rt, contract_id, min_version):
  if rt.is_null() { return NULL_HANDLE (u64::MAX); }

polyplug_find_by_bundle: same

polyplug_find_all_by_contract(rt, contract_id, min_version, out, out_cap):
  if rt.is_null() { return 0; }
  if out.is_null() && out_cap > 0 { write last_error; return 0; }
  // out.is_null() with out_cap == 0 is valid — caller probing for count.
  // Implement: if out_cap == 0, return total count without writing.

polyplug_resolve_plugin(rt, handle):
  if rt.is_null() { return null ptr; }
  if handle == NULL_HANDLE (u64::MAX) { return null ptr; }
  // null handle → null guard — not an error, caller checks return

polyplug_guard_free(guard):
  if guard.is_null() { return; }  // no-op, matches free(null) convention

polyplug_get_vtable(guard):
  if guard.is_null() { return null ptr; }

polyplug_reload_bundle(rt, path, path_len):
  if rt.is_null() || path.is_null() { write last_error; return non-zero; }

polyplug_last_error(rt, out, out_cap):
  if rt.is_null() || out.is_null() { return 0; }

─────────────────────────────────────────────────────────────
FIX 3 — Double-free detection in TrackingAllocator (debug builds)
─────────────────────────────────────────────────────────────

Location: crates/polyplug/src/allocator/tracking/mod.rs

Current state: allocator tracks live allocations to detect leaks (alloc adds
to set, free removes from set). Double-free is not detected.

Fix: when free() is called on a pointer not in the live set, panic with a
descriptive message including the pointer address. This is debug-only behavior
(cfg(debug_assertions) or a feature flag — use cfg(debug_assertions)).

Implementation:
  fn dealloc(&self, ptr: *mut u8, layout: Layout) {
      #[cfg(debug_assertions)]
      {
          let addr = ptr as usize;
          let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
          if !live.remove(&addr) {
              panic!(
                  "TrackingAllocator: double-free detected at address {:#x}",
                  addr
              );
          }
      }
      self.inner.dealloc(ptr, layout);
  }

Also: add ASan support in CI.
  In .cargo/config.toml (or CI workflow):
    [target.x86_64-unknown-linux-gnu]
    rustflags = ["-Z", "sanitizer=address"]
  This is a nightly-only flag — add a separate CI job:
    cargo +nightly test -Z build-std --target x86_64-unknown-linux-gnu
  Document in BENCHMARKS.md or a new CI.md that ASan job exists.

─────────────────────────────────────────────────────────────
TEST 4 — Null pointer tests for all FFI functions
─────────────────────────────────────────────────────────────

New test file: tests/integration_ffi_null/mod.rs

Test cases — one per null scenario from Fix 2:
  a. polyplug_runtime_free(null) → no crash, no panic
  b. polyplug_load_bundle(null rt, ...) → non-zero return, last_error set
  c. polyplug_load_bundle(rt, null path, ...) → non-zero return
  d. polyplug_find_all_by_contract(rt, ..., null out, cap=0) → returns count
  e. polyplug_find_all_by_contract(rt, ..., null out, cap=5) → non-zero / 0
  f. polyplug_resolve_plugin(rt, NULL_HANDLE) → returns null ptr, no panic
  g. polyplug_guard_free(null) → no crash
  h. polyplug_get_vtable(null guard) → returns null ptr, no panic
  i. polyplug_last_error(null rt, ...) → returns 0, no crash

Each test calls the C facade directly via unsafe extern "C" fn calls.
Tests run in a single process — isolation via careful ordering.

─────────────────────────────────────────────────────────────
TEST 5 — Invalid UTF-8 from plugin
─────────────────────────────────────────────────────────────

New test file: tests/integration_invalid_utf8/mod.rs

Approach: create a test fixture that is a valid .so but exports a contract
name that contains invalid UTF-8 bytes. Load it. Assert:
  - load_bundle returns Err(PolyplugError::InvalidUtf8 { .. })
  - Runtime remains healthy after rejection (can load valid plugins after)

Fixture construction: build a small Rust plugin with a deliberately mangled
contract name embedded as raw bytes (use include_bytes! or a build.rs that
patches bytes post-compilation). Alternatively: construct a minimal valid ELF
.so in the test itself using object crate or raw bytes — keep it simple.

Simplest approach: write a tiny C file with:
  const char* _polyplug_contract_name = "\xff\xfe invalid utf8";
Compile it in build.rs and commit the .so as a test fixture.

─────────────────────────────────────────────────────────────
TEST 6 — Malformed binary tests
─────────────────────────────────────────────────────────────

New test file: tests/integration_malformed/mod.rs

Test cases — all should return clean Err, never panic:
  a. Truncated .so (valid ELF header, truncated body):
     Create by truncating tests/fixtures/test_plugin.so at byte 512.
     load_bundle → Err, error message mentions load failure.

  b. Wrong magic bytes (not ELF):
     Create a file containing b"NOTANELF\x00" * 100.
     load_bundle → Err.

  c. Missing init symbol:
     Build a tiny Rust cdylib that exports NO symbols (empty lib.rs).
     load_bundle → Err(PolyplugError::MissingSymbol { symbol: "init" })
     or similar — whatever the current error type is for missing symbols.

  d. Valid ELF but init has wrong signature (returns wrong type):
     Build a Rust cdylib that exports:
       pub extern "C" fn init() -> u64 { 42 }
     polyplug calls init(registrar, ctx) — ABI mismatch.
     This is technically UB but in practice the call will misfire.
     Document the expected behavior: undefined, out of scope for now.
     Skip this sub-case — add a comment explaining why it's untestable safely.

  e. Manifest present but .so file missing from bundle directory:
     Create a bundle directory with manifest.toml pointing to nonexistent.so.
     load_bundle → Err with clear "file not found" message.

  f. Manifest with unknown runtime value:
     bundle.toml with runtime = "cobol".
     load_bundle → Err(PolyplugError::UnknownRuntime { runtime: "cobol" })

Fixture construction for a/b/e/f: done inline in test (write bytes to tmpdir).
Fixture for c: build a new Rust cdylib fixture tests/fixtures/no_init_plugin/.
Fixture for d: documented as skipped.

─────────────────────────────────────────────────────────────
TEST 7 — Quiescence timeout
─────────────────────────────────────────────────────────────

New test file: tests/integration_quiescence/mod.rs

Test the existing 5-second quiescence timeout during hot-reload.

Setup:
  1. Load reload_plugin_v1 fixture.
  2. Spawn a thread that calls resolve_plugin, holds the guard, and sleeps
     for 6 seconds (longer than timeout) WITHOUT dropping the guard.
  3. Main thread calls reload_bundle() with reload_plugin_v2.
  4. Assert: reload_bundle returns Err(PolyplugError::QuiescenceTimeout)
     (or whatever the current error type is — read the source).
  5. Join the thread (let it drop the guard).
  6. Call reload_bundle again — assert it succeeds this time.
  7. Assert: calls now use v2 vtable.

This test takes ~5 seconds to run. Mark it with #[ignore] so it does not run
in default `cargo test`. Run via `cargo test -- --ignored` in CI only.
Add a comment: // Takes ~5s — run with `cargo test -- --ignored`

─────────────────────────────────────────────────────────────
TEST 8 — Embedded null bytes in StringView
─────────────────────────────────────────────────────────────

New test file: tests/integration_stringview_nulls/mod.rs

Test that StringView with embedded null bytes round-trips correctly through
all layers that accept StringView — polyplug never treats it as a C string.

Test cases:
  a. Create StringView { ptr: b"hello\x00world".as_ptr(), len: 11 }
     Pass it through host_alloc/host_free cycle — assert no truncation.
  b. If any codepath does ptr+len → CString conversion, it must use
     explicit length — verify by inspection and add a lint comment.
  c. In generated code: StringView passed as a contract function parameter
     with embedded null — assert the receiving plugin sees the full 11 bytes.

─────────────────────────────────────────────────────────────
TEST 9 — Double-free test (uses enhanced TrackingAllocator from Fix 3)
─────────────────────────────────────────────────────────────

New test in tests/stress_memory/mod.rs (add to existing file):

  #[test]
  #[should_panic(expected = "double-free")]
  #[cfg(debug_assertions)]
  fn test_double_free_detected() {
      // Allocate via host_alloc, free twice, assert panic
      let runtime = /* build test runtime with TrackingAllocator */;
      let ptr = runtime.alloc(64);
      runtime.free(ptr);
      runtime.free(ptr);  // should panic with "double-free detected"
  }

─────────────────────────────────────────────────────────────
NON-GOAL — Plugin segfault isolation
─────────────────────────────────────────────────────────────

A plugin that calls an invalid function pointer, dereferences a null pointer,
or causes a SIGSEGV takes down the entire host process. This is expected and
intentional behavior in polyplug's trust model.

Isolating plugin crashes requires either:
  - Out-of-process execution (IPC): violates the zero-overhead hot path goal
  - OS-level sandboxing (seccomp, pledge): platform-specific, adds complexity

Neither is acceptable for v1. Document this explicitly:

Update TRUST_MODEL.md:
  Add section: "Plugin crash isolation"
  Content: plugins run in-process. A plugin segfault kills the host.
  This is by design. App developers who need crash isolation should run
  plugins in a separate worker process and communicate via IPC — polyplug
  does not provide this. See section 27 (Non-Goals) in the PRD.

Update PRD section 27 (Non-Goals):
  Add: "Plugin crash isolation (segfault in plugin kills host — by design,
  see TRUST_MODEL.md)"

---

VERIFICATION CHECKLIST

- grep -rn "from_utf8_unchecked" crates/ → only appears with SAFETY comments
  explaining host-owned data; zero occurrences on plugin-provided data
- grep -rn "from_utf8_unchecked" crates/polyplug/src/loader/ → zero results
- PolyplugError::InvalidUtf8 variant exists and is returned on bad plugin UTF-8
- polyplug_runtime_free(null) → no crash (test passes)
- polyplug_resolve_plugin(rt, NULL_HANDLE) → null return, no panic (test passes)
- polyplug_find_all_by_contract(rt, ..., null, 0) → returns count (test passes)
- TrackingAllocator double-free test panics with "double-free" in debug builds
- TrackingAllocator double-free test does NOT run/panic in release builds
- Truncated .so → Err, no panic (test passes)
- Missing init symbol → Err with symbol name in message (test passes)
- Nonexistent bundle file → Err with clear message (test passes)
- Unknown runtime → Err (test passes)
- Invalid UTF-8 contract name → Err(InvalidUtf8), runtime healthy after (test passes)
- StringView with embedded null bytes round-trips correctly (test passes)
- Quiescence timeout test is marked #[ignore], passes when run with --ignored
- TRUST_MODEL.md updated with plugin crash isolation section
- PRD section 27 updated with segfault non-goal
- No .unwrap() added in production code
- clippy passes with zero warnings
- cargo test --workspace passes
- cargo test --workspace -- --ignored passes (quiescence test)
```