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
Do not write the plan until you have interviewed me and I have answered your questions.

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

polyplug-dotnet uses Microsoft's hostfxr native API to:
1. Initialize the CLR once per process (shared across all .NET plugins)
2. Load the managed assembly
3. Obtain a native fn ptr to the [UnmanagedCallersOnly] Init method
4. Call Init(registrar) — identical vtable exchange from here

The crate scaffold exists from Epic 8 at crates/polyplug-dotnet/.
This epic fills it in completely.

This epic also covers:
- C# guest lib (guest-libs/csharp/)
- C# host lib (host-libs/csharp/)
- CSharpGenerator in polyplugc

---

EPIC GOAL

1. polyplug-dotnet crate (crates/polyplug-dotnet/):
   DotnetLoader fully implementing BundleLoader:
   - runtime_name returns "dotnet"
   - Locates hostfxr: checks DOTNET_ROOT, PATH, well-known install paths
   - Initializes CLR via hostfxr_initialize_for_runtime_config (once per process)
   - Loads managed assembly via load_assembly_and_get_function_pointer
   - Obtains fn ptr to Init method marked [UnmanagedCallersOnly]
   - Calls Init(registrar) — vtable exchange identical to native path
   - CLR shared across all .NET plugins loaded in same process
   - Proper PolyplugError variants for every failure mode:
     hostfxr not found, CLR init failed, assembly not found, Init symbol missing

2. C# guest lib (guest-libs/csharp/):
   - ABI types as [StructLayout(LayoutKind.Sequential)] structs
   - StringView and Buffer as ref structs (stack-only, no GC)
   - PluginRegistrar P/Invoke wrappers
   - PluginError type
   - [UnmanagedCallersOnly] entry point infrastructure

3. C# host lib (host-libs/csharp/):
   - P/Invoke declarations for all runtime C ABI functions
   - Polyplug.Runtime class with builder pattern matching PRD section 8
   - ref struct wrappers for StringView and Buffer

4. CSharpGenerator in polyplugc
   (crates/polyplugc/src/generators/csharp/mod.rs — new file):

   From --api api.toml:
   - generated/host/Types.cs         domain types as ref structs
   - generated/host/Callers.cs       contract caller classes
   - generated/guest/Types.cs        same domain types
   - generated/guest/Contracts.cs    interfaces for plugin developer to implement

   From --bundle bundle.toml:
   - generated/Types.cs
   - generated/Contracts.cs
   - generated/Vtables.cs            vtable construction
   - generated/Init.cs               [UnmanagedCallersOnly] Init with try/catch
   - generated/manifest.toml         runtime = "dotnet"

5. UTF-16 to UTF-8 transcoding in generated C# code:
   - ASCII fast path (strip high byte — near zero cost)
   - Full transcoding for non-ASCII
   - Transcoding buffer uses host_alloc, not managed heap

6. polyplugc generate --lang csharp wired into CLI

7. Cross-language integration tests:
   - Rust host loads standard .NET C# plugin
   - C# host (standard .NET) loads Rust plugin
   - C# host loads standard .NET C# plugin
   - UTF-16/UTF-8 round-trip: ASCII content and non-ASCII content
   - C# exception in plugin does not crash Rust host
   - GC stress test: trigger GC during plugin call, assert no corruption

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- GC stress test passes with no corruption
- UTF-16/UTF-8 round-trip passes for ASCII and non-ASCII
- C# exception does not crash Rust host
- CLR initialized once even when multiple .NET plugins are loaded
- hostfxr not found produces clear actionable error
- polyplugc generate --lang csharp produces compilable C# output
- No .unwrap() in Rust production code
- clippy passes with zero warnings

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- .NET version to target (net8.0 recommended — confirm)
- C# error model in generated code: exceptions (throws) or Result-style
- Whether async support is in scope for this epic
- How C# plugin .dll compilation is triggered in integration tests
  (MSBuild? dotnet CLI? pre-compiled fixture?)
- NuGet package publishing: in scope or just local structure for tests
- How to locate hostfxr on the CI machine used for testing
- Which comes first: host lib, guest lib, or generator — any dependency order

