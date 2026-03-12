# Comprehensive Test Coverage Plan for Polyplug

## Executive Summary

This plan identifies **REAL and NEEDED** tests across all 8 crates in the polyplug project. After analyzing:
- 92 Rust source files
- 22 existing integration tests
- 33 files with inline test modules
- 8 crates (polyplug, polyplugc, polyplug_codegen, polyplug_dotnet, polyplug_js, polyplug_js_deno, polyplug_lua, polyplug_python)

**Current State**: Strong "happy path" coverage with good structural and integration tests, but significant gaps in:
1. **Error handling paths** (malformed inputs, permission errors, edge cases)
2. **Concurrency stress tests** (simultaneous registry mutation, thundering herd)
3. **Adversarial testing** (malicious guest behavior, memory corruption)
4. **Cross-language ABI validation** (layout correctness, type mapping)
5. **CLI/Codegen edge cases** (TOML parsing, language generation)
6. **Language binding tests** (.NET, JS, Lua, Python lack dedicated test suites)

---

## Crate 1: polyplug (Runtime Core)

### Current Test Coverage

**Existing Tests (24 files):**
- `integration_*.rs` - 19 integration tests covering: load, dispatch, reload, version, memory, panic, graph, context, discovery, extension, FFI null, invalid UTF8, malformed, stringview nulls, quiescence
- `library_lifetime.rs` - Library handle lifecycle
- `stress_error.rs` - Error handling stress tests
- `stress_memory.rs` - Memory allocation stress tests
- `fnv1a_compat.rs` - Hash algorithm compatibility
- `benches/vtable_dispatch.rs` - Performance benchmarks

**Inline Tests:**
- `version.rs` - Version parsing and comparison
- `graph.rs` - Topological sort and cycle detection
- `registry.rs` - Plugin registration and lookup
- `loader/mod.rs` - Basic manifest loading
- `loader/scanner.rs` - Directory scanning

### Missing Tests - CRITICAL PRIORITY

#### 1.1 FFI Safety & Robustness

**File:** `tests/integration_ffi_robustness.rs`

**Purpose:** Test host resilience against malformed ABI calls from plugins

**Scenarios:**
1. **NULL StringView with non-zero length**
   - Plugin passes `StringView { ptr: null, len: 5 }`
   - Expected: Host returns `ABI_ERROR_INVALID_ARGS`, no panic

2. **Invalid UTF-8 in StringView**
   - Plugin passes invalid UTF-8 bytes (0x80-0xBF sequences)
   - Expected: Host detects and returns error, no panic

3. **Misaligned Buffer pointer**
   - Request alignment 16 but provide pointer not 16-byte aligned
   - Expected: Host detects or handles gracefully

4. **StringView with embedded NULLs**
   - `StringView` with `len=10` containing `\0` at position 5
   - Expected: Host respects `len`, doesn't treat as null-terminated

5. **Buffer overflow attempt**
   - Plugin writes beyond `Buffer.cap` (requires memory protection test)
   - Expected: Detect via canary values or segfault handling

6. **Cross-thread StringView/Buffer usage**
   - Allocate in thread A, free in thread B
   - Expected: Thread-safe operation

#### 1.2 Concurrency Stress Tests

**File:** `tests/stress_concurrency.rs`

**Purpose:** Thundering herd and race condition testing

**Scenarios:**
1. **Concurrent registry resolution (100 threads)**
   - 100 threads calling `resolve_plugin()` simultaneously
   - No deadlocks, all return valid handles or proper errors

2. **Reload during heavy dispatch**
   - 50 threads calling plugin function continuously
   - 1 thread reloading the plugin every 100ms
   - Expected: Zero dropped calls, zero stale handle errors after refresh

3. **Concurrent bundle loading**
   - 20 threads loading different bundles simultaneously
   - Expected: All load successfully, no cross-contamination

4. **Registry exhaustion**
   - Register plugins until `u32::MAX` handles
   - Expected: Graceful error, no overflow

