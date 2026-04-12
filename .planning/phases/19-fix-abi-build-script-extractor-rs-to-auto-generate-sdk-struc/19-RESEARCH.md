# Phase 19: Fix ABI Build Script + Auto-Generate SDK Structs - Research

**Researched:** 2026-04-12
**Domain:** Rust build.rs code generation, cross-language FFI struct emission, ast-grep CLI integration
**Confidence:** HIGH

## Summary

The polyplug_abi build script (`build/main.rs`) currently reads only `src/lib.rs` and uses hardcoded whitelists (`ABI_TYPES`, `ABI_FUNCTIONS`, `ABI_CONSTANTS`) to filter which types to extract. The ABI types have been modularized into a deep module tree (7 sub-modules, 35+ source files) but the extractor still only parses the barrel file, meaning it discovers almost nothing. The generated `sdks/*/abi/abi.*` files are broken placeholders containing invalid syntax (e.g., Python `def fnv1a_64(data: &[u8])` -- Rust syntax in Python).

Meanwhile, the SDK host files (`runtime.py`, `NativeMethods.cs`, `runtime.lua`, `mod.js`, `runtime.hpp`) contain hand-written FFI struct definitions that are often WRONG (wrong sizes, stale fields like `generation` in `GuestContractHandle`, missing `function_count` in `NativeDispatch`). These need to be replaced by auto-generated code from the canonical Rust `#[repr(C)]` definitions.

The fix involves three major changes: (1) rewrite the extractor to recursively walk the module tree, auto-discovering all `pub` types with `#[repr(C)]`, `#[repr(u32)]`, etc.; (2) enhance the codegen generators to produce typed function pointer signatures and layout assertions; (3) delete all hand-written structs from SDK files and replace with imports from auto-generated `abi.*` files. The existing `AstGrepRunner` in `sdk_validator` provides the CLI wrapper for ast-grep, which will be used to preserve hand-written helper method bodies across regenerations.

**Primary recommendation:** Rewrite `extractor.rs` to recursively walk the module tree using `syn`, auto-discover `#[repr(C)]`/`#[repr(u*)]` types, remove all whitelists, and extend the `polyplug_codegen` generators with typed fn pointer support and layout assertions per language.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions
- **D-01:** Walk the module tree recursively -- parse `lib.rs` for `pub mod X;` declarations, then read and parse each sub-module file. Continue recursively into sub-modules.
- **D-02:** Auto-discover all `pub` structs/enums/unions with `#[repr(C)]` -- no whitelist. Remove `ABI_TYPES` list entirely.
- **D-03:** Auto-discover `pub const` constants starting with `POLYPLUG_` -- no whitelist. Remove `ABI_CONSTANTS`.
- **D-04:** Do NOT extract functions -- `fnv1a_64`, `contract_id`, etc. are Rust-only implementation details.
- **D-05:** Remove `ABI_FUNCTIONS` list and `create_hash_functions()` from mapper.rs entirely.
- **D-06:** Stay as `build.rs` in polyplug_abi crate. Do not move to polyplugc CLI or justfile.
- **D-07:** Track all sub-module source files for `cargo:rerun-if-changed` (not just `src/lib.rs`).
- **D-08:** Build script scans multiple crates -- polyplug_abi AND loader crates -- to discover loader config structs.
- **D-09:** Fail the build if a type cannot be represented in a target language. Clear error message.
- **D-10:** All #[repr(C)] structs auto-generated in separate `abi.*` files per SDK. SDK host/guest files import from them.
- **D-11:** Delete all `sdks/*/abi/` files that are currently broken placeholders. Codegen writes fresh files.
- **D-12:** Merge helper files into abi.* via ast-grep preservation of method bodies.
- **D-13:** C++ output stays at `sdks/cpp/abi/polyplug/abi.hpp`.
- **D-14:** Use ast-grep as CLI tool (`sg` binary). NOT a Rust library.
- **D-15:** ast-grep surgically updates method signatures while preserving method bodies. Deleted methods get `DELETED_` prefix.
- **D-16:** ast-grep does NOT handle FFI function call cleanup -- only struct definitions and methods.
- **D-17:** Execution order: (1) Fix build script extractor, (2) Implement ast-grep integration, (3) Delete hand-written structs.
- **D-18:** Validation is Rust-driven -- extract method names from Rust `impl` blocks.
- **D-19:** Convention-based auto-derivation for method names (snake_case -> PascalCase/camelCase).
- **D-20:** HostInterface function pointer fields: TYPED signatures (not opaque void*).
- **D-21:** Array<T> represented as single generic `Array` with `void*` items + `size_t` len + `size_t` align.
- **D-22:** RuntimeConfig matches Rust exactly: 16 bytes, 3 fields. 24-byte version is wrong.
- **D-23:** GuestContractHandle matches Rust exactly: 4 bytes, single `index: u32` field.
- **D-24:** NativeDispatch matches Rust exactly: 16 bytes, `function_count: u32` + `functions: *const *const ()`.
- **D-25:** HostContractInterface generated as flat struct matching Rust layout (9 fields). NOT header+dispatch decomposition.
- **D-26:** ALL hand-written FFI struct definitions in SDK host files get deleted. No exceptions.
- **D-27:** Files affected: `runtime.py`, `NativeMethods.cs`, `runtime.lua`, `mod.js`, `runtime.hpp`, plus guest files.
- **D-28:** Python `polyplug_abi` shared package replaced by auto-generated `sdks/python/abi/abi.py`.
- **D-29:** Remove `PluginRegistrar` type from ALL guest SDKs and documentation.
- **D-30:** VERIFICATION REQUIRED: No remaining `PluginRegistrar` references after execution.
- **D-31:** Auto-generate layout test source files per SDK.
- **D-32:** Test scaffolding created manually per SDK once. Build script only generates test source files.
- **D-33:** JS abi.ts emits both TypeScript interfaces AND binary offset constants.
- **D-34:** Host SDK is Deno-only. Generated abi.ts targets the host (Deno).
- **D-35:** Each SDK uses consistent, idiomatic naming per language.