Do not write the plan until I have answered all questions.
```

---

## Epic 10 — polyplug-python: Python Adapter

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

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
This epic fills it in completely.

This epic also covers:
- Python guest lib (guest-libs/python/)
- Python host lib (host-libs/python/)
- PythonGenerator in polyplugc

---

EPIC GOAL

1. polyplug-python crate (crates/polyplug-python/):
   PythonLoader fully implementing BundleLoader:
   - runtime_name returns "python"
   - Initializes CPython interpreter once per process
   - Imports plugin module from bundle path
   - Calls init(registrar_ptr) passing registrar as integer/ctypes pointer
   - Python init registers vtables via ctypes calls back into C ABI
   - Python interpreter shared across all Python plugins in process
   - Proper PolyplugError variants: interpreter init failed,
     module import failed, init function missing, init raised exception

2. Python guest lib (guest-libs/python/):
   - pip package structure
   - ctypes bindings for PluginRegistrar, HostVTable, PluginVTable
   - StringView and Buffer as ctypes.Structure (C memory, not Python heap)
   - Plugin entry point decorator
   - Exception boundary: catch all Python exceptions, convert to AbiError

3. Python host lib (host-libs/python/):
   - pip package structure
   - ctypes bindings for all runtime C ABI functions
   - PluginRuntime class with builder pattern

4. PythonGenerator in polyplugc
   (crates/polyplugc/src/generators/python/mod.rs — new file):

   From --api api.toml:
   - generated/host/types.py        domain types as ctypes.Structure
   - generated/host/callers.py      contract caller classes
   - generated/host/types.pyi       type stubs
   - generated/host/callers.pyi
   - generated/guest/types.py
   - generated/guest/contracts.py   ABC per contract

   From --bundle bundle.toml:
   - generated/types.py
   - generated/contracts.py
   - generated/vtables.py           vtable construction via ctypes
   - generated/init.py              init function with try/except boundary
   - generated/manifest.toml        runtime = "python"

5. polyplugc generate --lang python wired into CLI

6. .pyi stubs generated alongside .py files for IDE support

7. Cross-language integration tests:
   - Rust host loads Python plugin
   - Python host loads Rust plugin
   - Python host loads Python plugin
   - Python exception in plugin does not crash Rust host
   - UTF-8 string round-trip test

8. Generated Python passes mypy --strict with zero errors

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- Python exception does not crash Rust host
- ctypes.Structure used for all cross-boundary types — no Python heap objects crossing
- Generated Python passes mypy --strict
- .pyi stubs generated alongside .py files
- polyplugc generate --lang python produces runnable output
- No .unwrap() in Rust production code
- clippy passes with zero warnings

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Minimum Python version to support
- pyo3 vs direct libpython for embedding (discuss tradeoffs)
- ctypes vs cffi preference (discuss tradeoffs)
- Whether Python plugins are always .py files or can be compiled extensions
- How Python interpreter is located at runtime
  (PYTHONHOME, PATH, system default, configurable?)
- One shared interpreter per process or one per bundle (discuss tradeoffs)
- pip package publishing: in scope or just local structure for tests
- How Python plugin packaging works in the integration test suite

Do not write the plan until I have answered all questions.
```

---

## Epic 11 — polyplug-lua: Lua Adapter

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step without
making any architectural decisions. Every ambiguity must be resolved in the plan.
Do not write the plan until you have interviewed me and I have answered your questions.

---

READ FIRST
- AGENTS.md — every rule applies
- polyplug_prd.md — section 10 (polyplug-lua subsection),
  section 15 (Memory Model, Lua entries), section 16 (Error Handling, Lua entry)

---

PROJECT CONTEXT

LuaJIT is strongly recommended over standard Lua for near-native performance
via JIT compilation and its FFI library which enables zero-copy struct passing.

The crate scaffold exists from Epic 8 at crates/polyplug-lua/.
This epic fills it in completely.

This epic also covers:
- Lua guest lib (guest-libs/lua/)
- Lua host lib (host-libs/lua/)
- LuaGenerator in polyplugc

---

EPIC GOAL

1. polyplug-lua crate (crates/polyplug-lua/):
   LuaLoader fully implementing BundleLoader:
   - runtime_name returns "lua"
   - Initializes Lua VM via mlua crate
   - Loads plugin script from bundle path
   - Calls init(registrar_ptr) passing registrar as lightuserdata
   - Lua init registers vtables via FFI calls back into C ABI
   - Proper PolyplugError variants: VM init failed, script load failed,
     init function missing, init raised error

2. Lua guest lib (guest-libs/lua/):
   - polyplug_guest.lua file
   - LuaJIT FFI declarations for PluginRegistrar, HostVTable, PluginVTable
   - StringView and Buffer as FFI cdata structs (zero-copy via LuaJIT FFI)
   - Plugin entry point registration helper
   - Error boundary using pcall at ABI level

3. Lua host lib (host-libs/lua/):
   - polyplug.lua + C extension .so
   - LuaJIT FFI declarations for all runtime C ABI functions
   - PluginRuntime table with builder pattern

