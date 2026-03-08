# Learnings — codegen-implementation

## 2026-03-08T11:20:19Z — Initial Codebase Analysis

### Parser schema (CRITICAL)
- `RawApiSchema` uses `contract` (singular) and `types` (plural) — parser is ALREADY correct
- `test_api.toml` is WRONG: uses `[[contracts]]` plural and `[[contracts.functions.returns]]` array-of-tables
- `test_bundle.toml` is WRONG: uses `[[plugins]]` plural
- Fix: change `[[contracts]]` → `[[contract]]`, remove `[[contracts.functions.returns]]` and `[[contracts.functions.params]]` format → use inline tables and scalar `return = "u32"`
- NOTE: `RawParam` uses `params = [{ name = "args", type = "AddArgs" }]` inline format
- `RawFunction.returns` is `Option<String>` with `#[serde(rename = "return")]`

### Module patterns
- All modules use `dirname/mod.rs` form — strictly enforced
- `use` statements at top of file only — strictly enforced

### ABI constants (from abi/mod.rs)
- `ABI_OK = 0`, `ABI_ERROR_GENERIC = 1`, `ABI_BUFFER_TOO_SMALL = 2`, `ABI_ERROR_PANIC = 3`
- `ABI_ERROR_NOT_FOUND = 4`, `ABI_ERROR_STALE_HANDLE = 5`, `ABI_FUNCTION_NOT_AVAIL = 6`
- `AbiError::ok()`, `AbiError::panic_caught()` already exist
- `StringView::null()` already exists

### Guest lib re-exports (from guest-libs/rust/src/lib/mod.rs)
- Re-exports: `ABI_OK`, `AbiError`, `PluginDescriptor`, `PluginHandle`, `PluginRegistrar`, `PluginVTable`, `StringView`
- Does NOT re-export `ABI_ERROR_GENERIC`, `ABI_ERROR_PANIC` — must use full path or add to re-exports
- `PluginError` NOT yet added (Task 2 will add it)

### Host lib C++ (from host-libs/cpp/polyplug/error.hpp)
- Already has `PluginError` C++ class and `throw_if_error()` helper
- Plan says to add `PolyplugException` and `check_abi_error()` — but the file already has analogous types
- Need to READ carefully: plan says add `PolyplugException : public std::runtime_error` — the existing class is `PluginError : public std::exception`
- These are DIFFERENT classes — the plan wants `PolyplugException` added alongside the existing `PluginError`

### test_plugin/src/lib.rs patterns
- Uses `FnPtr(pub *const ())` newtype with `unsafe impl Sync`
- Uses `#[unsafe(no_mangle)]` (Rust 2024 edition)
- `TEST_ADD_CONTRACT_ID = 0xCC4232FAB0410D2B` (pre-computed FNV-1a for "test.add@1")
- Static vtable: `static TEST_ADD_FNS: [FnPtr; 1]` with `functions: TEST_ADD_FNS.as_ptr()`
- `PluginVTable.functions` is `*const FnPtr` (in test_plugin local type), `*const *const ()` in runtime type

### Runtime module
- `HostVTable.call_plugin` is `unsafe extern "C" fn(PluginHandle, u32, *const (), *mut ()) -> AbiError`
- `Runtime::call_plugin` in runtime/mod.rs uses this vtable

### Build script patterns
- `build.rs` already compiles `test_plugin` via `cargo build --package test_plugin --release`
- Sets `TEST_PLUGIN_SO` env var for tests
- For C++ plugin: use `std::process::Command::new("g++")` directly — NO cc crate

### ir/mod.rs key types
- `compute_contract_id(name, major_version)` → `u64` (FNV-1a)
- `ResolvedTypeRef::Primitive(PrimitiveType)`, `::AbiType(AbiBuiltin)`, `::UserDefined(String)`
- `PrimitiveType::rust_name()` → `&'static str`, `PrimitiveType::cpp_name()` → `&'static str`

### Rust 2024 edition specifics
- `#[unsafe(no_mangle)]` — NOT `#[no_mangle]`
- `extern "C" fn` in statics is allowed