### Claude's Discretion
- Exact ast-grep rule patterns and integration points
- Loader crate source file discovery implementation details
- Test file content generation specifics
- Error message wording for build failures

### Deferred Ideas (OUT OF SCOPE)
- QuickJS guest representation (split-pointer for 32-bit pairs)
- `ResolveHandle` forward declaration in C++
- Full `sdk_validator` rewrite to be Rust-driven
- Additional helper methods beyond current set
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| D-01 | Walk module tree recursively from lib.rs | Module tree fully mapped (7 sub-modules, 35+ files). `syn` parses `pub mod X;` declarations |
| D-02 | Auto-discover #[repr(C)] types | 25+ #[repr(C)] structs, 7 #[repr(u32)] enums, 1 #[repr(C)] union identified across module tree |
| D-03 | Auto-discover POLYPLUG_ constants | Only `POLYPLUG_ABI_VERSION` exists in `lib.rs:51`. Filter by prefix pattern |
| D-04/D-05 | Remove function extraction | `create_hash_functions()` in mapper.rs and `ABI_FUNCTIONS` in extractor.rs to be deleted |
| D-06/D-07 | Build script stays, add rerun-if-changed | All 35+ source files must be tracked for rebuild triggers |
| D-08 | Scan loader crates for config structs | 5 loader crates with config structs identified. Path resolution from workspace root |
| D-09 | Fail on unrepresentable types | Each codegen generator must return Result, not panic silently |
| D-10-D-13 | Generated file structure | Output paths verified: `sdks/{lang}/abi/abi.{ext}`. C++ sub-dir `polyplug/abi.hpp` |
| D-14-D-16 | ast-grep integration | `sg` v0.42.0 installed. `AstGrepRunner` wraps CLI. Lua uses tree-sitter directly |
| D-17 | Execution ordering | Three waves: extractor fix, ast-grep integration, struct deletion |
| D-20 | Typed function pointer signatures | Codegen generators already have `convert_function_pointer()` in C++/JS/Lua generators |
| D-22-D-25 | Exact layout matching | Rust layout tests verified: RuntimeConfig=16B, GuestContractHandle=4B, NativeDispatch=16B, HostInterface=144B |
| D-26-D-28 | Delete hand-written structs | 13+ SDK files identified with hand-written structs. All import from auto-generated abi.* |
| D-29/D-30 | Remove PluginRegistrar | 19 files reference PluginRegistrar. All must be cleaned |
| D-31/D-32 | Layout test generation | Test frameworks per language: pytest, xUnit, assert (Lua), Deno.test, assert (C++) |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| syn | 2.x (workspace) | Rust source parsing | Already used in extractor.rs. Parse module declarations, struct/enum/union definitions |
| quote | 1.x (workspace) | Token serialization | Already used for `type_to_string()`. Converts syn types to string |
| polyplug_codegen | workspace | Multi-language code generation | Already has 5 language generators. The codegen trait and data model are established |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| ast-grep (CLI) | 0.42.0 | AST-based code transformation | Preserving helper method bodies across regeneration |
| tree-sitter | 0.25 (workspace) | Lua source parsing | Lua validation (ast-grep has limited Lua support) |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| syn (manual walk) | ra_ap_syntax (rust-analyzer) | ra_ap_syntax is heavier, requires proc-macro crate type. syn is already a dependency and sufficient for parsing `pub mod X;` declarations and type definitions |
| ast-grep CLI | ast-grep Rust library | Library adds heavy dependency to build script. CLI already installed in CI, `AstGrepRunner` wraps it cleanly |
| build.rs codegen | polyplugc CLI | polyplugc generates host/guest contract bindings, not raw ABI type definitions. Different scope. build.rs is correct for ABI types |

