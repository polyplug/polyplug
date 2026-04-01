# Polyplug Code Generation Refactoring Plan

## TL;DR

> **Goal**: Consolidate ALL code generation into `polyplug_codegen` while maintaining clean separation of concerns between crates.
> 
> **Deliverables**:
> - New `polyplug_utils` crate with hash functions (fnv1a_64, contract_id, bundle_id, host_contract_id, plugin_contract_id)
> - Refactored `polyplug_codegen` as a "dumb" code generator (data → string, no semantic knowledge)
> - Working `build.rs` in `polyplug_abi` that generates ABI SDKs for C++, C#, Python, Lua, JavaScript
> - Hash functions exposed to all languages through ABI SDK generation
> - All duplicate code eliminated
> 
> **Estimated Effort**: Medium (3-4 days of focused work)
> **Parallel Execution**: YES - 3 waves with 4-6 tasks per wave
> **Critical Path**: Wave 1 (utils + codegen data) → Wave 2 (ABI build.rs) → Wave 3 (integration + tests)

---

## Context

### Original Request
Consolidate ALL code generation into a single location (`polyplug_codegen`) while maintaining clean separation:
- `polyplug_codegen` is a **dumb code generator** - knows nothing about ABI semantics or contracts
- `polyplug_abi` owns ABI type knowledge - parses its own source and maps to generator data
- `polyplug_utils` provides shared utilities (hash functions)
- Each crate is responsible for preparing its own data for generation

### Metis Review Findings

**Issues Identified**:
1. **5 duplicate FNV-1a implementations** across codebase
2. **API inconsistency**: `plugin_contract_id` exists in polyplug_abi but missing from polyplug_codegen and C++ SDK
3. **Broken build system**: `build/main.rs` has empty `fn main() {}` - not actually running
4. **IR duplication**: Separate IR types in polyplug_codegen and polyplug_abi/build

**User Decisions**:
- SDK generation: **build.rs automatically** (not CLI)
- polyplug_utils: **Separate crate**
- Existing build/ code: **Migrate generators to polyplug_codegen** (dumb generator pattern)
- Generated SDKs: **Generated on-demand** (not committed)
- Hash functions: **Expose to all languages through ABI SDK** (not just Rust)

---

## Work Objectives

### Core Objective
Create a clean separation where `polyplug_codegen` is a pure code generation library, `polyplug_abi` owns ABI semantics and triggers generation via build.rs, and `polyplug_utils` provides shared utilities accessible to all crates and languages.

### Concrete Deliverables
1. `crates/polyplug_utils/` - New crate with hash functions
2. `crates/polyplug_codegen/src/data.rs` - Language-agnostic data types (Item, Field, TypeInfo)
3. `crates/polyplug_codegen/src/generator.rs` - `CodeGenerator` trait for modular language support
4. `crates/polyplug_codegen/src/context.rs` - Generation context with type mappings
5. `crates/polyplug_codegen/src/languages/` - Modular language generators:
   - `cpp.rs` - `impl CodeGenerator for CppGenerator`
   - `csharp.rs` - `impl CodeGenerator for CSharpGenerator`
   - `python.rs` - `impl CodeGenerator for PythonGenerator`
   - `lua.rs` - `impl CodeGenerator for LuaGenerator`
   - `js.rs` - `impl CodeGenerator for JsGenerator`
6. `crates/polyplug_abi/build.rs` - Actual build script (not build/main.rs)
7. `crates/polyplug_abi/src/build/` - Type extraction and ABI generator implementation
8. Generated SDK files for 5 languages in `sdks/{lang}/abi/`

### Definition of Done
- [x] `polyplug_codegen` has zero runtime dependencies (only polyplug_utils, serde, thiserror, toml - no syn)
- [x] `polyplug_codegen` has no syn parsing (moved to polyplug_abi_build)
- [x] `polyplug_abi` has working build.rs that generates SDKs during compile
- [x] All existing tests pass (core packages: polyplug_utils, polyplug_abi, polyplug_abi_build)
- [x] Generated SDK files created for all 5 languages
- [x] No hash function duplication (consolidated in polyplug_utils)
- [x] Clean separation: abi types owned by polyplug_abi, generation by polyplug_codegen
- [x] Hash functions available in all language SDKs

### Must Have
- Working polyplug_utils crate
- Modular polyplug_codegen with CodeGenerator trait
- Language-specific generators (CppGenerator, CSharpGenerator, etc.) implementing the trait
- Working build.rs in polyplug_abi
- SDK generation for C++, C#, Python, Lua, JavaScript
- Hash functions in all SDKs
- Each language has its own way to generate structs/classes/enums (modular design)

### Must NOT Have (Guardrails)
- Changes to frozen ABI types in polyplug_abi/src/lib.rs
- New features not currently in SDKs
- Changes to polyplugc CLI interface
- Changes to loader crates (polyplug_python, polyplug_lua, etc.)
- Changes to polyplug runtime
- **NO backward compatibility** - Breaking changes are intentional and expected

### Breaking Changes (Intentional)
- Old `AbiGenerator` trait will be REMOVED and replaced with `CodeGenerator`
- Old `AbiInfo` batch-based API will be replaced with `Item`/`GenerationContext` API
- Language generators will be completely rewritten (item-by-item instead of batch)
- Existing SDK output format may change (ordering, formatting)
- Old `build/` directory will be deleted entirely

---

## Verification Strategy

### Test Decision
- **Infrastructure exists**: YES (existing test suite in polyplug_abi)
- **Automated tests**: Tests after (existing tests + new ones)
- **Framework**: cargo test (Rust built-in)
- **Agent-Executed QA**: YES - All verification via commands, zero human intervention

### QA Policy
Every task MUST include agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Backend/Library**: Use Bash (cargo test, cargo build, cargo check, diff)
- **File verification**: Use Read tool to verify file contents match expectations

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation - Start Immediately):
├── T1: Create polyplug_utils crate with hash functions
├── T2: Create polyplug_codegen data.rs with GenerationData types
├── T3: Create polyplug_codegen generate.rs with dumb generator
└── T4: Update polyplug_codegen Cargo.toml (remove syn, toml if present)

