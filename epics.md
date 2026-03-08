Looking at your current state, here's what's already done:

- ✅ Epic 1 (Project Skeleton)
- ✅ Epic 2 (IR and Schema Parser)
- ✅ Epic 3 (Core ABI Structs)
- ✅ Epic 4 (Runtime Core)
- ✅ Epic 7 partial (C++ host/guest libs exist)
- 🔄 Epic 5 partial (Rust generator exists, C++ generator stub exists, but codegen-implementation.md not done)

So you need epics starting from completing codegen. Here are the revised planner prompts:

---

## Epic 5 — Complete Rust and C++ Code Generators

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies to every file the executer will touch.

Current project state:
- polyplug-runtime crate: fully implemented (ABI, allocator, error, graph, loader, registry, runtime)
- polyplugc crate: IR and parser implemented, Rust and C++ generator stubs exist
- C++ host-libs and guest-libs: implemented under host-libs/cpp and guest-libs/cpp
- Integration tests exist for dispatch, graph, and load
- codegen-implementation.md plan exists but has NOT been executed yet

The executer will be working primarily in:
- crates/polyplugc/src/generators/rust/mod.rs
- crates/polyplugc/src/generators/cpp/mod.rs
- crates/polyplugc/src/main.rs (CLI wiring)
- tests/ (new integration tests)

---

EPIC GOAL

Complete the Rust and C++ code generators in polyplugc so that:

1. `polyplugc generate --api api.toml --lang rust --out ./out` produces:
   - Domain types as #[repr(C)] Rust structs
   - Contract caller wrappers for the host side
   - Contract traits for the guest side (pure Rust, no unsafe, no ABI)
   - Generated ABI wrapper functions (panic::catch_unwind, caller-provides-buffer)
   - Generated PluginVTable construction
   - Generated init() bundle entry point
   - Generated manifest.toml

2. `polyplugc generate --api api.toml --lang cpp --out ./out` produces:
   - Domain types as C++ structs with static_assert layout checks
   - Contract caller classes for the host side
   - Abstract base classes per contract for guest side
   - Generated ABI wrapper functions (exception boundary)
   - Generated vtable construction
   - Generated init() entry point
   - Generated manifest.toml

3. `polyplugc validate --api api.toml` works correctly
4. `polyplugc validate --bundle bundle.toml` works correctly

---

PLAN REQUIREMENTS

The plan the planner produces must include:

1. Exact file paths for every file to be created or modified
2. Exact function signatures for every new function
3. Code generation rules to apply (these are non-negotiable):
   - Non-primitive params always passed by reference
   - Non-primitive returns use caller-provides-buffer (hidden from developer-facing API)
   - Primitive returns (u8-u64, f32, f64, bool) returned directly by value
   - Every ABI function wrapped in panic::catch_unwind on guest side
   - All generated files have auto-generated header comment
   - No .unwrap() in any generated or generator production code
4. Integration test plan:
   - End-to-end test: write api.toml → generate Rust → compile plugin → load with runtime → call → assert
   - End-to-end test: write api.toml → generate C++ → compile plugin → load with runtime → call → assert
   - Cross-language test: C++ host loads Rust plugin
   - Cross-language test: Rust host loads C++ plugin
   - Panic isolation test: plugin panic does not crash host (both languages)
5. Verification checklist the executer must pass before marking the epic complete:
   - All integration tests pass
   - Generated Rust compiles with clippy -D warnings zero warnings
   - Generated C++ compiles with -Wall -Wextra -Werror
   - No .unwrap() anywhere (grep check)
   - cargo test --workspace passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Before writing the plan, ask me about:
- Exact naming conventions for generated types, traits, and functions
- Whether generated Rust guest traits should use Result<T, PluginError> or a custom error type
- C++ error handling preference: std::expected, exceptions, or error codes
- Whether the CLI validate subcommand is already wired or needs wiring
- Any constraints from the existing generator stubs you should know about
- Anything in codegen-implementation.md that conflicts with the PRD decisions