**Installation:**
No new dependencies required. All libraries are already workspace dependencies.

**Version verification:** Already locked in `Cargo.lock`. syn 2.x and quote 1.x are build-dependencies of polyplug_abi.

## Architecture Patterns

### Recommended Project Structure (Current, no changes needed)
```
crates/polyplug_abi/
  build.rs              -> delegates to build/main.rs (already exists)
  build/
    main.rs             -> entry point (rewrite: module walk, rerun-if-changed)
    extractor.rs        -> type extraction (rewrite: recursive module walk, remove whitelists)
    mapper.rs           -> AbiType -> codegen Item mapping (simplify: remove functions)
    generate.rs         -> orchestrate codegen per language (extend: layout tests, ast-grep step)
    types.rs            -> extracted type definitions (minor: remove AbiFunction variant)
  src/
    lib.rs              -> barrel file with pub mod declarations
    types/              -> 7 files: string_view, buffer, version, abi_error, error_code, array, dependency_info
    dispatch/           -> 5 files: dispatch_type, native_dispatch, vm_dispatch, vm_loader_data, dispatch_mechanisms
    guest/              -> 2 files: guest_contract_interface, guest_contract_instance
    host/               -> 4 files: host_interface, host_contract_interface, host_contract_instance, runtime_interface
    plugin/             -> 3 files: guest_contract_handle, plugin_descriptor, plugin_context
    runtime/            -> 3 files: runtime_config, compatibility, reload_phase
    contract_type.rs    -> ContractType enum
    runtime_language.rs -> RuntimeLanguage enum
    ffi.rs              -> polyplug_host_alloc, polyplug_host_free
    tracking.rs         -> tracking utilities
```

### Pattern 1: Recursive Module Tree Walking
**What:** Parse `lib.rs` for `pub mod X;`, then resolve each module to a file or directory, parse that file recursively.
**When to use:** Discovering all types across the modular ABI source tree.
**Example:**
```rust
// Walk pattern for extractor.rs
fn walk_module_tree(dir: &Path, module_file: &Path, types: &mut AbiTypes) -> Result<Vec<PathBuf>> {
    let mut visited = vec![module_file.to_path_buf()];
    let source = fs::read_to_string(module_file)?;
    let file = parse_file(&source)?;

    for item in &file.items {
        if let Item::Mod(item_mod) = item {
            if let Some(mod_name) = extract_public_mod(item_mod) {
                // Try dir/{name}.rs first, then dir/{name}/mod.rs
                let file_path = dir.join(format!("{}.rs", mod_name));
                let sub_dir = dir.join(mod_name);
                let mod_rs = sub_dir.join("mod.rs");

                let target = if file_path.exists() { &file_path }
                             else if mod_rs.exists() { &mod_rs }
                             else { continue };

                types.extract_from_file(target)?;
                let sub_visited = walk_module_tree(&sub_dir, target, types)?;
                visited.extend(sub_visited);
            }
        }
    }
    Ok(visited)
}
```

