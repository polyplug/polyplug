# Coding Conventions

**Analysis Date:** 2026-04-02

## Naming Patterns

**Files:**
- Source files: `snake_case.rs` (e.g., `runtime.rs`, `plugin_registry.rs`)
- Test files: `snake_case.rs` matching test purpose (e.g., `integration_load.rs`, `stress_error.rs`)
- Benchmark files: `snake_case.rs` (e.g., `vtable_dispatch.rs`)
- Module directories: `snake_case` (e.g., `loader/`, `registry/`)

**Functions:**
- Public functions: `snake_case` (e.g., `find_by_contract`, `load_bundle`)
- FFI exports: `polyplug_` prefix (e.g., `polyplug_runtime_create`, `polyplug_init`)
- Callbacks: descriptive `snake_case` with purpose (e.g., `capture_register_callback`, `bench_find_by_contract`)
- Test functions: `snake_case` with test prefix or descriptive name (e.g., `test_load_and_abi_version`, `stress_error_code_and_message_received_correctly`)

**Variables:**
- Local variables: `snake_case` (e.g., `bundle_id`, `contract_id`, `registry`)
- Thread-local statics: `SCREAMING_SNAKE_CASE` (e.g., `CAPTURED_VTABLE_PTR`, `BENCH_REGISTRY`, `ERROR_REGISTRY`)
- Constants: `SCREAMING_SNAKE_CASE` (e.g., `TEST_PLUGIN_SO`, `POLYPLUG_ABI_VERSION`, `ABI_OK`)

**Types:**
- Structs: `PascalCase` (e.g., `Runtime`, `PluginRegistry`, `HostContext`)
- Enums: `PascalCase` with `PascalCase` variants (e.g., `RuntimeError`, `LoaderError`, `ReloadPhaseType`)
- Type aliases: `PascalCase` (e.g., `RuntimeError`, `WarningCb`, `ReloadCb`)
- Traits: `PascalCase` (e.g., `BundleLoader`, `TestAddPlugin`)

## Code Style

**Formatting:**
- No explicit rustfmt configuration detected (uses default Rust style)
- Max line length appears to follow standard Rust conventions
- Indentation: 4 spaces
- Use trailing commas in multi-line constructs

**Linting:**
- Clippy via workspace lints in `Cargo.toml`
- Key lint rules enforced at workspace level:
  ```toml
  [workspace.lints.clippy]
  undocumented_unsafe_blocks = "warn"
  clone_on_ref_ptr = "warn"
  unwrap_used = "deny"
  expect_used = "warn"
  std_instead_of_core = "warn"
  improper_ctypes_definitions = "deny"

  [workspace.lints.rust]
  unsafe_op_in_unsafe_fn = "warn"
  ```
- Tests suppress `expect_used` with `#![allow(clippy::expect_used)]` at crate root

**Edition:**
- Rust edition 2024 (workspace-wide)
- Minimum Rust version: 1.85

## Import Organization

**Order:**
1. `extern crate` (if any)
2. `use core::*` items
3. `use std::*` items
4. External crates (e.g., `use polyplug_abi::*`)
5. Workspace crate imports (e.g., `use crate::error::*`)
6. Current module imports

**Style:**
- Grouped imports with braces: `use polyplug_abi::{ABI_OK, AbiError, HostInterface};`
- Types explicitly imported, not glob-imported in public API
- Test files may use more glob imports for brevity

**Path Aliases:**
- Workspace dependencies use path aliases: `polyplug_utils`, `polyplug_abi`, `polyplug`
- External crate imports from workspace deps: `thiserror`, `anyhow`, `serde`, `petgraph`

## Error Handling

**Patterns:**
- Use `thiserror::Error` derive for error types
- Error enums organized by domain: `RuntimeError`, `LoaderError`, `RegistryError`, `GraphError`, `AllocatorError`, `HostContractError`
- Error variants use structured fields with descriptive messages:
  ```rust
  #[error("init failed for bundle `{bundle}`: {error}")]
  InitFailed { bundle: String, error: String },
  ```
- Type alias for top-level error: `pub type RuntimeError = RuntimeError;`
- Error chaining via `#[error(transparent)]` and `#[from]`:
  ```rust
  #[error(transparent)]
  Loader(#[from] LoaderError),
  ```
- FFI boundary stores errors in `last_error: Mutex<String>` field, retrieved via `get_last_error()`

**Result Usage:**
- Public API returns `Result<T, RuntimeError>` or domain-specific error
- Internal functions may use domain-specific errors
- FFI functions return error codes (`u32`), with messages in thread-local storage
- All FFI exports wrapped in `std::panic::catch_unwind` to prevent panics crossing ABI boundary

**Error Codes:**
- ABI error codes defined in `polyplug_abi`: `ABI_OK = 0`, `ABI_ERROR_GENERIC = 1`, `ABI_ERROR_PANIC = 3`
- `AbiError` struct: `{ code: u32, message: StringView }`

## Async Patterns

**Runtime Choices:**
- No async runtime used (fully synchronous design)
- Plugin dispatch is direct pointer dereference (zero overhead)
- Thread-safe via `RwLock`, `Mutex`, and `Arc`
- `#[inline(always)]` on hot-path functions like `find_by_contract`

