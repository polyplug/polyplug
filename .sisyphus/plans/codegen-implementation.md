# polyplugc Codegen Implementation

## TL;DR

> **Quick Summary**: Implement the Rust and C++ code generators in `polyplugc` from their current stubs to full working implementations, fix the CLI dispatch logic, repair broken test fixtures, and create end-to-end integration tests.
>
> **Deliverables**:
> - `crates/polyplugc/src/generators/rust/mod.rs` — full Rust generator (host callers + guest SDK)
> - `crates/polyplugc/src/generators/cpp/mod.rs` — full C++ generator (host callers + guest SDK)
> - `crates/polyplugc/src/main.rs` — fixed CLI dispatch (`--api` → host+guest, `--bundle` → guest only) + bundle chain-loading
> - `guest-libs/rust/src/lib/mod.rs` — `PluginError` type added
> - `host-libs/cpp/polyplug/error.hpp` — `PolyplugException` type added
> - `tests/fixtures/test_api.toml` + `test_bundle.toml` — fixed to use correct TOML schema (singular `[[contract]]`, `[[plugin]]`, scalar `return = "..."`)
> - `tests/fixtures/test_plugin/src/lib.rs` — `panicking_fn` export added
> - `tests/integration_codegen_rust/mod.rs` — generate → compile → load → call → assert
> - `tests/integration_codegen_cpp/mod.rs` — generate → compile C++ → load → call → assert
> - `tests/integration_panic/mod.rs` — panic isolation test
>
> **Estimated Effort**: XL
> **Parallel Execution**: YES — 4 waves
> **Critical Path**: Task 1 (fixtures) → Task 2 (parser chain-load) → Task 3 (CLI dispatch) → Task 4 (PluginError) → Task 5 (Rust host gen) → Task 7 (Rust guest gen) → Task 9 (Rust integration test) → F1-F4

---

## Context

### Original Request
Complete the Rust and C++ code generators in polyplugc so that `polyplugc generate --api api.toml --lang rust --out ./out` and `--lang cpp` produce real compilable code, and integration tests verify end-to-end behavior.

### Interview Summary
**Key Discussions**:
- Rust guest traits: `Result<T, polyplug_guest::PluginError>` where `PluginError = struct { code: u32, message: String }`
- C++ host callers: exceptions (`throw PolyplugException`), defined in `host-libs/cpp/polyplug/error.hpp`
- Multiple output files per PRD §10: `types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`, `manifest.toml`
- `--api` flag → call both `generate_host()` + `generate_guest()` (host callers + guest SDK types/traits)
- `--bundle` flag → `generate_guest()` only (full plugin glue, chain-loads api.toml from `bundle.api` field)
- C++ integration tests: `cc` crate in `build.rs` to compile C++ test plugin
- Panic isolation: add `panicking_fn` export to existing `test_plugin` fixture
- Generated Rust: emit well-formatted code, verify with `cargo fmt --check`
- `PolyplugException`: added to existing `host-libs/cpp/polyplug/error.hpp`
- Host caller struct: holds `&'static polyplug_runtime::runtime::Runtime`, calls `Runtime::call_plugin()`
- Bundle chain-loading: `parse_bundle()` reads `bundle.api` field and chain-loads the referenced `api.toml`

**Research Findings**:
- Existing `test_api.toml` + `test_bundle.toml` use WRONG TOML keys (`[[contracts]]`, `[[plugins]]`, `returns` as array-of-tables) — must be fixed first
- `generate_guest()` has `#[allow(dead_code)]` — never called by CLI yet
- `loader/mod.rs:registrar_callback` is a stub returning `AbiError::ok()` without registering — existing integration tests work around this by injecting their own registrar
- `#[unsafe(no_mangle)]` required (Rust 2024 edition) — NOT `#[no_mangle]`
- `FnPtr` wrapper newtype needed for `Sync` in static vtable arrays
- Contract IDs must be hardcoded as `const` in generated code (not computed at runtime)
- `catch_unwind` requires `std::panic::AssertUnwindSafe` wrapper

### Metis Review
**Identified Gaps** (addressed):
- Bundle→API chain-loading: `parse_bundle()` will read `bundle.api` field and chain-load; decided by user.
- Host caller dispatch target: holds `&'static Runtime`; decided by user.
- PluginError shape: `struct { code: u32, message: String }`; decided by user.
- PolyplugException location: `host-libs/cpp/polyplug/error.hpp`; decided by user.
- Fixture TOML bugs: Task 1 explicitly fixes these before any other task.
- `FnPtr` wrapper pattern: codegen guardrail in Tasks 7 and 8.
- `catch_unwind(AssertUnwindSafe(...))`: explicitly specified in Tasks 7 and 8.
- `#[unsafe(no_mangle)]` vs `#[no_mangle]`: guardrail in all guest-side tasks.
- Buffer resize-retry protocol: explicitly OUT of scope.
- Name collision detection: explicitly OUT of scope (future validation enhancement).

---

## Work Objectives

### Core Objective
Implement complete Rust and C++ code generators in `polyplugc` so that the CLI produces compilable, correct output from `api.toml` and `bundle.toml` inputs, verified by end-to-end integration tests.

### Concrete Deliverables
- `crates/polyplugc/src/generators/rust/mod.rs` — produces 5 Rust files + manifest.toml
- `crates/polyplugc/src/generators/cpp/mod.rs` — produces 5 C++ files + manifest.toml
- `crates/polyplugc/src/main.rs` — correct dispatch + bundle chain-loading
- `guest-libs/rust/src/lib/mod.rs` — `PluginError` type
- `host-libs/cpp/polyplug/error.hpp` — `PolyplugException` type
- Fixed `tests/fixtures/test_api.toml`, `tests/fixtures/test_bundle.toml`
- Extended `tests/fixtures/test_plugin/src/lib.rs` with `panicking_fn`
- `tests/integration_codegen_rust/mod.rs` — end-to-end Rust codegen test
- `tests/integration_codegen_cpp/mod.rs` — end-to-end C++ codegen test
- `tests/integration_panic/mod.rs` — panic isolation test