Do not write the plan until I have answered your questions.
```

---

## Epic 6 — Memory and Error Model Hardening

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- Runtime core is fully implemented
- Rust and C++ generators are complete and tested
- Cross-language Rust↔C++ tests pass

This epic adds NO new features. It hardens what exists before more languages are added.
Fixing memory or error model bugs now costs 1x. Fixing after 3 more languages costs 4x.

---

EPIC GOAL

1. Memory model stress tests covering:
   - Large Buffer across boundary, plugin fills, host reads correctly
   - StringView with non-ASCII UTF-8 content
   - Zero-length Buffer and StringView
   - Multiple concurrent plugin calls with no shared memory
   - Plugin allocates memory, returns to host, host frees — no leak
   - Caller allocates output buffer, plugin fills, freed after use — no leak

2. Error model stress tests covering:
   - Plugin returns non-zero error code — host receives correct code and message
   - Plugin panics — host receives ABI_ERROR_PANIC, continues running
   - Chain: Plugin A calls Plugin B, B errors, A propagates error to host
   - Error message StringView lifetime is valid when host reads it

3. Performance baselines using criterion:
   - No-op function call through vtable (measures pure dispatch overhead)
   - Function call with Buffer argument
   - Function call with struct argument and struct return
   - Cross-plugin call through dispatcher
   - Results documented in BENCHMARKS.md

4. Memory leak detection:
   - Custom tracking allocator that counts alloc/free
   - Every test asserts alloc count equals free count at end
   - Zero tolerance

5. Fix any issues found — changes may touch runtime, generators, or ABI layer

---

PLAN REQUIREMENTS

The plan must include:
1. Exact location of every new test (which file under tests/)
2. Exact structure of the tracking allocator
3. Criterion benchmark setup — exact benchmark names and what each measures
4. BENCHMARKS.md template the executer fills in with real numbers
5. Decision: if a bug is found in the runtime or ABI layer, the plan must describe
   how to fix it without breaking existing tests
6. Verification checklist:
   - All stress tests pass
   - All memory leak tests show zero leaks
   - Benchmarks run and BENCHMARKS.md is populated with real numbers
   - cargo test --workspace passes
   - No .unwrap() in production code
   - clippy passes with zero warnings

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Whether criterion is already in the workspace or needs adding
- Any memory issues already observed in existing tests
- Thread safety requirements — is concurrent plugin calling already tested
- Whether valgrind or address sanitizer should be part of the plan
- Any specific error scenarios that concern you based on the current implementation

Do not write the plan until I have answered your questions.
```

---

## Epic 7 — C# Generator and Host/Guest Libs

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- Runtime core fully implemented
- Rust and C++ generators complete and hardened
- host-libs/cpp and guest-libs/cpp exist and are tested

C# is the hardest language in the MVP. Reasons:
- GC must never see cross-boundary data
- Strings are UTF-16 internally, ABI requires UTF-8
- Marshaling requires explicit struct layout control
- ref structs enforce stack-only allocation for cross-boundary types

---

EPIC GOAL

1. C# host lib at host-libs/csharp/:
   - P/Invoke declarations for all runtime C ABI functions
   - PluginRuntime class with builder pattern
   - ref struct wrappers for StringView and Buffer (stack-only, no GC)
   - [StructLayout(LayoutKind.Sequential)] on all ABI-visible structs
   - UTF-16 to UTF-8 transcoding with ASCII fast path

2. C# guest lib at guest-libs/csharp/:
   - Bundle entry point attribute
   - Unmanaged memory helpers (host_alloc backed, not managed heap)
   - Exception boundary: catch all managed exceptions, convert to AbiError

3. CSharpGenerator in polyplugc at crates/polyplugc/src/generators/csharp/mod.rs:
   HOST SIDE:
   - Domain types as ref structs wrapping unmanaged pointers
   - Properties reading/writing directly to unmanaged memory (no copy)
   - Contract caller classes

   GUEST SIDE:
   - Same domain types
   - Interface per contract
   - Generated ABI wrapper methods
   - Generated vtable construction
   - Generated bundle registration
   - Generated manifest.toml