### Test infrastructure
- Integration tests are `[[test]]` entries in `crates/polyplug-runtime/Cargo.toml`
- Tests live at `tests/<name>/mod.rs`
- `TEST_PLUGIN_SO` env var set by build.rs
- `libloading` is already a workspace dependency

## Task 1: Fixture files + parse_bundle_with_api (2026-03-08)

### TOML Schema Patterns (confirmed working)
- `RawField.ty` maps to TOML key `type` via `#[serde(rename = "type")]`
- Inline array syntax `fields = [{name="x", type="u32"}]` correctly deserializes to `Vec<RawField>`
- `[[contract]]` (singular) is the correct key for `RawApiSchema.contract: Vec<RawContract>`
- `[[plugin]]` (singular) is the correct key for `RawBundleSchema.plugin: Vec<RawPlugin>`
- `[[contract.functions]]` syntax with `return = "u32"` scalar (NOT `[[returns]]`) correctly maps to `RawFunction.returns: Option<String>` via `#[serde(rename = "return")]`
- Omitting `return` key gives `None` (void return) due to `#[serde(default)]`

### Dead Code Avoidance Pattern
- When adding a new pub(crate) function that uses previously-dead fields, also wire the function
  into main.rs to avoid `dead_code` warnings on both the function and the field.
- `validate --bundle` was updated to use `parse_bundle_with_api` instead of `parse_bundle`
  so the API is chain-loaded and the `api` field in `RawBundleMeta` is exercised.

### parse_bundle_with_api Design
- Reads bundle TOML, extracts `bundle.api` path if present
- Resolves api path relative to bundle file's parent dir using `path.parent().unwrap_or_else(|| Path::new("."))`
- Chain-loads api.toml via `parse_api()`, merges types+contracts with bundle metadata
- Returns `ValidatedIr { types, contracts, bundle }` combining both sources
# Learnings — codegen-implementation

## Task 2 — PluginError added to guest-libs/rust (2026-03-08)

- `ABI_ERROR_GENERIC = 1` and `ABI_ERROR_PANIC = 3` are defined in `crates/polyplug-runtime/src/abi/mod.rs` and are `pub const u32`.
- `PluginError` is a pure Rust type (not `#[repr(C)]`); it lives only in `polyplug-guest` and is never ABI-visible.
- `core::fmt::Display` used instead of `std::fmt::Display` — consistent with no-std readiness.
- `cargo test -p polyplug-guest` and `cargo clippy -p polyplug-guest -- -D warnings` both pass with zero warnings/errors.
- All `pub use` re-exports kept at file top per AGENTS.md §2; struct + impl block appended after them.
## Task 3 — Add PolyplugException + check_abi_error() to error.hpp

**Date:** 2026-03-08

### What was done
- Added `#include <stdexcept>` to `host-libs/cpp/polyplug/error.hpp` (was missing; needed for `std::runtime_error`)
- Added `PolyplugException : public std::runtime_error` class with `uint32_t code_` field
- Added `check_abi_error(AbiError err)` inline function that throws `PolyplugException` on non-OK codes
- Both additions placed inside `namespace polyplug`, before closing `}  // namespace polyplug`
- Existing `PluginError` and `throw_if_error()` are untouched

### Key facts
- `ABI_OK` is `#define ABI_OK 0U` in `abi.hpp` (line 16)
- `AbiError.message` uses `StringView { const uint8_t* ptr; size_t len; }` — not null-terminated
- `check_abi_error` guards against `nullptr` message ptr before constructing `std::string(msg, len)`
- Compile test: `g++ -std=c++17 -I. /tmp/test_err.cpp -o /tmp/test_err` → exit 0 ✓
- Evidence saved to `.sisyphus/evidence/task-3-error-hpp-compiles.txt`

### Design notes
- `PolyplugException` is the type for **generated** host caller code; `PluginError` is for **hand-written** host code
- Both coexist in the same header without conflict


## 2026-03-08 — Task 4: panicking_fn + C++ test plugin