5. **Generation counter wrap-around**
   - Reload a plugin `u32::MAX` times (simulated or actual)
   - Expected: Generation counter handles overflow correctly

#### 1.3 Quiescence & Reload Edge Cases

**File:** `tests/integration_reload_edge_cases.rs`

**Scenarios:**
1. **Quiescence timeout**
   - Plugin call that never returns (intentionally stuck)
   - Trigger reload with 5s timeout
   - Expected: `PolyplugError::QuiescenceTimeout`, no crash

2. **Cascading reload deep tree**
   - Dependency tree: A -> B -> C -> D -> ... (16+ levels)
   - Reload root A
   - Expected: All 16+ plugins reloaded in correct order

3. **Circular reload cascade**
   - A depends on B, B depends on A
   - Expected: Proper error or handling, no infinite loop

4. **Reload during initialization**
   - Start loading bundle, trigger reload before `polyplug_init` returns
   - Expected: Clean abort or completion, no leaked resources

5. **In-flight call interruption**
   - Call plugin function, reload during execution
   - Expected: Call completes with old vtable, subsequent calls use new

#### 1.4 Memory Allocator Tests

**File:** `tests/stress_allocator.rs`

**Scenarios:**
1. **Alignment torture**
   - Allocations with align = 1, 2, 4, 8, 16, 32, 64, 4096
   - Verify returned pointers are properly aligned

2. **Huge allocation requests**
   - Request `usize::MAX` or 10GB+
   - Expected: Graceful failure, no crash

3. **Zero-size allocations**
   - `polyplug_host_alloc(0, 1)` - should return NULL or valid ptr
   - Expected: Consistent behavior, no crash

4. **Double-free detection**
   - `polyplug_host_free(ptr); polyplug_host_free(ptr);`
   - Expected: Debug build aborts, release build handles gracefully

5. **Tracking allocator leak detection**
   - Allocate without freeing, verify leak is detected
   - Expected: Debug build reports leak on shutdown

#### 1.5 Graph Algorithm Edge Cases

**File:** `tests/integration_graph_edge_cases.rs`

**Scenarios:**
1. **Self-dependency**
   - Bundle A declares dependency on contract provided by A
   - Expected: `PolyplugError::DependencyCycle`

2. **Multi-bundle cycle**
   - A -> B -> C -> A (3-bundle cycle)
   - Expected: Error names all participants

3. **Disconnected graphs**
   - Load two independent sets of bundles with no shared deps
   - Expected: Both load successfully

4. **Diamond dependency**
   - A depends on B and C, both B and C depend on D
   - Expected: D loaded once, correct topological order

5. **Version-aware graph resolution**
   - Bundle A needs B@1.2, only B@1.1 present
   - Expected: Version mismatch error with specific versions

6. **Deep dependency chain (1000+ bundles)**
   - Linear chain A -> B -> C -> ... (1000 levels)
   - Expected: Handles deep recursion/stack correctly

#### 1.6 Registry Edge Cases

**File:** `tests/integration_registry_edge_cases.rs`

**Scenarios:**
1. **Handle exhaustion**
   - Fill registry to `u32::MAX` entries
   - Expected: Graceful error on overflow attempt

2. **Hash collision handling**
   - Two different contract names with same FNV-1a hash (rare but possible)
   - Expected: Detection and handling

3. **Zombie handles**
   - Resolve handle, unload plugin, load different plugin at same slot
   - Try to use old handle
   - Expected: `StaleHandle` error

4. **find_all_by_contract buffer overflow**
   - Provide buffer smaller than number of matching plugins
   - Expected: Partial fill, documented behavior

5. **is_dependency_declared cross-bundle isolation**
   - Bundle A declares dep on X, Bundle B doesn't
   - Ensure B cannot resolve X
   - Expected: Access denied for undeclared deps

### Missing Tests - HIGH PRIORITY

#### 1.7 Error Propagation Tests