4. polyplugc generate --lang csharp support wired into CLI

5. Cross-language integration tests:
   - C# host loads Rust plugin
   - Rust host loads C# plugin
   - C# host loads C# plugin
   - UTF-8/UTF-16 string round-trip including non-ASCII content
   - C# exception in plugin does not crash Rust host
   - GC stress test: trigger GC during plugin call, assert no corruption

---

PLAN REQUIREMENTS

The plan must include:
1. Exact directory structure for host-libs/csharp/ and guest-libs/csharp/
2. .NET project file structure (.csproj)
3. Exact P/Invoke signatures for every C ABI function
4. UTF-16 to UTF-8 transcoding algorithm with ASCII fast path explicitly described
5. How the GC stress test will be implemented
6. How C# plugin compilation is integrated into the test suite
7. Exact ref struct definitions for StringView and Buffer
8. How exception boundary is implemented (try/catch placement, what is caught)
9. Verification checklist:
   - All cross-language tests pass
   - GC stress test passes with no corruption
   - UTF-8/UTF-16 round-trip test passes for ASCII and non-ASCII
   - No managed heap allocations for cross-boundary data (verified with tooling)
   - C# exception does not crash host
   - polyplugc generate --lang csharp produces compilable output
   - No .unwrap() in Rust production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- .NET version to target
- NativeAOT for C# plugins vs standard .NET runtime
- Exception vs Result-style error model preference in generated C# code
- Whether async support is in scope for this epic
- How C# plugin .dll compilation will be triggered in tests
- Any marshaling approaches to prefer or avoid
- Whether NuGet packaging is in scope for this epic or deferred

Do not write the plan until I have answered your questions.
```

---

## Epic 8 — Python Generator and Host/Guest Libs

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- Runtime, Rust generator, C++ generator, C# generator all complete
- host-libs and guest-libs exist for Rust, C++, C#

For Python: the Python runtime itself is the performance bottleneck, not polyplug.
The goal is zero polyplug overhead on top of Python's inherent cost.
ctypes.Structure keeps cross-boundary data in C memory, outside the Python GC entirely.

---

EPIC GOAL

1. Python host lib at host-libs/python/:
   - ctypes bindings for all runtime C ABI functions
   - PluginRuntime class with builder pattern
   - StringView and Buffer as ctypes.Structure (C memory, not Python heap)

2. Python guest lib at guest-libs/python/:
   - Plugin entry point decorator
   - ctypes.Structure base for all cross-boundary types
   - Exception boundary: catch all Python exceptions, convert to AbiError

3. PythonGenerator in polyplugc at crates/polyplugc/src/generators/python/mod.rs:
   HOST SIDE:
   - Domain types as ctypes.Structure subclasses
   - Type-annotated caller classes
   - UTF-8 encoding of Python str at boundary

   GUEST SIDE:
   - Same domain types
   - Abstract base class (ABC) per contract
   - Generated ABI wrapper functions using ctypes
   - Generated vtable construction
   - Generated bundle init function
   - Generated manifest.toml

4. polyplugc generate --lang python wired into CLI

5. .pyi stub files generated alongside .py files for IDE support

6. Cross-language integration tests:
   - Python host loads Rust plugin
   - Rust host loads Python plugin
   - Python host loads Python plugin
   - Python exception in plugin does not crash Rust host
   - UTF-8 string round-trip test

---

PLAN REQUIREMENTS

The plan must include:
1. Exact directory structure for host-libs/python/ and guest-libs/python/
2. Python package structure (pyproject.toml or setup.py)
3. Exact ctypes declarations for every C ABI function
4. How Python plugin loading works at runtime (Python interpreter initialization,
   script loading, function extraction)
5. How .pyi stubs are generated (what tool or manual generation)
6. How Python plugin compilation/packaging works for tests
7. Verification checklist:
   - All cross-language tests pass
   - Python exception does not crash host
   - ctypes.Structure used for all cross-boundary types
   - Generated Python passes mypy --strict with zero errors
   - polyplugc generate --lang python produces runnable output
   - No .unwrap() in Rust production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Minimum Python version to support
- ctypes vs cffi preference (explain tradeoffs if I am unsure)
- Whether Python plugins are interpreted scripts or compiled native extensions
- How the Python interpreter is embedded or located at runtime
- Whether pip packaging is in scope for this epic or deferred
- Type annotation style in generated Python

Do not write the plan until I have answered your questions.
```

