# ABI Unification & SDK Restructure

## TL;DR

> **Quick Summary**: Reorganize host-libs and guest-libs into unified `sdks/` folder with auto-generated shared ABI libraries. Eliminate PluginVTable duplication. Add string helpers to ABI libraries (shared by host and guest, native per language for zero overhead).
> 
> **Deliverables**:
> - `crates/polyplug_abi/build/` - Modular ABI code generator
> - `sdks/csharp/`, `sdks/python/`, `sdks/cpp/`, `sdks/lua/`, `sdks/js/` - Unified SDK folders
> - Each SDK contains: `abi/` (shared), `host/`, `guest/`, `loaders/`
> - PluginInterface everywhere (PluginVTable eliminated)
> - String helpers (`strip_prefix`, `starts_with`, `split`) in ABI libraries (native per language for zero overhead)
> 
> **Estimated Effort**: Large (XL)
> **Parallel Execution**: YES - Multiple waves with parallelizable tasks
> **Critical Path**: build.rs trait → C# generator → C# SDK → E2E test

---

## Context

### Original Request
Major refactor to:
1. Fix PluginVTable → PluginInterface (incomplete migration in C#/Python)
2. Create shared ABI libraries auto-generated from Rust source
3. Reorganize into `sdks/` folder with one subfolder per language
4. Add string helpers to guest-libs

### Interview Summary
**Key Discussions**:
- Folder name: `sdks/` with nested subfolders (abi/, host/, guest/, loaders/)
- Generation scope: Full ABI file via modular build.rs
- Source of truth: polyplug_abi Rust types parsed with syn/quote
- PluginVTable: Remove entirely, use PluginInterface directly
- No backward compatibility needed (pre-MVP)
- Helpers: Add strip_prefix, starts_with, split to guest-libs
- JS: Uses BigInt for 64-bit (no lo/hi split)
- Test strategy: Full E2E tests
- **Rust**: Keep in `crates/` (Cargo workspace conventions - no `sdks/rust/`)

**Research Findings**:
- C# host MISSING 3 constants (ABI_ERROR_NOT_FOUND, STALE_HANDLE, FUNCTION_NOT_AVAIL)
- Python host MISSING 4 constants
- C++ host/guest abi.hpp are nearly identical (duplication)
- Python host/guest abi.py are nearly identical (duplication)
- 25 guest plugins all use strip_prefix pattern - massive duplication
- No Rust host-lib folder exists (crates/polyplug IS the Rust host)

### Metis Review
**Identified Gaps** (addressed):
- C# HostVTable has wrong field order (AllocPtr first, should be RegisterPluginPtr) → Fixed by regeneration from Rust source
- C# PluginContext missing HostAbiVersion and BundleId fields → Fixed by regeneration from Rust source
- Need ABI layout tests to verify struct sizes match across languages → Added in Task 5 QA scenario

### Architecture Clarification

**AbiGenerator vs CodeGenerator**:
- `CodeGenerator` (in `polyplug_codegen`) generates **contract callers** for host and **vtable stubs** for guest
- `AbiGenerator` (NEW, in `polyplug_abi/build/`) generates **only ABI types and constants** (StringView, PluginInterface, ABI_OK, etc.)
- These are **separate concerns**:
  - AbiGenerator: Foundation types used by all code
  - CodeGenerator: Contract-specific generated code that uses ABI types

**Why Rust stays in crates/**:
- Cargo workspace conventions require Rust crates in `crates/` for proper dependency resolution
- `crates/polyplug_abi` = Rust ABI types
- `crates/polyplug` = Rust host runtime
- `crates/polyplug_guest` = Rust guest library
- No need for `sdks/rust/` - Rust handles this differently

---

## Migration Strategy

### Branch Strategy
- Work on feature branch `feature/unified-sdk`
- Each wave is a separate commit for easy rollback
- Merge to main only after Final Verification Wave passes

### Rollback Strategy
- Each wave is a separate commit: `git revert <commit>` to roll back specific wave
- Keep `host-libs/` and `guest-libs/` until Task 55 (Final E2E test) passes
- If migration fails, restore from backup branch

### Breaking Changes
Since this is **pre-MVP**:
- No deprecation period needed
- No backward compatibility
- PluginVTable removed entirely, all code uses PluginInterface
- This will be the first stable release

### String Helper API Specification

**Location**: **ABI library** (shared by host and guest), NOT in guest-lib

All ABI libraries will implement these exact APIs **natively** (not via FFI) for maximum performance:

**Why Native (not FFI)**:
- FFI overhead: ~5-10ns per call
- Simple string ops: ~1-2ns native
- FFI would make helpers **3-6x slower** - defeats zero-overhead goal

```rust
// Rust (in crates/polyplug_abi/src/helpers.rs)
pub fn strip_prefix(sv: StringView, prefix: &str) -> &str;
pub fn starts_with(sv: StringView, prefix: &str) -> bool;
pub fn split(sv: StringView, delimiter: char) -> Vec<&str>;
```

```csharp
// C# (in sdks/csharp/abi/StringHelpers.cs)
public static string StripPrefix(StringView sv, string prefix);
public static bool StartsWith(StringView sv, string prefix);
public static string[] Split(StringView sv, char delimiter);
```

```python
# Python (in sdks/python/abi/helpers.py)
def strip_prefix(sv: StringView, prefix: str) -> str;
def starts_with(sv: StringView, prefix: str) -> bool;
def split(sv: StringView, delimiter: str) -> list[str];
```

```cpp
// C++ (in sdks/cpp/abi/polyplug/helpers.hpp)
std::string_view strip_prefix(StringView sv, std::string_view prefix);
bool starts_with(StringView sv, std::string_view prefix);
std::vector<std::string_view> split(StringView sv, char delimiter);
```

```lua
-- Lua (in sdks/lua/abi/polyplug_abi.lua)
function strip_prefix(sv, prefix) -- returns string
function starts_with(sv, prefix) -- returns boolean
function split(sv, delimiter) -- returns table of strings
```

```typescript
// JavaScript (in sdks/js/abi/polyplug_abi.ts)
function stripPrefix(sv: StringView, prefix: string): string;
function startsWith(sv: StringView, prefix: string): boolean;
function split(sv: StringView, delimiter: string): string[];
```

**Behavior**:
- `strip_prefix`: Returns the string without prefix if it starts with prefix, otherwise returns original string
- `starts_with`: Returns true if string starts with prefix
- `split`: Splits string by delimiter, returns array of strings

---

## Work Objectives

### Core Objective
Unify all language bindings under a single `sdks/` structure with auto-generated shared ABI libraries, eliminating duplication and ensuring consistency across all languages.

### Concrete Deliverables
- `crates/polyplug_abi/build/` - Modular code generator with trait-based architecture
- `sdks/csharp/` - Complete C# SDK with Polyplug.ABI, Polyplug.Host, Polyplug.Guest
- `sdks/python/` - Complete Python SDK with polyplug_abi, host, guest packages
- `sdks/cpp/` - Complete C++ SDK with shared headers
- `sdks/lua/` - Complete Lua SDK with shared FFI definitions
- `sdks/js/` - Complete JavaScript/TypeScript SDK
- String helpers added to all guest-libs
- PluginVTable removed everywhere, PluginInterface used directly

### Definition of Done
- [ ] `cargo build` succeeds
- [ ] `cargo test --all` passes
- [ ] All 5 SDKs compile and pass E2E tests with example hosts/guests
- [ ] No PluginVTable references remain (except in comments/docs)
- [ ] All ABI constants exported in all languages

### Must Have
- Modular build.rs with AbiGenerator trait
- One folder per language in sdks/
- Shared ABI library used by both host and guest
- PluginInterface used everywhere (no PluginVTable)
- String helpers in ABI libraries (native per language for zero overhead)
- E2E tests passing for all languages

### Must NOT Have (Guardrails)
- Do NOT change #[repr(C)] struct layouts (frozen ABI)
- Do NOT add new ABI types not in scope
- Do NOT refactor polyplug_codegen architecture
- Do NOT mix generated and hand-written code in same file
- Do NOT create deprecated aliases (no backward compatibility needed)

---

## Verification Strategy (MANDATORY)

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test, examples/)
- **Automated tests**: TDD for build.rs modules, E2E for SDKs
- **Framework**: cargo test, dotnet test, pytest, etc.
- **Agent-Executed QA**: Full E2E tests with example hosts/guests

### QA Policy
Every task MUST include agent-executed QA scenarios.
Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation - build.rs infrastructure):
├── Task 1: Create AbiGenerator trait in build/mod.rs [quick]
├── Task 2: Create Rust type parser using syn/quote [deep]
├── Task 3: Create output file writer utilities [quick]
└── Task 4: Integrate build.rs main orchestrator [quick]

Wave 2 (C# SDK - first language, establishes patterns):
├── Task 5: Create C# AbiGenerator implementation [deep]
├── Task 6: Create sdks/csharp/ folder structure [quick]
├── Task 7: Create Polyplug.ABI project (generated) [unspecified-high]
├── Task 8: Migrate Polyplug.Host to new structure [unspecified-high]
├── Task 9: Migrate Polyplug.Guest to new structure [unspecified-high]
└── Task 10: Migrate C# loaders to sdks/csharp/loaders/ [unspecified-high]

Wave 3 (C# Verification & Cleanup):
├── Task 11: Update C# examples to use new SDK [unspecified-high]
├── Task 12: Remove PluginVTable from C# code [quick]
├── Task 13: Add string helpers to C# guest-lib [quick]
└── Task 14: E2E test C# SDK with examples [deep]

Wave 4 (Python SDK):
├── Task 15: Create Python AbiGenerator implementation [deep]
├── Task 16: Create sdks/python/ folder structure [quick]
├── Task 17: Create polyplug_abi package (generated) [unspecified-high]
├── Task 18: Migrate polyplug host to sdks/python/host/ [unspecified-high]
├── Task 19: Migrate polyplug_guest to sdks/python/guest/ [unspecified-high]
├── Task 20: Migrate Python loaders to sdks/python/loaders/ [unspecified-high]
├── Task 21: Remove PluginVTable from Python code [quick]
├── Task 22: Add string helpers to Python guest-lib [quick]
└── Task 23: E2E test Python SDK with examples [deep]

Wave 5 (C++ SDK):
├── Task 24: Create C++ AbiGenerator implementation [deep]
├── Task 25: Create sdks/cpp/ folder structure [quick]
├── Task 26: Create generated abi.hpp [unspecified-high]
├── Task 27: Migrate C++ host to sdks/cpp/host/ [unspecified-high]
├── Task 28: Migrate C++ guest to sdks/cpp/guest/ [unspecified-high]
├── Task 29: Migrate C++ loaders to sdks/cpp/loaders/ [unspecified-high]
├── Task 30: Remove PluginVTable from C++ code [quick]
├── Task 31: Add string helpers to C++ guest-lib [quick]
└── Task 32: E2E test C++ SDK with examples [deep]

Wave 6 (Lua SDK):
├── Task 33: Create Lua AbiGenerator implementation [deep]
├── Task 34: Create sdks/lua/ folder structure [quick]
├── Task 35: Create generated polyplug_abi.lua [unspecified-high]
├── Task 36: Migrate Lua host to sdks/lua/host/ [unspecified-high]
├── Task 37: Migrate Lua guest to sdks/lua/guest/ [unspecified-high]
├── Task 38: Migrate Lua loaders to sdks/lua/loaders/ [unspecified-high]
├── Task 39: Remove PluginVTable from Lua code [quick]
├── Task 40: Add string helpers to Lua guest-lib [quick]
└── Task 41: E2E test Lua SDK with examples [deep]

Wave 7 (JavaScript SDK):
├── Task 42: Create JavaScript AbiGenerator implementation [deep]
├── Task 43: Create sdks/js/ folder structure [quick]
├── Task 44: Create generated polyplug_abi.js + .d.ts [unspecified-high]
├── Task 45: Migrate JS host to sdks/js/host/ [unspecified-high]
├── Task 46: Migrate JS guest to sdks/js/guest/ [unspecified-high]
├── Task 47: Migrate JS loaders to sdks/js/loaders/ [unspecified-high]
├── Task 48: Remove PluginVTable from JS code [quick]
├── Task 49: Add string helpers to JS guest-lib [quick]
└── Task 50: E2E test JS SDK with examples [deep]

Wave 8 (Cleanup & Documentation):
├── Task 51: Remove old host-libs/ directory [quick]
├── Task 52: Remove old guest-libs/ directory [quick]
├── Task 53: Remove scripts/ directory (duplicates docs/) [quick]
├── Task 54: Update Cargo.toml workspace members [quick]
├── Task 55: Documentation cleanup and accuracy review [writing]
└── Task 56: Final E2E test all SDKs [deep]

Wave FINAL (Verification - parallel reviews):
├── Task F1: Plan compliance audit (oracle)
├── Task F2: Code quality review (unspecified-high)
├── Task F3: Full E2E test matrix (deep)
└── Task F4: Scope fidelity check (deep)
-> Present results -> Get explicit user okay

Critical Path: Task 1 → Task 2 → Task 5 → Task 7 → Task 14 → Wave 4 → Wave 5 → Wave 6 → Wave 7 → Wave 8 → FINAL
Parallel Speedup: ~60% faster than sequential
Max Concurrent: 4 (Waves 4-7 can overlap for different languages)
```

### Agent Dispatch Summary
- **Wave 1**: 4 tasks → `quick`, `deep`, `quick`, `quick`
- **Wave 2-3**: 10 tasks → `deep`, `quick`, `unspecified-high` x3, `quick` x2, `deep`
- **Wave 4-7**: 9 tasks each → `deep`, `quick`, `unspecified-high` x3, `quick` x2, `deep`
- **Wave 8**: 6 tasks → `quick` x4, `writing`, `deep`
- **FINAL**: 4 tasks → `oracle`, `unspecified-high`, `deep`, `deep`

---

## TODOs

- [x] 1. Create AbiGenerator trait in build/mod.rs

  **What to do**:
  - Create `crates/polyplug_abi/build/mod.rs`
  - Define `AbiGenerator` trait with methods: `generate_constants()`, `generate_structs()`, `generate_helpers()`, `file_extension()`, `output_path()`
  - Define `AbiInfo` struct to hold extracted type information
  - Create helper functions for common formatting

  **Must NOT do**:
  - Do NOT implement language-specific logic here (that goes in language modules)
  - Do NOT hardcode any language-specific output

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO - Foundation task
  - **Blocks**: Tasks 2-4, all subsequent tasks
  - **Blocked By**: None

  **References**:
  - `crates/polyplug_codegen/src/generators/mod.rs:68-87` - CodeGenerator trait pattern to follow

  **Acceptance Criteria**:
  - [ ] `crates/polyplug_abi/build/mod.rs` exists
  - [ ] `AbiGenerator` trait defined with required methods
  - [ ] `cargo check -p polyplug_abi` passes

**QA Scenarios**:
  ```
  Scenario: build.rs runs without errors
    Tool: Bash
    Steps:
      1. cd crates/polyplug_abi && cargo build
    Expected Result: build.rs executes, no panics
    Evidence: .sisyphus/evidence/task-04-build-rs.log
  ```

- [x] 2. Create Rust type parser using syn/quote (combined with Task 1)

- [x] 5. Create C# AbiGenerator implementation

  **What to do**:
  - Create `crates/polyplug_abi/build/csharp.rs`
  - Implement `AbiGenerator` trait for C#
  - Generate C# code for: constants (`public const uint`), structs (`[StructLayout]`), enums (`public enum`), helpers
  - Handle C#-specific: `nint` for pointers, `ulong` for u64, `[FieldOffset]` for unions
  - Generate `AbiConstants` static class
  - Generate FNV-1a hash functions (`ContractId.Compute()`)

  **Must NOT do**:
  - Do NOT generate PluginVTable (use PluginInterface only)
  - Do NOT change any struct field order from Rust source

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO - First language implementation, sets pattern
  - **Blocks**: Tasks 6-14
  - **Blocked By**: Tasks 1-4

  **References**:
  - `crates/polyplug_abi/src/lib.rs:332-361` - PluginInterface definition
  - `crates/polyplug_abi/src/lib.rs:19-28` - Constants
  - `guest-libs/csharp/src/Abi.cs` - Current C# implementation (for reference)

  **Acceptance Criteria**:
  - [ ] `crates/polyplug_abi/build/csharp.rs` exists
  - [ ] Generated code compiles with `dotnet build`
  - [ ] All constants present (8 total)
  - [ ] All structs present with correct [StructLayout]
  - [ ] PluginInterface generated, no PluginVTable

  **QA Scenarios**:
  ```
  Scenario: Generated C# compiles
    Tool: Bash
    Steps:
      1. cargo build -p polyplug_abi
      2. cd sdks/csharp/abi && dotnet build
    Expected Result: Build succeeds, 0 errors
    Evidence: .sisyphus/evidence/task-05-csharp-compile.log

  Scenario: Struct sizes match Rust
    Tool: Bash
    Steps:
      1. cd sdks/csharp/abi && dotnet test --filter "AbiSizeTests"
    Expected Result: All size assertions pass
    Evidence: .sisyphus/evidence/task-05-sizes.log
  ```

- [x] 6. Create sdks/csharp/ folder structure

  **What to do**:
  - Create `sdks/csharp/` directory
  - Create `sdks/csharp/Polyplug.slnx` solution file
  - Create subdirectories: `abi/`, `host/`, `guest/`, `loaders/`
  - Create `Directory.Build.props` for shared settings

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 5
  - **Blocks**: Tasks 7-10
  - **Blocked By**: None (can start immediately)

  **Acceptance Criteria**:
  - [ ] `sdks/csharp/` exists with all subdirectories
  - [ ] `Polyplug.slnx` created and valid
  - [ ] `Directory.Build.props` with shared settings

- [x] 7. Create Polyplug.ABI project (generated)

  **What to do**:
  - Create `sdks/csharp/abi/Polyplug.ABI.csproj`
  - Configure project: target net10.0, allow unsafe code
  - Run build.rs to generate `Abi.cs`, `Constants.cs`, `Fnv1a.cs`
  - Add project to solution

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO - Depends on Tasks 5, 6
  - **Blocks**: Tasks 8, 9
  - **Blocked By**: Tasks 5, 6

  **Acceptance Criteria**:
  - [ ] `sdks/csharp/abi/Polyplug.ABI.csproj` exists
  - [ ] Generated `Abi.cs` with all structs
  - [ ] Generated `Constants.cs` with all constants
  - [ ] `dotnet build` succeeds
  - [ ] Project added to Polyplug.slnx

  **QA Scenarios**:
  ```
  Scenario: ABI project builds
    Tool: Bash
    Steps:
      1. cd sdks/csharp/abi && dotnet build
    Expected Result: Build succeeds
    Evidence: .sisyphus/evidence/task-07-abi-build.log

  Scenario: All constants present
    Tool: Bash
    Steps:
      1. grep -c "public const uint ABI_" sdks/csharp/abi/Constants.cs
    Expected Result: 8 matches
    Evidence: .sisyphus/evidence/task-07-constants.log
  ```

- [x] 8. Migrate Polyplug.Host to new structure

  **What to do**:
  - Create `sdks/csharp/host/Polyplug.Host.csproj`
  - Copy Runtime.cs, RuntimeConfig.cs, ReloadPhase.cs, PluginGuard.cs, NativeMethods.cs from `host-libs/csharp/Polyplug/src/`
  - Update namespaces from `Polyplug` to `Polyplug.Host`
  - Add project reference to Polyplug.ABI
  - Update all code to use PluginInterface (remove PluginVTable)
  - Migrate all loader projects to `sdks/csharp/loaders/`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 9
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Task 7

  **References**:
  - `host-libs/csharp/Polyplug/src/` - Source files to migrate
  - `host-libs/csharp/Loaders/` - Loader projects to migrate

  **Acceptance Criteria**:
  - [ ] `sdks/csharp/host/Polyplug.Host.csproj` exists
  - [ ] All source files migrated
  - [ ] `dotnet build` succeeds
  - [ ] No PluginVTable references

- [x] 9. Migrate Polyplug.Guest to new structure

  **What to do**:
  - Create `sdks/csharp/guest/Polyplug.Guest.csproj`
  - Copy StringViewHelper.cs, PinnedStringView.cs, PluginException.cs, AbiSizeTests.cs from `guest-libs/csharp/src/`
  - Update namespaces from `Polyplug.Guest` to `Polyplug.Guest`
  - Add project reference to Polyplug.ABI
  - Update all code to use PluginInterface

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 8
  - **Blocks**: Tasks 11-14
  - **Blocked By**: Task 7

  **Acceptance Criteria**:
  - [ ] `sdks/csharp/guest/Polyplug.Guest.csproj` exists
  - [ ] All source files migrated
  - [ ] `dotnet build` succeeds
  - [ ] No PluginVTable references

- [x] 10. Migrate C# loaders to sdks/csharp/loaders/

  **What to do**:
  - Create `sdks/csharp/loaders/` directory structure
  - Migrate: Native/, Python/, Lua/, Js/, Dotnet/ loaders
  - Update project references to point to Polyplug.Host and Polyplug.ABI
  - Update namespaces

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Tasks 8, 9
  - **Blocks**: Task 14
  - **Blocked By**: Tasks 7, 8

  **Acceptance Criteria**:
  - [ ] All loader projects migrated
  - [ ] All loaders build successfully
  - [ ] All loaders added to solution

- [x] 11. Update C# examples to use new SDK

  **What to do**:
  - Update `examples/hosts/csharp/` to reference sdks/csharp/host/
  - Update `examples/guests/csharp/*/` to reference sdks/csharp/guest/
  - Update project references
  - Fix any namespace issues

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Blocks**: Task 14
  - **Blocked By**: Tasks 8, 9

  **Acceptance Criteria**:
  - [ ] All C# examples compile
  - [ ] Project references point to sdks/

- [x] 12. Remove PluginVTable from C# code

  **What to do**:
  - Search all C# files for PluginVTable references
  - Replace with PluginInterface
  - Remove PluginVTable struct definition from generated Abi.cs
  - Update HostVTable to use PluginInterface* in function signatures

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Blocks**: Task 14
  - **Blocked By**: Tasks 7-11

  **Acceptance Criteria**:
  - [ ] `grep -r "PluginVTable" sdks/csharp/` returns 0 results
  - [ ] All code uses PluginInterface
  - [ ] Build succeeds

- [x] 13. Add string helpers to C# ABI library

  **What to do**:
  - Add `StringHelpers.cs` to `sdks/csharp/abi/` with:
    - `StripPrefix(StringView sv, string prefix)` - strip prefix, return original if not present
    - `StartsWith(StringView sv, string prefix)` - check if starts with prefix
    - `Split(StringView sv, char delimiter)` - split into string array
  - Native C# implementation (no FFI) for zero overhead
  - Add corresponding tests

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Tasks 11, 12
  - **Blocks**: Task 14
  - **Blocked By**: Task 7 (ABI project must exist)

  **Acceptance Criteria**:
  - [ ] `sdks/csharp/abi/StringHelpers.cs` exists with 3 helper methods
  - [ ] Unit tests pass
  - [ ] Methods work with StringView correctly
  - [ ] Available to both host and guest (ABI lib)

  **QA Scenarios**:
  ```
  Scenario: StripPrefix works correctly
    Tool: Bash
    Steps:
      1. cd sdks/csharp/guest && dotnet test --filter "StringHelpersTests.StripPrefix"
    Expected Result: Test passes
    Evidence: .sisyphus/evidence/task-13-strip-prefix.log
  ```

- [x] 14. E2E test C# SDK with examples

  **What to do**:
  - Build all C# example guests (decoder, encoder, transformer, reporter, validator)
  - Build C# example host
  - Run host with all plugins
  - Verify pipeline works correctly

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO - Verification task
  - **Blocks**: Python SDK (Wave 4)
  - **Blocked By**: Tasks 11-13

  **Acceptance Criteria**:
  - [ ] All 5 C# guest plugins build
  - [ ] C# host runs successfully
  - [ ] Pipeline processes test data correctly
  - [ ] No runtime errors

  **QA Scenarios**:
  ```
  Scenario: Full C# pipeline works
    Tool: Bash
    Steps:
      1. cd examples/guests/csharp && dotnet build --configuration Release
      2. cd examples/hosts/csharp && dotnet run --configuration Release
    Expected Result: Pipeline output matches expected
    Evidence: .sisyphus/evidence/task-14-e2e-csharp.log
  ```

- [x] 15. Create Python AbiGenerator implementation

  **What to do**:
  - Create `crates/polyplug_abi/build/python.rs`
  - Implement `AbiGenerator` trait for Python
  - Generate Python code for: constants (module-level), structs (ctypes.Structure), helpers
  - Handle Python-specific: `_fields_` lists, `ctypes.c_void_p`, `ctypes.c_uint64`, etc.
  - Generate `__init__.py` with exports
  - Generate `abi.pyi` type stubs

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with other language generators
  - **Blocks**: Tasks 16-23
  - **Blocked By**: Tasks 1-4

  **References**:
  - `guest-libs/python/polyplug_guest/abi.py` - Current Python implementation
  - `crates/polyplug_abi/src/lib.rs` - Source of truth

  **Acceptance Criteria**:
  - [ ] `crates/polyplug_abi/build/python.rs` exists
  - [ ] Generated `abi.py` imports and runs
  - [ ] All constants present
  - [ ] All ctypes.Structure classes generated

- [x] 16. Create sdks/python/ folder structure

  **What to do**:
  - Create `sdks/python/` directory
  - Create subdirectories: `abi/`, `host/`, `guest/`, `loaders/`
  - Create `pyproject.toml` for each package
  - Create `setup.py` if needed for compatibility

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 15
  - **Blocks**: Tasks 17-20
  - **Blocked By**: None

  **Acceptance Criteria**:
  - [ ] `sdks/python/` exists with all subdirectories
  - [ ] Each package has pyproject.toml

- [x] 17. Create polyplug_abi package (generated)

  **What to do**:
  - Create `sdks/python/abi/pyproject.toml`
  - Run build.rs to generate `abi.py`, `constants.py`, `fnv1a.py`
  - Generate `__init__.py` with exports
  - Generate `abi.pyi` type stubs

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: NO - Depends on Tasks 15, 16
  - **Blocks**: Tasks 18, 19
  - **Blocked By**: Tasks 15, 16

  **Acceptance Criteria**:
  - [ ] `sdks/python/abi/` package exists
  - [ ] `pip install -e .` succeeds
  - [ ] `import polyplug_abi` works
  - [ ] All constants accessible

  **QA Scenarios**:
  ```
  Scenario: Python ABI package imports
    Tool: Bash
    Steps:
      1. cd sdks/python/abi && pip install -e .
      2. python -c "from polyplug_abi import ABI_OK, StringView, PluginInterface"
    Expected Result: No ImportError
    Evidence: .sisyphus/evidence/task-17-python-import.log
  ```

- [x] 18. Migrate polyplug host to sdks/python/host/

  **What to do**:
  - Create `sdks/python/host/pyproject.toml`
  - Copy runtime.py, runtime_config.py, scanner.py, helpers.py from `host-libs/python/polyplug/`
  - Update imports to use polyplug_abi
  - Remove PluginVTable references, use PluginInterface

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 19
  - **Blocks**: Tasks 21, 23
  - **Blocked By**: Task 17

  **Acceptance Criteria**:
  - [ ] `sdks/python/host/` package exists
  - [ ] `pip install -e .` succeeds
  - [ ] No PluginVTable references

- [x] 19. Migrate polyplug_guest to sdks/python/guest/

  **What to do**:
  - Create `sdks/python/guest/pyproject.toml`
  - Copy __init__.py from `guest-libs/python/polyplug_guest/`
  - Update imports to use polyplug_abi
  - Remove PluginVTable references, use PluginInterface

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Task 18
  - **Blocks**: Tasks 22, 23
  - **Blocked By**: Task 17

  **Acceptance Criteria**:
  - [ ] `sdks/python/guest/` package exists
  - [ ] `pip install -e .` succeeds
  - [ ] No PluginVTable references

- [x] 20. Migrate Python loaders to sdks/python/loaders/

  **What to do**:
  - Create `sdks/python/loaders/` directory structure
  - Migrate: native/, python/, js/, lua/, dotnet/ loaders
  - Update imports and dependencies

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Tasks 18, 19
  - **Blocks**: Task 23
  - **Blocked By**: Task 18

  **Acceptance Criteria**:
  - [ ] All loader packages migrated
  - [ ] All loaders installable

- [x] 21. Remove PluginVTable from Python code

  **What to do**:
  - Remove PluginVTable class from generated abi.py
  - Update all code to use PluginInterface
  - Update HostVTable function signatures

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Tasks 20, 22
  - **Blocks**: Task 23
  - **Blocked By**: Tasks 17-19

  **Acceptance Criteria**:
  - [ ] `grep -r "PluginVTable" sdks/python/` returns 0 results
  - [ ] All code uses PluginInterface

- [x] 22. Add string helpers to Python ABI library

  **What to do**:
  - Add `helpers.py` to `sdks/python/abi/` with native Python implementations:
    - `strip_prefix(sv: StringView, prefix: str) -> str`
    - `starts_with(sv: StringView, prefix: str) -> bool`
    - `split(sv: StringView, delimiter: str) -> list[str]`
  - Native Python (no FFI) for zero overhead
  - Available to both host and guest (ABI lib)

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with Tasks 20, 21
  - **Blocks**: Task 23
  - **Blocked By**: Task 17 (ABI package must exist)

  **Acceptance Criteria**:
  - [ ] All 3 helper functions implemented
  - [ ] Functions work with StringView correctly
  - [ ] Available to both host and guest

- [x] 23. E2E test Python SDK with examples

  **What to do**:
  - Run all Python example guests
  - Run Python example host
  - Verify pipeline works

  **Recommended Agent Profile**:
  - **Category**: `deep`
  - **Skills**: None needed

  **Parallelization**:
  - **Can Run In Parallel**: YES - with other SDK E2E tests
  - **Blocks**: Wave 8
  - **Blocked By**: Tasks 18-22

  **QA Scenarios**:
  ```
  Scenario: Full Python pipeline works
    Tool: Bash
    Steps:
      1. cd examples/hosts/python && python host.py
    Expected Result: Pipeline output matches expected
    Evidence: .sisyphus/evidence/task-23-e2e-python.log
  ```

- [x] 24. Create C++ AbiGenerator implementation

  **What to do**: Create `crates/polyplug_abi/build/cpp.rs`, implement trait, generate `abi.hpp` with `constexpr` constants, `struct` definitions, `extern "C"` declarations.

  **Recommended Agent Profile**: `deep`

  **Parallelization**: YES - with other language generators

- [x] 25. Create sdks/cpp/ folder structure

  **What to do**: Create `sdks/cpp/` with `abi/`, `host/`, `guest/`, `loaders/` subdirectories. Create CMakeLists.txt structure.

  **Recommended Agent Profile**: `quick`

- [x] 26. Create generated abi.hpp

  **What to do**: Generate `sdks/cpp/abi/polyplug/abi.hpp` with all structs, constants, FNV-1a functions.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 27. Migrate C++ host to sdks/cpp/host/

  **What to do**: Migrate `host-libs/cpp/polyplug/` to `sdks/cpp/host/`. Update includes to use shared abi. Remove PluginVTable, use PluginInterface.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 28. Migrate C++ guest to sdks/cpp/guest/

  **What to do**: Migrate `guest-libs/cpp/polyplug/` to `sdks/cpp/guest/`. Update includes. Remove PluginVTable.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 29. Migrate C++ loaders to sdks/cpp/loaders/

  **What to do**: Migrate `host-libs/cpp/loaders/` to `sdks/cpp/loaders/`.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 30. Remove PluginVTable from C++ code

  **What to do**: `using PluginVTable = PluginInterface;` should NOT be generated. All code uses PluginInterface directly.

  **Recommended Agent Profile**: `quick`

- [x] 31. Add string helpers to C++ ABI library

  **What to do**: Add native `strip_prefix`, `starts_with`, `split` functions to `sdks/cpp/abi/polyplug/helpers.hpp`. Native C++ (no FFI) for zero overhead. Available to both host and guest.

  **Recommended Agent Profile**: `quick`

- [x] 32. E2E test C++ SDK with examples

  **What to do**: Build and run all C++ examples.

  **Recommended Agent Profile**: `deep`

  **QA Scenarios**:
  ```
  Scenario: Full C++ pipeline works
    Tool: Bash
    Steps:
      1. cd examples/hosts/cpp && mkdir build && cd build && cmake .. && make
      2. ./host
    Expected Result: Pipeline output matches expected
    Evidence: .sisyphus/evidence/task-32-e2e-cpp.log
  ```

- [x] 33. Create Lua AbiGenerator implementation

  **What to do**: Create `crates/polyplug_abi/build/lua.rs`, generate `polyplug_abi.lua` with FFI definitions, constants, helpers.

  **Recommended Agent Profile**: `deep`

- [x] 34. Create sdks/lua/ folder structure

  **What to do**: Create `sdks/lua/` with `abi/`, `host/`, `guest/`, `loaders/`. Create rockspec files.

  **Recommended Agent Profile**: `quick`

- [x] 35. Create generated polyplug_abi.lua

  **What to do**: Generate FFI cdef block, constants, helper functions.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 36. Migrate Lua host to sdks/lua/host/

  **What to do**: Migrate `host-libs/lua/polyplug.lua` and related files. Remove PluginVTable.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 37. Migrate Lua guest to sdks/lua/guest/

  **What to do**: Migrate `guest-libs/lua/polyplug_guest.lua`. Remove PluginVTable.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 38. Migrate Lua loaders to sdks/lua/loaders/

  **What to do**: Migrate `host-libs/lua/loaders/`.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 39. Remove PluginVTable from Lua code

  **What to do**: All FFI definitions use PluginInterface only.

  **Recommended Agent Profile**: `quick`

- [x] 40. Add string helpers to Lua ABI library

  **What to do**: Add native `strip_prefix`, `starts_with`, `split` functions to `sdks/lua/abi/polyplug_abi.lua`. Native Lua (no FFI) for zero overhead. Available to both host and guest.

  **Recommended Agent Profile**: `quick`

- [x] 41. E2E test Lua SDK with examples

  **What to do**: Run all Lua examples.

  **Recommended Agent Profile**: `deep`

  **QA Scenarios**:
  ```
  Scenario: Full Lua pipeline works
    Tool: Bash
    Steps:
      1. cd examples/hosts/lua && lua host.lua
    Expected Result: Pipeline output matches expected
    Evidence: .sisyphus/evidence/task-41-e2e-lua.log
  ```

- [x] 42. Create JavaScript AbiGenerator implementation

  **What to do**: Create `crates/polyplug_abi/build/js.rs`, generate TypeScript/JavaScript with BigInt for 64-bit values.

  **Recommended Agent Profile**: `deep`

- [x] 43. Create sdks/js/ folder structure

  **What to do**: Create `sdks/js/` with `abi/`, `host/`, `guest/`, `loaders/`. Create package.json and deno.json.

  **Recommended Agent Profile**: `quick`

- [x] 44. Create generated polyplug_abi.js + .d.ts

  **What to do**: Generate `abi.ts` with all types, constants, FNV-1a. Use BigInt for u64. Generate `.d.ts` type declarations.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 45. Migrate JS host to sdks/js/host/

  **What to do**: Migrate `host-libs/js-deno/polyplug/`. Remove PluginVTable.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 46. Migrate JS guest to sdks/js/guest/

  **What to do**: Migrate `guest-libs/js/polyplug-guest.js`. Remove PluginVTable.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 47. Migrate JS loaders to sdks/js/loaders/

  **What to do**: Migrate `host-libs/js-deno/loaders/`.

  **Recommended Agent Profile**: `unspecified-high`

- [x] 48. Remove PluginVTable from JS code

  **What to do**: All TypeScript interfaces use PluginInterface only.

  **Recommended Agent Profile**: `quick`

- [x] 49. Add string helpers to JS ABI library

  **What to do**: Add native `stripPrefix`, `startsWith`, `split` functions to `sdks/js/abi/polyplug_abi.ts`. Native TypeScript (no FFI) for zero overhead. Available to both host and guest.

  **Recommended Agent Profile**: `quick`

- [x] 50. E2E test JS SDK with examples

  **What to do**: Run all JavaScript examples with Deno.

  **Recommended Agent Profile**: `deep`

  **QA Scenarios**:
  ```
  Scenario: Full JS pipeline works
    Tool: Bash
    Steps:
      1. cd examples/hosts/js && deno run host.ts
    Expected Result: Pipeline output matches expected
    Evidence: .sisyphus/evidence/task-50-e2e-js.log
  ```

- [x] 51. Remove old host-libs/ directory

  **What to do**: Delete `host-libs/` directory after confirming all migration complete.

  **Recommended Agent Profile**: `quick`

  **Must NOT do**: Delete before all SDKs verified working.

- [x] 52. Remove old guest-libs/ directory

  **What to do**: Delete `guest-libs/` directory after confirming all migration complete.

  **Recommended Agent Profile**: `quick`

- [x] 53. Remove scripts/ directory (duplicates docs/)

  **What to do**: 
  - Delete `scripts/` directory entirely
  - `scripts/install.sh` and `scripts/install.ps1` are duplicates of `docs/install.sh` and `docs/install.ps1`
  - The `docs/` versions are served from GitHub Pages and are the canonical location

  **Recommended Agent Profile**: `quick`

- [x] 54. Update Cargo.toml workspace members

  **What to do**: Update root `Cargo.toml` to reflect new structure. Remove old paths, add new ones if needed.

  **Recommended Agent Profile**: `quick`

- [ ] 55. Documentation cleanup and accuracy review

  **What to do**:
  - Update README.md to reflect new sdks/ structure
  - Create/update README.md in each SDK folder
  - Update examples/README.md
  - **Add docs/design-decisions.md**: Document why native helpers vs FFI was chosen (performance: 3-6x faster, zero-overhead goal)
  - **Documentation cleanup** (human-oriented, not verbose, no old vs new comparisons):
    - Update `docs/HOT_RELOAD_DESIGN.md`: Replace PluginVTable with PluginInterface in code examples
    - Review `PRD.md`: Update PluginVTable references, remove old architecture descriptions
    - Mark `docs/PLUGIN_INTERFACE_DESIGN.md` as historical or archive it (documents old vs new comparison)
    - Add executive summaries to `TRUST_MODEL.md` sections
    - Remove any "old vs new", "now vs earlier", language comparison comments
    - Ensure all docs are written for human readers, not AI/bots

  **Documentation Principles**:
  - Accurate and correct
  - Not verbose (add summaries for long sections)
  - No comparison docs (old vs new, js vs c# vs python)
  - Written for humans, not AI

  **Recommended Agent Profile**: `writing`

  **References**:
  - String Helper API Specification section in this plan (performance analysis table)
  - docs/HOT_RELOAD_DESIGN.md:26,226,903,919 - outdated PluginVTable examples
  - PRD.md - needs PluginVTable terminology update
  - docs/PLUGIN_INTERFACE_DESIGN.md - old vs new comparison (historical)
  - TRUST_MODEL.md - needs section summaries

- [ ] 56. Final E2E test all SDKs

  **What to do**: Run complete test matrix: all languages, all examples, all hosts, all guests.

  **Recommended Agent Profile**: `deep`

  **QA Scenarios**:
  ```
  Scenario: All SDKs pass E2E
    Tool: Bash
    Steps:
      1. just build-examples
      2. just run-example-rust
    Expected Result: All 5 language pipelines complete successfully
    Evidence: .sisyphus/evidence/task-56-all-e2e.log
  
  Scenario: Examples build script works
    Tool: Bash
    Steps:
      1. cd examples && ./build_all.sh
    Expected Result: Build completes without errors
    Evidence: .sisyphus/evidence/task-56-examples-build.log
  ```

---

## Final Verification Wave (MANDATORY — after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Present consolidated results to user and get explicit "okay" before completing.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, curl endpoint, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist in .sisyphus/evidence/. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy -- -D warnings` + `cargo fmt --check` + `cargo test`. Review all changed files for: `as any`/`@ts-ignore`, empty catches, console.log in prod, commented-out code, unused imports. Check AI slop: excessive comments, over-abstraction, generic names (data/result/item/temp).
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Full E2E Test Matrix** — `deep`
  Run complete test matrix for ALL languages:
  - Rust: `cargo test --all`
  - C#: `dotnet test` in each SDK
  - Python: `pytest` in each SDK
  - C++: `ctest` in each SDK
  - Lua: run all examples
  - JS: `deno test` in each SDK
  Run all example hosts with all example guests.
  Output: `Rust [PASS/FAIL] | C# [PASS/FAIL] | Python [PASS/FAIL] | C++ [PASS/FAIL] | Lua [PASS/FAIL] | JS [PASS/FAIL] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual diff (git log/diff). Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance. Detect cross-task contamination.
  Output: `Tasks [N/N compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

> **Commit after EACH wave passes E2E tests.** This enables easy `git bisect` and `git revert` for rollback.

### Wave 1: Foundation
```
feat(abi): add modular code generator infrastructure

- Add AbiGenerator trait in build/mod.rs
- Add syn/quote parser for Rust types in build/parser.rs
- Add file writer utilities in build/writer.rs
- Integrate build.rs orchestrator

Files: crates/polyplug_abi/build/*.rs, crates/polyplug_abi/Cargo.toml
Pre-commit: cargo test -p polyplug_abi
```

### Wave 2-3: C# SDK
```
feat(csharp): migrate to unified SDK structure

- Create sdks/csharp/ with abi/, host/, guest/, loaders/
- Add C# AbiGenerator in build/csharp.rs
- Generate Polyplug.ABI with all structs and constants
- Migrate Polyplug.Host and Polyplug.Guest
- Remove PluginVTable, use PluginInterface
- Add string helpers to ABI library

Files: sdks/csharp/**, crates/polyplug_abi/build/csharp.rs
Pre-commit: cd sdks/csharp && dotnet build && dotnet test
```

### Wave 4: Python SDK
```
feat(python): migrate to unified SDK structure

- Create sdks/python/ with abi/, host/, guest/, loaders/
- Add Python AbiGenerator in build/python.rs
- Generate polyplug_abi package with all types
- Migrate host and guest packages
- Remove PluginVTable, use PluginInterface
- Add string helpers to ABI library

Files: sdks/python/**, crates/polyplug_abi/build/python.rs
Pre-commit: cd sdks/python && pip install -e . && pytest
```

### Wave 5: C++ SDK
```
feat(cpp): migrate to unified SDK structure

- Create sdks/cpp/ with abi/, host/, guest/, loaders/
- Add C++ AbiGenerator in build/cpp.rs
- Generate abi.hpp with all structs and constants
- Migrate host and guest libraries
- Remove PluginVTable, use PluginInterface
- Add string helpers to ABI library

Files: sdks/cpp/**, crates/polyplug_abi/build/cpp.rs
Pre-commit: cd sdks/cpp && cmake --build . && ctest
```

### Wave 6: Lua SDK
```
feat(lua): migrate to unified SDK structure

- Create sdks/lua/ with abi/, host/, guest/, loaders/
- Add Lua AbiGenerator in build/lua.rs
- Generate polyplug_abi.lua with FFI definitions
- Migrate host and guest libraries
- Remove PluginVTable, use PluginInterface
- Add string helpers to ABI library

Files: sdks/lua/**, crates/polyplug_abi/build/lua.rs
Pre-commit: lua examples/hosts/lua/host.lua
```

### Wave 7: JavaScript SDK
```
feat(js): migrate to unified SDK structure

- Create sdks/js/ with abi/, host/, guest/, loaders/
- Add JS AbiGenerator in build/js.rs
- Generate polyplug_abi.ts with all types (BigInt for 64-bit)
- Migrate host and guest libraries
- Remove PluginVTable, use PluginInterface
- Add string helpers to ABI library

Files: sdks/js/**, crates/polyplug_abi/build/js.rs
Pre-commit: deno test sdks/js/
```

### Wave 8: Cleanup
```
chore: remove old host-libs, guest-libs, and scripts

- Delete host-libs/ directory
- Delete guest-libs/ directory
- Delete scripts/ directory (duplicates docs/install.sh and docs/install.ps1)
- Update Cargo.toml workspace members

Files: (removed files), Cargo.toml
Pre-commit: cargo build --all && cargo test --all
```

### Final: Documentation
```
docs: update documentation for new SDK structure

- Update README.md with new sdks/ structure
- Add SDK READMEs for each language
- Add docs/design-decisions.md (native helpers vs FFI)
- Update examples documentation

Files: README.md, sdks/*/README.md, docs/design-decisions.md, examples/README.md
Pre-commit: cargo doc --no-deps
```

---

## Success Criteria

### Verification Commands
```bash
# Build and test Rust
cargo build --all
cargo test --all

# Build examples (using just or build script)
just build-examples
# OR: cd examples && ./build_all.sh

# C# SDK
cd sdks/csharp && dotnet build && dotnet test

# Python SDK
cd sdks/python && pip install -e . && pytest

# C++ SDK
cd sdks/cpp && cmake --build . && ctest

# Run examples
just run-example-rust
# OR: cd examples && ./verify_hosts.sh
```

### Final Checklist
- [ ] All 5 SDKs exist in sdks/
- [ ] All SDKs have abi/, host/, guest/, loaders/ subfolders
- [ ] No PluginVTable references in code
- [ ] All string helpers present in ABI libraries
- [ ] All E2E tests pass
- [ ] Documentation updated