### Pattern 2: Auto-Discovery by Attribute (No Whitelists)
**What:** Accept any `pub` type with `#[repr(C)]` or `#[repr(u*)]`, any `pub const` starting with `POLYPLUG_`.
**When to use:** Instead of maintaining ABI_TYPES/ABI_CONSTANTS lists.
**Example:**
```rust
fn should_extract_struct(item: &ItemStruct) -> bool {
    is_public(&item.vis) && has_repr_c(&item.attrs)
}

fn should_extract_enum(item: &ItemEnum) -> bool {
    is_public(&item.vis) && has_repr_int(&item.attrs)  // #[repr(u32)], #[repr(u8)], etc.
}

fn should_extract_const(item: &ItemConst) -> bool {
    item.ident.to_string().starts_with("POLYPLUG_")
}
```

### Pattern 3: Typed Function Pointer Generation per Language
**What:** Each codegen generator converts `unsafe extern "C" fn(args) -> Ret` to language-specific typed callable.
**When to use:** Generating HostInterface struct fields with typed fn ptrs instead of `void*`.
**Example (Python ctypes):**
```python
# Instead of: ("register_contract", ctypes.c_void_p)
# Generate:
_register_contract_t = ctypes.CFUNCTYPE(
    ctypes.c_uint32,                    # AbiError
    ctypes.c_void_p,                    # *const HostInterface
    ctypes.c_void_p,                    # *const PluginDescriptor
    ctypes.c_void_p,                    # *const GuestContractInterface
)
```

### Pattern 4: ast-grep Method Body Preservation
**What:** After generating fresh `abi.*`, run ast-grep to find existing method definitions in the old file, then splice their bodies into the new file.
**When to use:** Preserving hand-written helpers like `StringView.from_ptr()`, `StringViewHelper.StripPrefix()`.
**Example:**
```bash
# ast-grep scan for method definitions in old file
sg scan --inline-rules 'id: find_methods\nlanguage: python\nrule:\n  pattern: def $METHOD($$$ARGS):\n    $$$BODY' old_abi.py
```

### Anti-Patterns to Avoid
- **Hardcoded whitelists:** The current `ABI_TYPES`/`ABI_CONSTANTS`/`ABI_FUNCTIONS` lists are already stale (reference `RuntimeAbi`, `HostContext`, etc. that no longer exist). Must be removed entirely.
- **Flat file parsing:** Only reading `src/lib.rs` misses all types in sub-modules. The module tree is 3 levels deep.
- **Rust syntax in other languages:** Current generated Python has `data: &[u8]` which is Rust syntax. Each codegen generator must produce idiomatic target-language code.
- **Ignoring padding/alignment:** Hand-written SDK structs often get sizes wrong. The generated code must include size assertions matching the Rust layout tests.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Rust source parsing | Regex-based extraction | `syn::parse_file()` | syn handles all Rust syntax, including nested types, generics, attributes |
| Module resolution | Manual path construction | `syn` for `pub mod` + filesystem probing (`{name}.rs` or `{name}/mod.rs`) | Rust module resolution rules are well-defined |
| Name transformation | Custom case conversion | `transform_name()` from `sdk_validator::ast_grep` | Already handles snake/Pascal/camel bidirectionally with tests |
| AST pattern matching | String-based find/replace | ast-grep CLI (`sg`) | Preserves syntax validity, handles whitespace/formatting |
| Function pointer conversion | Manual string manipulation | Existing `convert_function_pointer()` in each codegen generator | Already handles parsing `extern "C" fn(...)` type strings |