### Definition of Done
- [ ] `cargo test --workspace` passes with zero failures
- [ ] `cargo clippy --workspace -- -D warnings` passes with zero warnings
- [ ] `cargo fmt --check` passes on all modified/generated files
- [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen` produces valid compilable Rust
- [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/gen` produces valid compilable C++
- [ ] `polyplugc validate --api tests/fixtures/test_api.toml` prints `OK:`
- [ ] `polyplugc validate --bundle tests/fixtures/test_bundle.toml` prints `OK:`
- [ ] All integration tests pass including panic isolation

### Must Have
- `#[unsafe(no_mangle)]` (Rust 2024) on all exported symbols in generated guest code
- `std::panic::catch_unwind(std::panic::AssertUnwindSafe(||{...}))` wrapping every ABI function body in generated Rust guest code
- `try { ... } catch(const std::exception&) { ... } catch(...) { ... }` wrapping every ABI function in generated C++ guest code
- `FnPtr` newtype wrapper (`#[repr(transparent)] pub struct FnPtr(pub *const ())` with `unsafe impl Sync`) in generated guest vtable code
- Contract IDs hardcoded as `const` in generated code using pre-computed FNV-1a values from IR
- `SAFETY:` comments on every `unsafe` block in generated code (justified in generator source)
- Auto-generated header comments in every generated file
- No `.unwrap()` / `.expect()` anywhere in generator production code (AGENTS.md §4)
- Explicit type annotations on all bindings (AGENTS.md §3)
- All new module files at `dirname/mod.rs` paths (AGENTS.md §1)
- `use` statements only at top of files (AGENTS.md §2)

### Must NOT Have (Guardrails)
- No modifications to `crates/polyplug-runtime/src/abi/` (frozen ABI — AGENTS.md §7)
- No new workspace dependencies for production code (no template engines, no new crates in `[dependencies]`). Build-time-only dependencies (in `[build-dependencies]`) are permitted when necessary for test infrastructure.
- No fixing the runtime stubs (`registrar_callback`, `host_find_plugin`, `host_call_plugin`) — out of scope
- No C#, Python, Lua generators — out of scope
- No extension system codegen — out of scope
- No `#[no_mangle]` (old form) — must use `#[unsafe(no_mangle)]`
- No `use` inside functions or impl blocks (AGENTS.md §2)
- No `import polyplug-runtime` directly in generated cdylib guest code — use `polyplug-guest` re-exports
- No Buffer resize-retry protocol in this iteration
- No name collision detection in generators (future work)

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed. No exceptions.

### Test Decision
- **Infrastructure exists**: YES (cargo test)
- **Automated tests**: Tests-after (implement, then write integration tests)
- **Framework**: `cargo test` (existing workspace test infrastructure)

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{slug}.txt`.

- **CLI**: Use Bash (`cargo run -p polyplugc`) — run command, assert exit code, check output
- **Compilation**: Use Bash (`cargo test`, `cargo clippy`) — check exit code
- **Integration tests**: Use Bash (`cargo test --test <name>`) — assert test passes

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — foundation, all parallel):
├── Task 1: Fix broken TOML fixtures + parser chain-loading [quick]
├── Task 2: Add PluginError to polyplug-guest crate [quick]
├── Task 3: Add PolyplugException to host-libs/cpp/polyplug/error.hpp [quick]
└── Task 4: Add panicking_fn to test_plugin + extend build.rs for cc [quick]

Wave 2 (After Wave 1 — core generators, parallel):
├── Task 5: Fix CLI dispatch in main.rs [quick]
├── Task 6: Implement Rust generate_host() [unspecified-high]
├── Task 7: Implement Rust generate_guest() [unspecified-high]
└── Task 8: Implement C++ generate_host() [unspecified-high]

Wave 3 (After Wave 2 — C++ guest + integration tests):
├── Task 9: Implement C++ generate_guest() [unspecified-high]
├── Task 10: Write integration_codegen_rust test [unspecified-high]
├── Task 11: Write integration_panic test [quick]
└── Task 12: Write integration_codegen_cpp test [unspecified-high]

Wave FINAL (After ALL tasks — independent review, 4 parallel):
├── Task F1: Plan Compliance Audit [oracle]
├── Task F2: Code Quality Review [unspecified-high]
├── Task F3: Real QA [unspecified-high]
└── Task F4: Scope Fidelity Check [deep]
```

### Dependency Matrix

- **1**: None — 5, 6, 7, 8, 9, 10, 11, 12
- **2**: None — 7, 10
- **3**: None — 8, 12
- **4**: None — 11, 12
- **5**: 1 — 6, 7, 8, 9, 10, 11, 12
- **6**: 1, 2, 5 — 10
- **7**: 1, 2, 5 — 10
- **8**: 1, 3, 5 — 9, 12
- **9**: 3, 5, 8 — 12
- **10**: 2, 5, 6, 7 — F1-F4
- **11**: 4, 5 — F1-F4
- **12**: 3, 4, 5, 8, 9 — F1-F4

### Agent Dispatch Summary

- **Wave 1**: Task 1 → `quick`, Task 2 → `quick`, Task 3 → `quick`, Task 4 → `quick`
- **Wave 2**: Task 5 → `quick`, Task 6 → `unspecified-high`, Task 7 → `unspecified-high`, Task 8 → `unspecified-high`
- **Wave 3**: Task 9 → `unspecified-high`, Task 10 → `unspecified-high`, Task 11 → `quick`, Task 12 → `unspecified-high`
- **Final**: F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

---

- [ ] 1. Fix broken TOML fixtures and extend parser for bundle chain-loading



  **What to do**:

  - Fix `tests/fixtures/test_api.toml`: change `[[contracts]]` → `[[contract]]`, `[[contracts.functions]]` → `[[contract.functions]]`, change `[[contracts.functions.returns]]` (array-of-tables) → `return = "u32"` (scalar string). **DO NOT change `[[types]]`** — `crates/polyplugc/src/parser/mod.rs:28-32` defines `RawApiSchema { types: Vec<RawType>, contract: Vec<RawContract> }`, so `[[types]]` (plural) is correct and must be kept.

  - Fix `tests/fixtures/test_bundle.toml`: change `[[plugins]]` → `[[plugin]]`

  - Create new `tests/fixtures/test_api.toml` with a richer contract that exercises: primitive return, user-defined struct return, `StringView` param, void return, and zero-param function — minimum contract definition:

    ```toml

    [[types]]

    name = "AddArgs"

    fields = [

      { name = "a", type = "u32" },

      { name = "b", type = "u32" }

    ]



    [[contract]]

    name = "test.add"

    version = "1.0.0"



    [[contract.functions]]

    name = "add"

    params = [{ name = "args", type = "AddArgs" }]

    return = "u32"



    [[contract.functions]]

    name = "add_primitive"

    params = [{ name = "a", type = "u32" }, { name = "b", type = "u32" }]

    return = "u32"



    [[contract.functions]]

    name = "version"

    return = "StringView"



    [[contract.functions]]

    name = "reset"

    ```

  - Fix `tests/fixtures/test_bundle.toml` to use `[[plugin]]` (singular) and add `api = "test_api.toml"` field

  - Extend `crates/polyplugc/src/parser/mod.rs` → add `parse_bundle_with_api(bundle_path: &Path) -> Result<ValidatedIr, CodegenError>` function:

    - Parse `bundle.toml` to get `RawBundleSchema`

    - Read `raw_bundle.bundle.api` field: if `Some(api_path_str)`, resolve it relative to the bundle file's parent directory

    - Call `parse_api(resolved_api_path)` to get types + contracts

    - Merge: return `ValidatedIr { types: api_ir.types, contracts: api_ir.contracts, bundle: Some(bundle) }`

    - If `bundle.api` is `None`: return `ValidatedIr { types: vec![], contracts: vec![], bundle: Some(bundle) }`

    - Error if api path cannot be read: `CodegenError::WriteFailed` (reuse existing variant)

  - Expose `parse_bundle_with_api` as `pub(crate)` in `parser/mod.rs`

  - All new bindings must have explicit type annotations (AGENTS.md §3)

  - No `.unwrap()` / `.expect()` (AGENTS.md §4)



  **Must NOT do**:

  - Do NOT modify `crates/polyplug-runtime/src/abi/` (frozen)

  - Do NOT add new workspace dependencies

  - Do NOT change the `RawApiSchema` or `RawBundleSchema` struct field types — only add the new chaining function



  **Recommended Agent Profile**:

  - **Category**: `quick`

    - Reason: Mechanical TOML text fixes + small function addition to existing parser module

  - **Skills**: none needed



  **Parallelization**:

  - **Can Run In Parallel**: YES

  - **Parallel Group**: Wave 1 (with Tasks 2, 3, 4)

  - **Blocks**: Tasks 5, 6, 7, 8, 9, 10, 11, 12

  - **Blocked By**: None (can start immediately)



  **References**:

  - `crates/polyplugc/src/parser/mod.rs` — existing `parse_api()`, `parse_bundle()`, `lower_api()`, `lower_bundle()` functions to follow the pattern

  - `crates/polyplugc/src/ir/mod.rs` — `ValidatedIr`, `ResolvedBundle` types

  - `crates/polyplugc/src/error/mod.rs` — `CodegenError::WriteFailed` to reuse for file read errors

  - `tests/fixtures/test_api.toml` (current broken version) — this IS the file to fix in-place

  - `tests/fixtures/test_bundle.toml` (current broken version) — fix in-place



  **Acceptance Criteria**:



  QA Scenarios:



  ```

  Scenario: parse_api parses the fixed test_api.toml without error

    Tool: Bash

    Steps:

      1. cargo run -p polyplugc -- validate --api tests/fixtures/test_api.toml

    Expected Result: exits 0 and prints "OK: tests/fixtures/test_api.toml"

    Evidence: .sisyphus/evidence/task-1-validate-api.txt



  Scenario: parse_bundle_with_api chains-loads api types

    Tool: Bash

    Steps:

      1. cargo run -p polyplugc -- validate --bundle tests/fixtures/test_bundle.toml

    Expected Result: exits 0 and prints "OK: tests/fixtures/test_bundle.toml"

    Evidence: .sisyphus/evidence/task-1-validate-bundle.txt



  Scenario: polyplugc unit tests still pass after changes

    Tool: Bash

    Steps:

      1. cargo test -p polyplugc

    Expected Result: exit code 0, all tests pass

    Evidence: .sisyphus/evidence/task-1-unit-tests.txt

  ```



  - [ ] `cargo run -p polyplugc -- validate --api tests/fixtures/test_api.toml` exits 0

  - [ ] `cargo run -p polyplugc -- validate --bundle tests/fixtures/test_bundle.toml` exits 0

  - [ ] `cargo test -p polyplugc` passes (unit tests still green)

  - [ ] `cargo clippy --workspace -- -D warnings` passes



  **Commit**: YES (Wave 1 group)

  - Message: `fix(fixtures): repair TOML schema format and add bundle chain-loading`

  - Files: `tests/fixtures/test_api.toml`, `tests/fixtures/test_bundle.toml`, `crates/polyplugc/src/parser/mod.rs`

  - Pre-commit: `cargo test -p polyplugc`


- [ ] 2. Add `PluginError` type to `guest-libs/rust`



  **What to do**:

  - Edit `guest-libs/rust/src/lib/mod.rs` to add:

    ```rust

    /// Error returned from guest-side plugin trait methods.

    ///

    /// Produced by generated ABI wrappers when an ABI call returns a non-zero code.

    /// Plugin developers return `Result<T, PluginError>` from their trait implementations.

    #[derive(Debug)]

    pub struct PluginError {

        /// ABI error code (non-zero).

        pub code: u32,

        /// Human-readable error message. May be empty.

        pub message: String,

    }



    impl core::fmt::Display for PluginError {

        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {

            write!(f, "PluginError(code={}, message={})", self.code, self.message)

        }

    }

    ```

  - All `use` statements at top of file only (AGENTS.md §2)

  - Explicit `pub` visibility on struct and fields (AGENTS.md §5)

  - No `.unwrap()` / `.expect()` in the impl blocks



  **Must NOT do**:

  - Do NOT make `PluginError` a repr(C) type — it is Rust-only, NOT an ABI boundary type

  - Do NOT add `PluginError` to the ABI types in `polyplug-runtime`



  **Recommended Agent Profile**:

  - **Category**: `quick`

    - Reason: Small type addition to an existing file, no new dependencies

  - **Skills**: none



  **Parallelization**:

  - **Can Run In Parallel**: YES

  - **Parallel Group**: Wave 1 (with Tasks 1, 3, 4)

  - **Blocks**: Tasks 7, 10

  - **Blocked By**: None



  **References**:

  - `guest-libs/rust/src/lib/mod.rs:1-18` — existing file to extend (add after existing re-exports)

  - `crates/polyplug-runtime/src/error/mod.rs` — style reference for error types in this codebase



  **Acceptance Criteria**:



  ```

  Scenario: PluginError compiles as part of the workspace

    Tool: Bash

    Steps:

      1. cargo test -p polyplug-guest

    Expected Result: exit code 0

    Evidence: .sisyphus/evidence/task-2-plugin-error-compiles.txt

  ```



  - [ ] `cargo test -p polyplug-guest` exits 0

  - [ ] `cargo clippy -p polyplug-guest -- -D warnings` passes



  **Commit**: YES (Wave 1 group)



- [ ] 3. Add `PolyplugException` to `host-libs/cpp/polyplug/error.hpp`



  **What to do**:

  - Read existing `host-libs/cpp/polyplug/error.hpp` (which currently exists but may be empty or minimal)

  - Add a C++ exception class:

    ```cpp

    // THIS FILE IS PART OF polyplug — host-side C++ error type.

    #pragma once

    #include "abi.hpp"

    #include <cstdint>

    #include <stdexcept>

    #include <string>



    namespace polyplug {



    /// Exception thrown by generated host callers when an ABI call returns a non-zero code.

    class PolyplugException : public std::runtime_error {

    public:

        explicit PolyplugException(uint32_t code, const std::string& message)

            : std::runtime_error(message), code_(code) {}

        uint32_t code() const noexcept { return code_; }

    private:

        uint32_t code_;

    };



    /// Throw a PolyplugException if the AbiError indicates failure.

    inline void check_abi_error(AbiError err) {

        if (err.code != ABI_OK) {

            const char* msg = (err.message.ptr != nullptr)

                ? reinterpret_cast<const char*>(err.message.ptr)

                : "unknown error";

            throw PolyplugException{err.code, std::string(msg, err.message.len)};

        }

    }



    }  // namespace polyplug

    ```

  - Generated C++ host callers will `#include "polyplug/error.hpp"` and call `polyplug::check_abi_error(err)` after each vtable dispatch



  **Must NOT do**:

  - Do NOT modify `host-libs/cpp/polyplug/abi.hpp` (frozen ABI)

  - Do NOT use exceptions that cross `extern "C"` boundaries



  **Recommended Agent Profile**:

  - **Category**: `quick`

    - Reason: Small C++ header addition, no compilation step needed for this task

  - **Skills**: none



  **Parallelization**:

  - **Can Run In Parallel**: YES

  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 4)

  - **Blocks**: Tasks 8, 12

  - **Blocked By**: None



  **References**:

  - `host-libs/cpp/polyplug/abi.hpp` — `AbiError`, `ABI_OK` definitions to include

  - `host-libs/cpp/polyplug.hpp` — main host header that includes `error.hpp`

  - `guest-libs/cpp/polyplug/guest.hpp` — style reference for C++ polyplug headers



  **Acceptance Criteria**:



  ```

  Scenario: error.hpp can be compiled by a C++17 compiler

    Tool: Bash

    Steps:

      1. echo '#include "host-libs/cpp/polyplug/error.hpp"\nint main() { return 0; }' > /tmp/test_err.cpp

      2. g++ -std=c++17 -I. /tmp/test_err.cpp -o /tmp/test_err

    Expected Result: exit code 0, no errors

    Evidence: .sisyphus/evidence/task-3-error-hpp-compiles.txt

  ```



  - [ ] `g++ -std=c++17` compiles a file that includes the modified `error.hpp`



  **Commit**: YES (Wave 1 group)