4. LuaGenerator in polyplugc
   (crates/polyplugc/src/generators/lua/mod.rs — new file):

   From --api api.toml:
   - generated/host/types.lua       domain type constructors via FFI
   - generated/host/callers.lua     contract caller tables
   - generated/guest/types.lua
   - generated/guest/contracts.lua  contract interface tables

   From --bundle bundle.toml:
   - generated/types.lua
   - generated/contracts.lua
   - generated/vtables.lua          vtable construction via FFI
   - generated/init.lua             init function with pcall boundary
   - generated/manifest.toml        runtime = "lua"

5. polyplugc generate --lang lua wired into CLI

6. Cross-language integration tests:
   - Rust host loads Lua plugin
   - Lua host loads Rust plugin
   - Lua host loads Lua plugin
   - Lua error() in plugin does not crash Rust host
   - LuaJIT performance test: call overhead within 2x of native baseline
     from BENCHMARKS.md

---

VERIFICATION CHECKLIST

- All cross-language tests pass
- Lua error does not crash Rust host
- LuaJIT FFI used for zero-copy struct passing (verified by code inspection)
- polyplugc generate --lang lua produces runnable output
- LuaJIT performance test: overhead within 2x of native baseline
- No .unwrap() in Rust production code
- clippy passes with zero warnings

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- LuaJIT vs standard Lua: final decision
- mlua crate configuration for LuaJIT
- One VM per process or one per bundle (discuss tradeoffs)
- LuaJIT FFI vs classic C userdata for struct passing
- How Lua plugins are structured as bundles
  (single .lua file? directory with manifest.toml? what does the file field contain?)
- How the Lua host lib C extension is built and distributed for tests
- Any Lua-specific performance requirements beyond the 2x baseline

Do not write the plan until I have answered all questions.
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

The BundleLoader trait and all five loader types (native + three adapters) exist.
The manifest.toml runtime field is read by the parser (from Epic 8).
This epic wires everything together into a complete discovery pipeline.

---

EPIC GOAL

1. Directory scanner in crates/polyplug/src/loader/ (new submodule or extend existing):
   - Scans configured directories for bundle files
   - Recognizes .so, .dll, .dylib for compiled native bundles
   - Recognizes directories containing manifest.toml for script bundles (Python, Lua)
   - Finds companion manifest.toml for each compiled bundle
   - NEVER calls any loader or dlopen during scanning phase
   - Returns Vec<(bundle_path, ManifestData)>

2. Manifest reader:
   - Reads manifest.toml from disk
   - Parses into ManifestData with explicit types on all fields:
     name: String, version: String, runtime: String, file: String,
     provides: Vec<String>, requires: Vec<String>,
     function_count: HashMap<String, u32>
   - Skips malformed manifests with warning, does not abort entire scan
   - Logs which manifests were skipped and why

3. Full capability graph resolution across multiple discovered bundles:
   - Extends or replaces graph module to work across multiple bundle manifests
   - Collects all provides from all ManifestData
   - Validates all requires are satisfied
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
Do not write the plan until you have interviewed me and I have answered your questions.

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

All five language generators must be updated to emit extension query code
in generated guest init() when the bundle.toml optional list includes extensions.

---

EPIC GOAL

1. Extension trait in crates/polyplug/src/ (new module extensions/mod.rs):
   pub trait Extension: Send + Sync {
       fn extension_id(&self) -> u32;
       fn as_vtable_ptr(&self) -> *const ();
   }

2. Extension registry in PluginRuntime:
   - Stores Box<dyn Extension> entries indexed by extension_id
   - PluginRuntime::builder().extension(impl Extension) registration
   - get_extension(id) implementation returns stored ptr or null if absent
   - Thread-safe reads (RwLock or equivalent)

3. Trace extension (built-in reference implementation) in crates/polyplug/:
   - EXT_TRACE_ID: u32 = 1  (or discuss constant value with me)
   - TraceVTable as #[repr(C)] struct:
     pub struct TraceVTable {
         pub emit: unsafe extern "C" fn(msg: StringView),
     }
   - TraceExtension { vtable: TraceVTable } implementing Extension
   - Constructor takes callback: TraceExtension::new(callback: impl Fn(&str) + Send + Sync)
   - as_vtable_ptr() returns &self.vtable as *const _ as *const ()

4. Custom extension support demonstrated in tests:
   - CounterExtension with get_count() → u64 and increment()
   - Shows app developer can define arbitrary extensions