**Key insight:** The codegen generators already have the infrastructure for typed function pointers (`convert_function_pointer()` in C++/JS/Lua generators, `rust_type_to_*()` in all generators). The gap is in the EXTRACTOR not feeding them the right data, not in the generators themselves.

## Runtime State Inventory

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None -- build script generates code files only, no databases or persistent stores | None |
| Live service config | None -- no external services configured by generated code | None |
| OS-registered state | None -- build artifacts only | None |
| Secrets/env vars | None -- no secrets or env vars involved in codegen | None |
| Build artifacts | `sdks/*/abi/abi.*` files -- currently broken placeholders, will be regenerated | Regenerate via build script |

**Nothing found in any category requiring runtime migration.** This phase is purely a code generation and file replacement operation.

## Common Pitfalls

### Pitfall 1: Module Resolution for `mod` vs File
**What goes wrong:** Rust modules can be either `pub mod name;` (file-based) or `pub mod name { ... }` (inline). The extractor currently only handles flat file parsing.
**Why it happens:** The original code was written when all types were in a single file.
**How to avoid:** Use syn's `ItemMod` to detect both `mod name;` (needs file resolution) and `mod name { ... }` (inline content). The polyplug_abi crate uses file-based modules only, but defensive code should handle both.
**Warning signs:** `extract_types()` returning empty AbiTypes after the rewrite.

### Pitfall 2: Stale Hand-Written Structs with Wrong Sizes
**What goes wrong:** SDK host files have structs with wrong sizes (e.g., Python RuntimeConfig claims 24 bytes but Rust is 16 bytes; C++ GuestContractHandle has `generation` field but Rust only has `index`).
**Why it happens:** SDKs were written manually before the Rust types were finalized.
**How to avoid:** D-22 through D-25 mandate matching Rust layouts exactly. Auto-generated code will include `static_assert` / `ctypes.sizeof` checks.
**Warning signs:** `sizeof()` mismatches at runtime, segfaults during FFI calls.

### Pitfall 3: ast-grep Not Supporting Lua
**What goes wrong:** ast-grep has limited Lua support. The `sdk_validator` already uses tree-sitter directly for Lua.
**Why it happens:** ast-grep's Lua grammar may not handle LuaJIT FFI cdef syntax well.
**How to avoid:** For Lua, use the same tree-sitter approach that `sdk_validator` already uses, or write a simple regex-based method body extractor for Lua FFI blocks.
**Warning signs:** ast-grep failing to parse Lua files with `ffi.cdef` blocks.

### Pitfall 4: Build Script Ordering (Loader Crate Configs)
**What goes wrong:** Build script tries to scan loader crate sources (D-08) but the paths may not be correct during `cargo build` if using `CARGO_MANIFEST_DIR` resolution.
**Why it happens:** Loader crates are siblings in the workspace, not sub-directories of polyplug_abi.
**How to avoid:** The workspace root is already resolved in `build/main.rs` (line 43-49). Use `workspace_root.join("crates/polyplug_native/src/config.rs")` etc. Add `cargo:rerun-if-changed` for each loader config file.
**Warning signs:** Build succeeds but loader config structs are missing from generated output.

### Pitfall 5: Circular Dependency Between polyplug_abi and polyplug_codegen
**What goes wrong:** polyplug_abi depends on polyplug_codegen in build-dependencies. If codegen generators are extended significantly, changes there could break the ABI build.
**Why it happens:** Build-dependencies create a dependency edge that Cargo must resolve.
**How to avoid:** Test the build script changes with `cargo build -p polyplug_abi` after any codegen modifications. The dependency is one-way (polyplug_abi -> polyplug_codegen), no circular risk.
**Warning signs:** Cargo build errors about circular dependencies.

### Pitfall 6: Function Pointer Type String Parsing
**What goes wrong:** The `type_to_string()` function uses `quote::quote!()` which produces compact output like `unsafeextern"C"fn(...)` without spaces. The codegen generators' `convert_function_pointer()` relies on finding `fn(` and `)->` delimiters.
**Why it happens:** `quote!()` is a token serializer, not a pretty-printer.
**How to avoid:** Test with the actual HostInterface field types. The existing generators already handle this format (the `contains("extern\"C\"fn")` check works on the compact form). Verify by checking the existing C++/JS/Lua generators' handling of the compact form.
**Warning signs:** Generated function pointer fields producing `void(*)()` instead of typed signatures.