Wave 2 (ABI Integration - After Wave 1):
├── T5: Create polyplug_abi build.rs with syn parsing
├── T6: Create polyplug_abi src/build/ with type extraction
├── T7: Implement type mapping (Rust → C++/C#/Python/Lua/JS)
├── T8: Integrate language generators from old build/
└── T9: Add hash function generation for all languages

Wave 3 (Cleanup & Tests - After Wave 2):
├── T10: Delete old polyplug_abi/build/ directory
├── T11: Remove duplicate hash functions from polyplug_abi/src/lib.rs
├── T12: Remove duplicate hash functions from polyplug_codegen
├── T13: Update workspace Cargo.toml
├── T14: Run full test suite
└── T15: Verify generated SDKs match expected output

Wave FINAL (4 Parallel Reviews):
├── F1: Plan compliance audit (oracle)
├── F2: Code quality review (unspecified-high)
├── F3: Real manual QA (unspecified-high)
└── F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: T1 → T4 → T5 → T9 → T14 → F1-F4 → user okay
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Wave 1), 5 (Wave 2), 6 (Wave 3)
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| T1 | - | T2, T4, T11 |
| T2 | - | T3, T5 |
| T3 | T2 | T4, T8 |
| T4 | T1, T3 | T5 |
| T5 | T2, T4 | T6, T7 |
| T6 | T5 | T7, T8 |
| T7 | T5, T6 | T8, T9 |
| T8 | T3, T6, T7 | T9, T10 |
| T9 | T7, T8 | T14 |
| T10 | T8 | - |
| T11 | T1 | - |
| T12 | T1 | - |
| T13 | T1, T4 | - |
| T14 | T9, T10, T11, T12 | F1-F4 |
| T15 | T9 | T14 |

### Agent Dispatch Summary

  - **Wave 1**: 4 tasks → `quick` (crate creation, trait + data types, language generators)
  - **Wave 2**: 5 tasks → `unspecified-high` (syn parsing, type mapping, ABI generator integration)
  - **Wave 3**: 6 tasks → mix of `quick` (cleanup) and `unspecified-high` (testing)
  - **FINAL**: 4 tasks → oracle, unspecified-high, unspecified-high, deep

---

## TODOs

- [x] 1. Create polyplug_utils crate with hash functions

  **What to do**:
  - Create `crates/polyplug_utils/Cargo.toml` with no dependencies
  - Create `crates/polyplug_utils/src/lib.rs` with:
    - `fnv1a_64(data: &[u8]) -> u64`
    - `contract_id(name: &str, major: u32) -> u64`
    - `bundle_id(name: &str) -> u64`
    - `host_contract_id(name: &str, major: u32) -> u64`
    - `plugin_contract_id(name: &str, major: u32) -> u64`
  - Add comprehensive unit tests with golden values
  - Ensure zero external dependencies (only std)

  **Must NOT do**:
  - Add any dependencies beyond std
  - Include any code generation logic
  - Reference polyplug_abi or polyplug_codegen types

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Straightforward crate creation, well-defined functions, minimal logic
  - **Skills**: []
    - No special skills needed - pure Rust utility functions

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3, T4)
  - **Blocks**: T2 (polyplug_codegen needs utils), T4 (Cargo.toml updates), T11 (removing duplicates)
  - **Blocked By**: None

  **References**:
  - `crates/polyplug_abi/src/lib.rs:631-640` - Reference fnv1a_64 implementation to copy
  - `crates/polyplug_abi/src/lib.rs:642-653` - Reference contract_id implementation
  - `crates/polyplug_abi/src/lib.rs:655-667` - Reference bundle_id implementation
  - `crates/polyplug_abi/src/lib.rs:669-681` - Reference host_contract_id implementation
  - `crates/polyplug_abi/src/lib.rs:683-695` - Reference plugin_contract_id implementation
  - Workspace `Cargo.toml` - Follow workspace dependency pattern

  **WHY Each Reference Matters**:
  - Copy the exact FNV-1a algorithm to ensure identical hash outputs
  - Copy the ID computation logic to maintain compatibility
  - Follow workspace structure for consistent crate setup

  **Acceptance Criteria**:
  - [ ] `cargo check --package polyplug_utils` passes with zero warnings
  - [ ] `cargo test --package polyplug_utils` passes all tests
  - [ ] `cargo tree --package polyplug_utils` shows NO dependencies beyond std

  **QA Scenarios**:

  ```
  Scenario: Hash functions produce correct golden values
    Tool: Bash (cargo test)
    Preconditions: polyplug_utils crate created
    Steps:
      1. Run: cargo test --package polyplug_utils
      2. Verify: All tests pass (fnv1a_64, contract_id, bundle_id, host_contract_id, plugin_contract_id)
    Expected Result: test result: ok. N tests passed
    Failure Indicators: Any test failures, compilation errors
    Evidence: .sisyphus/evidence/task-1-hash-tests.log

  Scenario: No external dependencies
    Tool: Bash (cargo tree)
    Preconditions: polyplug_utils compiled
    Steps:
      1. Run: cargo tree --package polyplug_utils --depth 1
      2. Verify: Output shows only polyplug_utils vX.Y.Z with no dependencies
    Expected Result: "polyplug_utils v0.1.0" with no child dependencies
    Failure Indicators: Any non-std dependencies listed
    Evidence: .sisyphus/evidence/task-1-deps-check.log
  ```

  **Evidence to Capture**:
  - [ ] Terminal output of cargo test (task-1-hash-tests.log)
  - [ ] Terminal output of cargo tree (task-1-deps-check.log)

  **Commit**: YES
  - Message: `feat(utils): create polyplug_utils crate with hash functions`
  - Files: `crates/polyplug_utils/Cargo.toml`, `crates/polyplug_utils/src/lib.rs`
  - Pre-commit: `cargo test --package polyplug_utils`