**Concurrency:**
- `Arc<PluginRegistry>` for shared registry
- `RwLock<HashMap>` for host contracts
- `Mutex<HashMap>` for bundle manifests
- `Mutex<String>` for last error storage
- Thread-local `RefCell` for test state capture

## Memory Management

**Ownership Patterns:**
- `Arc<T>` for shared ownership across threads (e.g., `Arc<PluginRegistry>`, `Arc<VTableSlot>`)
- `Box<dyn BundleLoader>` for trait object ownership
- `Box::leak()` for static vtable references in tests
- `core::mem::forget(library)` to prevent `dlclose` on loaded plugin libraries

**Smart Pointers:**
- `Arc` for shared, thread-safe ownership
- `Mutex` and `RwLock` for interior mutability
- `RefCell` for thread-local test state
- Raw pointers for FFI boundary (e.g., `*const GuestContractInterface`, `*mut c_void`)

**FFI Memory:**
- Host allocator functions: `polyplug_host_alloc(size, align)` and `polyplug_host_free(ptr, size, align)`
- Plugin-allocated strings must be freed by caller after reading
- Static string views (`from_static`) must NOT be freed

**Lifetime Annotations:**
- Vtables are `'static` (live for process lifetime after registration)
- `PluginGuard` provides scoped access with RAII pattern
- FFI callbacks receive `*const` pointers with explicit lifetime documentation

## Documentation Patterns

**Module Level:**
- Every module has `//!` doc comment describing purpose
- Example from `crates/polyplug/src/runtime.rs`:
  ```rust
  //! Runtime — core runtime logic, builder pattern, and two-phase lifecycle.
  //!
  //! Phase 1 (initialization, single-threaded):
  //!  - Load manifests
  //!  - Build capability graph
  //!  - dlopen bundles in topological order
  //!  - Call init() on each bundle
  //!  - Register vtables
  //!
  //! Phase 2 (runtime, multi-threaded, lock-free):
  //!  - Plugin dispatch is a direct pointer dereference
  //!  - find_by_contract() is a read-only RwLock read guard
  //!  - No locks in the hot path
  ```

**Item Level:**
- Public functions have `///` doc comments with purpose
- `# Safety` section required for all unsafe functions (enforced by clippy lint)
- Example safety documentation:
  ```rust
  /// HostInterface.register_plugin callback — registers a plugin vtable with the runtime.
  ///
  /// # Safety
  /// - rt_ctx must be a valid pointer to a HostContext
  /// - descriptor must point to a valid PluginDescriptor
  /// - vtable must point to a valid GuestContractInterface that remains valid for the Runtime lifetime
  pub(crate) unsafe extern "C" fn host_register_plugin(...)
  ```

**Inline Comments:**
- `// SAFETY:` comments before every unsafe block explaining why it's safe
- `// ---` section separators for logical grouping in long files
- Comment blocks use `// ───` Unicode dashes for visual sectioning

**Test Documentation:**
- Test files have `//!` module doc explaining test purpose
- Test functions have `///` doc comments describing what is tested
- Step-by-step comments in complex tests (e.g., `// ── Step 1: Locate workspace root`)

## Comments

**When to Comment:**
- Every unsafe operation requires a `// SAFETY:` comment
- Complex FFI interactions need step-by-step comments
- Non-obvious business logic (e.g., dependency enforcement rules)
- Build.rs script logic with path resolution explanations

**JSDoc/TSDoc:**
- Not applicable (Rust project)
- Use `///` for rustdoc

**Section Markers:**
- Use Unicode dash separators: `// ─── Section Name ───────────────────────`
- Example pattern:
  ```rust
  // ─── HostInterface C ABI callbacks ───────────────────────────────────────────────
  ```

## Function Design

**Size:**
- Functions typically under 50 lines for clarity
- Complex functions broken into helper functions
- Hot-path functions marked `#[inline(always)]` for zero overhead

**Parameters:**
- Prefer references over owned values when not transferring ownership
- Use `&Path` for file paths
- Use struct args for multi-parameter FFI calls (e.g., `AddArgs`, `ChainArgs`)
- FFI uses raw pointers: `*const T`, `*mut T`

**Return Values:**
- `Result<T, E>` for fallible operations
- `Option<T>` for nullable results
- `AbiError` for FFI boundary returns
- Handle packing for FFI: `u64` packed from `GuestContractHandle { index, generation }`

## Module Design

**Exports:**
- Public API exported from `lib.rs` via `pub use`
- Internal modules marked `pub(crate)` or private
- Re-export convenience types: `pub use reload::ReloadEvent;`

**Barrel Files:**
- `lib.rs` acts as barrel file for crate exports
- Submodules have their own `mod.rs` or direct file structure
- Example pattern:
  ```rust
  pub mod compatibility;
  pub mod error;
  pub mod ffi;
  pub mod loader;
  pub mod registry;
  pub mod reload;
  pub mod runtime;
  pub mod runtime_builder;
  mod runtime_config;

  pub use reload::ReloadEvent;
  pub use runtime_config::RuntimeConfig;
  ```

**Visibility:**
- `pub` for public API
- `pub(crate)` for internal shared functions
- `pub(super)` or `pub(in path)` for restricted visibility
- Private by default for implementation details

---

*Convention analysis: 2026-04-02*