**File:** `tests/integration_error_chain.rs`

**Scenarios:**
1. **Error through multiple plugins**
   - Host -> Plugin A -> Plugin B -> error
   - Verify error propagates correctly through chain

2. **Panic in nested call**
   - Host calls A, A calls B, B panics
   - Expected: `ABI_ERROR_PANIC` returned to A, A handles, returns to host

3. **Error message UTF-8 validation**
   - Plugin returns error with invalid UTF-8 message
   - Expected: Host handles gracefully

#### 1.8 Version Compatibility Tests

**File:** `tests/integration_version_edge_cases.rs`

**Scenarios:**
1. **Pre-release version handling**
   - Versions like "1.0.0-alpha", "1.0.0-rc.1"
   - Expected: Defined behavior (reject or accept)

2. **Wildcard version requirements**
   - Contract requires "*" or ">=1.0"
   - Expected: Proper matching

3. **Version parsing edge cases**
   - "1", "1.2", "1.2.3", "1.2.3.4" (overflow)
   - Empty version string
   - Non-numeric components

#### 1.9 Loader Edge Cases

**File:** `tests/integration_loader_edge_cases.rs`

**Scenarios:**
1. **Symlink attacks**
   - Bundle `file` points to symlink outside plugin dir
   - Expected: Rejection or safe resolution

2. **Resource exhaustion**
   - Manifest with 10,000 dependencies
   - Manifest with 1,000,000 `provides` entries
   - Expected: Graceful rejection

3. **Permission errors**
   - Bundle file exists but not readable
   - Expected: Proper permission error

4. **Truncated/malformed .so files**
   - Valid magic but truncated at various points
   - Expected: `LoaderError::InvalidBundle`

5. **Wrong architecture binary**
   - ARM .so on x86 host
   - Expected: Clear error message

### Missing Tests - MEDIUM PRIORITY

#### 1.10 Unit Tests for Internal Functions

**Add to source files:**

**`abi.rs`:**
- `contract_id()` stability and collision resistance
- `bundle_id()` stability
- `StringView::from_static()` safety
- `StringView::as_str()` with edge cases

**`error.rs`:**
- `Display` implementation for all error types
- Error message formatting with special characters

**`loader/manifest.rs`:**
- `ManifestData::validate_file()` edge cases
- `RawManifestDependency::resolve()` with various `kind` values
- Path normalization across OSes

**`loader/scanner.rs`:**
- Permission error handling
- Directory traversal limits
- Symlink following behavior

---

## Crate 2: polyplugc (CLI Tool)

### Current Test Coverage

**Existing Tests:**
- `tests/smoke.rs` - Basic CLI smoke tests
- `tests/showcase.rs` - Showcase example
- `tests/integration_host_deno.rs` - Deno host integration

**Inline Tests:**
- None in `main.rs`

### Missing Tests - CRITICAL PRIORITY

#### 2.1 CLI Argument Validation

**File:** `tests/cli_validation.rs`

**Scenarios:**
1. **Missing required arguments**
   - `polyplugc generate` without `--api`
   - Expected: Helpful error message

2. **Invalid language strings**
   - `polyplugc generate --lang invalid_lang`
   - Expected: Error listing valid languages

3. **Conflicting flags**
   - Both `--api` and `--bundle` provided
   - Expected: Clear error

4. **Language aliases**
   - `c#`, `c++`, `py`, `js` should work
   - Expected: Accepted and mapped correctly

5. **Non-existent paths**
   - `--api /nonexistent/file.toml`
   - Expected: File not found error

6. **Directory instead of file**
   - `--api /path/to/directory/`
   - Expected: Appropriate error

7. **Read-only output directory**
   - Output to read-only filesystem
   - Expected: Permission error

8. **Parent directory creation**
   - `--out-dir /nonexistent/parent/output`
   - Expected: Creates parent dirs or clear error

#### 2.2 Parser Error Handling

**File:** `tests/parser_errors.rs`