- [x] 2. Create polyplug_codegen item-by-item trait architecture

  **What to do**:
  - Create `crates/polyplug_codegen/src/data.rs` - Language-agnostic data types:
    ```rust
    pub enum Item {
        Const(ConstInfo),
        Struct(StructInfo),
        Enum(EnumInfo),
        Union(UnionInfo),
        Function(FunctionInfo),
    }
    
    pub struct StructInfo {
        pub name: String,
        pub fields: Vec<FieldInfo>,
        pub doc: Option<String>,
        pub attributes: Vec<String>, // #[repr(C)], etc.
    }
    
    pub struct FieldInfo {
        pub name: String,
        pub rust_type: String,
        pub doc: Option<String>,
    }
    
    pub struct EnumInfo {
        pub name: String,
        pub repr: String, // "u32", "u64"
        pub variants: Vec<EnumVariant>,
        pub doc: Option<String>,
    }
    
    pub struct EnumVariant {
        pub name: String,
        pub value: Option<u64>,
        pub doc: Option<String>,
    }
    
    pub struct UnionInfo {
        pub name: String,
        pub variants: Vec<UnionVariant>,
        pub doc: Option<String>,
    }
    
    pub struct FunctionInfo {
        pub name: String,
        pub params: Vec<FieldInfo>,
        pub return_type: Option<String>,
        pub is_constexpr: bool, // For C++ constexpr functions
        pub doc: Option<String>,
    }
    
    pub struct ConstInfo {
        pub name: String,
        pub rust_type: String,
        pub value: String,
        pub doc: Option<String>,
    }
    ```
  
  - Create `crates/polyplug_codegen/src/context.rs` - Generation context:
    ```rust
    pub struct GenerationContext {
        /// Target language
        pub language: Language,
        /// Type mappings: Rust type → Target type
        pub type_map: HashMap<String, String>,
        /// Current indentation level
        pub indent: usize,
        /// Indentation string (e.g., "    ")
        pub indent_str: String,
    }
    
    impl GenerationContext {
        /// Map a Rust type to target language type
        pub fn map_type(&self, rust_type: &str) -> String {
            self.type_map.get(rust_type)
                .cloned()
                .unwrap_or_else(|| rust_type.to_string())
        }
        
        /// Create C++ context with type mappings
        pub fn cpp() -> Self { ... }
        
        /// Create C# context with type mappings
        pub fn csharp() -> Self { ... }
        
        // etc.
    }
    
    pub enum Language {
        Cpp,
        CSharp,
        Python,
        Lua,
        JavaScript,
    }
    ```
  
  - Create `crates/polyplug_codegen/src/generator.rs` - Item-by-item trait:
    ```rust
    /// Item-by-item code generator trait.
    ///
    /// Each language implements this trait to generate code one item at a time.
    /// This allows fine-grained control over ordering and interleaving.
    pub trait CodeGenerator {
        /// Generate a single constant declaration.
        fn generate_const(&self, item: &ConstInfo, ctx: &GenerationContext) -> String;
        
        /// Generate a single struct definition.
        fn generate_struct(&self, item: &StructInfo, ctx: &GenerationContext) -> String;
        
        /// Generate a single enum definition.
        fn generate_enum(&self, item: &EnumInfo, ctx: &GenerationContext) -> String;
        
        /// Generate a single union definition.
        fn generate_union(&self, item: &UnionInfo, ctx: &GenerationContext) -> String;
        
        /// Generate a single function declaration/implementation.
        fn generate_function(&self, item: &FunctionInfo, ctx: &GenerationContext) -> String;
        
        /// File extension for this language (e.g., "hpp", "cs", "py").
        fn file_extension(&self) -> &'static str;
        
        /// Language name for identification.
        fn language_name(&self) -> &'static str;
        
        /// Generate file header (includes, namespace start, etc.).
        fn generate_header(&self, ctx: &GenerationContext) -> String {
            String::new() // Default: no header
        }
        
        /// Generate file footer (namespace end, etc.).
        fn generate_footer(&self, ctx: &GenerationContext) -> String {
            String::new() // Default: no footer
        }
    }
    ```

  **Must NOT do**:
  - Add any parsing logic (no syn)
  - Add any file I/O
  - Know what ABI means
  - Maintain backward compatibility with old AbiGenerator

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex trait design with many types
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3, T4)
  - **Blocks**: T3 (language generators need trait), T5 (ABI build.rs needs trait)
  - **Blocked By**: None

  **References**:
  - `crates/polyplug_abi/build/abi_type_info.rs` - Reference existing type structures
  - `crates/polyplug_abi/build/laguages/cpp.rs:45-101` - Type mapping logic to extract
  - `crates/polyplug_abi/build/laguages/cpp.rs:213-237` - generate_struct pattern
  - `crates/polyplug_abi/build/laguages/cpp.rs:304-332` - generate_enum pattern

  **WHY Each Reference Matters**:
  - Study existing type structures to design Item enum
  - Extract type mapping logic to GenerationContext
  - Understand per-item generation patterns

  **Acceptance Criteria**:
  - [ ] All Item types defined (Const, Struct, Enum, Union, Function)
  - [ ] GenerationContext with type mapping works
  - [ ] CodeGenerator trait compiles with all methods
  - [ ] Breaking change: NO compatibility with old AbiGenerator

  **QA Scenarios**:

  ```
  Scenario: Item types compile correctly
    Tool: Bash (cargo check)
    Preconditions: data.rs created
    Steps:
      1. Run: cargo check --package polyplug_codegen
      2. Verify: No errors for Item, StructInfo, etc.
    Expected Result: Finished dev [unoptimized] target(s)
    Failure Indicators: Missing fields, type errors
    Evidence: .sisyphus/evidence/task-2-data-types.log

  Scenario: Context type mapping works
    Tool: Bash (cargo test)
    Preconditions: context.rs created
    Steps:
      1. Create test: ctx = GenerationContext::cpp();
      2. Assert ctx.map_type("u64") == "uint64_t"
      3. Assert ctx.map_type("usize") == "size_t"
    Expected Result: All type mappings correct
    Failure Indicators: Wrong mappings
    Evidence: .sisyphus/evidence/task-2-type-map.log

  Scenario: CodeGenerator trait compiles
    Tool: Bash (cargo check)
    Preconditions: generator.rs created
    Steps:
      1. Run: cargo check
      2. Verify: Trait with all methods exists
    Expected Result: Trait definition valid
    Failure Indicators: Missing methods, wrong signatures
    Evidence: .sisyphus/evidence/task-2-trait-compile.log
  ```

  **Evidence to Capture**:
  - [ ] Data types test (task-2-data-types.log)
  - [ ] Type mapping test (task-2-type-map.log)
  - [ ] Trait compile (task-2-trait-compile.log)

  **Commit**: YES
  - Message: `feat(codegen): add item-by-item CodeGenerator trait with Item enum`
  - Files: `crates/polyplug_codegen/src/data.rs`, `crates/polyplug_codegen/src/context.rs`, `crates/polyplug_codegen/src/generator.rs`
  - Pre-commit: `cargo test --package polyplug_codegen`