---

## Epic 9 — Lua Generator and Host/Guest Libs

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- Runtime, Rust, C++, C#, Python generators all complete
- All host-libs and guest-libs for those languages exist and are tested

LuaJIT is strongly recommended over standard Lua for near-native performance via JIT
compilation and the FFI library which allows zero-copy struct passing.
The Lua stack model requires careful planning for struct argument passing.

---

EPIC GOAL

1. Lua host lib at host-libs/lua/:
   - .lua file + C extension (.so)
   - LuaJIT FFI declarations for runtime C ABI (if LuaJIT chosen)
   - PluginRuntime table with builder pattern
   - LuaJIT cdata structs for StringView and Buffer (zero-copy via FFI)

2. Lua guest lib at guest-libs/lua/:
   - .lua file
   - Plugin registration helpers
   - Error boundary using pcall at ABI boundary
   - lightuserdata for host allocator pointers

3. LuaGenerator in polyplugc at crates/polyplugc/src/generators/lua/mod.rs:
   HOST SIDE:
   - Domain type constructors using LuaJIT FFI cdata
   - Contract caller tables

   GUEST SIDE:
   - Domain type definitions
   - Contract interface table for plugin developer to implement
   - Generated ABI wrapper functions
   - Generated vtable construction
   - Generated bundle init
   - Generated manifest.toml

4. polyplugc generate --lang lua wired into CLI

5. Cross-language integration tests:
   - Lua host loads Rust plugin
   - Rust host loads Lua plugin
   - Lua host loads Lua plugin
   - Lua error() in plugin does not crash Rust host
   - LuaJIT performance test: call overhead within 2x of native baseline

---

PLAN REQUIREMENTS

The plan must include:
1. Exact directory structure for host-libs/lua/ and guest-libs/lua/
2. Decision on LuaJIT vs standard Lua with explicit justification
3. How the Lua interpreter is embedded in the host (via mlua crate or direct C API)
4. How structs are passed through the Lua stack without unnecessary copies
5. How the pcall error boundary is implemented at the ABI level
6. How Lua plugin loading works (file discovery, execution, function extraction)
7. LuaJIT performance test setup and pass/fail criteria
8. Verification checklist:
   - All cross-language tests pass
   - Lua error does not crash host
   - LuaJIT FFI used for zero-copy struct passing (verified by inspection)
   - polyplugc generate --lang lua produces runnable output
   - Performance test: call overhead within 2x of native baseline from BENCHMARKS.md
   - No .unwrap() in Rust production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- LuaJIT vs standard Lua final decision
- Whether to use the mlua Rust crate or raw Lua C API for embedding
- Lua version (5.1 for LuaJIT, 5.4 for standard Lua)
- How Lua plugins are packaged and distributed
- Whether LuaJIT FFI or classic userdata approach is preferred for structs
- Any Lua-specific performance requirements beyond the 2x baseline

