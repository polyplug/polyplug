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
- `cargo test -p polyplug-runtime --test integration_dispatch` — 3/3 PASS