- [ ] 4. Add `panicking_fn` to test_plugin and extend build.rs for C++ compilation



  **What to do**:

  - Edit `tests/fixtures/test_plugin/src/lib.rs` to add:

    - A new exported symbol `polyplug_panicking_fn`:

      ```rust

      // ─── Panic test ───────────────────────────────────────────────────────────



      /// A function that always panics — used to verify panic isolation.

      ///

      /// # Safety

      /// args and out must be valid pointers (they are ignored, but must be non-null).

      #[unsafe(no_mangle)]

      pub extern "C" fn polyplug_panicking_fn(_args: *const (), _out: *mut ()) -> AbiError {

          panic!("intentional test panic");

      }

      ```

    - This is a raw ABI function, NOT registered in the vtable. The panic test calls it directly via `dlsym`.

    - Note: `std::panic::catch_unwind` is NOT wrapped here — this function is intentionally unprotected so the CALLER can demonstrate that catching it at the caller site (with `catch_unwind`) works

  - Edit `crates/polyplug-runtime/build.rs` to ALSO compile a C++ test plugin:

    - **Do NOT add the `cc` crate**. Compile the C++ plugin using `std::process::Command` directly in `build.rs` (no external crate needed). Build scripts are allowed to use `std::process::Command::new("g++")` directly.

    - In `build.rs`, after the existing `cargo build -p test_plugin` block, add a block that:

      1. Checks if `g++` (or `c++`) is available via `Command::new("g++").arg("--version")`

      2. Writes a minimal C++ plugin source to a temp file in `OUT_DIR`:

         ```cpp

         // THIS FILE IS AUTO-GENERATED BY polyplug build.rs — test only

         #include <cstdint>

         // Mirror ABI types inline (no header dependency)

         struct StringView { const uint8_t* ptr; size_t len; };

         struct AbiError { uint32_t code; StringView message; };

         struct PluginHandle { uint32_t index; uint32_t generation; };

         struct PluginVTable { uint64_t contract_id; uint32_t contract_version; uint32_t function_count; void* const* functions; };

         struct PluginDescriptor { StringView name; StringView contract_name; uint32_t version_major; uint32_t version_minor; uint32_t version_patch; };

         struct PluginRegistrar {

             AbiError (*register_plugin)(PluginRegistrar*, const PluginDescriptor*, const PluginVTable*) ;

             const void* host;

         };

         constexpr uint32_t ABI_OK = 0;

         // test.add contract_id = FNV-1a("test.add@1") = 0xCC4232FAB0410D2BU

         constexpr uint64_t TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2BULL;

         struct AddArgs { uint32_t a; uint32_t b; };



         extern "C" AbiError cpp_test_add(const void* args, void* out) {

             const AddArgs* add_args = static_cast<const AddArgs*>(args);

             uint32_t result = add_args->a + add_args->b;

             *static_cast<uint32_t*>(out) = result;

             AbiError ok{};

             ok.code = ABI_OK;

             ok.message.ptr = nullptr;

             ok.message.len = 0;

             return ok;

         }



         static void* const CPP_TEST_ADD_FNS[] = { reinterpret_cast<void*>(cpp_test_add) };

         static PluginVTable CPP_TEST_ADD_VTABLE = { TEST_ADD_CONTRACT_ID, 1U << 16, 1, CPP_TEST_ADD_FNS };

         static PluginDescriptor CPP_TEST_ADD_DESC = {

             { (const uint8_t*)"cpp_test_adder", 14 },

             { (const uint8_t*)"test.add", 8 },

             1, 0, 0

         };



         extern "C" uint32_t polyplug_abi_version() { return 1; }

         extern "C" AbiError polyplug_init(PluginRegistrar* registrar) {

             return registrar->register_plugin(registrar, &CPP_TEST_ADD_DESC, &CPP_TEST_ADD_VTABLE);

         }

         ```

      3. Compile with `g++ -std=c++17 -shared -fPIC -o {OUT_DIR}/libtest_plugin_cpp.so {temp_cpp_file}`

      4. Copy the resulting `.so` to `tests/fixtures/libtest_plugin_cpp.so`

      5. Emit `cargo:rustc-env=TEST_PLUGIN_CPP_SO={dest_so}`

      6. If `g++` is not available: emit `cargo:rustc-env=TEST_PLUGIN_CPP_SO=` (empty) and a warning; the cpp test will skip via `#[cfg_attr(...)]` or a runtime check

    - Build script is exempt from `.expect()` / `panic!()` restriction per AGENTS.md §4 footnote ("Build scripts are permitted to use .expect() freely")



  **Must NOT do**:

  - Do NOT add the panicking function to the vtable

  - Do NOT register `panicking_fn` via `polyplug_init`

  - Do NOT catch the panic in the test_plugin — the panic test in Task 11 tests the generated ABI wrapper's `catch_unwind` which catches it INSIDE the plugin boundary



  **Recommended Agent Profile**:

  - **Category**: `quick`

    - Reason: Small additions to existing files, C++ compilation logic in build script

  - **Skills**: none



  **Parallelization**:

  - **Can Run In Parallel**: YES

  - **Parallel Group**: Wave 1 (with Tasks 1, 2, 3)

  - **Blocks**: Tasks 11, 12

  - **Blocked By**: None



  **References**:

  - `tests/fixtures/test_plugin/src/lib.rs:202-235` — existing `polyplug_abi_version` and `polyplug_init` exports to follow the pattern for `polyplug_panicking_fn`

  - `crates/polyplug-runtime/build.rs` — existing build script to extend

  - `crates/polyplug-runtime/Cargo.toml` — add `cc` to `[build-dependencies]`



  **Acceptance Criteria**:



  ```

  Scenario: test_plugin still compiles and test suite passes

    Tool: Bash

    Steps:

      1. cargo build -p test_plugin

      2. cargo test -p polyplug-runtime --test integration_dispatch

    Expected Result: both exit 0

    Evidence: .sisyphus/evidence/task-4-test-plugin-compiles.txt



  Scenario: C++ test plugin is compiled by build.rs

    Tool: Bash

    Steps:

      1. cargo build -p polyplug-runtime

      2. ls tests/fixtures/libtest_plugin_cpp.so

    Expected Result: file exists

    Evidence: .sisyphus/evidence/task-4-cpp-plugin-built.txt

  ```



  - [ ] `cargo test -p polyplug-runtime --test integration_dispatch` still passes

  - [ ] `tests/fixtures/libtest_plugin_cpp.so` exists after `cargo build`



  **Commit**: YES (Wave 1 group)

  - Message: `fix(fixtures): repair TOML schema format and add bundle chain-loading`

  - Files: All Wave 1 files

  - Pre-commit: `cargo test -p polyplugc && cargo test -p polyplug-runtime --test integration_dispatch`



---

- [ ] 5. Fix CLI dispatch in `main.rs`

  **What to do**:

  - Edit `crates/polyplugc/src/main.rs`, inside `Command::Generate { ... }` match arm (lines 78–109):

    1. Change the `--bundle` branch to call `parser::parse_bundle_with_api(&bundle_path)?` instead of `parser::parse_bundle(&bundle_path)?`

    2. After the `let ir:` binding, change the single `generator.generate_host(&ir, &mut files)?;` call to:
       ```rust
       if api.is_some() {
           generator.generate_host(&ir, &mut files)?;
           generator.generate_guest(&ir, &mut files)?;
       } else {
           // bundle path: guest SDK only
           generator.generate_guest(&ir, &mut files)?;
       }
       ```
       Note: the `api` / `bundle` variables are moved into the `ir` binding, so track which was `Some` with a boolean or restructure the match arm to record source before consuming. Recommended: add `let from_api: bool = api.is_some();` before the `let ir: ...` binding, then use `from_api` in the dispatch.

  - Update `Command::Validate` branch (lines 111–123): in the `--bundle` branch, call `parser::parse_bundle_with_api(&bundle_path)?` instead of `parser::parse_bundle(&bundle_path)?`.

  - Remove the `#[allow(dead_code)]` attribute from `generate_guest` in `crates/polyplugc/src/generators/mod.rs:36` — it is now called.

  - Explicit type annotation required on the `from_api` binding: `let from_api: bool = api.is_some();`

  - All `use` at file top only — do NOT add any `use` inside `run()` (AGENTS.md §2)

  **Must NOT do**:

  - Do NOT change the CLI argument structure (no new flags)
  - Do NOT call `generate_host()` when `--bundle` is provided
  - Do NOT modify `ValidatedIr` or parser internals — only change call sites in `main.rs`

  **Recommended Agent Profile**:

  - **Category**: `quick`
    - Reason: Mechanical 4-line logic change in a single file
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 2 (with Tasks 6, 7, 8) — but must be complete before 6/7/8 attempt to call `generate_guest()`
  - **Blocks**: Tasks 6, 7, 8
  - **Blocked By**: Task 1 (needs `parse_bundle_with_api` to exist)

  **References**:

  - `crates/polyplugc/src/main.rs:70-126` — entire `run()` function to rewrite dispatch logic
  - `crates/polyplugc/src/generators/mod.rs:36-41` — `generate_guest` declaration where `#[allow(dead_code)]` lives
  - `crates/polyplugc/src/parser/mod.rs` — `parse_bundle_with_api` to call (added in Task 1)

  **Acceptance Criteria**:

  ```
  Scenario: --api flag calls both generate_host and generate_guest
    Tool: Bash
    Steps:
      1. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen_api
      2. ls /tmp/gen_api/
    Expected Result: exit 0; output directory contains both host_callers.rs AND contracts.rs (or guest_sdk.rs)
    Evidence: .sisyphus/evidence/task-5-api-dispatch.txt

  Scenario: --bundle flag calls generate_guest only (no host_callers file)
    Tool: Bash
    Steps:
      1. cargo run -p polyplugc -- generate --bundle tests/fixtures/test_bundle.toml --lang rust --out /tmp/gen_bundle
      2. ls /tmp/gen_bundle/
    Expected Result: exit 0; output does NOT contain host_callers.rs
    Evidence: .sisyphus/evidence/task-5-bundle-dispatch.txt

  Scenario: cargo test -p polyplugc still passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplugc
    Expected Result: exit 0
    Evidence: .sisyphus/evidence/task-5-unit-tests.txt
  ```

  - [ ] `polyplugc generate --api ... --lang rust` produces both host and guest files
  - [ ] `polyplugc generate --bundle ... --lang rust` produces guest-only files
  - [ ] `cargo clippy --workspace -- -D warnings` passes

  **Commit**: NO (group with Wave 2)