**Scenarios:**
1. **Malformed TOML syntax**
   - Missing closing bracket
   - Invalid escape sequences
   - Mixed table/array syntax
   - Expected: Line-specific error message

2. **Missing required fields**
   - Contract without `version`
   - Bundle without `name`
   - Expected: Clear "missing required field" error

3. **Duplicate names**
   - Two contracts with same name
   - Two types with same name
   - Expected: Name conflict error

4. **Invalid type references**
   - Reference to non-existent type
   - Circular type definitions
   - Expected: Type resolution error

5. **Enum expression validation**
   - Deeply nested parentheses: `((A | B) << 2) + (C & D)`
   - Invalid operators: `!`, `&`, `*`, `/`
   - Forward references (not yet defined variant)
   - Chained references > 2 levels
   - Expected: Specific validation errors

6. **Version string parsing**
   - Non-numeric: "a.b.c"
   - Negative numbers: "-1.0"
   - Extra components: "1.2.3.4.5"
   - Expected: Version parse error

7. **Function definition limits**
   - 0 functions in contract
   - 1000+ functions in contract
   - Expected: Warning or limit error

#### 2.3 Cross-Language Codegen Verification

**File:** `tests/codegen_verification.rs`

**Purpose:** Verify generated code is syntactically valid for each language

**Scenarios:**
1. **Rust code generation**
   - Generate host and guest
   - Verify compiles with `cargo check`

2. **C++ code generation**
   - Generate host and guest
   - Verify compiles with `g++ -c`

3. **C# code generation**
   - Generate host and guest
   - Verify compiles with `dotnet build`

4. **Python code generation**
   - Generate host and guest
   - Verify valid Python with `python -m py_compile`

5. **Lua code generation**
   - Generate host and guest
   - Verify valid Lua with `luac -p`

6. **JavaScript (Deno/QuickJS) code generation**
   - Generate host and guest
   - Verify valid JS with `deno check` or syntax check