Do not write the plan until I have answered your questions.
```

---

## Epic 10 — Plugin Discovery and Manifest System

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- All five language generators complete
- Runtime loads plugins when given explicit paths
- Basic integration tests use explicit bundle paths

This epic makes discovery automatic. The runtime must be able to find, evaluate,
and load plugins from directories without the app developer specifying each path.

---

EPIC GOAL

1. Directory scanner in polyplug-runtime:
   - Scans configured directories for bundle files
   - Recognizes .so, .dll, .dylib for compiled bundles
   - Recognizes directories with manifest.toml for script bundles (Python, Lua)
   - Finds companion manifest.toml for each bundle
   - Never calls dlopen during scanning phase
   - Returns list of (bundle_path, manifest) pairs

2. Manifest reader:
   - Reads manifest.toml efficiently
   - Parses into ManifestData struct with explicit types on all fields
   - Validates manifest format
   - Skips malformed manifests with warning, does not abort
   - Extracts: name, version, file, provides[], requires[]

3. Full capability graph resolution across multiple discovered bundles:
   - Collects all provides from all manifests
   - Validates all requires are satisfied before any dlopen
   - Detects cycles across bundle boundaries
   - Produces ordered load list (topological sort)

4. Explicit registration API alongside discovery:
   - runtime.load_bundle(path) → Result
   - runtime.load_bundle_with(path, LoadOptions) → Result

5. Multi-bundle integration tests:
   - Three bundles: A provides X, B requires X provides Y, C requires Y
     → correct load order: A then B then C
   - Missing dependency: B requires X, no bundle provides X
     → clear error before any dlopen
   - Cycle: A requires B, B requires A
     → detected, clear error naming both participants
   - All five language bundles discovered from same directory
   - Malformed manifest in one bundle does not prevent others from loading
   - Two bundles provide same contract: both load, host gets first-registered

---

PLAN REQUIREMENTS

The plan must include:
1. Exact module location for scanner and manifest reader (new files or extend existing)
2. ManifestData struct definition with all fields and explicit types
3. Exact algorithm for topological sort (Kahn's algorithm or DFS — specify which)
4. How the scanner differentiates compiled vs script bundles
5. How malformed manifest warnings are surfaced to the app developer
6. How "two bundles provide same contract" resolution is handled
   (first-registered wins — document this clearly)
7. Verification checklist:
   - All multi-bundle tests pass
   - Correct load order verified
   - Cycle detection test passes with clear error
   - Malformed manifest test: other bundles load successfully
   - No dlopen before full graph resolution (verified by test)
   - No .unwrap() in production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Manifest file naming convention: bundle_name.manifest.toml or fixed name manifest.toml
- Recursive directory scanning or single-level only
- How to handle bundles with no companion manifest file (skip silently, warn, or error)
- Platform-specific bundle file extension handling (.so vs .dll vs .dylib)
- Whether symlinks should be followed
- Whether the existing graph module needs extension or replacement for multi-bundle support

Do not write the plan until I have answered your questions.
```

---

## Epic 11 — Extension System

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point:
- All five generators complete
- Discovery system complete
- Full multi-bundle loading works

The extension system lets the runtime evolve without touching the frozen core ABI.
Extensions are optional. Plugins query them at init time and handle absence gracefully.
A plugin must never require an extension to function.

---

EPIC GOAL

1. Extension trait in polyplug-runtime:
   - Extension trait with extension_id() → u32 and as_vtable_ptr() → *const ()
   - Extension registry storing extensions by ID
   - get_extension(id) in HostVTable returning null if absent

2. Trace extension (built-in reference implementation):
   - TraceExtension implementing Extension
   - TraceVTable with emit(StringView) function pointer
   - App developer provides callback at init time
   - EXT_TRACE_ID constant
   - Zero overhead when absent (null check only)

3. Custom extension support:
   - App developer implements Extension trait for their own extensions
   - Passed to runtime at init via .extension(my_ext)
   - Example custom extension in tests: GameStateExtension with get_frame_count() → u64

4. Extension querying in generated code for all five languages:
   - Generated guest init() queries optional extensions and stores pointers
   - Trace extension usage generated automatically if trace is in bundle.toml optional list
   - Null check pattern is idiomatic per language

5. Tests:
   - Trace extension: plugin emits messages, host callback receives them correctly
   - Absent extension: plugin handles absence, no crash, correct behavior
   - Custom extension: app-defined extension passes data to plugin correctly
   - get_extension for unregistered ID returns null safely across all five languages
   - All five language plugins use trace extension successfully