---

- [ ] 6. Implement Rust `generate_host()` — `types.rs` + `host_callers.rs`

  **What to do**:

  Completely rewrite the body of `RustGenerator::generate_host()` in `crates/polyplugc/src/generators/rust/mod.rs`.
  The current implementation puts everything into one `host_callers.rs`. Split into two separate files:

  **File 1: `types.rs`**

  Emit auto-generated header comment, then for each `ResolvedType` in `ir.types`:
  - Emit `#[repr(C)]`, `#[derive(Debug, Clone, Copy)]`, and the struct definition with `pub` fields
  - Use `rust_type_name()` helper (already exists) for field types
  - Types that are user-defined structs must be `pub` and re-exported from `types.rs`

  **File 2: `host_callers.rs`**

  Emit auto-generated header comment. Then emit these `use` imports at the top:
  ```rust
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  // Re-generate with: polyplugc generate --api api.toml --lang rust --out <dir>

  use polyplug_guest::PluginError;
  use polyplug_runtime::abi::AbiError;
  use polyplug_runtime::abi::PluginHandle;
  use polyplug_runtime::runtime::Runtime;
  use super::types::*;
  ```

  For each `ResolvedContract` in `ir.contracts`:
  - Emit a struct `{ContractName}Contract { handle: PluginHandle, runtime: &'static Runtime }` with `pub` visibility
  - Emit an `impl {ContractName}Contract` block with:
    - `pub fn new(handle: PluginHandle, runtime: &'static Runtime) -> Self`
    - For each `ResolvedFunction` in `contract.functions`:
      - Determine if primitive return: `matches!(returns, Some(ResolvedTypeRef::Primitive(_)))` or `returns.is_none()` (void)
      - **ALL return types** (primitive, user-defined struct, void): dev-facing function ALWAYS returns `Result<RetType, PluginError>` — call can always fail with an ABI error. This is the consistent rule: no special-casing primitives at the type level.
      - **Non-primitive return** (user-defined struct or `StringView` / `Buffer`): no special casing — same `Result<RetType, PluginError>` pattern as all other returns.
      - Params that are **non-primitive**: pass by reference (`arg: &MyStruct`), then cast `arg as *const MyStruct as *const ()`
      - Params that are **primitive**: pass by value (`a: u32`), take address: `&a as *const u32 as *const ()`; if multiple primitive params, wrap them in a generated tuple-struct or use the function's single-param pattern — for simplicity, if a function has only primitive params, pack them into a `#[repr(C)] struct {ContractName}{FuncName}Args { ... }` declared in `types.rs` by the host generator
      - The function body calls `unsafe { self.runtime.call_plugin(self.handle, {fn_id}_u32, args_ptr, out_ptr) }` and checks the returned `AbiError`:
        ```rust
        let err: AbiError = unsafe { self.runtime.call_plugin(self.handle, {fn_id}_u32, args_ptr, out_ptr) };
        if err.code != polyplug_runtime::abi::ABI_OK {
            return Err(PluginError { code: err.code, message: String::new() });
        }
        Ok(out_val)  // or Ok(()) for void
        ```
  - Add a `// SAFETY:` comment on every unsafe block explaining why args_ptr and out_ptr are valid

  **File 3: `manifest.toml`**

  Emit a minimal manifest file for the generated crate:
  ```toml
  # THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  [manifest]
  schema_version = 1
  lang = "rust"
  generated_by = "polyplugc"
  ```

  Push all 3 files (`types.rs`, `host_callers.rs`, `manifest.toml`) into `files.files`.

  Preserve all existing helper functions (`rust_type_name`, `contract_name_to_struct`, `generate_rust_type`) — extend them as needed.
  All new bindings: explicit type annotations. No `.unwrap()`. All `use` at file top in generated output.

  **Must NOT do**:

  - Do NOT emit `contracts.rs`, `vtables.rs`, `init.rs` in `generate_host()` — those are guest-only files
  - Do NOT call `generate_guest()` from within `generate_host()`
  - Do NOT add new crate dependencies to `Cargo.toml`
  - Do NOT use template engines — all code is built with `String::push_str()`

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Non-trivial code generation logic, reference pattern matching, type dispatch
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 7, 8)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 1, 2, 5

  **References**:

  - `crates/polyplugc/src/generators/rust/mod.rs:22-95` — current `generate_host()` and `generate_guest()` stubs to replace
  - `crates/polyplugc/src/generators/rust/mod.rs:117-208` — `generate_rust_type()`, `generate_rust_host_contract()`, `generate_rust_host_fn()`, `rust_type_name()`, `contract_name_to_struct()` helper functions to extend (not replace)
  - `crates/polyplugc/src/ir/mod.rs:142-225` — `ResolvedTypeRef`, `ResolvedType`, `ResolvedContract`, `ResolvedFunction`, `ResolvedParam` types
  - `crates/polyplugc/src/ir/mod.rs:54-67` — `PrimitiveType` enum — use `matches!(ty, ResolvedTypeRef::Primitive(_))` to test
  - `crates/polyplug-runtime/src/runtime/mod.rs:130-164` — `Runtime::call_plugin()` exact signature: `unsafe fn call_plugin(&self, handle: PluginHandle, fn_id: u32, args: *const (), out: *mut ()) -> AbiError`
  - `tests/fixtures/test_plugin/src/lib.rs:161-173` — `plugin_add` function — reference for how args/out are cast and `core::ptr::write` is used
  - `guest-libs/rust/src/lib/mod.rs` — `PluginError` type (added in Task 2)

  **Acceptance Criteria**:

  ```
  Scenario: generate_host produces types.rs and host_callers.rs for test_api.toml
    Tool: Bash
    Steps:
      1. rm -rf /tmp/gen_rust_host && mkdir /tmp/gen_rust_host
      2. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen_rust_host
      3. ls /tmp/gen_rust_host/
    Expected Result: exit 0; files include types.rs, host_callers.rs, manifest.toml
    Evidence: .sisyphus/evidence/task-6-host-files-exist.txt

  Scenario: generated host_callers.rs contains contract struct and call_plugin dispatch
    Tool: Bash
    Steps:
      1. grep -n 'call_plugin\|PluginError\|Runtime' /tmp/gen_rust_host/host_callers.rs
    Expected Result: shows lines with `self.runtime.call_plugin(`, `PluginError`, and `Runtime`
    Evidence: .sisyphus/evidence/task-6-host-content-check.txt

  Scenario: generator unit tests pass
    Tool: Bash
    Steps:
      1. cargo test -p polyplugc
    Expected Result: exit 0
    Evidence: .sisyphus/evidence/task-6-unit-tests.txt
  ```

  - [ ] `/tmp/gen_rust_host/` contains `types.rs`, `host_callers.rs`, `manifest.toml`
  - [ ] `host_callers.rs` contains `AUTO-GENERATED` header comment
  - [ ] `host_callers.rs` contains at least one struct with `call_plugin` call
  - [ ] `cargo test -p polyplugc` passes

  **Commit**: NO (group with Wave 2)

- [ ] 7. Implement Rust `generate_guest()` — `types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`

  **What to do**:

  Completely rewrite the body of `RustGenerator::generate_guest()` in `crates/polyplugc/src/generators/rust/mod.rs`.
  Produce 4 files plus the manifest. All must start with the auto-generated header comment.

  **File 1: `types.rs`** — Same content as in `generate_host()` (user-defined structs). The executer may extract a shared `emit_types_file()` helper that both `generate_host()` and `generate_guest()` call.

  **File 2: `contracts.rs`** — Developer-facing trait per contract:
  ```rust
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  use polyplug_guest::PluginError;
  use super::types::*;

  pub trait {ContractName}Plugin: Send + Sync {
      fn {fn_name}(&self, args: &{ArgType}) -> Result<{RetType}, PluginError>;
      // ... one method per function
      // For void return: -> Result<(), PluginError>
      // For primitive return: -> Result<u32, PluginError>  (still Result — guest can still fail)
  }
  ```
  One trait per `ResolvedContract`. Each method corresponds to a `ResolvedFunction`.
  Non-primitive params: `arg: &ArgType`. Primitive params: `a: u32` etc. (pass by value).

  **File 3: `vtables.rs`** — Static vtable + ABI wrapper functions + FnPtr + IMPL static:
  ```rust
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  use std::sync::OnceLock;
  use polyplug_guest::AbiError;
  // FnPtr is NOT re-exported by polyplug_guest. Define it in generated vtables.rs:
  // #[repr(transparent)] pub struct FnPtr(pub *const ()); unsafe impl Send for FnPtr {} unsafe impl Sync for FnPtr {}
  use super::contracts::{ContractName}Plugin;
  use super::types::*;

  /// Contract ID constant — pre-computed FNV-1a of "{name}@{major}".
  pub(crate) const {CONTRACT_UPPER}_CONTRACT_ID: u64 = 0x{contract_id:016X};

  // OnceLock holding the developer's trait object. Set by polyplug_init.
  static {CONTRACT_UPPER}_IMPL: OnceLock<Box<dyn {ContractName}Plugin>> = OnceLock::new();

  /// ABI wrapper for {fn_name} (function_id = {fn_id}).
  //
  // SAFETY: args points to valid {ArgType} and out points to valid {RetType}.
  // Enforced by the host runtime's generated caller code.
  extern "C" fn {contract_upper}_{fn_name}_abi(args: *const (), out: *mut ()) -> AbiError {
      match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
          // SAFETY: args is a valid *const {ArgType} per ABI contract.
          let result = {CONTRACT_UPPER}_IMPL
              .get()
              .expect("IMPL not set — polyplug_init not called")
              .{fn_name}(unsafe { &*(args as *const {ArgType}) });
          match result {
              Ok(val) => {
                  // SAFETY: out is a valid *mut {RetType} per ABI contract.
                  unsafe { std::ptr::write(out as *mut {RetType}, val); }
                  AbiError::ok()
              }
              Err(e) => AbiError { code: e.code, message: ... }
          }
      })) {
          Ok(err) => err,
          Err(_) => AbiError::panic_caught(),
      }
  }

  static {CONTRACT_UPPER}_FNS: [FnPtr; {N}] = [
      FnPtr({contract_upper}_{fn_name0}_abi as *const ()),
      // ... one entry per function in declaration order
  ];

  pub(crate) static {CONTRACT_UPPER}_VTABLE: PluginVTable = PluginVTable {
      contract_id: {CONTRACT_UPPER}_CONTRACT_ID,
      contract_version: {version_minor} << 16 | {version_patch},
      function_count: {N},
      functions: {CONTRACT_UPPER}_FNS.as_ptr(),
  };
  ```
  **Important codegen details**:
  - `AbiError::ok()` and `AbiError::panic_caught()` are both defined as methods on `AbiError` in `crates/polyplug-runtime/src/abi/mod.rs:90-105` and re-exported from `polyplug_guest` at crate root. Emit `use polyplug_guest::AbiError;` (no `abi::` submodule exists).
  - `AbiError::panic_caught()` already exists as a method (returns `AbiError { code: 3 (ABI_ERROR_PANIC), message: StringView::null() }`). Emit `AbiError::panic_caught()` directly. Do NOT construct it inline.
  - For void-return functions: no `ptr::write`, just return `AbiError::ok()` or the error
  - For primitive-return functions (e.g. `-> u32`): `unsafe { std::ptr::write(out as *mut u32, val); }`
  - For user-defined struct return: same `ptr::write` pattern
  - `.expect()` on `OnceLock::get()` is inside a test-equivalent panic-catch context; however the rules forbid `.expect()` in production — instead use an explicit match: `match {CONTRACT_UPPER}_IMPL.get() { Some(impl_) => ..., None => return AbiError { code: ABI_ERROR_GENERIC, ... } }`
  - `#[unsafe(no_mangle)]` is NOT used on these inner ABI wrappers — they are referenced by pointer, not exported by name

  **File 4: `init.rs`** — The exported `polyplug_init` function:
  ```rust
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  use polyplug_guest::AbiError;
  use polyplug_guest::ABI_OK;  // ABI_OK is re-exported from polyplug_guest crate root
  use polyplug_guest::PluginDescriptor;
  use polyplug_guest::PluginRegistrar;
  use polyplug_guest::StringView;
  use crate::vtables::{CONTRACT_UPPER}_VTABLE;
  // ... one vtable import per contract

  #[unsafe(no_mangle)]
  pub extern "C" fn polyplug_abi_version() -> u32 { 1 }

  /// Register all plugin vtables with the host.
  ///
  /// # Safety
  /// `registrar` must be a valid non-null pointer to a PluginRegistrar.
  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_init(registrar: *mut PluginRegistrar) -> AbiError {
      if registrar.is_null() {
          return AbiError { code: ABI_ERROR_GENERIC, message: StringView::null() };
      }
      // SAFETY: registrar is non-null and valid per ABI contract.
      let reg = unsafe { &mut *registrar };
      // For each contract:
      let desc: PluginDescriptor = PluginDescriptor { ... };  // built from contract metadata
      // SAFETY: desc and vtable are 'static.
      let err: AbiError = unsafe { (reg.register_plugin)(registrar, &desc as *const _, &{CONTRACT_UPPER}_VTABLE as *const _) };
      if err.code != 0 { return err; }
      // ... repeat for each additional contract
      AbiError::ok()
  }
  ```
  PluginDescriptor fields: `name` = plugin crate name as `StringView` (hardcode from bundle metadata or use contract name), `contract_name` = contract name as `StringView`, `version_major/minor/patch` from contract version.
  Both `StringView` values must be byte literals (`b"test.add".as_ptr(), 8`) — NOT heap-allocated strings.

  **File 5: `manifest.toml`** — same as in Task 6 (lang = "rust").

  Push all 5 files (`types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`, `manifest.toml`) to `files.files`.

  **Must NOT do**:

  - Do NOT emit `host_callers.rs` in `generate_guest()`
  - Do NOT use `#[no_mangle]` (old form) — must be `#[unsafe(no_mangle)]`
  - Do NOT use `.unwrap()` anywhere in generator production code
  - Do NOT import `polyplug-runtime` directly in generated guest code — use `polyplug_guest::*` crate-root re-exports (there is no `polyplug_guest::abi` submodule; items are at crate root: `polyplug_guest::AbiError`, `polyplug_guest::ABI_OK`, etc.)

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Complex multi-file codegen with ABI wrapper generation, OnceLock, catch_unwind, FnPtr
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 8)
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 1, 2, 5

  **References**:

  - `crates/polyplugc/src/generators/rust/mod.rs:60-94` — current `generate_guest()` stub to replace
  - `tests/fixtures/test_plugin/src/lib.rs:124-235` — gold standard for: `FnPtr` newtype, `OnceLock`-less static vtable, `#[unsafe(no_mangle)]`, `polyplug_init` pattern, `SAFETY:` comments
  - `tests/fixtures/test_plugin/src/lib.rs:155-173` — `plugin_add` ABI wrapper pattern (the `catch_unwind` version is what codegen must produce)
  - `crates/polyplug-runtime/src/abi/mod.rs` — `ABI_ERROR_PANIC`, `ABI_ERROR_GENERIC`, `ABI_OK`, `StringView::null()` — reference these values by name in generated code
  - `guest-libs/rust/src/lib/mod.rs` — `PluginError { code, message }` shape used in trait return types
  - `crates/polyplugc/src/ir/mod.rs:184-192` — `ResolvedContract` fields: `name`, `contract_id`, `version`, `functions`
  - `crates/polyplugc/src/ir/mod.rs:176-182` — `ResolvedFunction` fields: `name`, `function_id`, `params`, `returns`

  **Acceptance Criteria**:

  ```
  Scenario: generate_guest produces all 5 files for test_api.toml
    Tool: Bash
    Steps:
      1. rm -rf /tmp/gen_rust_guest && mkdir /tmp/gen_rust_guest
      2. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen_rust_guest
      3. ls /tmp/gen_rust_guest/
    Expected Result: exit 0; files include types.rs, contracts.rs, vtables.rs, init.rs, manifest.toml
    Evidence: .sisyphus/evidence/task-7-guest-files-exist.txt

  Scenario: vtables.rs contains catch_unwind and no_mangle
    Tool: Bash
    Steps:
      1. grep -n 'catch_unwind\|no_mangle\|FnPtr\|OnceLock' /tmp/gen_rust_guest/vtables.rs
    Expected Result: all 4 patterns found
    Evidence: .sisyphus/evidence/task-7-vtable-content-check.txt

  Scenario: init.rs contains polyplug_init export
    Tool: Bash
    Steps:
      1. grep -n 'polyplug_init\|polyplug_abi_version\|no_mangle' /tmp/gen_rust_guest/init.rs
    Expected Result: all 3 patterns found
    Evidence: .sisyphus/evidence/task-7-init-content-check.txt
  ```

  - [ ] `/tmp/gen_rust_guest/` contains `types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`, `manifest.toml`
  - [ ] `vtables.rs` contains `catch_unwind`, `AssertUnwindSafe`, `FnPtr`, `OnceLock`
  - [ ] `init.rs` contains `#[unsafe(no_mangle)]` and `polyplug_init`
  - [ ] `cargo test -p polyplugc` passes

  **Commit**: NO (group with Wave 2)