7. **Type mapping correctness**
   - Each `PrimitiveType` maps to correct language type
   - Verify `U64` -> `uint64_t` (C++), `ulong` (C#), etc.

#### 2.4 Pack Command Tests

**File:** `tests/pack_command.rs`

**Scenarios:**
1. **Cargo.toml generation**
   - Generated `Cargo.toml` is valid TOML
   - Contains correct dependencies

2. **CMakeLists.txt generation**
   - Generated file is valid CMake

3. **Package.json generation**
   - Generated file is valid JSON

4. **Naming conversions**
   - Hyphenated bundle names: `my-bundle` -> `MyBundle` (C#), `my_bundle` (Python)

5. **Missing metadata handling**
   - Bundle without description/license
   - Expected: Uses sensible defaults

---

## Crate 3: polyplug_codegen (Codegen Library)

### Current Test Coverage

**Existing Tests:**
- `tests/integration_codegen_*.rs` - 6 files (rust, csharp, python, lua, js_deno, js_quickjs)

**Inline Tests:**
- `ir.rs` - Version parsing, primitive types, FNV-1a hashing
- `parser.rs` - API/bundle parsing, enum validation
- `generators/*.rs` - Each generator has basic tests

### Missing Tests - CRITICAL PRIORITY

#### 3.1 IR (Intermediate Representation) Tests

**Add to `ir.rs` or `tests/ir_edge_cases.rs`:**

1. **Type resolution edge cases**
   - Circular type definitions
   - Forward references across contracts
   - Built-in vs user-defined precedence
   - Case sensitivity in type names

2. **Hash function cross-validation**
   - `compute_contract_id()` matches runtime `contract_id()`
   - `compute_bundle_id()` matches runtime `bundle_id()`

3. **Version comparison edge cases**
   - "1.0" vs "1.0.0" vs "1"
   - Pre-release comparison: "1.0.0-alpha" vs "1.0.0"
   - Wildcard matching: "^1.2.0", "~1.2.0", ">=1.0"

4. **ReprType mapping**
   - Verify `U8`, `U16`, `U32`, `U64`, `I8`, `I16`, `I32`, `I64`, `F32`, `F64`, `Bool`
   - All map correctly to target language types

#### 3.2 Parser Tests

**Add to `parser.rs` or `tests/parser_edge_cases.rs`:**

1. **Bundle + API integration**
   - `parse_bundle_with_api()` with missing API file
   - API file with conflicting type definitions
   - Relative path resolution edge cases

2. **Enum validation depth**
   - Expression parsing: `A | (B << 2) & ~C`
   - Hex literals: `0xFF`, `0b1010`
   - Overflow detection: `U8` variant with value 256

3. **Dependency resolution**
   - `kind: "contract"` vs `kind: "bundle"`
   - Missing required fields in dependencies

#### 3.3 Generator Output Correctness

**File:** `tests/generator_output_correctness.rs`

**Scenarios:**
1. **Memory layout verification**
   - Generate struct in all languages
   - Verify `#[repr(C)]` in Rust matches C++ struct
   - Verify C# `[StructLayout]` matches
   - Python `ctypes` matches

2. **VTable generation**
   - Verify function pointer order matches contract definition
   - Verify slot indices are correct

3. **StringView/Buffer handling**
   - Each language correctly handles `StringView` (ptr+len, not null-term)
   - Each language correctly handles `Buffer` (ptr+len+cap)

4. **BigInt handling (JS)**
   - `u64`/`i64` mapped to `BigInt` correctly

---

## Crate 4-8: Language Bindings (.NET, JS, Lua, Python)

### Current State: NO DEDICATED TESTS

These crates have **zero test coverage**:
- `polyplug_dotnet` - No tests directory
- `polyplug_js` - No tests directory
- `polyplug_js_deno` - No tests directory
- `polyplug_lua` - No tests directory
- `polyplug_python` - No tests directory

### Missing Tests - CRITICAL PRIORITY

#### 4.1 .NET Loader Tests (`polyplug_dotnet`)

**File:** `tests/dotnet_loader.rs`

**Scenarios:**
1. **Framework version detection**
   - Read TFM from assembly correctly
   - Handle missing TFM
   - Handle invalid TFM format

2. **Version compatibility checking**
   - `net6.0` meets `net6.0` requirement
   - `net7.0` meets `net6.0` requirement (higher minor)
   - `net5.0` fails `net6.0` requirement
   - Empty TFM handling

3. **Assembly loading**
   - Load valid assembly
   - Load missing assembly
   - Load invalid/corrupted assembly

4. **CLR initialization**
   - First load initializes CLR
   - Subsequent loads reuse CLR
   - Concurrent CLR access

5. **Hostfxr location**
   - Auto-detect on Windows/Linux/macOS
   - Explicit path provided
   - Missing hostfxr

#### 4.2 JavaScript Loader Tests (`polyplug_js`)

**File:** `tests/js_quickjs_loader.rs`

**Scenarios:**
1. **QuickJS runtime initialization**
   - First load creates runtime
   - Subsequent loads reuse runtime
   - Thread safety

2. **Bundle evaluation**
   - Valid JS bundle loads
   - Syntax error in bundle
   - Runtime error during init

3. **VTable registration**
   - `registerVtable()` callback works
   - Missing `registerVtable` call

4. **Trampoline dispatch**
   - Function calls dispatch correctly
   - Slot index out of range

5. **Memory management**
   - JS values don't leak
   - Proper cleanup on unload

#### 4.3 Deno Loader Tests (`polyplug_js_deno`)

**File:** `tests/js_deno_loader.rs`

**Scenarios:**
1. **Deno runtime initialization**
   - Start Deno isolate
   - Load module from file

2. **Permission handling**
   - Test with various `--allow-*` flags
   - Deny permissions and verify behavior

3. **TypeScript support**
   - Load TS plugin
   - Compilation errors

#### 4.4 Lua Loader Tests (`polyplug_lua`)

**File:** `tests/lua_loader.rs`

**Scenarios:**
1. **LuaJIT VM initialization**
   - First load creates VM
   - Subsequent loads reuse VM
   - Thread safety (Mutex protection)

2. **Lua bundle loading**
   - Valid Lua script loads
   - Syntax errors
   - Runtime errors

3. **Function registry**
   - Functions registered correctly
   - Slot assignment
   - Registry cleanup on unload

4. **Trampoline dispatch**
   - `dispatch_lua_call()` with valid slot
   - Invalid slot handling
   - Pointer passing (i64 conversion)

5. **Guest library loading**
   - `GUEST_LUA_DIR` embedded correctly
   - Guest libs load from embedded path

#### 4.5 Python Loader Tests (`polyplug_python`)

**File:** `tests/python_loader.rs`

**Scenarios:**
1. **Python interpreter initialization**
   - First load initializes Python
   - GIL handling

2. **Module loading**
   - Import valid module
   - Import missing module
   - Import error handling

3. **Function dispatch**
   - Call Python function from host
   - Exception handling
   - Return value marshalling

---

## Test Implementation Priority

### Phase 1: Critical Safety (Week 1-2)
1. `integration_ffi_robustness.rs` - FFI safety
2. `stress_concurrency.rs` - Concurrency stress
3. `cli_validation.rs` - CLI validation
4. `parser_errors.rs` - Parser error handling
5. Language binding basic tests (dotnet, js, lua, python)

### Phase 2: Edge Cases (Week 3-4)
1. `integration_reload_edge_cases.rs` - Reload edge cases
2. `stress_allocator.rs` - Allocator stress
3. `integration_graph_edge_cases.rs` - Graph edge cases
4. `integration_registry_edge_cases.rs` - Registry edge cases
5. `codegen_verification.rs` - Cross-language codegen

### Phase 3: Integration & Stress (Week 5-6)
1. `integration_error_chain.rs` - Error propagation
2. `integration_version_edge_cases.rs` - Version edge cases
3. `integration_loader_edge_cases.rs` - Loader edge cases
4. `pack_command.rs` - Pack command
5. `generator_output_correctness.rs` - Output correctness

### Phase 4: Unit Tests (Week 7-8)
1. Inline tests for `abi.rs`
2. Inline tests for `error.rs`
3. Inline tests for `loader/manifest.rs`
4. Inline tests for `loader/scanner.rs`
5. Inline tests for `ir.rs` and `parser.rs`

---

## Estimated Test Count

| Category | Estimated Tests | Priority |
|----------|----------------|----------|
| FFI Robustness | 15 | Critical |
| Concurrency Stress | 10 | Critical |
| Reload Edge Cases | 10 | Critical |
| Allocator Stress | 8 | Critical |
| Graph Edge Cases | 10 | Critical |
| Registry Edge Cases | 8 | Critical |
| CLI Validation | 15 | Critical |
| Parser Errors | 20 | Critical |
| Codegen Verification | 12 | Critical |
| Pack Command | 8 | Critical |
| Language Binding Tests | 25 | Critical |
| Error Chain | 5 | High |
| Version Edge Cases | 8 | High |
| Loader Edge Cases | 10 | High |
| Generator Output | 10 | High |
| Unit Tests | 50 | Medium |
| **TOTAL** | **~224** | |

---

## Success Criteria

A test is considered "REAL and NEEDED" if it:
1. Tests an error path that could crash or corrupt
2. Tests concurrent access that could race
3. Tests security boundary (guest cannot breach host)
4. Tests correctness of code generation
5. Tests compatibility across language boundaries
6. Tests resource exhaustion handling
7. Tests malformed input handling

**Current test count:** ~60 tests (integration + unit)
**Target test count:** ~280+ tests (4x increase)
**Focus areas:** Error handling, concurrency, FFI safety, cross-language correctness