- [x] 3. Create item-by-item language generators in polyplug_codegen

  **What to do**:
  - Create `crates/polyplug_codegen/src/languages/mod.rs`:
    ```rust
    pub mod cpp;
    pub mod csharp;
    pub mod python;
    pub mod lua;
    pub mod js;
    
    pub use cpp::CppGenerator;
    pub use csharp::CSharpGenerator;
    pub use python::PythonGenerator;
    pub use lua::LuaGenerator;
    pub use js::JsGenerator;
    ```
  
  - Create `crates/polyplug_codegen/src/languages/cpp.rs`:
    ```rust
    use crate::{CodeGenerator, GenerationContext, ConstInfo, StructInfo, EnumInfo, UnionInfo, FunctionInfo};
    
    pub struct CppGenerator;
    
    impl CppGenerator {
        pub fn new() -> Self { CppGenerator }
        
        /// C++ type mapping: Rust → C++
        fn rust_to_cpp(rust_type: &str) -> String {
            match rust_type {
                "u64" => "uint64_t".to_string(),
                "u32" => "uint32_t".to_string(),
                "u16" => "uint16_t".to_string(),
                "u8" => "uint8_t".to_string(),
                "i64" => "int64_t".to_string(),
                "i32" => "int32_t".to_string(),
                "usize" => "size_t".to_string(),
                "isize" => "ptrdiff_t".to_string(),
                "bool" => "bool".to_string(),
                t if t.starts_with("*const ") => {
                    let inner = t.trim_start_matches("*const ").trim();
                    format!("const {}*", Self::rust_to_cpp(inner))
                }
                t if t.starts_with("*mut ") => {
                    let inner = t.trim_start_matches("*mut ").trim();
                    format!("{}*", Self::rust_to_cpp(inner))
                }
                other => other.to_string(), // ABI types like StringView
            }
        }
    }
    
    impl CodeGenerator for CppGenerator {
        fn generate_const(&self, item: &ConstInfo, _ctx: &GenerationContext) -> String {
            // C++: constexpr uint64_t NAME = value;
            format!("constexpr {} {} = {};\n", 
                Self::rust_to_cpp(&item.rust_type),
                item.name,
                item.value
            )
        }
        
        fn generate_struct(&self, item: &StructInfo, _ctx: &GenerationContext) -> String {
            // C++: struct Name { type field; };
            let mut output = String::new();
            if let Some(doc) = &item.doc {
                output.push_str(&format!("/// {}\n", doc));
            }
            output.push_str(&format!("struct {} {{\n", item.name));
            for field in &item.fields {
                let cpp_type = Self::rust_to_cpp(&field.rust_type);
                output.push_str(&format!("    {} {};\n", cpp_type, field.name));
            }
            output.push_str("};\n\n");
            output
        }
        
        fn generate_enum(&self, item: &EnumInfo, _ctx: &GenerationContext) -> String {
            // C++: enum class Name : repr { Variant = value, };
            let mut output = String::new();
            if let Some(doc) = &item.doc {
                output.push_str(&format!("/// {}\n", doc));
            }
            let repr = Self::rust_to_cpp(&item.repr);
            output.push_str(&format!("enum class {} : {} {{\n", item.name, repr));
            for (i, variant) in item.variants.iter().enumerate() {
                if let Some(val) = variant.value {
                    output.push_str(&format!("    {} = {},\n", variant.name, val));
                } else {
                    output.push_str(&format!("    {} = {},\n", variant.name, i));
                }
            }
            output.push_str("};\n\n");
            output
        }
        
        fn generate_union(&self, item: &UnionInfo, _ctx: &GenerationContext) -> String {
            // C++: union Name { type variant; };
            let mut output = String::new();
            output.push_str(&format!("union {} {{\n", item.name));
            for variant in &item.variants {
                let cpp_type = Self::rust_to_cpp(&variant.type_name);
                output.push_str(&format!("    {} {};\n", cpp_type, variant.name));
            }
            output.push_str("};\n\n");
            output
        }
        
        fn generate_function(&self, item: &FunctionInfo, _ctx: &GenerationContext) -> String {
            // C++: constexpr return_type name(params) { body }
            let ret_type = item.return_type.as_ref()
                .map(|t| Self::rust_to_cpp(t))
                .unwrap_or_else(|| "void".to_string());
            
            let params: String = item.params.iter()
                .map(|p| format!("{} {}", Self::rust_to_cpp(&p.rust_type), p.name))
                .collect::<Vec<_>>()
                .join(", ");
            
            if item.is_constexpr {
                format!("constexpr {} {}({}) {{ /* implementation */ }}\n\n", 
                    ret_type, item.name, params)
            } else {
                format!("{} {}({});\n\n", ret_type, item.name, params)
            }
        }
        
        fn file_extension(&self) -> &'static str { "hpp" }
        fn language_name(&self) -> &'static str { "cpp" }
        
        fn generate_header(&self, _ctx: &GenerationContext) -> String {
            "#pragma once\n#include <cstdint>\n#include <cstddef>\n\n".to_string()
        }
        
        fn generate_footer(&self, _ctx: &GenerationContext) -> String {
            "\n".to_string()
        }
    }
    ```
  
  - Create `crates/polyplug_codegen/src/languages/csharp.rs`:
    - Same pattern: `impl CodeGenerator for CSharpGenerator`
    - C# type mappings: "u64" → "ulong", "*const u8" → "IntPtr"
    - Generates: `public struct Name { public ulong Field; }`
  
  - Create `crates/polyplug_codegen/src/languages/python.rs`:
    - Same pattern: `impl CodeGenerator for PythonGenerator`
    - Python type mappings: "u64" → "ctypes.c_uint64"
    - Generates: `class Name(ctypes.Structure): _fields_ = [...]`
  
  - Create `crates/polyplug_codegen/src/languages/lua.rs`:
    - Lua-specific generation with local functions
  
  - Create `crates/polyplug_codegen/src/languages/js.rs`:
    - JavaScript/TypeScript generation with interfaces

  **Must NOT do**:
  - Parse any source files
  - Know ABI semantics
  - Maintain backward compatibility
  - Copy old batch-based logic - rewrite item-by-item

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: 5 language generators, complete rewrite
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T4)
  - **Blocks**: T4 (Cargo.toml update), T8 (ABI needs language generators)
  - **Blocked By**: T2 (needs CodeGenerator trait)

  **References**:
  - `crates/polyplug_abi/build/laguages/cpp.rs:45-101` - Type mapping logic to adapt
  - `crates/polyplug_abi/build/laguages/cpp.rs:213-237` - Per-struct generation pattern
  - `crates/polyplug_abi/build/laguages/csharp.rs` - C# patterns
  - `crates/polyplug_abi/build/laguages/python.rs` - Python patterns
  - `crates/polyplug_abi/build/laguages/lua.rs` - Lua patterns
  - `crates/polyplug_abi/build/laguages/js.rs` - JavaScript patterns

  **WHY Each Reference Matters**:
  - Extract type mapping logic and convert to item-by-item
  - Study language-specific syntax patterns
  - BREAKING CHANGE: Rewrite from batch to item-by-item

  **Acceptance Criteria**:
  - [ ] All 5 language generators compile
  - [ ] Each implements CodeGenerator trait with item-by-item methods
  - [ ] Type mappings work correctly for each language
  - [ ] Generated code is syntactically valid (no need to match old output exactly)

  **QA Scenarios**:

  ```
  Scenario: C++ generator produces valid struct
    Tool: Bash (cargo test)
    Preconditions: CppGenerator implemented
    Steps:
      1. Create StructInfo { name: "StringView", fields: [...] }
      2. Call generate_struct(&struct_info, &cpp_context)
      3. Parse output as C++ code (basic validation)
    Expected Result: Contains "struct StringView", valid field declarations
    Failure Indicators: Invalid C++ syntax
    Evidence: .sisyphus/evidence/task-3-cpp-struct.log

  Scenario: C# generator produces valid struct
    Tool: Bash (cargo test)
    Preconditions: CSharpGenerator implemented
    Steps:
      1. Create same StructInfo
      2. Call generate_struct with C# context
      3. Verify "public struct" and C# field syntax
    Expected Result: Valid C# struct with proper types
    Failure Indicators: Wrong syntax
    Evidence: .sisyphus/evidence/task-3-csharp-struct.log

  Scenario: Type mappings are correct
    Tool: Bash (cargo test)
    Steps:
      1. Test u64 → uint64_t (C++), UInt64 (C#), c_uint64 (Python)
      2. Test *const u8 → const uint8_t* (C++), IntPtr (C#)
    Expected Result: All type mappings correct per language
    Failure Indicators: Wrong types
    Evidence: .sisyphus/evidence/task-3-type-mappings.log
  ```

  **Evidence to Capture**:
  - [ ] C++ struct test (task-3-cpp-struct.log)
  - [ ] C# struct test (task-3-csharp-struct.log)
  - [ ] Type mappings test (task-3-type-mappings.log)

  **Commit**: YES
  - Message: `feat(codegen): add item-by-item language generators (cpp, csharp, python, lua, js)`
  - Files: `crates/polyplug_codegen/src/languages/*.rs`
  - Pre-commit: `cargo test --package polyplug_codegen -- languages`