---

- [ ] 8. Implement C++ `generate_host()` — `types.hpp` + `host_callers.hpp`

  **What to do**:

  Completely rewrite the body of `CppGenerator::generate_host()` in `crates/polyplugc/src/generators/cpp/mod.rs`.
  Split output into 2 header files plus manifest:

  **File 1: `types.hpp`**

  ```cpp
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  // Re-generate with: polyplugc generate --api api.toml --lang cpp --out <dir>
  #pragma once
  #include <cstdint>
  #include "polyplug/abi.hpp"

  using namespace polyplug;

  // For each ResolvedType:
  struct {TypeName} {
      {cpp_type} {field_name};  // one per field
  };
  ```

  **File 2: `host_callers.hpp`**

  ```cpp
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  #pragma once
  #include "types.hpp"
  #include "polyplug/error.hpp"  // PolyplugException, check_abi_error
  #include "polyplug/abi.hpp"

  namespace polyplug_generated {

  // For each ResolvedContract:
  class {ContractName}Contract {
  public:
      explicit {ContractName}Contract(PluginHandle handle, const HostVTable* host) noexcept
          : handle_(handle), host_(host) {}

      // For each ResolvedFunction:
      {ReturnType} {fn_name}({params}) {
          // For non-primitive/non-void params: pack into args struct
          // For primitive params: take address of local copy
          const void* args_ptr = ...; // cast to const void*
          {ReturnType} out{};         // zero-initialized output
          void* out_ptr = &out;       // for non-void returns
          AbiError err = (host_->call_plugin)(handle_, {fn_id}U, args_ptr, out_ptr);
          polyplug::check_abi_error(err);  // throws PolyplugException on failure
          return out;  // omit for void return
      }

  private:
      PluginHandle handle_;
      const HostVTable* host_;
  };

  }  // namespace polyplug_generated
  ```

  Dispatch uses `host_->call_plugin` (the `HostVTable.call_plugin` function pointer), NOT a direct Rust call.
  The `HostVTable` pointer is passed at construction and stored as a member.
  Call syntax: `(host_->call_plugin)(handle_, fn_id, args_ptr, out_ptr)` — calling through a function pointer stored in the struct.

  For void-return functions: omit the output buffer and `return` statement.
  For primitive params: `const {type} local_{name} = {name}; const void* args_ptr = &local_{name};`
  For user-defined struct params: `const void* args_ptr = &{name};` (already a struct).

  **File 3: `manifest.toml`** — same format as Task 6 with `lang = "cpp"`.

  Push all 3 files to `files.files`.

  **Must NOT do**:

  - Do NOT emit `contracts.hpp`, `vtables.hpp`, `init.hpp` in `generate_host()` — those are guest-only
  - Do NOT use C++ exceptions crossing `extern "C"` boundaries
  - Do NOT add new workspace dependencies

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: C++ class generation with vtable dispatch, header-only design
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 2 (with Tasks 5, 6, 7)
  - **Blocks**: Tasks 9, 12
  - **Blocked By**: Tasks 1, 3, 5

  **References**:

  - `crates/polyplugc/src/generators/cpp/mod.rs:26-54` — current `generate_host()` stub to replace
  - `crates/polyplugc/src/generators/cpp/mod.rs:90-164` — `generate_cpp_type()`, `generate_cpp_host_contract()`, `cpp_type_name()`, `contract_name_to_class()` helpers to extend
  - `host-libs/cpp/polyplug/error.hpp` — `PolyplugException`, `check_abi_error()` (added in Task 3)
  - `host-libs/cpp/polyplug/abi.hpp` — `HostVTable`, `PluginHandle`, `AbiError`, `ABI_OK` definitions
  - `crates/polyplug-runtime/src/abi/mod.rs` — `HostVTable` struct: `call_plugin` field is `unsafe extern "C" fn(PluginHandle, u32, *const (), *mut ()) -> AbiError`
  - `tests/fixtures/test_plugin/src/lib.rs:98-111` — `HostVTable` layout in C-compatible Rust for cross-referencing field order

  **Acceptance Criteria**:

  ```
  Scenario: generate_host produces types.hpp and host_callers.hpp for test_api.toml
    Tool: Bash
    Steps:
      1. rm -rf /tmp/gen_cpp_host && mkdir /tmp/gen_cpp_host
      2. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/gen_cpp_host
      3. ls /tmp/gen_cpp_host/
    Expected Result: exit 0; files include types.hpp, host_callers.hpp, manifest.toml
    Evidence: .sisyphus/evidence/task-8-host-files-exist.txt

  Scenario: host_callers.hpp contains call_plugin dispatch and check_abi_error
    Tool: Bash
    Steps:
      1. grep -n 'call_plugin\|check_abi_error\|PolyplugException' /tmp/gen_cpp_host/host_callers.hpp
    Expected Result: all 3 patterns found
    Evidence: .sisyphus/evidence/task-8-host-content-check.txt

  Scenario: types.hpp and host_callers.hpp compile with g++
    Tool: Bash
    Steps:
      1. echo '#include "/tmp/gen_cpp_host/host_callers.hpp"
int main() { return 0; }' > /tmp/test_h.cpp
      2. g++ -std=c++17 -I. /tmp/test_h.cpp -o /tmp/test_h
    Expected Result: exit 0
    Evidence: .sisyphus/evidence/task-8-headers-compile.txt
  ```

  - [ ] `/tmp/gen_cpp_host/` contains `types.hpp`, `host_callers.hpp`, `manifest.toml`
  - [ ] `host_callers.hpp` contains `AUTO-GENERATED` header
  - [ ] Headers compile clean with `g++ -std=c++17`
  - [ ] `cargo test -p polyplugc` passes

  **Commit**: YES (Wave 2 group commit)
  - Message: `feat(codegen): implement Rust and C++ host generators with full ABI dispatch`
  - Files: `crates/polyplugc/src/generators/rust/mod.rs`, `crates/polyplugc/src/generators/cpp/mod.rs`, `crates/polyplugc/src/main.rs`, `crates/polyplugc/src/generators/mod.rs`
  - Pre-commit: `cargo test -p polyplugc && cargo clippy --workspace -- -D warnings`