## Code Examples

### Recursive Module Walking with syn
```rust
// Source: Verified against crates/polyplug_abi/src/lib.rs module declarations
fn walk_modules(base_dir: &Path, source: &str, abi_types: &mut AbiTypes) -> Result<Vec<PathBuf>> {
    let file: File = parse_file(source)?;
    let mut tracked_files = Vec::new();

    for item in &file.items {
        if let Item::Mod(item_mod) = item {
            // Only process `pub mod name;` (not `pub mod name { ... }`)
            if !is_public(&item_mod.vis) || item_mod.content.is_some() {
                continue;
            }
            let mod_name = item_mod.ident.to_string();
            let file_path = base_dir.join(format!("{}.rs", mod_name));
            let dir_path = base_dir.join(&mod_name);
            let mod_path = dir_path.join("mod.rs");

            let target = if file_path.exists() { file_path }
                         else if mod_path.exists() { mod_path }
                         else { continue };

            tracked_files.push(target.clone());
            let sub_source = fs::read_to_string(&target)?;
            extract_types_from_source(&sub_source, abi_types);

            // Recurse into sub-modules
            if dir_path.is_dir() {
                let sub_tracked = walk_modules(&dir_path, &sub_source, abi_types)?;
                tracked_files.extend(sub_tracked);
            }
        }
    }
    Ok(tracked_files)
}
```

### Auto-Discovery Filters (Replacing Whitelists)
```rust
// Replace ABI_TYPES whitelist with attribute-based discovery
fn extract_struct(item: &ItemStruct) -> Option<AbiStruct> {
    if !is_public(&item.vis) { return None; }
    if !has_repr_c(&item.attrs) { return None; }  // Only #[repr(C)] structs
    // ... extract fields, doc, etc.
}

fn extract_enum(item: &ItemEnum) -> Option<AbiEnum> {
    if !is_public(&item.vis) { return None; }
    let repr = extract_enum_repr(&item.attrs);
    if repr == "u32" { return None; }  // Skip if no integer repr
    // ... extract variants, doc, etc.
}

fn extract_const(item: &ItemConst) -> Option<AbiConst> {
    let name = item.ident.to_string();
    if !name.starts_with("POLYPLUG_") { return None; }  // Auto-discover by prefix
    // ... extract value, type, doc.
}
```

### Python ctypes Typed Function Pointer
```python
# Source: Pattern for D-20 typed fn ptrs in Python
# Instead of opaque void*, generate CFUNCTYPE signatures
_register_contract_t = ctypes.CFUNCTYPE(
    ctypes.c_uint32,                    # AbiError
    ctypes.c_void_p,                    # *const HostInterface
    ctypes.c_void_p,                    # *const PluginDescriptor
    ctypes.c_void_p,                    # *const GuestContractInterface
)
```