- [x] 4. Update polyplug_codegen Cargo.toml (remove syn, toml deps)

  **What to do**:
  - Read current `crates/polyplug_codegen/Cargo.toml`
  - Remove syn dependency (if present)
  - Remove toml dependency (if present)
  - Remove serde dependency (if only used for parsing)
  - Add polyplug_utils dependency
  - Update crate to use polyplug_utils for hash functions
  - Ensure Cargo.toml uses `{ workspace = true }` for remaining deps

  **Must NOT do**:
  - Remove dependencies that are still needed
  - Add new dependencies beyond polyplug_utils
  - Change version numbers

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple dependency cleanup
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2, T3)
  - **Blocks**: T5 (ABI build.rs needs clean codegen deps), T12 (removing duplicate hashes)
  - **Blocked By**: T1 (polyplug_utils must exist first), T3 (generate.rs needs utils for hash functions)

  **References**:
  - `crates/polyplug_codegen/Cargo.toml` - Current dependencies to audit
  - `Cargo.toml` (workspace) - Reference for workspace dependency pattern
  - `crates/polyplug_codegen/src/ir.rs` - Check what hash functions are currently defined

  **WHY Each Reference Matters**:
  - Audit current deps to know what to remove
  - Follow workspace pattern for consistency
  - Find hash function usages to replace with polyplug_utils

  **Acceptance Criteria**:
  - [ ] `cargo tree --package polyplug_codegen` shows NO syn, NO toml
  - [ ] `cargo check --package polyplug_codegen` still passes
  - [ ] polyplug_utils appears in dependencies

  **QA Scenarios**:

  ```
  Scenario: Verify no parsing dependencies
    Tool: Bash (cargo tree)
    Preconditions: Cargo.toml updated
    Steps:
      1. Run: cargo tree --package polyplug_codegen | grep -E "^(syn|toml)"
      2. Verify: No matches found
    Expected Result: Empty output (no syn or toml in tree)
    Failure Indicators: syn or toml appears in output
    Evidence: .sisyphus/evidence/task-4-no-parsing-deps.log
  ```

  **Evidence to Capture**:
  - [ ] Terminal output showing dependency tree (task-4-no-parsing-deps.log)

  **Commit**: YES (grouped with T2, T3)
  - Message: `refactor(codegen): remove parsing deps, add polyplug_utils`
  - Files: `crates/polyplug_codegen/Cargo.toml`
  - Pre-commit: `cargo check --package polyplug_codegen`

- [x] 5. Create polyplug_abi build.rs with syn parsing

  **What to do**:
  - Create `crates/polyplug_abi/build.rs` (actual build script, not build/main.rs)
  - Add syn, quote as build-dependencies in Cargo.toml
  - Add polyplug_codegen and polyplug_utils as build-dependencies
  - Implement build.rs that:
    1. Reads `src/lib.rs` source
    2. Parses with syn to extract ABI types
    3. Calls polyplug_codegen for each target language
    4. Writes generated SDK files to `sdks/{lang}/abi/`
  - Add cargo:rerun-if-changed=src/lib.rs to avoid unnecessary rebuilds

  **Must NOT do**:
  - Put generation logic in src/ (must be build.rs)
  - Skip the rerun-if-changed directive

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex syn parsing, build.rs integration
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Wave 1)
  - **Parallel Group**: Wave 2
  - **Blocks**: T6 (type extraction), T7 (type mapping), T8 (language integration)
  - **Blocked By**: T2, T4 (data types and clean deps needed)

  **References**:
  - `crates/polyplug_abi/build/main.rs` - Current (broken) build entry
  - `crates/polyplug_abi/build/parser.rs` - Reference syn parsing logic
  - `crates/polyplug_abi/Cargo.toml` - Reference current build configuration
  - AGENTS.md Rule 11 - Workspace dependency management

  **WHY Each Reference Matters**:
  - Study current parser.rs for syn usage patterns
  - Ensure build.rs follows Cargo conventions
  - Follow workspace dependency rules

  **Acceptance Criteria**:
  - [ ] `cargo build --package polyplug_abi` triggers build.rs
  - [ ] build.rs runs syn parser on src/lib.rs
  - [ ] cargo:rerun-if-changed=src/lib.rs is emitted

  **QA Scenarios**:

  ```
  Scenario: Build.rs runs during compilation
    Tool: Bash (cargo build)
    Preconditions: build.rs created
    Steps:
      1. Run: cargo build --package polyplug_abi 2>&1
      2. Verify: Build script runs (look for "Running build script" or similar)
    Expected Result: build.rs executes, shows progress
    Failure Indicators: build.rs not triggered, compilation errors
    Evidence: .sisyphus/evidence/task-5-build-runs.log
  ```

  **Evidence to Capture**:
  - [ ] Terminal output of cargo build (task-5-build-runs.log)

  **Commit**: YES
  - Message: `feat(abi): add build.rs with syn parsing`
  - Files: `crates/polyplug_abi/build.rs`, `crates/polyplug_abi/Cargo.toml` (build-deps)
  - Pre-commit: `cargo build --package polyplug_abi`