- [ ] 9. Implement C++ `generate_guest()` — `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`

  **What to do**:

  Completely rewrite the body of `CppGenerator::generate_guest()` in `crates/polyplugc/src/generators/cpp/mod.rs`.
  Produce 4 header files plus manifest. All start with the auto-generated header comment.

  **File 1: `types.hpp`** — Same as in `generate_host()` (user-defined structs). Extract a shared `emit_cpp_types_header()` helper.

  **File 2: `contracts.hpp`** — Abstract base class per contract (pure-virtual interface):
  ```cpp
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  #pragma once
  #include "types.hpp"

  namespace polyplug_plugin {

  struct PolyplugError { uint32_t code; };

  // For each contract:
  class {ContractName}Plugin {
  public:
      virtual ~{ContractName}Plugin() = default;
      // For each function:
      virtual {ReturnType} {fn_name}({params}) = 0;
      // void return -> virtual void {fn_name}({params}) = 0;
  };

  }  // namespace polyplug_plugin
  ```

  **File 3: `vtables.hpp`** — Extern-C ABI wrappers + vtable construction:
  ```cpp
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  #pragma once
  #include "contracts.hpp"
  #include "polyplug/abi.hpp"  // AbiError, PluginVTable, etc.

  namespace polyplug_plugin {

  // Forward-declared impl pointer (set by polyplug_init before use).
  // Use void* + static_cast for simplicity; the developer sets this before the host calls.
  extern {ContractName}Plugin* g_{contract_lower}_impl;

  constexpr uint64_t {CONTRACT_UPPER}_CONTRACT_ID = 0x{contract_id:016X}ULL;

  // ABI wrapper for {fn_name}:
  inline AbiError {contract_lower}_{fn_name}_abi(const void* args, void* out) noexcept {
      try {
          // SAFETY (in generator source): args points to valid {ArgType}, out to valid {RetType}.
          // Enforced by the generated host caller.
          auto result = g_{contract_lower}_impl->{fn_name}(
              *static_cast<const {ArgType}*>(args));
          // For non-void return:
          *static_cast<{RetType}*>(out) = result;
          return AbiError{ABI_OK, StringView{nullptr, 0}};
      } catch (const std::exception& e) {
          return AbiError{1, StringView{nullptr, 0}};  // ABI_ERROR_GENERIC
      } catch (...) {
          return AbiError{2, StringView{nullptr, 0}};  // ABI_ERROR_PANIC
      }
  }

  // Function pointer array:
  static void* const {CONTRACT_UPPER}_FNS[] = {
      reinterpret_cast<void*>({contract_lower}_{fn_name0}_abi),
      // ... one per function
  };

  static PluginVTable {CONTRACT_UPPER}_VTABLE = {
      {CONTRACT_UPPER}_CONTRACT_ID,
      {version},  // contract_version = (minor << 16 | patch)
      {N},        // function_count
      {CONTRACT_UPPER}_FNS
  };

  }  // namespace polyplug_plugin
  ```
  Note: The ABI error constants are defined in `crates/polyplug-runtime/src/abi/mod.rs`: `ABI_OK = 0`, `ABI_ERROR_GENERIC = 1`, `ABI_ERROR_NOT_FOUND = 2`, `ABI_ERROR_PANIC = 3`. **Do NOT change these values — the ABI is frozen.** In C++ generated code, include `polyplug/abi.hpp` to get these constants by name (check `host-libs/cpp/polyplug/abi.hpp` for the exact macro/constant names), OR emit the numeric values (0, 3) with comments. The catch block should return code 3 for panics.

  **File 4: `init.hpp`** — The `polyplug_init` and `polyplug_abi_version` exports:
  ```cpp
  // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
  #pragma once
  #include "vtables.hpp"
  #include "polyplug/abi.hpp"

  // Forward declaration of developer's impl factory function (developer must define this):
  // namespace polyplug_plugin { {ContractName}Plugin* create_{contract_lower}_impl(); }

  extern "C" uint32_t polyplug_abi_version() { return 1; }

  extern "C" AbiError polyplug_init(PluginRegistrar* registrar) {
      if (!registrar) return AbiError{1, StringView{nullptr, 0}};
      // Set the impl pointer (developer must have defined the factory):
      polyplug_plugin::g_{contract_lower}_impl = polyplug_plugin::create_{contract_lower}_impl();
      // Register each vtable:
      PluginDescriptor desc = {
          { (const uint8_t*)"{plugin_name}", {len} },  // name
          { (const uint8_t*)"{contract_name}", {len} }, // contract_name
          {version_major}, {version_minor}, {version_patch}
      };
      AbiError err = registrar->register_plugin(registrar, &desc, &polyplug_plugin::{CONTRACT_UPPER}_VTABLE);
      if (err.code != 0) return err;
      // repeat for each contract...
      return AbiError{0, StringView{nullptr, 0}};
  }
  ```

  **File 5: `manifest.toml`** — lang = "cpp".

  Push all 5 files to `files.files`.

  **Must NOT do**:

  - Do NOT emit `host_callers.hpp` in `generate_guest()`
  - Do NOT use C++ exceptions crossing `extern "C"` boundary — the `extern "C" AbiError polyplug_init` must be `noexcept`-safe; exceptions are caught in the ABI wrappers BEFORE returning to `extern "C"`
  - Do NOT add new workspace dependencies

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Complex multi-file C++ header generation with ABI wrapper pattern
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 10, 11, 12)
  - **Blocks**: Task 12
  - **Blocked By**: Tasks 3, 5, 8

  **References**:

  - `crates/polyplugc/src/generators/cpp/mod.rs:56-87` — current `generate_guest()` stub to replace
  - `guest-libs/cpp/polyplug/guest.hpp` — C++ guest library header — style reference and existing ABI wrapper pattern
  - `guest-libs/cpp/polyplug/abi.hpp` — C++ ABI types: exact constant names for `ABI_OK` etc.
  - `host-libs/cpp/polyplug/abi.hpp` — `PluginRegistrar`, `PluginDescriptor`, `PluginVTable`, `StringView` C++ definitions
  - `tests/fixtures/test_plugin/src/lib.rs:161-235` — conceptual reference for vtable/init pattern (Rust, but maps 1:1 to C++)
  - Task 4 inline C++ plugin source (lines 769-815 in original plan) — minimal working example of C++ plugin

  **Acceptance Criteria**:

  ```
  Scenario: generate_guest produces all 5 cpp files for test_api.toml
    Tool: Bash
    Steps:
      1. rm -rf /tmp/gen_cpp_guest && mkdir /tmp/gen_cpp_guest
      2. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/gen_cpp_guest
      3. ls /tmp/gen_cpp_guest/
    Expected Result: exit 0; files include types.hpp, contracts.hpp, vtables.hpp, init.hpp, manifest.toml
    Evidence: .sisyphus/evidence/task-9-guest-files-exist.txt

  Scenario: vtables.hpp contains try/catch ABI wrappers
    Tool: Bash
    Steps:
      1. grep -n 'try {\|catch.*std::exception\|catch.*\.\.\.\|noexcept' /tmp/gen_cpp_guest/vtables.hpp
    Expected Result: all 4 patterns found
    Evidence: .sisyphus/evidence/task-9-vtable-content-check.txt

  Scenario: vtables.hpp compiles with g++
    Tool: Bash
    Steps:
      1. echo '#include "/tmp/gen_cpp_guest/vtables.hpp"
' > /tmp/test_v.cpp && g++ -std=c++17 -I. /tmp/test_v.cpp -c -o /tmp/test_v.o
    Expected Result: exit 0
    Evidence: .sisyphus/evidence/task-9-vtables-compile.txt
  ```

  - [ ] `/tmp/gen_cpp_guest/` contains `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `manifest.toml`
  - [ ] `vtables.hpp` contains `try`, `catch`, and function pointer array
  - [ ] `cargo test -p polyplugc` passes

  **Commit**: NO (group with Wave 3)

---

- [ ] 10. Write `tests/integration_codegen_rust/mod.rs` — end-to-end Rust codegen test

  **What to do**:

  Create `tests/integration_codegen_rust/mod.rs`. This test binary:
  1. Calls `polyplugc`'s parser + Rust generator directly (as a library call, not subprocess) to produce files into a tempdir
  2. Writes a minimal `Cargo.toml` for a cdylib plugin crate into the tempdir
  3. Spawns `cargo build` to compile the generated code into a `.so`
  4. Loads the `.so` with `libloading` and calls `polyplug_init`
  5. Dispatches the `add` function and asserts the result

  **Step-by-step**:

  ```rust
  //! Integration test: full Rust codegen pipeline from test_api.toml to running plugin.
  //!
  //! AGENTS.md Rule 1: module roots use dirname/mod.rs.

  #![allow(clippy::expect_used)]

  use std::path::Path;
  use std::path::PathBuf;
  use polyplugc_test_support::generate_rust_guest;  // see note below
  // OR: call parser/generator directly (preferred — see below)
  ```

  **Preferred approach (no test-support crate needed)**: Call the library functions directly.
  `polyplugc` exposes its modules as `pub(crate)` — this won't work from an external test.
  **Solution**: Add a `[dev-dependencies]` entry in `crates/polyplugc/Cargo.toml` is unnecessary.
  Instead, declare the test binary in `crates/polyplug-runtime/Cargo.toml` under `[[test]]` and add `polyplugc` as a dev-dependency there, OR simply invoke `cargo run -p polyplugc` as a subprocess from the test.

  **Use the subprocess approach** (simpler, no visibility changes needed):
  ```rust
  use std::process::Command;

  fn run_polyplugc(args: &[&str]) -> std::process::Output {
      let output: std::process::Output = Command::new(env!("CARGO_BIN_EXE_polyplugc"))
          .args(args)
          .output()
          .expect("failed to run polyplugc");
      output
  }
  ```
  `CARGO_BIN_EXE_polyplugc` is auto-set by Cargo when the test binary is in the same workspace as the `polyplugc` bin target.

  **Test body**:
  ```rust
  #[test]
  fn test_rust_codegen_compile_and_run() {
      let out_dir: PathBuf = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("codegen_rust_test");
      std::fs::create_dir_all(&out_dir).expect("create tmpdir");

      // 1. Generate files
      let gen_output: std::process::Output = run_polyplugc(&[
          "generate",
          "--api", "tests/fixtures/test_api.toml",
          "--lang", "rust",
          "--out", out_dir.to_str().expect("utf8 path"),
      ]);
      assert!(gen_output.status.success(), "polyplugc generate failed: {:?}", gen_output);

      // 2. Verify expected files exist
      assert!(out_dir.join("types.rs").exists());
      assert!(out_dir.join("contracts.rs").exists());
      assert!(out_dir.join("vtables.rs").exists());
      assert!(out_dir.join("init.rs").exists());
      assert!(out_dir.join("host_callers.rs").exists());

      // 3. Write a minimal Cargo.toml for the plugin crate
      let manifest: &str = r#"
  [package]
  name = "generated_test_plugin"
  version = "0.1.0"
  edition = "2021"

  [lib]
  crate-type = ["cdylib"]

  [dependencies]
  polyplug-guest = { path = "../path/to/guest-libs/rust" }
  "#;
      std::fs::write(out_dir.join("Cargo.toml"), manifest).expect("write Cargo.toml");

      // 4. Write a minimal src/lib.rs that implements the generated trait and calls polyplug_init
      // The test implements TestAddPlugin with the `add` function returning a.wrapping_add(b)
      let src_dir: PathBuf = out_dir.join("src");
      std::fs::create_dir_all(&src_dir).expect("create src");
      let lib_src: String = format!("{}", GENERATED_IMPL_TEMPLATE);
      std::fs::write(src_dir.join("lib.rs"), &lib_src).expect("write lib.rs");

      // 5. Build the plugin crate
      let build_output: std::process::Output = Command::new("cargo")
          .args(["build", "--manifest-path", out_dir.join("Cargo.toml").to_str().expect("utf8")])
          .output()
          .expect("cargo build");
      assert!(build_output.status.success(), "cargo build failed: {:?}", build_output);

      // 6. Load and dispatch
      let so_path: PathBuf = out_dir.join("target/debug/libgenerated_test_plugin.so");
      // Load, call polyplug_init (using integration_dispatch pattern), call add, assert result == 8
      // ... follow the same pattern as tests/integration_dispatch/mod.rs
  }
  ```

  This test must be declared as a `[[test]]` entry in `crates/polyplug-runtime/Cargo.toml`:
  ```toml
  [[test]]
  name = "integration_codegen_rust"
  path = "tests/integration_codegen_rust/mod.rs"
  ```
  And `polyplugc` must be declared as a dev-dependency:
  ```toml
  [dev-dependencies]
  polyplugc = { path = "../polyplugc" }
  ```
  Wait — but `polyplugc` is a binary crate, not a library. The subprocess approach is better: use `CARGO_BIN_EXE_polyplugc` instead of a library import. The test will just be under `tests/integration_codegen_rust/mod.rs` as a standalone test file in the workspace.

  **Correct approach**: Create `tests/integration_codegen_rust/mod.rs` as a test binary in the `polyplug-runtime` crate. It invokes `polyplugc` via subprocess and loads the result via `libloading`, following the exact pattern in `tests/integration_dispatch/mod.rs`.

  The `Cargo.toml` for the generated plugin needs to point to the `polyplug-guest` crate at `../../guest-libs/rust` (relative to the generated output dir, which is CARGO_TARGET_TMPDIR). The test must write the correct relative path.

  GENERATED_IMPL_TEMPLATE: a string literal that produces a valid Rust plugin implementation:
  ```rust
  // include all generated files
  mod types;
  mod contracts;
  mod vtables;
  mod init;

  use contracts::TestAddPlugin;
  use polyplug_guest::PluginError;

  struct MyTestPlugin;

  impl TestAddPlugin for MyTestPlugin {
      fn add(&self, args: &AddArgs) -> Result<u32, PluginError> {
          Ok(args.a.wrapping_add(args.b))
      }
      // ... other functions return Ok(default)
  }
  ```
  (The exact trait name and method signatures come from the generated `contracts.rs` — the template must match the generated output. This means the template is only writeable after Task 7 is done. The executer of Task 10 must read the actual generated `contracts.rs` to write a correct implementation template.)

  **Must NOT do**:

  - Do NOT skip the compilation step — the test MUST actually compile the generated code
  - Do NOT hardcode absolute paths — use `CARGO_TARGET_TMPDIR` and relative paths from workspace root
  - Do NOT call `.unwrap()` in the test without `#![allow(clippy::expect_used)]`

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Integration test requiring subprocess invocation, file I/O, dynamic library loading
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 11, 12)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 2, 5, 6, 7

  **References**:

  - `tests/integration_dispatch/mod.rs:1-282` — complete reference for libloading load + vtable dispatch pattern
  - `crates/polyplug-runtime/Cargo.toml` — existing `[[test]]` entries to follow for adding `integration_codegen_rust`
  - `crates/polyplug-runtime/build.rs` — how `TEST_PLUGIN_SO` env var is set — follow same pattern for `CARGO_BIN_EXE_polyplugc`
  - `guest-libs/rust/src/lib/mod.rs` — `PluginError` shape needed for GENERATED_IMPL_TEMPLATE

  **Acceptance Criteria**:

  ```
  Scenario: integration_codegen_rust test passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug-runtime --test integration_codegen_rust -- --nocapture
    Expected Result: exit 0; output shows 'test test_rust_codegen_compile_and_run ... ok'
    Evidence: .sisyphus/evidence/task-10-integration-rust.txt

  Scenario: generated files exist after test run
    Tool: Bash
    Steps:
      1. ls $CARGO_TARGET_DIR/tmp/codegen_rust_test/
    Expected Result: types.rs, contracts.rs, vtables.rs, init.rs, host_callers.rs present
    Evidence: .sisyphus/evidence/task-10-gen-files-exist.txt
  ```

  - [ ] `cargo test -p polyplug-runtime --test integration_codegen_rust` exits 0
  - [ ] Test calls `polyplugc generate`, compiles output, loads `.so`, calls `add(3, 5)`, asserts result == 8


  **Commit**: NO (group with Wave 3)