---

PLAN REQUIREMENTS

The plan must include:
1. Exact Extension trait definition with all method signatures
2. How extension vtables are stored and retrieved (type erasure approach)
3. How each language generator is modified to emit extension query code
4. Null check pattern per language (Rust: if let, C++: if ptr, C#: != IntPtr.Zero, etc.)
5. TraceExtension vtable layout as a #[repr(C)] struct
6. How the custom extension test is structured end to end
7. Verification checklist:
   - All extension tests pass across all five languages
   - Absent extension never causes crash in any language
   - Trace extension test passes for all five languages
   - Custom extension test passes
   - Benchmark: absent extension adds zero measurable overhead vs baseline
   - No .unwrap() in production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- How extension IDs are assigned (enum, constants, hash of name, user-assigned u32)
- Whether extensions need versioning at this stage
- Thread safety requirements for extension calls
- Whether the trace extension should integrate with Rust tracing crate or use a raw callback
- Whether any other built-in extensions beyond trace are in scope for this epic

Do not write the plan until I have answered your questions.
```

---

## Epic 12 — Versioning and Compatibility

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point the full system works end to end. This epic hardens the version negotiation
that happens at load time between host and plugin. Previous epics assumed versions always
match. This epic enforces and tests what happens when they do not.

---

EPIC GOAL

1. Contract version negotiation at load time:
   - Host requests contract by name + minimum version
   - Plugin provides contract at a specific version
   - Compatible: provided major == required major AND provided minor >= required minor
   - Strict mode: incompatible → fail load, clear error naming contract and versions
   - Relaxed mode: incompatible → warn, load anyway
   - Yolo mode: load with no checks, no warnings

2. Function count validation:
   - manifest.toml includes function count per contract
   - Runtime validates count matches schema expectation
   - Mismatch in strict mode: fail load
   - Mismatch in relaxed mode: warn, use minimum of the two counts

3. Per-bundle compatibility override:
   - Global default set at runtime init
   - Per-bundle override via LoadOptions
   - LoadOptions { compatibility: Compatibility, ignore_function_count_mismatch: bool }

4. Compatibility tests covering every scenario:
   - v1.0 requested, v1.0 provided → compatible all modes
   - v1.0 requested, v1.2 provided → compatible all modes (superset)
   - v1.2 requested, v1.0 provided → incompatible (missing functions)
     strict: fail, relaxed: warn+load, yolo: load silently
   - v1.0 requested, v2.0 provided → incompatible (major break)
     strict: fail, relaxed: warn+load, yolo: load silently
   - Function count mismatch: each mode handled correctly
   - Per-bundle override correctly overrides global setting

5. Error messages for all incompatibility scenarios must be human-readable and name:
   contract name, required version, found version, what was incompatible

---

PLAN REQUIREMENTS

The plan must include:
1. Where version negotiation logic lives (which module in polyplug-runtime)
2. Exact Version struct definition and comparison logic
3. How function count is added to generated manifest.toml (generator changes)
4. How LoadOptions is threaded through the existing load_bundle API
5. Exact error variants added to PolyplugError for each incompatibility type
6. Test structure: one test function per compatibility scenario listed above
7. Verification checklist:
   - All compatibility scenario tests pass
   - Error messages are human-readable (reviewed manually before marking complete)
   - Per-bundle override overrides global setting in all cases
   - Function count added to manifest.toml in all five language generators
   - No .unwrap() in production code
   - clippy passes

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Whether Version is already defined in the IR or needs defining in the runtime
- Exact semver subset to support (major.minor only, or full major.minor.patch)
- Whether relaxed mode should log warnings through the trace extension or a separate mechanism
- Whether function count mismatch and version mismatch should be separate error variants
- Any versioning edge cases specific to your use cases

Do not write the plan until I have answered your questions.
```

---

## Epic 13 — Integration Showcase

```
You are the PLANNER agent for the `polyplug` project.

Your job is to create a detailed, step-by-step execution plan for the EXECUTER agent.
The plan must be granular enough that the executer can implement each step independently
without making architectural decisions. Every ambiguity must be resolved in the plan.

Before writing the plan, interview me with any questions you need answered to remove
all ambiguity. Do not guess. Do not assume. Ask first.

---

PROJECT CONTEXT

polyplug is a universal, cross-language plugin runtime platform built in Rust.
Read AGENTS.md before planning anything. Every rule applies.

At this point the full system is complete:
- All five language generators
- Discovery system
- Extension system
- Version negotiation

This epic builds a showcase that exercises every feature across all five languages.
It is NOT a game engine. It is a realistic multi-language data processing pipeline
that covers every case a real app developer would encounter.

The showcase lives at showcase/ in the project root.

---

EPIC GOAL

The showcase is a data processing pipeline. The host app runs:
  load input → decode → transform → encode → report

Implemented as four contracts:
- Decoder:     decode(Buffer) → DataRecord
- Transformer: transform(DataRecord) → DataRecord
- Encoder:     encode(DataRecord) → Buffer
- Reporter:    report(DataRecord) → StringView

One plugin per language:
- Rust plugin:   implements Decoder (parses CSV bytes into DataRecord)
- C++ plugin:    implements Transformer (uppercases all string fields)
- C# plugin:     implements Encoder (serializes DataRecord back to CSV bytes)
- Python plugin: implements Reporter (formats a human-readable summary)
- Lua plugin:    implements Transformer (reverses all string fields, alternative transformer)

The host application (language chosen with me) does:
- Initializes runtime with trace extension enabled
- Scans showcase/plugins/ directory
- Loads all five plugins
- Runs the pipeline: decode → transform (C++) → encode → report
- Then runs again with Lua transformer instead of C++
- Prints results and trace output
- Handles all errors gracefully

Every polyplug feature demonstrated:
- Cross-language calls across all five languages
- Plugin discovery via directory scanning
- Cross-plugin communication: Encoder calls Decoder to validate round-trip
- Trace extension: all plugins emit messages, host prints them
- Versioning: Python Reporter ships at contract v1.1, host requests v1.0 (compatible)
- Compatibility: Lua Transformer ships with one extra function (relaxed mode)
- Error handling: Decoder returns error for malformed input, host handles and continues

---

PLAN REQUIREMENTS

The plan must include:
1. showcase/ directory structure (all files, all languages)
2. api.toml for the four contracts with exact type definitions
3. Exact DataRecord type definition usable from all five languages
4. Build instructions per plugin (how each language's plugin is compiled)
5. How the host discovers and loads all five plugins
6. How both transformer variants are demonstrated in one run
7. How the error handling scenario is triggered and handled
8. How the version and compatibility scenarios are set up
9. README content outline: what to build, how to run, expected output
10. Test that runs the full pipeline automatically and asserts correct output
11. Verification checklist:
    - Showcase builds for all five language plugins
    - Host runs end to end with correct output
    - Both transformer variants work
    - Trace output visible
    - Error scenario handled gracefully
    - Version and compatibility scenarios behave as documented
    - cargo test --workspace still passes
    - No .unwrap() in any production code including showcase host

---

QUESTIONS TO ASK ME BEFORE PLANNING

Ask me about:
- Which language you want for the host application
- Whether DataRecord should have fixed fields or a dynamic key-value structure
- How you want the showcase run (CLI with arguments, or hardcoded demo)
- Whether the showcase should be runnable as a single cargo command or requires separate build steps per language
- Any additional scenarios you want demonstrated beyond the list above
- Whether the showcase should have its own README or be documented in the project root README

Do not write the plan until I have answered your questions.
```

---

That's 9 epics covering everything from where you are now to a complete working showcase. Each one is addressed to the planner, interviews you before planning, and gives the executer a verifiable checklist to confirm completion.