5. Generator updates — all five languages emit extension query code:
   When bundle.toml plugin optional list includes "trace":
   Generated init() queries EXT_TRACE_ID and stores ptr if present.
   Null check pattern per language (idiomatic, not verbose):
     Rust:   if let Some(trace) = ...
     C++:    if (trace_ptr != nullptr)
     C#:     if (tracePtr != IntPtr.Zero)
     Python: if trace_ctypes_ptr:
     Lua:    if trace_ptr ~= nil then

6. Tests across all five languages:
   - Trace: plugin emits messages, host callback receives them in order
   - Absent trace: plugin with trace in optional list but no TraceExtension
     registered → plugin loads and runs correctly, no crash
   - Custom: CounterExtension passes data from host to plugin correctly
   - Unregistered ID: get_extension for unknown ID returns null safely
   - Benchmark: compare call with absent extension vs baseline — confirms zero overhead

---

VERIFICATION CHECKLIST

- All extension tests pass across all five languages
- Absent extension never causes crash in any language
- Trace extension test passes for all five languages
- CounterExtension custom test passes
- Benchmark confirms absent extension overhead is zero
- No .unwrap() in production code
- clippy passes with zero warnings
- All existing integration tests still pass

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- How extension IDs are assigned: hardcoded constants, hash of name, or sequential
- EXT_TRACE_ID value: 1, or some other convention
- Whether extensions need versioning at this stage
- Thread safety requirements: can plugins call extension functions concurrently
- Whether trace should integrate with the Rust tracing crate or use a raw callback
- Whether extension query code is always generated or only when listed in optional
- Whether all five language generators need updating in this epic or only some
  are ready (check which generators are complete from previous epics)
- Any other built-in extensions beyond trace in scope for this epic

Do not write the plan until I have answered all questions.
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

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Whether Version is already defined in the IR or runtime, or new here
- Exact semver subset: major.minor only, or full major.minor.patch
- Warning mechanism for Relaxed mode
- Whether VersionMismatch and FunctionCountMismatch should be separate
  error variants or combined
- Whether host-side api.toml version is stored in the IR and available
  at load time, or needs threading through
- Any versioning edge cases from your specific use cases

Do not write the plan until I have answered all questions.
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
   For each of the five generators (Rust, C++, C#, Python, Lua):
   - Verify host-side output: types, callers
   - Verify guest SDK output: types, contracts
   - Verify guest bundle output: types, contracts, vtables, init, manifest.toml
   - Fix any gaps or inconsistencies found
   The planner must ask me what gaps were identified in previous epics.

2. Consistent output conventions across all generators:
   - All generated files have "THIS FILE IS AUTO-GENERATED BY polyplugc" header
   - manifest.toml always has: name, version, runtime, file, provides,
     requires, function_count
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
   - Rust: crate directory (Cargo.toml, src/, ready for cargo publish)
   - C++: header directory with single-include entry point
   - C#: NuGet package directory structure
   - Python: pip package directory (pyproject.toml, package dir)
   - Lua: module directory (.lua files + C ext if needed)

5. The 25 cross-language combination tests:
   For every pair (host_lang, guest_lang) in {Rust, C++, C#, Python, Lua}²:
   - Generate host callers for host_lang from test api.toml
   - Generate guest bundle for guest_lang from test bundle.toml
   - Build both (discuss build orchestration with me)
   - Rust host loads plugin: runtime.load_bundle(path)
   - Call at least two contract functions
   - Assert correct return values
   All 25 combinations must pass.

---

VERIFICATION CHECKLIST

- All 25 cross-language combination tests pass
- Generated code for all five languages compiles without warnings
- All generated files have auto-generated header comment
- manifest.toml always has all required fields for all five generators
- Incremental: schema change → regeneration; no change → skip
- polyplugc pack produces valid package structure for all five languages
- No .unwrap() in polyplugc production code
- clippy passes with zero warnings
- cargo test --workspace passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Known generator gaps or failing tests from each language's epic
- File naming conventions that differ between languages (confirm per language)
- How the 25 combination tests are organized in the test suite
  (one test file, parameterized, or per-combination files)
- How non-Rust plugin compilation is handled in the test suite
  (pre-built fixtures? build script triggered by build.rs? CI only?)
- Whether polyplugc pack is in scope for this epic or deferred to post-showcase
- Any generator output inconsistencies already noticed

Do not write the plan until I have answered all questions.
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

Five plugins, one per language:
  Rust:   Decoder    — parses CSV bytes into DataRecord
  C++:    Transformer — uppercases all string fields
  C#:     Encoder    — serializes DataRecord back to CSV bytes
  Python: Reporter   — formats a human-readable summary string
  Lua:    Transformer — reverses all string fields (alternative transformer)

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
```