---

- [ ] 11. Write `tests/integration_panic/mod.rs` — panic isolation test

  **What to do**:

  Create `tests/integration_panic/mod.rs`. This test verifies that the **generated ABI wrapper's `catch_unwind`** correctly catches a panic and returns `ABI_ERROR_PANIC` to the caller, WITHOUT aborting the process.

  **Critical design note**: Do NOT call a bare panicking `extern "C"` function at the host level. Calling an `extern "C"` function that panics without an internal `catch_unwind` causes process abort on stable Rust (per the Rust Reference, unwinding through `extern "C"` is defined as immediate abort). The only safe way to test panic isolation is to call a function where `catch_unwind` is INSIDE the function before the `extern "C"` boundary is crossed — i.e., the generated ABI wrappers from Task 7.

  **How this test works**:
  1. Create a minimal `tests/fixtures/test_panic_api.toml` with a single void function contract:
     ```toml
     [[contract]]
     name = "test.panic"
     version = "1.0.0"

     [[contract.functions]]
     name = "do_panic"
     ```
     (No `return` line = void return. No `params` line = no params.)
  2. Call `polyplugc generate --api tests/fixtures/test_panic_api.toml --lang rust --out <tmpdir>` via subprocess
  3. Write `<tmpdir>/Cargo.toml` + `<tmpdir>/src/lib.rs` where the `TestPanicPlugin::do_panic()` impl calls `panic!("intentional panic for test")`
  4. `cargo build` the plugin cdylib
  5. Load with `libloading`, call `do_panic` (function_id 0) through the vtable ABI wrapper
  6. Assert: `AbiError.code == ABI_ERROR_PANIC` (= **3**, the frozen ABI value in `crates/polyplug-runtime/src/abi/mod.rs:10`) and the process did NOT abort

  The generated vtable wrapper (`extern "C" fn test_panic_do_panic_abi(args: *const (), out: *mut ()) -> AbiError`) contains `catch_unwind(AssertUnwindSafe(...))` which catches the panic and returns the error code.

  **`ABI_ERROR_PANIC` constant**: `ABI_ERROR_PANIC = 3` is ALREADY defined in `crates/polyplug-runtime/src/abi/mod.rs:10`. Do NOT modify it. The test should use `polyplug_runtime::abi::ABI_ERROR_PANIC` directly; no changes to the ABI file are needed.

  Add to `crates/polyplug-runtime/Cargo.toml`:
  ```toml
  [[test]]
  name = "integration_panic"
  path = "tests/integration_panic/mod.rs"
  ```

  **Must NOT do**:

  - Do NOT call a bare `extern "C"` panicking function at the host level without `catch_unwind` INSIDE — this causes process abort, not a catchable error
  - Do NOT rely on host-side `std::panic::catch_unwind` to catch panics across an `extern "C"` boundary
  - Do NOT use `polyplug_panicking_fn` (the unprotected symbol from Task 4) directly in this test

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Requires understanding of Rust panic mechanics across FFI, subprocess + libloading, generating a minimal panic plugin
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 10, 12)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 5, 7 (Task 7 generates the catch_unwind wrapper this test exercises)

  **References**:

  - `tests/integration_dispatch/mod.rs:1-282` — libloading + vtable dispatch pattern to follow exactly
  - `crates/polyplug-runtime/src/abi/mod.rs:10` — `ABI_ERROR_PANIC = 3` already defined; use `polyplug_runtime::abi::ABI_ERROR_PANIC` in the test (do NOT change the value)
  - Task 7 plan section — the exact `catch_unwind(AssertUnwindSafe(...))` + `AbiError::panic_caught()` pattern that the generated wrapper emits
  - `tests/fixtures/test_api.toml` — structural pattern for the new `tests/fixtures/test_panic_api.toml`
  - `crates/polyplug-runtime/Cargo.toml` — add `[[test]]` entry here

  **Acceptance Criteria**:

  ```
  Scenario: panic test passes (generated wrapper returns ABI_ERROR_PANIC, no abort)
    Tool: Bash
    Steps:
      1. cargo test -p polyplug-runtime --test integration_panic -- --nocapture
    Expected Result: exit 0; output shows 'test test_panic_returns_abi_error_panic ... ok'
    Evidence: .sisyphus/evidence/task-11-panic-test.txt

  Scenario: ABI_ERROR_PANIC constant is defined
    Tool: Bash
    Steps:
      1. grep -n 'ABI_ERROR_PANIC' crates/polyplug-runtime/src/abi/mod.rs
    Expected Result: shows the constant with value 3 (`ABI_ERROR_PANIC = 3`) — already defined in the repo, do NOT modify it
    Evidence: .sisyphus/evidence/task-11-panic-constant.txt
  ```

  - [ ] `cargo test -p polyplug-runtime --test integration_panic` exits 0
  - [ ] Test calls through the generated catch_unwind wrapper and asserts `AbiError.code == ABI_ERROR_PANIC`
  - [ ] No process abort

  **Commit**: NO (group with Wave 3)