### C# Delegate-Based Function Pointer
```csharp
// Source: Pattern for D-20 typed fn ptrs in C#
[UnmanagedFunctionPointer(CallingConvention.Cdecl)]
public delegate AbiError RegisterContractDelegate(
    IntPtr hostInterface,
    IntPtr descriptor,
    IntPtr guestInterface
);
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Single-file lib.rs with all types | Modular module tree (7 sub-modules, 35+ files) | Phase 1-2 (2026-04-04) | Extractor must walk the tree, not parse one file |
| Hardcoded ABI_TYPES whitelist | Auto-discovery by #[repr(C)] attribute | This phase | Removes maintenance burden, catches all types automatically |
| Hand-written SDK structs | Auto-generated from Rust source | This phase | Single source of truth, eliminates layout mismatches |
| Opaque void* for fn ptrs | Typed signatures per language | This phase | Compile-time type safety in SDKs |
| Separate helper files (string_view_helper.*) | Merged into abi.* via ast-grep | This phase | Single file per language, methods preserved across regeneration |

**Deprecated/outdated:**
- `ABI_TYPES` list: references `RuntimeAbi`, `HostContext`, `PluginDispatch`, `HostContractVTableHeader` which no longer exist or were renamed
- `ABI_FUNCTIONS` list: `fnv1a_64`, `contract_id`, etc. -- not needed by SDKs (D-04)
- `ABI_CONSTANTS` list: only `POLYPLUG_ABI_VERSION` -- auto-discovered by prefix
- `create_hash_functions()` in mapper.rs: emits Rust syntax in Python/JS/etc. -- broken, to be removed

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | ast-grep CLI can parse LuaJIT FFI cdef blocks | Architecture Patterns | May need tree-sitter fallback for Lua |
| A2 | Loader config structs (PythonConfig, LuaConfig, etc.) have `#[repr(C)]` and are simple (no complex generics) | D-08 | PythonConfig has `(u32, u32)` tuple -- needs handling in codegen |
| A3 | The `quote::quote!()` compact output for fn ptr types (`unsafeextern"C"fn(...)`) is handled by existing generators | Pitfall 6 | May need to normalize the string before feeding to generators |
| A4 | `Option<unsafe extern "C" fn(...)>` in RuntimeConfig.on_reload can be represented in all target languages | Standard Stack | C# has `IntPtr` for fn ptr, Python has `c_void_p`, Lua has `void*` |
| A5 | No inline `mod { ... }` blocks with ABI types exist in polyplug_abi src | Pitfall 1 | All modules are file-based; verified by reading all mod.rs files |

## Open Questions

1. **Lua ast-grep support level**
   - What we know: ast-grep 0.42.0 has Lua language support. `sdk_validator` uses tree-sitter directly for Lua.
   - What's unclear: Whether ast-grep can parse LuaJIT `ffi.cdef` blocks correctly.
   - Recommendation: Test ast-grep on a sample `abi.lua` with `ffi.cdef` blocks. If it fails, use the tree-sitter approach from `sdk_validator` for Lua.

2. **Loader config struct extraction depth**
   - What we know: D-08 says scan loader crates for config structs. PythonConfig has `(u32, u32)` tuple, DotnetConfig has String fields.
   - What's unclear: Whether these complex types need full codegen or just simple struct emission.
   - Recommendation: Start with simple cases (NativeConfig is empty, JsConfig is empty). Handle PythonConfig/DotnetConfig as they come.

3. **Array<T> generic handling across languages**
   - What we know: D-21 says single generic `Array` with `void*` + `size_t`. Rust has `Array<T>` with generic type parameter.
   - What's unclear: How many concrete Array instantiations exist and whether all languages need all of them.
   - Recommendation: Generate a single generic Array per language. If specific concrete types are needed, add them as type aliases.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | Build script | Yes | 1.94.0 | -- |
| syn/quote crates | Extractor | Yes | workspace deps | -- |
| polyplug_codegen | Code generation | Yes | workspace dep | -- |
| ast-grep CLI (`sg`) | Method body preservation | Yes | 0.42.0 | Skip preservation step (manual merge) |
| Python 3.10+ | Layout test execution | Yes | 3.14.4 | -- |
| .NET 10.0 SDK | C# layout test execution | Yes | 10.0.104 | -- |
| Deno 2.x | JS layout test execution | Yes | 2.7.12 | -- |
| Lua 5.4+ | Lua layout test execution | Yes | 5.5.0 | -- |
| C++ compiler | C++ layout test execution | Yes (system) | -- | -- |

**Missing dependencies with no fallback:**
- None identified.