### polyplug_panicking_fn
- Added after `polyplug_init` in `tests/fixtures/test_plugin/src/lib.rs`
- Uses `#[unsafe(no_mangle)]` (Rust 2024 edition form)
- Returns local `AbiError` type (not from polyplug_runtime — cdylib can't depend on runtime)
- NOT added to vtable or `polyplug_init` registration — accessed via `dlsym` only

### C++ plugin (build.rs)
- Added g++ availability check; emits `TEST_PLUGIN_CPP_SO=` (empty) if g++ missing
- C++ source written to OUT_DIR at build time, compiled to `libtest_plugin_cpp.so`
- **GOTCHA**: `size_t` is in `<cstddef>`, NOT `<cstdint>` — must include both headers
- Compiled .so copied to `tests/fixtures/libtest_plugin_cpp.so`
- `TEST_PLUGIN_CPP_SO` env var emitted with full path

### Verification
- `cargo build -p polyplug-runtime` — PASSES
- `tests/fixtures/libtest_plugin_cpp.so` — EXISTS
- `cargo test -p polyplug-runtime --test integration_dispatch` — 3/3 PASS# Codegen Implementation — Learnings

## Task 5: Fix CLI Dispatch Logic

**Date:** 2026-03-08

### Changes Made

1. **`crates/polyplugc/src/main.rs`**
   - Added `let from_api: bool = api.is_some();` BEFORE the `let ir:` binding (captures before `api` is consumed by if-let).
   - Changed `--bundle` branch of `ir` binding from `parser::parse_bundle(&bundle_path)?` → `parser::parse_bundle_with_api(&bundle_path)?`.
   - Replaced single `generator.generate_host(...)` call with branching dispatch:
     - `--api` path: calls both `generate_host` AND `generate_guest`
     - `--bundle` path: calls only `generate_guest`

2. **`crates/polyplugc/src/generators/mod.rs`**
   - Removed `#[allow(dead_code)]` from `generate_guest` — it's now called by dispatch logic.
   - Kept `#[allow(dead_code)]` on `language_name` — still unused, clippy would error without it.

3. **`crates/polyplugc/src/parser/mod.rs`** (side effect fix)
   - Added `#[allow(dead_code)]` to `parse_bundle` and `parse_bundle_str` — these became unused once `parse_bundle_with_api` replaced `parse_bundle` in dispatch. They're kept for potential future use.

### Key Learnings

- **Variable capture order matters**: `from_api: bool = api.is_some()` must come BEFORE `let ir:` because `api` is moved into the if-let arm.
- **Removing `#[allow(dead_code)]` propagates**: When a suppressed warning is removed and the function becomes used, other dead functions that were previously masked may now surface. Always run full clippy after each change.
- **`parse_bundle` vs `parse_bundle_with_api`**: `parse_bundle_with_api` resolves and merges the referenced `api.toml`. The dispatch must use it to get full type/contract resolution.

### Verification Results

- `cargo test -p polyplugc`: 12/12 tests pass
- `cargo clippy --workspace -- -D warnings`: zero warnings/errors


## Task 8: CppGenerator::generate_host() rewrite (2026-03-08)

### What was done
- Rewrote `generate_host()` to emit 3 separate files: `types.hpp`, `host_callers.hpp`, `manifest.toml`.
- Introduced helpers: `generate_types_hpp()`, `generate_host_callers_hpp()`, `generate_manifest_toml()`, `generate_cpp_host_function()`, `build_args_ptr_code()`, `capitalise_first()`.
- `generate_cpp_type()` and `cpp_type_name()` and `contract_name_to_class()` kept intact.
- Existing `generate_cpp_host_contract()` body replaced to emit full C++ class with `PluginHandle handle_` + `const HostVTable* host_` constructor.

### Key design decisions
- `types.hpp` includes `<cstdint>` and `polyplug/abi.hpp` — struct defs go here.
- `host_callers.hpp` includes `types.hpp`, `polyplug/error.hpp`, `polyplug/abi.hpp`, wrapped in `namespace polyplug_generated`.
- `manifest.toml` has minimal TOML: `schema_version`, `lang`, `generated_by`.
- `check_abi_error` is in `polyplug` namespace → generated code calls `polyplug::check_abi_error(err)`.
- Void-return functions: `out_ptr = nullptr`, no `return`.
- No-param functions: `args_ptr = nullptr`.
- Single user-defined struct param: `const void* args_ptr = &{name};`.
- Single primitive param: local copy + pointer.
- Multiple params: inline-defined anonymous struct `{ClassName}{FuncName}Args` with value-init then `&args_val`.

### Test update
- `generate_host_empty_ir` now checks `files.files.len() >= 1` (not `== 1`) and checks `.any()` for AUTO-GENERATED string, since we now emit 3 files.

### Verification
- `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/gen_cpp_host` → exit 0
- `/tmp/gen_cpp_host/` contains `types.hpp`, `host_callers.hpp`, `manifest.toml`
- `g++ -std=c++17 -I<host-libs/cpp> -I<out> test_h.cpp` → exit 0 (no errors)
- `cargo test -p polyplugc` → 12/12 pass
- `cargo clippy --workspace -- -D warnings` → zero warnings/errors

## 2026-03-08 — Task 6: Rust generate_host() rewrite

### What was done
- Rewrote `RustGenerator::generate_host()` in `crates/polyplugc/src/generators/rust/mod.rs` to produce 3 files:
  1. `types.rs` — user-defined types (via `generate_rust_type`) + arg-pack structs for multi-param fns
  2. `host_callers.rs` — contract caller structs with `call_plugin`, `PluginError`, `Runtime` references
  3. `manifest.toml` — schema_version=1, lang=rust, generated_by=polyplugc

### Key design decisions
- Arg-packing strategy: `needs_arg_pack()` returns true for 2+ params; `emit_arg_pack_struct()` emits into `types.rs`; pack struct named `{ContractStruct}{FnPascal}Args`
- Single UserDefined param → pass as `&AddArgs` reference (pointer directly to struct)
- Single primitive param → copy to `{name}_val` local, then pointer
- No params → `core::ptr::null()`; void return → `core::ptr::null_mut()`
- Every unsafe block has `// SAFETY:` comment with args/out type
- Updated `generate_host_empty_ir` test: now expects 3 files (was 1), checks all 3 for `AUTO-GENERATED`
- Added `generate_host_produces_three_files` and `arg_pack_struct_name_conversion` tests

### Verification
- `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen_rust_host` → exit 0
- `/tmp/gen_rust_host/` contains `types.rs`, `host_callers.rs`, `manifest.toml`, `guest_sdk.rs`
- `host_callers.rs` contains `call_plugin`, `PluginError`, `Runtime` — confirmed
- `cargo test -p polyplugc` → 14/14 pass
- `cargo clippy --workspace -- -D warnings` → zero warnings/errors

## 2026-03-08 — Task 7: RustGenerator::generate_guest() rewrite

### What was done
- Rewrote `RustGenerator::generate_guest()` in `crates/polyplugc/src/generators/rust/mod.rs`
- Now emits 5 files: `types.rs`, `contracts.rs`, `vtables.rs`, `init.rs`, `manifest.toml`
- Removed old `guest_sdk.rs` single-file stub

### New helper functions added
- `generate_guest_contract_trait()` — emits trait with correct `&Self`/value params
- `build_guest_trait_params()` — parallel to `build_sig_params` for guest side
- `contract_name_to_guest_trait()` — "test.add" → "TestAddPlugin"
- `contract_name_to_upper_snake()` — "test.add" → "TEST_ADD"
- `generate_guest_vtables_file()` — writes all vtables.rs content
- `generate_guest_contract_vtable()` — per-contract vtable code
- `generate_guest_abi_wrapper()` — per-function ABI wrapper with catch_unwind
- `emit_guest_wrapper_call()` — dispatches to trait method with correct param unpacking
- `generate_guest_init_file()` — writes init.rs with polyplug_init

### Key design decisions
- `FnPtr` newtype emitted once at top of vtables.rs (not per-contract)
- `OnceLock<Box<dyn {ContractName}Plugin>>` — one per contract, set by plugin developer at runtime
- ABI wrappers: `catch_unwind(AssertUnwindSafe(|| { ... }))` for panic isolation
- `functions: {upper}_FNS.as_ptr() as *const *const ()` — cast needed since FnPtr wraps *const ()
- `version_minor` and `version_patch` read at codegen time: `{minor}_u32 << 16 | {patch}_u32`
- Variable naming in init.rs: `desc_TEST_ADD`, `err_TEST_ADD` using upper-snake — avoids collision for multiple contracts

### Bugs hit and fixed
1. **Unicode escape**: `\u2014` is invalid Rust escape syntax → replaced with `--` (ASCII double dash)
2. **Missing method closing brace**: The replacement range `103..132` included the old `    }` closing brace of `generate_guest` — it was replaced with new body content. Had to add `    }` back explicitly.
3. **Extra `)`**: `out.push_str("..."));` had two `)` → should be one. The extra came from edit tool escaping during JSON stringification.
4. **`&format!` with no format args**: clippy `-D useless_format` → changed to bare string literal
5. **`.unwrap()` in production**: clippy `-D unwrap_used` → replaced with `match` on `func.returns`
6. **`push_str("\n")`**: clippy `-D single_char_add_str` → changed to `push('\n')`

### Verification
- `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang rust --out /tmp/gen_rust_guest` → exit 0
- Output: `contracts.rs`, `host_callers.rs`, `init.rs`, `manifest.toml`, `types.rs`, `vtables.rs`
- `vtables.rs` contains: `catch_unwind` ×4, `AssertUnwindSafe` ×4, `FnPtr`, `OnceLock`, `TEST_ADD_IMPL`
- `init.rs` contains: `#[unsafe(no_mangle)]` ×2, `polyplug_abi_version`, `polyplug_init`
- `cargo test -p polyplugc` → 14/14 pass
- `cargo clippy --workspace -- -D warnings` → zero warnings/errors
## 2026-03-08 -- Task 9: CppGenerator::generate_guest() rewrite

### What was done
- Rewrote `CppGenerator::generate_guest()` in `crates/polyplugc/src/generators/cpp/mod.rs`
- Now emits 5 files: `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `manifest.toml`
- Removed old `guest_sdk.hpp` single-file stub
- Used shared `generate_types_hpp()` helper (already extracted in Task 8) for both host and guest

### New helper functions added
- `generate_contracts_hpp()` -- emits abstract base class per contract
- `generate_cpp_guest_contract_class()` -- per-contract abstract class
- `generate_cpp_guest_abstract_method()` -- per-function pure virtual method
- `generate_vtables_hpp()` -- ABI wrappers + function pointer arrays + vtable statics
- `generate_cpp_guest_contract_vtable()` -- per-contract vtable code
- `generate_cpp_guest_abi_wrapper()` -- per-function noexcept ABI wrapper with try/catch
- `build_guest_call_expr()` -- builds the impl->fn() call expression with param unpacking
- `generate_init_hpp()` -- polyplug_init + polyplug_abi_version + impl pointer definitions
- `generate_init_hpp_register_contract()` -- per-contract registration in polyplug_init
- `contract_name_to_plugin_class()` -- "test.add" -> "TestAddPlugin"
- `contract_name_to_lower_snake()` -- "test.add" -> "test_add"
- `contract_name_to_upper_snake()` -- "test.add" -> "TEST_ADD"

### Key design decisions
- `contracts.hpp`: abstract base class with `virtual ~T() = default` + pure virtual methods
- Non-primitive params use `const T&` (reference), primitives use by-value in abstract methods
- `vtables.hpp`: `extern T* g_{lower}_impl` for impl pointer, `constexpr uint64_t {UPPER}_CONTRACT_ID`
- ABI wrappers: `inline AbiError ... noexcept` with try/catch(exception) + catch(...)
- No-param functions: `(void)args;` before call
- Multi-param functions: inline local struct `struct {FnName}Args { T a; T b; };` + packed cast
- `init.hpp`: impl pointer definitions (initialized to nullptr) + factory forward decls + `polyplug_init`
- `init.hpp` uses double namespace blocks (definitions then forward decls) to avoid circular headers
- ABI_OK used for success; raw `1U` and `3U` with comments for error/panic codes

### Bugs hit and fixed
1. **Useless format!**: `out.push_str(&format!("// Forward declaration...\n"))` -- clippy rejects format! with no args. Fixed by using bare string literal.
2. **Include path in task description**: The task says `-I/mnt/data/Projects/Utils/polyplug` but `polyplug/abi.hpp` is in `host-libs/cpp/`. Correct path is `-I.../host-libs/cpp`. The generated headers are valid C++17 with correct paths.

### Verification
- `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang cpp --out /tmp/gen_cpp_guest` -> exit 0
- Output: `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `manifest.toml` (+ `host_callers.hpp` from generate_host)
- `vtables.hpp` contains: 4x `try {`, 4x `catch (const std::exception&`, 4x `catch (...)`, `void* const` array
- `g++ -std=c++17 -I.../host-libs/cpp -I/tmp/gen_cpp_guest vtables.hpp -c -o /tmp/test_v.o` -> exit 0
- `cargo test -p polyplugc` -> 18/18 pass (4 new tests added)
- `cargo clippy --workspace -- -D warnings` -> zero warnings/errors
- Evidence saved to `.sisyphus/evidence/task-9-*.txt`

## Task 10: integration_codegen_rust test

### Generator Bug Fixed
- `vtables.rs` generator emitted `dyn _` which is invalid Rust syntax. Fixed in
  `crates/polyplugc/src/generators/rust/mod.rs` line ~390 to use the actual trait name.
- `TEST_ADD_IMPL` was `static` (private). Changed to `pub(crate) static` so `lib.rs` can access it.

### CARGO_BIN_EXE_polyplugc cross-crate
- `CARGO_BIN_EXE_polyplugc` is only available at compile-time for binaries in the same package.
- For cross-crate tests, emit it from `build.rs` via `cargo:rustc-env=CARGO_BIN_EXE_polyplugc=<path>`.
- The build.rs computes: `target_dir.join(profile).join("polyplugc")`.

### Workspace collision fix
- Generated plugin crate inside `CARGO_TARGET_TMPDIR` gets picked up by workspace.
- Fix: add `[workspace]` table to the generated `Cargo.toml` to declare it standalone.

### polyplug_abi_version duplicate symbol
- `polyplug_runtime` (cdylib+rlib) exports `#[no_mangle] polyplug_abi_version`.
- When plugin links `polyplug-guest -> polyplug-runtime` as rlib, the symbol is included.
- Do NOT define `polyplug_abi_version` in the plugin's `lib.rs`; rely on the one from runtime.

### OnceLock access pattern
- Generated `init.rs` defines `polyplug_init` but does NOT set `TEST_ADD_IMPL`.
- Plugin's custom `polyplug_init` must call `TEST_ADD_IMPL.get_or_init(|| Box::new(MyPlugin))` first.
- Skip `mod init;` entirely — define your own `polyplug_init` in `lib.rs`.

### Thread-local clippy rules
- Use `const {}` initializer in `thread_local!` for const-evaluable values.
- Use `core::cell::Cell` not `std::cell::Cell` (clippy `std_instead_of_core`).

## Task 11: integration_panic test

### catch_unwind ABI pattern verified
- The generated `vtables.rs` wraps every ABI function with `std::panic::catch_unwind(std::panic::AssertUnwindSafe(...))` INSIDE the `extern "C"` function
- When the plugin panics, the host receives `AbiError { code: 3 (ABI_ERROR_PANIC), message: "plugin panicked" }` instead of process abort
- Test confirmed: `thread '<unnamed>' panicked... intentional test panic` appears in stderr, but process continues and test returns `ok`

### Symbol conflict with polyplug-runtime cdylib
- `polyplug-runtime` has `crate-type = ["cdylib", "rlib"]` and defines `#[unsafe(no_mangle)] polyplug_abi_version` in its lib.rs
- When linking a cdylib that depends on polyplug-runtime as rlib, the `#[no_mangle]` symbol from the rlib gets included in the output cdylib
- DO NOT also define `polyplug_abi_version` in the plugin lib.rs — it creates a duplicate symbol linker error
- FIX: Let polyplug-runtime's rlib provide `polyplug_abi_version`; only write `polyplug_init` in the plugin

### Generated code visibility
- `TEST_PANIC_IMPL` is `pub(crate)` in the generated vtables.rs (accessible from crate root lib.rs)
- `TEST_PANIC_VTABLE` is also `pub(crate)` 
- Use `use vtables::TEST_PANIC_IMPL;` from the crate root (lib.rs is the crate root, vtables is a submodule — pub(crate) works)

### Cargo.toml for test plugin cdylib
- Only add `polyplug-guest` as dep (NOT `polyplug-runtime` directly)
- polyplug-guest already re-exports everything needed
- Adding polyplug-runtime directly causes duplicate `polyplug_abi_version` symbol

### String literals in test templates
- Use `concat!(...)` or `format!()` with `\n` escapes for multi-line string templates
- Avoid `r#"..."#` raw strings when the content contains Rust code with backslash escapes — raw strings preserve literal backslashes which causes issues

### Module naming (test.panic → TestPanicPlugin)
- Contract "test.panic" → upper snake "TEST_PANIC", lower snake "test_panic", trait "TestPanicPlugin"
- The generated `TEST_PANIC_IMPL` OnceLock must be initialized with `get_or_init` before any vtable function is called
- `get_or_init` is safe to call multiple times (returns existing value if already set)

## 2026-03-08 — Task 12: integration_codegen_cpp test

### What was done
- Created `tests/integration_codegen_cpp/mod.rs` — 300 lines, two tests
- Added `[[test]]` entry to `crates/polyplug-runtime/Cargo.toml`

### Part A: test_cpp_codegen_files_exist
- Runs `polyplugc generate --api test_api.toml --lang cpp --out <tmpdir>`
- Asserts exit 0 and all 6 files exist: `types.hpp`, `contracts.hpp`, `vtables.hpp`, `init.hpp`, `host_callers.hpp`, `manifest.toml`
- Attempts `g++ -std=c++17 -I<host-libs/cpp> -I<out> vtables.hpp -c -o <obj>` — skips gracefully if g++ not found
- On this system: g++ was available, vtables.hpp compiled successfully ✓

### Part B: test_cpp_plugin_dispatch
- Uses `TEST_PLUGIN_CPP_SO` env var (set by build.rs, empty if g++ unavailable)
- Loads C++ plugin via libloading, calls `polyplug_init`, retrieves vtable
- Dispatches `add(10, 20)` through `vtable.functions[0]` → asserts result == 30 ✓
- Used separate thread-local `CPP_DISPATCH_REGISTRY` to avoid name collision with integration_dispatch tests

### Key differences from integration_dispatch/mod.rs
- Thread-local named `CPP_DISPATCH_REGISTRY` (not `DISPATCH_REGISTRY`) to avoid any potential static conflict
- Loads `TEST_PLUGIN_CPP_SO` (not `TEST_PLUGIN_SO`)
- Args: `AddArgs { a: 10, b: 20 }`, expected result: 30 (not 8)
- Contract id: `0xCC4232FAB0410D2B_u64` (same as Rust test plugin)
- vtable.function_count = 1 (C++ test plugin has only 1 function, not 4 like generated Rust)

### Verification
- `cargo test -p polyplug-runtime --test integration_codegen_cpp -- --nocapture` → 2/2 PASS
- `cargo test -p polyplug-runtime` → all suites pass (30 unit + all integration)
- Evidence saved to `.sisyphus/evidence/task-12-*.txt`