- [x] 6. Create polyplug_abi src/build/ with type extraction

  **What to do**:
  - Create `crates/polyplug_abi/src/build/` directory
  - Move/adapt parser logic from old `build/parser.rs`
  - Create `mod.rs` with type extraction functions:
    - `extract_structs(ast: &syn::File) -> Vec<StructInfo>`
    - `extract_enums(ast: &syn::File) -> Vec<EnumInfo>`
    - `extract_functions(ast: &syn::File) -> Vec<FunctionInfo>`
  - Create `types.rs` with local ABI-specific types:
    - `AbiType` enum (Struct, Enum, Union, Function)
    - `AbiStruct`, `AbiEnum`, `AbiFunction` structs
    - `AbiField` for struct/enum fields
  - Create `extractor.rs` with logic to extract from syn AST

  **Must NOT do**:
  - Delete old build/ yet (will do in T10)
  - Put code generation logic here (just extraction)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex syn AST traversal
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (within Wave 2)
  - **Parallel Group**: Wave 2 (with T7, T8)
  - **Blocks**: T7 (type mapping needs extracted types)
  - **Blocked By**: T5 (build.rs structure first)

  **References**:
  - `crates/polyplug_abi/build/parser.rs` - Reference existing parsing logic
  - `crates/polyplug_codegen/src/data.rs` - Know what GenerationData expects
  - syn crate docs for AST traversal

  **WHY Each Reference Matters**:
  - Adapt existing parser logic to new structure
  - Ensure extracted types map cleanly to GenerationData

  **Acceptance Criteria**:
  - [ ] `cargo check --package polyplug_abi` passes
  - [ ] All #[repr(C)] structs from src/lib.rs are extracted
  - [ ] All #[repr(C)] enums from src/lib.rs are extracted

  **QA Scenarios**:

  ```
  Scenario: Extract ABI types from source
    Tool: Bash (cargo test --package polyplug_abi)
    Preconditions: build/ module created
    Steps:
      1. Add test that parses src/lib.rs
      2. Verify StringView, Buffer, PluginInterface are extracted
    Expected Result: All frozen ABI types found
    Failure Indicators: Missing types, parsing errors
    Evidence: .sisyphus/evidence/task-6-extract-test.log
  ```

  **Evidence to Capture**:
  - [ ] Test output showing extracted types (task-6-extract-test.log)

  **Commit**: YES (grouped with T5)
  - Message: `feat(abi): add type extraction module`
  - Files: `crates/polyplug_abi/src/build/mod.rs`, `crates/polyplug_abi/src/build/types.rs`, `crates/polyplug_abi/src/build/extractor.rs`
  - Pre-commit: `cargo test --package polyplug_abi`

- [x] 7. Implement type mapping (Rust → target languages)

  **What to do**:
  - Create `crates/polyplug_abi/src/build/mapper.rs`
  - Implement `map_abi_to_generation_data(abi: &AbiType, lang: TargetLang) -> GenerationData`
  - Implement `map_rust_to_target(rust_type: &str, lang: TargetLang) -> String`:
    - For C++: `*const u8` → `const uint8_t*`, `usize` → `size_t`, etc.
    - For C#: `*const u8` → `IntPtr`, `usize` → `UIntPtr`, etc.
    - For Python: `*const u8` → `ctypes.POINTER(ctypes.c_uint8)`, etc.
    - For Lua: `*const u8` → `const char*`, etc.
    - For JavaScript: `*const u8` → `Uint8Array`, etc.
  - Handle special types: StringView, Buffer, PluginInterface, etc.
  - Add hash function GenerationData (so all SDKs get fnv1a_64, contract_id, etc.)

  **Must NOT do**:
  - Put type mapping in polyplug_codegen (must stay in polyplug_abi)
  - Assume types not in current SDKs

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex type mapping for 5 languages
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (within Wave 2)
  - **Parallel Group**: Wave 2 (with T6, T8)
  - **Blocks**: T8 (language generators need mapped data), T9 (hash functions)
  - **Blocked By**: T6 (needs AbiType definitions)

  **References**:
  - Current SDK files in `sdks/` - Reference existing type mappings
  - `crates/polyplug_abi/build/languages/*.rs` - See how types are currently mapped
    - `crates/polyplug_abi/build/languages/cpp.rs` - C++ type mappings
    - `crates/polyplug_abi/build/languages/csharp.rs` - C# type mappings
    - `crates/polyplug_abi/build/languages/python.rs` - Python type mappings
    - `crates/polyplug_abi/build/languages/lua.rs` - Lua type mappings
    - `crates/polyplug_abi/build/languages/javascript.rs` - JavaScript type mappings
  - `crates/polyplug_abi/src/lib.rs` - Reference all Rust ABI types

  **WHY Each Reference Matters**:
    - Study current SDK output to match existing type mappings
    - Ensure generated SDKs are byte-identical to current
    - Know all ABI types that need mapping

  **Acceptance Criteria**:
  - [ ] All ABI types from src/lib.rs are mapped to all 5 languages
  - [ ] Generated types match current SDK files byte-for-byte
  - [ ] Hash functions mapped to all languages

  **QA Scenarios**:

  ```
  Scenario: Type mapping produces correct output
    Tool: Bash (cargo test)
    Preconditions: mapper.rs implemented
    Steps:
      1. Map StringView to each language
      2. Compare output to current SDK files
    Expected Result: Generated code matches existing SDKs
    Failure Indicators: Type mismatches, wrong field names
    Evidence: .sisyphus/evidence/task-7-type-map.log

  Scenario: Hash functions in all languages
    Tool: Bash (cargo test)
    Preconditions: mapper.rs with hash functions
    Steps:
      1. Generate hash functions for C++, C#, Python, Lua, JS
      2. Verify fnv1a_64 exists in each
    Expected Result: All languages have fnv1a_64 implementation
    Failure Indicators: Missing functions, wrong signatures
    Evidence: .sisyphus/evidence/task-7-hash-funcs.log
  ```

  **Evidence to Capture**:
  - [ ] Generated SDK comparisons (task-7-type-map.log)
  - [ ] Hash function outputs (task-7-hash-funcs.log)

  **Commit**: YES (grouped with T6)
  - Message: `feat(abi): add type mapping to all languages`
  - Files: `crates/polyplug_abi/src/build/mapper.rs`
  - Pre-commit: `cargo test --package polyplug_abi`