**Missing dependencies with fallback:**
- ast-grep: If unavailable, method body preservation step can be done manually. The build will still generate correct struct definitions.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` (build script tests) + per-language test frameworks |
| Config file | `crates/polyplug_abi/Cargo.toml` (build-dependencies) |
| Quick run command | `cargo test -p polyplug_abi --lib` |
| Full suite command | `cargo test -p polyplug_abi && cargo build -p polyplug_abi` |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| D-01 | Module tree walk discovers all source files | unit | `cargo test -p polyplug_abi --lib test_walk_module_tree` | No -- Wave 0 |
| D-02 | All #[repr(C)] structs auto-discovered | unit | `cargo test -p polyplug_abi --lib test_auto_discover_repr_c` | No -- Wave 0 |
| D-03 | POLYPLUG_ constants auto-discovered | unit | `cargo test -p polyplug_abi --lib test_auto_discover_constants` | No -- Wave 0 |
| D-22 | RuntimeConfig generated as 16 bytes | layout | `pytest sdks/python/abi/test_layout.py` | No -- Wave 0 |
| D-23 | GuestContractHandle generated as 4 bytes | layout | `pytest sdks/python/abi/test_layout.py` | No -- Wave 0 |
| D-24 | NativeDispatch has function_count field | layout | C++ static_assert in abi.hpp | Generated |
| D-25 | HostContractInterface flat struct, 72 bytes | layout | `cargo test -p polyplug_abi --lib layout_host_contract_interface` | Yes (exists) |
| D-30 | No PluginRegistrar references remain | grep | `grep -r PluginRegistrar sdks/` | Manual |

### Sampling Rate
- **Per task commit:** `cargo test -p polyplug_abi --lib`
- **Per wave merge:** `cargo build -p polyplug_abi && grep -r 'sizeof\|static_assert\|ctypes.sizeof' sdks/*/abi/`
- **Phase gate:** Full build passes, all generated abi.* files contain valid code in each language, no PluginRegistrar references

### Wave 0 Gaps
- [ ] Build script tests for module walking (unit tests in `build/extractor.rs` or separate test file)
- [ ] `sdks/python/abi/test_layout.py` -- pytest layout assertions
- [ ] `sdks/csharp/abi/LayoutTests.cs` -- xUnit layout tests
- [ ] `sdks/lua/abi/test_layout.lua` -- assert-based layout checks
- [ ] `sdks/js/abi/test_layout.ts` -- Deno.test layout checks
- [ ] `sdks/cpp/abi/test_layout.cpp` -- static_assert layout checks

## Security Domain

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | No | N/A -- build tool, no auth |
| V3 Session Management | No | N/A |
| V4 Access Control | No | N/A |
| V5 Input Validation | Yes | `syn::parse_file()` validates Rust source. Build fails on parse errors. |
| V6 Cryptography | No | N/A |

### Known Threat Patterns for Build Script Code Generation

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Malicious input to codegen | Tampering | Build script reads only controlled source files from the same crate |
| Path traversal in module walk | Elevation | Module resolution only follows known `pub mod` declarations within crate source tree |

## Sources

### Primary (HIGH confidence)
- `crates/polyplug_abi/build/*.rs` -- full source read, current extractor/mapper/generate architecture understood
- `crates/polyplug_abi/src/lib.rs` + all sub-modules -- module tree structure verified, 25+ #[repr(C)] types counted
- `crates/polyplug_codegen/src/languages/*.rs` -- all 5 generators read, typed fn ptr handling verified
- `crates/sdk_validator/src/ast_grep.rs` -- AstGrepRunner API understood, transform_name() available
- `sdks/*/host/` files -- hand-written structs identified with wrong sizes/layouts

### Secondary (MEDIUM confidence)
- `crates/polyplug_*/src/config.rs` -- loader config structures verified (PythonConfig, LuaConfig, JsConfig, DotnetConfig, NativeConfig)
- `sdks/*/abi/` files -- confirmed broken placeholders with invalid syntax
- 19 files with PluginRegistrar references -- verified via grep

### Tertiary (LOW confidence)
- ast-grep Lua parsing capability -- assumed to work based on language support list, needs testing

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH -- all dependencies already in workspace, verified via Cargo.toml
- Architecture: HIGH -- module tree fully mapped, codegen generators fully read, integration points clear
- Pitfalls: HIGH -- based on direct codebase inspection, not external documentation

**Research date:** 2026-04-12
**Valid until:** 2026-05-12 (stable -- Rust ecosystem, no external API dependencies)