---

- [ ] 12. Write `tests/integration_codegen_cpp/mod.rs` — end-to-end C++ codegen test

  **What to do**:

  Create `tests/integration_codegen_cpp/mod.rs`. This test verifies C++ codegen produces compilable output, and that the pre-compiled C++ test plugin (`libtest_plugin_cpp.so`, built in Task 4) dispatches correctly.

  **The test has two parts**:

  **Part A — Codegen output check** (always runs):
  - Invoke `polyplugc generate --api tests/fixtures/test_api.toml --lang cpp --out <tmpdir>` via subprocess
  - Assert exit 0 and that expected files exist (`types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `host_callers.hpp`, `manifest.toml`)
  - Attempt to compile the headers with `g++ -std=c++17 -I. types.hpp -c` and assert exit 0

  **Part B — Runtime dispatch** (skips if `TEST_PLUGIN_CPP_SO` is empty):
  ```rust
  const TEST_PLUGIN_CPP_SO: &str = env!("TEST_PLUGIN_CPP_SO");

  #[test]
  fn test_cpp_plugin_dispatch() {
      if TEST_PLUGIN_CPP_SO.is_empty() {
          eprintln!("skipping cpp dispatch test: g++ not available");
          return;
      }

      // SAFETY: TEST_PLUGIN_CPP_SO is a compiled cdylib.
      let library: libloading::Library = unsafe {
          libloading::Library::new(TEST_PLUGIN_CPP_SO).expect("load cpp test plugin")
      };

      // ... same pattern as integration_dispatch/mod.rs:
      // load polyplug_init, call it with a registrar, dispatch add function
      // the C++ plugin was built to implement test.add contract (contract_id = 0xCC4232FAB0410D2B)
      // AddArgs { a: 10, b: 20 } should return 30

      std::mem::forget(library);
  }
  ```

  The `AddArgs` struct used in this test must be `#[repr(C)]` with fields `a: u32, b: u32` — matches the C++ plugin's layout.

  Add to `crates/polyplug-runtime/Cargo.toml`:
  ```toml
  [[test]]
  name = "integration_codegen_cpp"
  path = "tests/integration_codegen_cpp/mod.rs"
  ```

  **Must NOT do**:

  - Do NOT fail the test if `g++` is unavailable — skip gracefully via the `TEST_PLUGIN_CPP_SO.is_empty()` check
  - Do NOT hardcode the `.so` path — use `TEST_PLUGIN_CPP_SO` env var

  **Recommended Agent Profile**:

  - **Category**: `unspecified-high`
    - Reason: Integration test combining subprocess invocation, C++ compilation check, and libloading dispatch
  - **Skills**: none

  **Parallelization**:

  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 3 (with Tasks 9, 10, 11)
  - **Blocks**: F1-F4
  - **Blocked By**: Tasks 3, 4, 5, 8, 9

  **References**:

  - `tests/integration_dispatch/mod.rs:1-282` — libloading + registry + dispatch pattern to copy
  - `crates/polyplug-runtime/build.rs` — how `TEST_PLUGIN_CPP_SO` env var is emitted (set in Task 4)
  - Task 4 inline C++ plugin source — the plugin implements `test.add` with `contract_id = 0xCC4232FAB0410D2B`, function 0 is `cpp_test_add(AddArgs) -> u32`
  - `crates/polyplug-runtime/Cargo.toml` — add `[[test]]` here

  **Acceptance Criteria**:

  ```
  Scenario: C++ codegen output check passes
    Tool: Bash
    Steps:
      1. cargo test -p polyplug-runtime --test integration_codegen_cpp -- --nocapture
    Expected Result: exit 0; 'test test_cpp_codegen_files_exist ... ok' printed
    Evidence: .sisyphus/evidence/task-12-cpp-codegen-test.txt

  Scenario: C++ dispatch test runs (or skips gracefully)
    Tool: Bash
    Steps:
      1. cargo test -p polyplug-runtime --test integration_codegen_cpp -- test_cpp_plugin_dispatch --nocapture
    Expected Result: exit 0; test either passes or prints 'skipping cpp dispatch test: g++ not available'
    Evidence: .sisyphus/evidence/task-12-cpp-dispatch-test.txt
  ```

  - [ ] `cargo test -p polyplug-runtime --test integration_codegen_cpp` exits 0
  - [ ] Test asserts generated file names exist
  - [ ] Dispatch test either passes or skips gracefully when `g++` is absent

  **Commit**: YES (Wave 3 group commit)
  - Message: `feat(codegen): implement C++ guest generator and integration tests`
  - Files: `crates/polyplugc/src/generators/cpp/mod.rs`, `tests/integration_codegen_rust/mod.rs`, `tests/integration_panic/mod.rs`, `tests/integration_codegen_cpp/mod.rs`, `crates/polyplug-runtime/Cargo.toml`
  - Pre-commit: `cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Final Verification Wave

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`
  Read the plan end-to-end. For each "Must Have": verify implementation exists (read file, run command). For each "Must NOT Have": search codebase for forbidden patterns — reject with file:line if found. Check evidence files exist. Compare deliverables against plan.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [N/N] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`
  Run `cargo clippy --workspace -- -D warnings` + `cargo fmt --check`. Review all changed files for: `.unwrap()`, `.expect()` in production code, `use` inside functions, missing type annotations, `#[no_mangle]` (old form), `unsafe` blocks without `SAFETY:` comment. Check generated code for auto-generated header comment.
  Output: `Build [PASS/FAIL] | Lint [PASS/FAIL] | Format [PASS/FAIL] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real QA** — `unspecified-high`
  Run ALL integration tests from scratch. Execute every QA scenario from every task. Verify: `cargo test --workspace` passes, `polyplugc generate` produces files in correct format, generated Rust compiles, generated C++ compiles, panic isolation test passes.
  Output: `Tests [N/N pass] | Integration [N/N] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`
  For each task: read "What to do", read actual changes. Verify 1:1 — everything in spec was built (no missing), nothing beyond spec was built (no creep). Check "Must NOT do" compliance. Flag any changes to frozen ABI files.
  Output: `Tasks [N/N compliant] | Forbidden changes [CLEAN/N issues] | VERDICT`

---

## Commit Strategy

- Wave 1: `fix(fixtures): repair broken TOML schemas and add PluginError + PolyplugException` — wave1 tasks
- Wave 2: `feat(codegen): implement Rust and C++ host callers with full ABI dispatch` — tasks 5-8
- Wave 3: `feat(codegen): implement guest generators and end-to-end integration tests` — tasks 9-12

---

## Success Criteria

### Verification Commands
```bash
# 1. All tests pass
cargo test --workspace
# Expected: all tests pass, 0 failures

# 2. Lint clean
cargo clippy --workspace -- -D warnings
# Expected: exit code 0, no warnings

# 3. Format clean
cargo fmt --check
# Expected: exit code 0

# 4. Validate works
cargo run -p polyplugc -- validate --api tests/fixtures/test_api.toml
# Expected: "OK: tests/fixtures/test_api.toml"

cargo run -p polyplugc -- validate --bundle tests/fixtures/test_bundle.toml
# Expected: "OK: tests/fixtures/test_bundle.toml"

# 5. Generate works
mkdir -p /tmp/polyplug_gen
cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/polyplug_gen
ls /tmp/polyplug_gen/
# Expected: types.rs, contracts.rs, vtables.rs, init.rs, host_callers.rs, manifest.toml

cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/polyplug_gen
ls /tmp/polyplug_gen/
# Expected: types.hpp, contracts.hpp, vtables.hpp, init.hpp, host_callers.hpp, manifest.toml

# 6. No .unwrap() in production code
grep -rn "\.unwrap()" crates/polyplugc/src/ --include="*.rs"
# Expected: zero results

grep -rn "\.unwrap()" guest-libs/rust/src/ --include="*.rs"
# Expected: zero results
```

### Final Checklist
- [ ] All "Must Have" items present in code
- [ ] All "Must NOT Have" items absent
- [ ] All tests pass
- [ ] Generate command produces the expected set of files for both `--lang rust` and `--lang cpp`