- [x] 8. Integrate language generators from old build/

  **What to do**:
  - Create `crates/polyplug_abi/src/build/generate.rs`
  - Import/adapt language generators from `build/languages/*.rs`
  - Create `generate_language_sdk(lang: TargetLang, types: &[AbiType]) -> String`
  - For each language:
    1. Map AbiTypes to GenerationData (using T7 mapper)
    2. Call polyplug_codegen::generate() for each type
    3. Combine outputs into complete SDK file
    4. Add language-specific header/footer
  - Handle special cases:
    - C++: namespace polyplug, header guards
    - C#: namespace Polyplug.Abi
    - Python: module-level exports
    - Lua: local functions pattern
    - JavaScript: TypeScript interfaces + runtime

  **Must NOT do**:
  - Put generation logic in polyplug_codegen (it stays dumb)
  - Break existing SDK structure

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex multi-language integration
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (within Wave 2)
  - **Parallel Group**: Wave 2 (with T6, T7)
  - **Blocks**: T9 (hash functions need full SDK), T10 (cleanup after verification)
  - **Blocked By**: T3 (generators), T7 (type mapping)

  **References**:
  - `crates/polyplug_abi/build/laguages/cpp.rs` - C++ SDK generation (NOTE: path has typo "laguages")
  - `crates/polyplug_abi/build/laguages/csharp.rs` - C# SDK generation (NOTE: path has typo)
  - `crates/polyplug_abi/build/laguages/python.rs` - Python SDK generation (NOTE: path has typo)
  - `crates/polyplug_abi/build/laguages/lua.rs` - Lua SDK generation (NOTE: path has typo)
  - `crates/polyplug_abi/build/laguages/js.rs` - JavaScript SDK generation (NOTE: path has typo, file is js.rs)
  - Current SDK files in `sdks/` - Reference output format

  **WHY Each Reference Matters**:
  - Adapt existing generator logic to new dumb generator pattern
  - Ensure output matches current SDK structure

  **Acceptance Criteria**:
  - [ ] All 5 language SDKs are generated
  - [ ] Generated files match current SDKs byte-for-byte
  - [ ] SDKs include hash functions (fnv1a_64, contract_id, etc.)

  **QA Scenarios**:

  ```
  Scenario: Generate complete SDKs
    Tool: Bash (cargo build)
    Preconditions: generate.rs implemented
    Steps:
      1. Build polyplug_abi
      2. Check generated files in sdks/{lang}/abi/
    Expected Result: All SDK files present and valid
    Failure Indicators: Missing files, syntax errors
    Evidence: .sisyphus/evidence/task-8-sdk-files.log

  Scenario: Byte-identical output
    Tool: Bash (diff)
    Preconditions: SDKs generated
    Steps:
      1. diff sdks/cpp/abi/polyplug/abi.hpp expected/cpp_abi.hpp
      2. Repeat for other languages
    Expected Result: No differences (or documented intentional changes)
    Failure Indicators: Unexpected differences
    Evidence: .sisyphus/evidence/task-8-diff.log
  ```

  **Evidence to Capture**:
  - [ ] ls -la of generated SDKs (task-8-sdk-files.log)
  - [ ] diff output (task-8-diff.log)

  **Commit**: YES
  - Message: `feat(abi): integrate language SDK generators`
  - Files: `crates/polyplug_abi/src/build/generate.rs`
  - Pre-commit: `cargo build --package polyplug_abi && test -f sdks/cpp/abi/polyplug/abi.hpp`

