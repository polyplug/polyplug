# Phase 19: Fix ABI Build Script + Auto-Generate SDK Structs - Context

**Gathered:** 2026-04-12
**Status:** Ready for planning
**Source:** Extended user discussion with 5-SDK investigation

<domain>
## Phase Boundary

Fix the polyplug_abi build script (extractor.rs) so it correctly reads the modular Rust source files and generates accurate struct/enum/union definitions for all 5 SDK languages (Python, C#, Lua, JS, C++). Then remove all hand-written FFI struct definitions from SDK host and guest files, replacing them with imports from the auto-generated abi.* files.

**What this phase delivers:**
1. Build script that walks the module tree to discover all #[repr(C)] types
2. Auto-generated abi.* files per SDK language with correct struct layouts
3. ast-grep integration for preserving hand-written helper method bodies
4. Deletion of all hand-written FFI structs from host/guest SDK files
5. Auto-generated layout test files per SDK
6. Removal of PluginRegistrar alias (use HostInterface directly)

</domain>

<decisions>
## Implementation Decisions

### Build Script Extractor Fix
- **D-01:** Walk the module tree recursively — parse `lib.rs` for `pub mod X;` declarations, then read and parse each sub-module file. Continue recursively into sub-modules.
- **D-02:** Auto-discover all `pub` structs/enums/unions with `#[repr(C)]` — no whitelist. Remove `ABI_TYPES` list entirely.
- **D-03:** Auto-discover `pub const` constants starting with `POLYPLUG_` — no whitelist. Remove `ABI_CONSTANTS` list.
- **D-04:** Do NOT extract functions — `fnv1a_64`, `contract_id`, etc. are Rust-only implementation details. SDKs don't need them (runtime handles ID computation; codegen pre-computes IDs as constants).
- **D-05:** Remove `ABI_FUNCTIONS` list and `create_hash_functions()` from mapper.rs entirely.
- **D-06:** Stay as `build.rs` in polyplug_abi crate. Do not move to polyplugc CLI or justfile.
- **D-07:** Track all sub-module source files for `cargo:rerun-if-changed` (not just `src/lib.rs`).
- **D-08:** Build script scans multiple crates — polyplug_abi AND loader crates (polyplug_native, polyplug_python, polyplug_lua, polyplug_js, polyplug_dotnet) — to discover loader config structs (`NativeConfig`, `PythonConfig`, etc.). No manifest/TOML files involved.
- **D-09:** Fail the build if a type cannot be represented in a target language. Clear error message, no silent skipping.

### Generated File Structure
- **D-10:** All #[repr(C)] structs auto-generated in separate `abi.*` files per SDK. SDK host/guest files import from them.
- **D-11:** Delete all `sdks/*/abi/` files that are currently broken placeholders. Codegen writes fresh files.
- **D-12:** Merge helper files into abi.* — `string_view_helper.lua`, `string_view_helper.ts`, `StringViewHelper.cs`, `StringHelpers.cs` contents merge into the generated `abi.lua`, `abi.ts`, `Abi.cs` as methods on the generated struct classes. ast-grep preserves these method bodies across regenerations.
- **D-13:** C++ output stays at `sdks/cpp/abi/polyplug/abi.hpp` — idiomatic C++ header-in-subdirectory pattern (`#include "polyplug/abi.hpp"`).

### ast-grep Integration
- **D-14:** Use ast-grep as a CLI tool (`sg` binary) — NOT a Rust library. The project already uses this pattern in `sdk_validator` crate (`AstGrepRunner`). CI already installs it.
- **D-15:** ast-grep surgically updates method signatures in abi.* files while preserving hand-written method bodies. Deleted methods get `DELETED_` prefix. `sdk_validator` fails CI if any `DELETED_` prefix found.
- **D-16:** ast-grep does NOT handle FFI function call cleanup — only struct definitions and methods. The 2 FFI exports are a manual one-time fix.

### Ordering & Migration
- **D-17:** Execution order: (1) Fix build script extractor, (2) Implement ast-grep integration, (3) Delete hand-written structs from SDK files. Safe, incremental — each step testable independently.

### SDK Validation
- **D-18:** Validation is Rust-driven — extract method names from Rust `impl` blocks, validate all SDKs have equivalent methods.
- **D-19:** Method names use convention-based auto-derivation (no config file): snake_case → PascalCase for C#, camelCase for JS, keep snake_case for Python/Lua/C++. Each codegen generator knows its own convention.

### Type Representation
- **D-20:** HostInterface function pointer fields: TYPED signatures (not opaque void*). More type-safe, catches errors at compile time. Each language uses its native typed fn ptr pattern (C# delegates, Python CFUNCTYPE, C++ fn pointers, LuaJIT typed ffi.cdef).
- **D-21:** Array<T> represented as single generic `Array` with `void*` items + `size_t` len + `size_t` align per language. No monomorphization.
- **D-22:** RuntimeConfig matches Rust exactly: 16 bytes, 3 fields (`compatibility: u32`, `hot_reload_enabled: bool`, `on_reload: fn pointer`). The 24-byte version with retry fields that SDKs currently have is wrong and gets replaced.
- **D-23:** GuestContractHandle matches Rust exactly: 4 bytes, single `index: u32` field. The `generation` field that C++ and other SDKs have is stale — gets removed.
- **D-24:** NativeDispatch matches Rust exactly: 16 bytes, `function_count: u32` + `functions: *const *const ()`. SDKs currently missing `function_count` — gets fixed.
- **D-25:** HostContractInterface generated as flat struct matching Rust layout (9 fields: contract_id, version, singleton, dispatch_type, runtime, create_instance, destroy_instance, dispatch). NOT the wrong "header + dispatch union" decomposition that SDKs currently have.

### Hand-Written Struct Deletion
- **D-26:** ALL hand-written FFI struct definitions in SDK host files get deleted during execution. No exceptions. Only auto-generated abi.* files are source of truth.
- **D-27:** Files affected: `runtime.py` (Python), `NativeMethods.cs` (C#), `runtime.lua` (Lua), `mod.js` (JS), `runtime.hpp` (C++), plus guest files and `polyplug_abi` shared Python package.
- **D-28:** Python `polyplug_abi` shared package (`sdks/python/polyplug_abi/polyplug_abi/abi.py`) is replaced by the auto-generated `sdks/python/abi/abi.py`. The shared package imports from it (standard Python package convention).

### PluginRegistrar Removal
- **D-29:** Remove `PluginRegistrar` type from ALL guest SDKs and documentation. Use `HostInterface*` directly in `polyplug_init()` signatures. PluginRegistrar was just an alias for HostInterface — no semantic value.
- **D-30:** VERIFICATION REQUIRED: No remaining `PluginRegistrar` references anywhere in codebase after execution. Search all files.

### Layout Verification
- **D-31:** Auto-generate layout test source files per SDK: `test_layout.py` (pytest), `LayoutTest.cs` (xUnit), `test_layout.lua` (assert), `test_layout.ts` (Deno.test), `test_layout.cpp` (assert). Generated by build script alongside abi.* files.
- **D-32:** Test scaffolding (project files, conftest, etc.) created manually per SDK once. Build script only generates test source files.

### JS-Specific
- **D-33:** JS abi.ts emits both TypeScript interfaces AND binary offset constants (for `DataView`/`UnsafePointerView`). Single file serves both purposes.
- **D-34:** Host SDK is Deno-only. Guest SDK needs split-pointer representation for QuickJS. The generated abi.ts targets the host (Deno). Guest QuickJS representation is a separate concern.

### Namespace Conventions
- **D-35:** Each SDK uses consistent, idiomatic naming: C# → `Polyplug.Abi` namespace with PascalCase; Python → snake_case; Lua → snake_case; JS → camelCase; C++ → `polyplug` namespace for host, global for extern "C". Follow existing conventions per SDK.

### Claude's Discretion
- Exact ast-grep rule patterns and integration points
- Loader crate source file discovery implementation details
- Test file content generation specifics
- Error message wording for build failures

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### ABI Definition (Source of Truth)
- `crates/polyplug_abi/src/lib.rs` — barrel file, module tree root
- `crates/polyplug_abi/build/main.rs` — current build script with TODO comment (lines 27-39)
- `crates/polyplug_abi/build/extractor.rs` — current extractor (needs rewrite: module tree walk, auto-discovery)
- `crates/polyplug_abi/build/generate.rs` — current SDK generation module
- `crates/polyplug_abi/build/mapper.rs` — current type mapper (remove create_hash_functions)
- `crates/polyplug_abi/build/types.rs` — extracted type definitions

### Rust ABI Types (All #[repr(C)] types)
- `crates/polyplug_abi/src/types/string_view.rs` — StringView (16 bytes)
- `crates/polyplug_abi/src/types/buffer.rs` — Buffer (24 bytes)
- `crates/polyplug_abi/src/types/version.rs` — Version (12 bytes)
- `crates/polyplug_abi/src/types/abi_error.rs` — AbiError (24 bytes)
- `crates/polyplug_abi/src/types/error_code.rs` — AbiErrorCode (#[repr(u32)])
- `crates/polyplug_abi/src/types/array.rs` — Array<T> (24 bytes, generic)
- `crates/polyplug_abi/src/types/dependency_info.rs` — DependencyInfo (24 bytes)
- `crates/polyplug_abi/src/dispatch/dispatch_type.rs` — DispatchType (#[repr(u32)])
- `crates/polyplug_abi/src/dispatch/native_dispatch.rs` — NativeDispatch (16 bytes)
- `crates/polyplug_abi/src/dispatch/vm_dispatch.rs` — VmDispatch (16 bytes)
- `crates/polyplug_abi/src/dispatch/vm_loader_data.rs` — VmLoaderData (8 bytes)
- `crates/polyplug_abi/src/dispatch/dispatch_mechanisms.rs` — DispatchMechanisms (union, 16 bytes)
- `crates/polyplug_abi/src/guest/guest_contract_interface.rs` — GuestContractInterface (56 bytes)
- `crates/polyplug_abi/src/guest/guest_contract_instance.rs` — GuestContractInstance (16 bytes)
- `crates/polyplug_abi/src/host/host_interface.rs` — HostInterface (144 bytes, 18 typed fn ptrs)
- `crates/polyplug_abi/src/host/host_contract_interface.rs` — HostContractInterface (72 bytes)
- `crates/polyplug_abi/src/host/host_contract_instance.rs` — HostContractInstance (8 bytes)
- `crates/polyplug_abi/src/host/runtime_interface.rs` — RuntimeInterface (96 bytes)
- `crates/polyplug_abi/src/plugin/guest_contract_handle.rs` — GuestContractHandle (4 bytes)
- `crates/polyplug_abi/src/plugin/plugin_descriptor.rs` — PluginDescriptor (48 bytes)
- `crates/polyplug_abi/src/plugin/plugin_context.rs` — BundleInitContext (24 bytes)
- `crates/polyplug_abi/src/runtime/runtime_config.rs` — RuntimeConfig (16 bytes)
- `crates/polyplug_abi/src/runtime/compatibility.rs` — Compatibility (#[repr(u32)])
- `crates/polyplug_abi/src/runtime/reload_phase.rs` — ReloadPhase (48 bytes) + ReloadPhaseType (#[repr(u32)])
- `crates/polyplug_abi/src/runtime_language.rs` — RuntimeLanguage
- `crates/polyplug_abi/src/contract_type.rs` — ContractType
- `crates/polyplug_abi/src/ffi.rs` — polyplug_host_alloc, polyplug_host_free

### Code Generators (Must Update)
- `crates/polyplug_codegen/src/` — language generators (Python, C#, Lua, JS, C++)

### ast-grep Infrastructure (Already Exists)
- `crates/sdk_validator/src/ast_grep.rs` — AstGrepRunner (CLI wrapper for `sg` binary)
- `crates/sdk_validator/src/languages/` — per-language validators using ast-grep
- `.github/workflows/ci.yml` — already installs ast-grep CLI

### SDK Files to Delete/Replace
- `sdks/python/abi/abi.py` — broken, replace
- `sdks/python/polyplug_abi/polyplug_abi/abi.py` — wrong layouts, replace with imports
- `sdks/csharp/abi/Abi.cs` — incomplete, replace
- `sdks/csharp/abi/StringViewHelper.cs` — merge into Abi.cs
- `sdks/csharp/abi/StringHelpers.cs` — merge into Abi.cs
- `sdks/lua/abi/abi.lua` — broken, replace
- `sdks/lua/abi/polyplug_abi.lua` — wrong layouts, replace
- `sdks/lua/abi/string_view_helper.lua` — merge into abi.lua
- `sdks/js/abi/abi.ts` — broken, replace
- `sdks/js/abi/polyplug_abi.ts` — wrong layouts, replace
- `sdks/js/abi/string_view_helper.ts` — merge into abi.ts
- `sdks/cpp/abi/polyplug/abi.hpp` — empty stub, replace

### SDK Host Files to Clean (Remove Hand-Written Structs)
- `sdks/python/host/polyplug/runtime.py` — RuntimeConfig, HostInterface, HostContractInterface, etc.
- `sdks/csharp/host/NativeMethods.cs` — StringViewC, ReloadPhaseFfi, RuntimeConfig, HostInterface, etc.
- `sdks/lua/host/polyplug/runtime.lua` — HostInterface, RuntimeConfig, HostContractInterface, etc.
- `sdks/js/host/polyplug/mod.js` — HOST_INTERFACE_OFFSETS, RuntimeConfig binary construction
- `sdks/cpp/host/polyplug/runtime.hpp` — HostInterface, RuntimeConfig, etc.

### PluginRegistrar References to Remove
- `sdks/cpp/guest/polyplug/guest.hpp` — POLYPLUG_GUEST_MAIN macro uses PluginRegistrar
- `sdks/js/guest/polyplug_guest.js` — @typedef PluginRegistrar
- `sdks/rust/guest/src/lib.rs` — re-exports as HostInterface alias
- `docs/ABI_ARCHITECTURE.md` — references PluginRegistrar
- `docs/abi_types.md` — references PluginRegistrar
- `PRD.md` — references PluginRegistrar
- All README files in sdks/*/ — reference PluginRegistrar
- Test fixtures in `tests/fixtures/` — reference PluginRegistrar

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `AstGrepRunner` in `crates/sdk_validator/src/ast_grep.rs` — already wraps `sg` CLI binary, supports Rust/Python/C#/C++/TS/JS. Reuse for method body preservation.
- `polyplug_codegen` crate — already has per-language generators (`PythonGenerator`, `CSharpGenerator`, `LuaGenerator`, `JsGenerator`, `CppGenerator`) with `CodeGenerator` trait. Extend these to emit typed fn pointers and offset constants.
- Lua validator uses `tree-sitter` directly (not ast-grep) — same approach needed for Lua method preservation.

### Established Patterns
- `build.rs` in polyplug_abi already resolves workspace root path — can use same pattern to find loader crate source directories.
- Each SDK language already has a generator with `generate_struct`, `generate_enum`, `generate_union` methods — these need enhancement for typed fn pointers and layout assertions.
- CI already has `cargo install ast-grep --locked` step.

### Integration Points
- `crates/polyplug_abi/build.rs` (entry) → `build/main.rs` → `build/extractor.rs` (parse Rust) → `build/mapper.rs` (convert to codegen types) → `build/generate.rs` (call codegen per language) → writes to `sdks/*/abi/`
- `sdk_validator` is a separate tool that runs in CI — must be updated to validate against auto-generated types.
- Loader crates have their config structs in `src/lib.rs` — build script needs paths to each loader crate's source.

</code_context>

<specifics>
## Specific Ideas

- The TODO comment in `build/main.rs` (lines 27-39) describes the ast-grep approach — enhance it: ast-grep runs after codegen generation, finds method definitions in existing abi.* files, preserves method bodies while updating struct field definitions
- Each generated abi.* file should have a header comment: `// THIS FILE IS AUTO-GENERATED BY polyplug_abi — DO NOT EDIT STRUCT DEFINITIONS. Helper methods are preserved by ast-grep across regenerations.`
- Build script outputs expected struct sizes as part of generated code (comments or constants) for cross-verification
- Convention-based method naming: each codegen generator has a `transform_name` method that applies its language convention (already partially exists in sdk_validator)

</specifics>

<deferred>
## Deferred Ideas

- QuickJS guest representation (split-pointer for 32-bit pairs) — host SDK is Deno-only, QuickJS guest is separate concern
- `ResolveHandle` forward declaration in C++ — investigate if still needed after HostInterface generation
- Full `sdk_validator` rewrite to be Rust-driven (currently yaml-driven) — validation approach decided but implementation is follow-up work
- Additional helper methods beyond current set — ast-grep preservation handles future additions naturally

</deferred>

---

*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Context gathered: 2026-04-12*