- [x] 9. Add hash function generation for all languages

  **What to do**:
  - In `crates/polyplug_abi/src/build/generate.rs`, add hash function GenerationData
  - Create GenerationData for:
    - `fnv1a_64(data: &[u8]) -> u64` → function in all languages
    - `contract_id(name: &str, major: u32) -> u64` → function in all languages
    - `bundle_id(name: &str) -> u64` → function in all languages
    - `host_contract_id(name: &str, major: u32) -> u64` → function in all languages
    - `plugin_contract_id(name: &str, major: u32) -> u64` → function in all languages
  - Ensure each language gets idiomatic implementation:
    - C++: inline functions in namespace polyplug
    - C#: static methods in class Abi
    - Python: module-level functions
    - Lua: local functions exported to module
    - JavaScript: exported functions
  - Use polyplug_utils for Rust implementation

  **Must NOT do**:
  - Implement hash functions differently per language (use same algorithm)
  - Skip any hash function (all 5 must be present)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Algorithm consistency across 5 languages
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (within Wave 2)
  - **Parallel Group**: Wave 2 (can run with T8)
  - **Blocks**: T14 (full test suite needs complete SDKs)
  - **Blocked By**: T7 (type mapping), T8 (generator integration)

  **References**:
  - `crates/polyplug_utils/src/lib.rs` - Reference FNV-1a algorithm
  - `crates/polyplug_abi/src/lib.rs:631-695` - Reference all hash functions
  - `sdks/cpp/abi/polyplug/abi.hpp` - Check if hash functions already exist (they don't!)
  - `sdks/csharp/abi/Abi.cs` - Check for hash functions

  **WHY Each Reference Matters**:
  - Copy exact FNV-1a algorithm to all languages
  - Ensure API parity across languages

  **Acceptance Criteria**:
  - [ ] All 5 hash functions exist in all 5 language SDKs
  - [ ] Golden value tests pass for all languages
  - [ ] C++ SDK now has host_contract_id and plugin_contract_id

  **QA Scenarios**:

  ```
  Scenario: Hash functions in C++ SDK
    Tool: Read (file)
    Preconditions: SDKs generated
    Steps:
      1. Read sdks/cpp/abi/polyplug/abi.hpp
      2. Search for fnv1a_64, contract_id, bundle_id, host_contract_id, plugin_contract_id
    Expected Result: All 5 functions present
    Failure Indicators: Missing functions
    Evidence: .sisyphus/evidence/task-9-cpp-hash.log

  Scenario: Hash golden values match
    Tool: Bash (test)
    Preconditions: SDKs generated with hash functions
    Steps:
      1. Run golden value tests for each language
      2. Verify fnv1a_64("test") produces same value across all
    Expected Result: All languages produce identical hash values
    Failure Indicators: Mismatched hashes
    Evidence: .sisyphus/evidence/task-9-golden.log
  ```

  **Evidence to Capture**:
  - [ ] C++ SDK hash function listing (task-9-cpp-hash.log)
  - [ ] Golden value test results (task-9-golden.log)

  **Commit**: YES
  - Message: `feat(abi): add hash functions to all language SDKs`
  - Files: `crates/polyplug_abi/src/build/generate.rs` (updated)
  - Pre-commit: `grep -E "(fnv1a_64|contract_id|bundle_id)" sdks/cpp/abi/polyplug/abi.hpp`

- [x] 10. Delete old polyplug_abi/build/ directory

  **What to do**:
  - Remove `crates/polyplug_abi/build/` directory entirely
  - Remove `build = "build/main.rs"` from Cargo.toml (if still present)
  - Ensure no references to old build/ remain in code
  - Update .gitignore if build/ was ignored

  **Must NOT do**:
  - Delete until T8 and T9 verify new system works
  - Lose any code not yet migrated

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple cleanup
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3)
  - **Parallel Group**: Wave 3 (with T11, T12, T13)
  - **Blocks**: None (cleanup task)
  - **Blocked By**: T8, T9 (must verify new system works first)

  **References**:
  - `crates/polyplug_abi/build/` - Directory to delete
  - `crates/polyplug_abi/Cargo.toml` - Check for build = "build/main.rs"

  **Acceptance Criteria**:
  - [ ] build/ directory does not exist
  - [ ] `cargo build --package polyplug_abi` still works
  - [ ] No references to old build/ in codebase

  **QA Scenarios**:

  ```
  Scenario: Old build directory removed
    Tool: Bash (ls)
    Preconditions: T8 and T9 complete
    Steps:
      1. Run: ls crates/polyplug_abi/build/ 2>&1
      2. Verify: "No such file or directory"
    Expected Result: Directory successfully removed
    Failure Indicators: Directory still exists
    Evidence: .sisyphus/evidence/task-10-cleanup.log
  ```

  **Evidence to Capture**:
  - [ ] ls output showing directory removed (task-10-cleanup.log)

  **Commit**: YES
  - Message: `chore(abi): remove old build/ directory`
  - Files: `crates/polyplug_abi/build/` (deleted)
  - Pre-commit: `! test -d crates/polyplug_abi/build/`

- [x] 11. Remove duplicate hash functions from polyplug_abi/src/lib.rs

  **What to do**:
  - Read `crates/polyplug_abi/src/lib.rs`
  - Find hash function implementations (fnv1a_64, contract_id, etc.)
  - Replace with `use polyplug_utils::{fnv1a_64, contract_id, ...};`
  - Ensure all usages updated to use polyplug_utils
  - Update Cargo.toml to add polyplug_utils dependency

  **Must NOT do**:
  - Delete hash functions without adding polyplug_utils import
  - Change function signatures

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple refactoring
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3)
  - **Parallel Group**: Wave 3 (with T10, T12, T13)
  - **Blocks**: T14 (tests need working code)
  - **Blocked By**: T1 (polyplug_utils must exist)

  **References**:
  - `crates/polyplug_abi/src/lib.rs:631-695` - Hash functions to replace
  - `crates/polyplug_abi/Cargo.toml` - Add polyplug_utils dependency

  **Acceptance Criteria**:
  - [ ] Hash functions removed from lib.rs
  - [ ] `use polyplug_utils::...` added
  - [ ] `cargo check --package polyplug_abi` passes

  **QA Scenarios**:

  ```
  Scenario: No duplicate hash functions
    Tool: Bash (grep)
    Preconditions: Changes made
    Steps:
      1. Run: grep -n "fn fnv1a_64" crates/polyplug_abi/src/lib.rs
      2. Verify: Only import statement found, not function definition
    Expected Result: Import only, no duplicate implementation
    Failure Indicators: Function still defined in lib.rs
    Evidence: .sisyphus/evidence/task-11-no-dupes.log
  ```

  **Evidence to Capture**:
  - [ ] grep output showing imports only (task-11-no-dupes.log)

  **Commit**: YES
  - Message: `refactor(abi): use polyplug_utils for hash functions`
  - Files: `crates/polyplug_abi/src/lib.rs`, `crates/polyplug_abi/Cargo.toml`
  - Pre-commit: `cargo check --package polyplug_abi`

- [x] 12. Remove duplicate hash functions from polyplug_codegen

  **What to do**:
  - Read `crates/polyplug_codegen/src/ir.rs` and other files
  - Find hash function implementations
  - Replace with `use polyplug_utils::{...};`
  - Update all usages

  **Must NOT do**:
  - Delete hash functions without ensuring polyplug_utils is available

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple refactoring
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (Wave 3)
  - **Parallel Group**: Wave 3 (with T10, T11, T13)
  - **Blocks**: T14 (tests need working code)
  - **Blocked By**: T1 (polyplug_utils must exist), T4 (polyplug_codegen has utils dep)

  **References**:
  - `crates/polyplug_codegen/src/ir.rs:11-19` - fnv1a_64 to remove
  - Search for other hash function locations

  **Acceptance Criteria**:
  - [ ] No hash functions defined in polyplug_codegen
  - [ ] All usages use polyplug_utils
  - [ ] `cargo check --package polyplug_codegen` passes

  **QA Scenarios**:

  ```
  Scenario: No hash functions in codegen
    Tool: Bash (grep)
    Steps:
      1. Run: grep -rn "fn fnv1a_64" crates/polyplug_codegen/src/
      2. Verify: Only imports found
    Expected Result: Import only
    Failure Indicators: Function definitions remain
    Evidence: .sisyphus/evidence/task-12-codegen-clean.log
  ```

  **Evidence to Capture**:
  - [ ] grep output (task-12-codegen-clean.log)

  **Commit**: YES (grouped with T11)
  - Message: `refactor(codegen): use polyplug_utils for hash functions`
  - Files: `crates/polyplug_codegen/src/ir.rs`
  - Pre-commit: `cargo check --package polyplug_codegen`

