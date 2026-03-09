# Epic 11: Lua Plugin Support

## TL;DR

> **Quick Summary**: Add full LuaJIT plugin support to polyplug — runtime loader (`polyplug-lua`), codegen (`polyplugc` Lua generator), guest library (`polyplug_guest.lua`), fixture, and integration tests — mirroring the Python implementation exactly.
>
> **Deliverables**:
> - `crates/polyplug-lua`: fully working `LuaLoader` using mlua/LuaJIT vendored
> - `crates/polyplugc/src/generators/lua/`: full `LuaGenerator` wired into CLI
> - `guest-libs/lua/polyplug_guest.lua`: FFI guest library
> - `tests/fixtures/test_plugin.lua`: Lua fixture implementing `test.add@1`
> - `tests/integration_lua/mod.rs`: integration tests mirroring Python suite
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 4 waves
> **Critical Path**: T1 (error variants) → T2 (config) → T3 (guest lib) → T4 (loader) → T5 (build.rs) → T6 (fixture) → T7 (integration tests) → T8 (generator) → T9 (CLI wire-up) → F1–F4

---

## Context

### Original Request
Add complete Lua plugin support to polyplug (Epic 11), mirroring the existing Python implementation: loader, codegen generator, guest library, fixture, integration tests.

### Interview Summary
**Key Discussions**:
- **LuaJIT only** for Epic 11 scope; `LuaVersion` enum kept as documentation, compile-time fixed
- **No C host lib** (`polyplug_lua.so`) — mlua embedding only, no `cc` crate, no `host-libs/lua/`
- **mlua `0.10` series** with `luajit + vendored + send` features
- **64-bit pointer** passed as `i64` integer, reconstructed in Lua via `ffi.cast("uintptr_t", ...)`
- **u64 contract_id** via `ffi.new("uint64_t", "0xHEX")` string constructor
- **ffi.cdef** guarded with pcall at module load time, never on hot path
- **OnceLock<Lua>** for single global VM — mlua with `send` is `Send+Sync`
- **TEST_LUA_PLUGIN**: mlua vendors LuaJIT, always available — emit real path unconditionally; keep `SKIP_LUA=1` env var escape hatch

### Research Findings
- Python loader: `OnceLock<GILGuard>` pattern in `context/mod.rs` → mirror with `OnceLock<Lua>`
- Python generator: 778 lines, full type mapping, pcall-guarded cdef, metatype usage
- `build.rs` pattern: `Command::new(runtime).arg("-v")` → `rerun-if-changed` + env var emission
- ABI structs: `StringView`, `Buffer`, `AbiError`, `PluginHandle`, `PluginVTable`, `PluginDescriptor`, `PluginRegistrar` — all frozen, map to C struct declarations in `ffi.cdef`
- `test_api.toml` contract: `contract_id = 0xCC4232FAB0410D2B`, functions: `add`, `add_primitive`, `version`, `reset`

### Metis Review
**Identified Gaps** (addressed):
- B1 — mlua feature flag vs LuaVersion: LuaJIT-only, `luajit+vendored+send`
- B2 — 64-bit pointer precision: pass as `i64`, reconstruct via `ffi.cast("uintptr_t", ...)`
- B3 — u64 contract_id: `ffi.new("uint64_t", "0xCC4232FAB0410D2B")` string constructor
- B4 — ffi.cdef redefine error: pcall guard + `require` caching
- B5 — package.path for guest lib: `POLYPLUG_GUEST_LUA_DIR` set in `build.rs`, injected into VM
- B6 — polyplug_lua.so C lib: cut entirely from scope
- L2-L6, L8, E3, E6, A2, A5: all resolved (see Working Context doc)

---

## Work Objectives

### Core Objective
Implement first-class LuaJIT plugin support in polyplug so that Lua scripts can be loaded and executed as plugins via the same `BundleLoader` trait used by the Python runtime.

### Concrete Deliverables
- `crates/polyplug-lua/src/lib/config/mod.rs` — `LuaVersion`, `LuaConfig`
- `crates/polyplug-lua/src/lib/loader/mod.rs` — `ensure_lua_initialized`, `LuaLoader`
- `crates/polyplug-lua/src/lib/mod.rs` — re-exports + `LuaLoader` pub
- `crates/polyplug-lua/Cargo.toml` — mlua dependency added
- `crates/polyplug/src/error/mod.rs` — 4 new `LoaderError` variants
- `crates/polyplug/Cargo.toml` — `[[test]] integration_lua` entry
- `crates/polyplug/build.rs` — Lua fixture detection + `TEST_LUA_PLUGIN` emission
- `crates/polyplugc/src/generators/lua/mod.rs` — full `LuaGenerator`
- `crates/polyplugc/src/main.rs` — `"lua"` arm in dispatch
- `guest-libs/lua/polyplug_guest.lua` — FFI cdef + register helpers
- `tests/fixtures/test_plugin.lua` — complete Lua test plugin
- `tests/fixtures/test_plugin.manifest.toml` — Lua manifest
- `tests/integration_lua/mod.rs` — integration test suite

### Definition of Done
- [ ] `cargo clippy --workspace -- -D warnings` exits 0
- [ ] `cargo fmt --check` exits 0
- [ ] `cargo test --workspace` exits 0 (all integration_lua tests pass)
- [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/lua-out` produces valid Lua files

### Must Have
- `LuaLoader` implements `BundleLoader` trait exactly as Python loader does
- `polyplug_guest.lua` works as a `require`-able module (no redefine errors on second load)
- 64-bit pointers and u64 contract_id round-trip correctly through LuaJIT FFI
- All integration tests from `integration_lua` pass
- All error paths return typed `PolyplugError` (no `.unwrap()`, no string errors)
- All new modules use `filename/mod.rs` structure

### Must NOT Have (Guardrails)
- No `bare filename.rs` module roots anywhere
- No `.unwrap()` in any non-test production code
- No `use` statements inside functions or impl blocks
- No modification of ABI-visible structs in `crates/polyplug/src/abi/mod.rs`
- No C host library (`host-libs/lua/`), no `cc` crate dependency
- No `LuaVersion` runtime switching (compile-time LuaJIT only)
- No passing registrar pointer as a Lua `number` (must be integer i64)
- No `ffi.cdef` calls on hot path (module-load-time only)
- No editing of generated files by hand
- No `require("polyplug_guest")` before `package.path` is set

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (`cargo test`, existing `integration_python/` pattern)
- **Automated tests**: Tests-after (integration tests in Wave 3)
- **Framework**: `cargo test` (Rust built-in)

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{scenario-slug}.{ext}`.

- **Rust compilation/test**: `Bash` — `cargo clippy`, `cargo build`, `cargo test`
- **Lua script execution**: `Bash` — `lua` or embed via `mlua` REPL test
- **CLI codegen**: `Bash` — `polyplugc generate ... --lang lua`

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Foundation — all independent, start immediately):
├── Task 1: Add LoaderError Lua variants (error/mod.rs)          [quick]
├── Task 2: Create LuaConfig + LuaVersion (config/mod.rs)        [quick]
└── Task 3: Write guest-libs/lua/polyplug_guest.lua              [unspecified-high]

Wave 2 (Core loader — depends on T1+T2+T3):
├── Task 4: Implement LuaLoader (loader/mod.rs + lib/mod.rs)     [unspecified-high]
├── Task 5: Update build.rs + polyplug Cargo.toml                [quick]
└── Task 6: Write test_plugin.lua + manifest.toml                [unspecified-high]

Wave 3 (Tests + codegen — T4+T5+T6 complete):
├── Task 7: Write integration_lua/mod.rs                         [unspecified-high]
└── Task 8: Implement LuaGenerator (generators/lua/mod.rs)       [deep]

Wave 4 (Wire-up — T7+T8 complete):
└── Task 9: Wire LuaGenerator into polyplugc main.rs + crate Cargo.toml [quick]

Wave FINAL (All tasks complete — 4 parallel reviewers):
├── Task F1: Plan compliance audit                               [oracle]
├── Task F2: Code quality review                                 [unspecified-high]
├── Task F3: Real integration QA                                 [unspecified-high]
└── Task F4: Scope fidelity check                               [deep]

Critical Path: T1 → T4 → T7 → F1–F4
             T2 → T4
             T3 → T4
             T5 → T7
             T6 → T7
             T8 → T9
Parallel Speedup: ~60% faster than sequential
```

### Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| T1   | —         | T4     |
| T2   | —         | T4     |
| T3   | —         | T4, T6 |
| T4   | T1, T2, T3, **T5** | T7   |
| T5   | —         | T4, T7     |
| T6   | T3        | T7     |
| T7   | T4, T5, T6 | F1–F4 |
| T8   | —         | T9     |
| T9   | T8        | F1–F4  |
| F1–F4 | T7, T9  | —      |

### Agent Dispatch Summary

- **Wave 1** (3): T1 → `quick`, T2 → `quick`, T3 → `unspecified-high`
- **Wave 2** (3): T5 → `quick` (must run first — provides build.rs), T4 → `unspecified-high` (after T5), T6 → `unspecified-high` (parallel with T4)
- **Wave 3** (2): T7 → `unspecified-high`, T8 → `deep`
- **Wave 4** (1): T9 → `quick`
- **FINAL** (4): F1 → `oracle`, F2 → `unspecified-high`, F3 → `unspecified-high`, F4 → `deep`

---

## TODOs

- [ ] 1. Add Lua-specific `LoaderError` variants to `crates/polyplug/src/error/mod.rs`

  **What to do**:
  - Open `crates/polyplug/src/error/mod.rs`
  - Locate the `LoaderError` enum (it currently has Python-related variants)
  - Add exactly these four new variants at the end of the enum, before the closing `}`:
    ```rust
    #[error("lua vm init failed: {reason}")]
    LuaVmInitFailed { reason: String },

    #[error("lua script load failed: path={path}, reason={reason}")]
    LuaScriptLoadFailed { path: String, reason: String },

    #[error("lua plugin missing polyplug_init function: bundle={bundle}")]
    LuaInitFunctionMissing { bundle: String },

    #[error("lua polyplug_init raised error: bundle={bundle}, message={message}")]
    LuaInitRaisedError { bundle: String, message: String },
    ```
  - Do NOT modify any existing variants
  - Do NOT change any existing Python or generic variants
  - Do NOT modify `PolyplugError` or any other enum

  **Must NOT do**:
  - Do not add any `unwrap()` or `expect()` anywhere
  - Do not modify ABI structs
  - Do not add `use` inside functions

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file, additive change only, no logic to implement
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T2, T3)
  - **Blocks**: T4 (LuaLoader needs these error variants)
  - **Blocked By**: None (can start immediately)

  **References**:

  **Pattern References**:
  - `crates/polyplug/src/error/mod.rs` — existing `LoaderError` enum; the actual Python variants to mirror are `PythonInitFailed { reason: String }`, `PythonModuleImportFailed { path: String, reason: String }`, and `PythonInitRaisedException { bundle: String, message: String }`. Name the new Lua variants by replacing `Python` with `Lua` and adjusting field names to match the new semantics.

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug` exits 0
  - [ ] `cargo clippy -p polyplug -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: Error variants compile cleanly
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: output contains "Finished" or "Compiling polyplug"
      3. Assert: no "error[" lines in output
    Expected Result: exit code 0, no compile errors
    Evidence: .sisyphus/evidence/task-1-compile.txt

  Scenario: New variants are accessible from other crates
    Tool: Bash
    Steps:
      1. Run: cargo clippy -p polyplug -- -D warnings 2>&1
      2. Assert: exit code 0
    Expected Result: Zero warnings, zero errors
    Evidence: .sisyphus/evidence/task-1-clippy.txt
  ```

  **Commit**: YES — groups with T2
  - Message: `feat(error): add lua loader error variants`
  - Files: `crates/polyplug/src/error/mod.rs`
  - Pre-commit: `cargo build -p polyplug`

---

- [ ] 2. Create `crates/polyplug-lua/src/lib/config/mod.rs` with `LuaVersion` and `LuaConfig`

  **What to do**:
  - Create directory `crates/polyplug-lua/src/lib/config/` (it does not exist yet)
  - Create file `crates/polyplug-lua/src/lib/config/mod.rs` with this exact content:
    ```rust
    //! Configuration types for the Lua plugin loader.

    /// Lua implementation variant.
    ///
    /// NOTE: Epic 11 supports LuaJIT only at compile time.
    /// This enum is kept for future extensibility documentation.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum LuaVersion {
        /// LuaJIT (default, vendored via mlua `luajit+vendored` feature).
        LuaJit,
    }

    /// Configuration for the Lua plugin loader.
    #[derive(Debug, Clone)]
    pub struct LuaConfig {
        /// The Lua implementation to use. Currently only `LuaJit` is supported.
        pub version: LuaVersion,
    }

    impl Default for LuaConfig {
        fn default() -> Self {
            Self {
                version: LuaVersion::LuaJit,
            }
        }
    }
    ```
  - Open `crates/polyplug-lua/src/lib/mod.rs` (currently a stub)
  - Add `pub mod config;` at the top of the file (after any existing `//!` doc comment, before other content)
  - Do NOT remove existing content from `lib/mod.rs`

  **Must NOT do**:
  - Do not create `config.rs` directly under `src/lib/` — must be `src/lib/config/mod.rs`
  - Do not add runtime LuaVersion switching logic
  - Do not add `use` inside functions

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: New file with pure data types, no logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T3)
  - **Blocks**: T4
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/polyplug-python/src/lib/config/mod.rs` — `PythonVersion`, `PythonConfig`, `Default` impl — mirror exactly
  - `crates/polyplug-lua/src/lib/mod.rs` — current stub, add `pub mod config;` line here

  **Acceptance Criteria**:
  - [ ] File `crates/polyplug-lua/src/lib/config/mod.rs` exists
  - [ ] `cargo build -p polyplug-lua` exits 0

  **QA Scenarios**:
  ```
  Scenario: Config module compiles
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug-lua 2>&1
      2. Assert: exit code 0
      3. Assert: no "error[" lines
    Expected Result: Clean compilation
    Evidence: .sisyphus/evidence/task-2-compile.txt
  ```

  **Commit**: YES — groups with T1
  - Message: `feat(error): add lua loader error variants` (same commit as T1)
  - Files: `crates/polyplug-lua/src/lib/config/mod.rs`, `crates/polyplug-lua/src/lib/mod.rs`
  - Pre-commit: `cargo build -p polyplug-lua`

---

- [ ] 3. Create `guest-libs/lua/polyplug_guest.lua`

  **What to do**:
  - Create directory `guest-libs/lua/` (does not exist yet)
  - Create file `guest-libs/lua/polyplug_guest.lua` with the following content:

  ```lua
  -- polyplug_guest.lua
  -- LuaJIT FFI guest library for polyplug plugins.
  -- This module is loaded via require("polyplug_guest").
  -- All ffi.cdef calls are at module load time and guarded with pcall
  -- to prevent "already defined" errors when a second plugin calls require().

  local ffi = require("ffi")

  -- ABI struct declarations.
  -- Guards prevent "already defined" errors on second require().
  local function cdef_guarded(decl)
      local ok, err = pcall(ffi.cdef, decl)
      if not ok and not string.find(err, "already defined", 1, true) then
          error(err, 2)
      end
  end

  cdef_guarded([[
      typedef struct { const uint8_t* ptr; size_t len; } StringView;
      typedef struct { uint8_t* ptr; size_t len; size_t cap; } Buffer;
      typedef struct { uint32_t code; uint32_t _pad; StringView message; } AbiError;
      typedef struct { uint32_t index; uint32_t generation; } PluginHandle;
      typedef struct {
          uint64_t contract_id;
          uint32_t contract_version;
          uint32_t function_count;
          void* const* functions;
      } PluginVTable;
      typedef struct {
          StringView name;
          StringView contract_name;
          uint32_t version_major;
          uint32_t version_minor;
          uint32_t version_patch;
          uint32_t _tail_pad;
      } PluginDescriptor;
      typedef AbiError (*register_plugin_fn_t)(void*, const PluginDescriptor*, const PluginVTable*);
      typedef struct {
          register_plugin_fn_t register_plugin;
          const void* host;
      } PluginRegistrar;
  ]])

  local M = {}

  --- Reconstruct a PluginRegistrar pointer from the integer passed by the host.
  --- The host passes the pointer as an i64 to avoid Lua double precision loss.
  --- @param ptr_int number  The registrar pointer as a LuaJIT integer (int64_t).
  --- @return cdata          A typed PluginRegistrar pointer.
  function M.cast_registrar(ptr_int)
      -- PRECISION: ptr_int is a LuaJIT int64_t, not a double.
      -- ffi.cast via uintptr_t preserves all 64 bits.
      return ffi.cast("PluginRegistrar*", ffi.cast("uintptr_t", ptr_int))
  end

  --- Create a StringView from a Lua string.
  --- The string data is owned by Lua and must remain alive for the duration of the call.
  --- @param s string  A Lua string.
  --- @return cdata    A StringView cdata pointing into the Lua string.
  function M.string_view(s)
      return ffi.new("StringView", { ptr = ffi.cast("const uint8_t*", s), len = #s })
  end

  --- Create a zero AbiError (success).
  --- @return cdata  An AbiError with code=0.
  function M.ok()
      return ffi.new("AbiError", { code = 0 })
  end

  --- Create an AbiError with a given code and message.
  --- @param code    number  Error code (non-zero).
  --- @param message string  Error message (Lua string).
  --- @return cdata          An AbiError cdata.
  function M.err(code, message)
      return ffi.new("AbiError", { code = code, message = M.string_view(message) })
  end

  return M
  ```

  **Must NOT do**:
  - Do not call `ffi.cdef` outside of `cdef_guarded()` wrapper
  - Do not place `ffi.cdef` calls inside functions that are called repeatedly (hot path)
  - Do not use `dofile()` to load this module — it must be loaded via `require()`
  - Do not declare any types not present in `crates/polyplug/src/abi/mod.rs`
  - Do not modify any ABI struct layouts

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: LuaJIT FFI requires careful understanding of ABI layout and precision rules
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES
  - **Parallel Group**: Wave 1 (with T1, T2)
  - **Blocks**: T4 (loader needs guest lib path), T6 (test plugin requires this module)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `guest-libs/python/polyplug_guest/abi.py` — Python ABI declarations; map each ctypes struct to the equivalent LuaJIT FFI cdef
  - `crates/polyplug/src/abi/mod.rs` — canonical Rust ABI struct definitions (source of truth for field types/order)

  **External References**:
  - LuaJIT FFI semantics: http://luajit.org/ext_ffi_semantics.html — esp. "Passing structs by value", "64-bit integers"
  - `ffi.new` string constructor for uint64_t: http://luajit.org/ext_ffi_api.html#ffi_new

  **Acceptance Criteria**:
  - [ ] File `guest-libs/lua/polyplug_guest.lua` exists
  - [ ] `luajit -e "require('guest-libs/lua/polyplug_guest')"` exits 0 (or test via mlua in T4)
  - [ ] Second `require()` of the module does not raise "already defined" error

  **QA Scenarios**:
  ```
  Scenario: Module loads without error via mlua
    Tool: Bash (verified as part of T4 integration; standalone if luajit available)
    Steps:
      1. Run: luajit -e "package.path='guest-libs/lua/?.lua;' .. package.path; local m = require('polyplug_guest'); print(type(m))" 2>&1
      2. Assert: output is "table"
      3. Assert: exit code 0
    Expected Result: Module loads, returns table
    Evidence: .sisyphus/evidence/task-3-load.txt

  Scenario: Double require does not error
    Tool: Bash
    Steps:
      1. Run: luajit -e "package.path='guest-libs/lua/?.lua;' .. package.path; require('polyplug_guest'); require('polyplug_guest'); print('ok')" 2>&1
      2. Assert: output contains "ok"
      3. Assert: no "already defined" in output
    Expected Result: Second require() silently returns cached module
    Evidence: .sisyphus/evidence/task-3-double-require.txt
  ```

  **Commit**: YES (standalone)
  - Message: `feat(guest-lua): add polyplug_guest.lua FFI guest library`
  - Files: `guest-libs/lua/polyplug_guest.lua`

---

- [ ] 4. Implement `LuaLoader` in `crates/polyplug-lua/src/lib/loader/mod.rs` and update `lib/mod.rs`

  **What to do**:
  - Create directory `crates/polyplug-lua/src/lib/loader/`
  - Create file `crates/polyplug-lua/src/lib/loader/mod.rs` with the full implementation:

  ```rust
  //! LuaJIT VM initialization and plugin loader implementation.

  use std::path::Path;
  use std::sync::OnceLock;

  use mlua::Lua;
  use mlua::Function;

  use crate::config::LuaConfig;
  use polyplug::error::LoaderError;
  use polyplug::error::PolyplugError;
  use polyplug::loader::BundleLoader;
  use polyplug::abi::PluginRegistrar;

  /// The path to the guest-libs/lua/ directory, set at compile time by build.rs.
  const GUEST_LUA_DIR: &str = env!("POLYPLUG_GUEST_LUA_DIR");

  /// Process-global LuaJIT VM. Created on first use.
  /// mlua::Lua with the `send` feature is Send+Sync, so OnceLock<Lua> is valid.
  static LUA_VM: OnceLock<Lua> = OnceLock::new();

  /// Ensures the global Lua VM is initialized with the correct package.path.
  /// Idempotent: subsequent calls return the already-initialized VM.
  ///
  /// # Errors
  /// Returns `PolyplugError::Loader(LoaderError::LuaVmInitFailed)` if VM creation fails.
  pub(crate) fn ensure_lua_initialized(
      _config: &LuaConfig,
  ) -> Result<&'static Lua, PolyplugError> {
      LUA_VM.get_or_try_init(|| {
          // mlua 0.10: Lua::new() returns Lua, not Result.
          // SAFETY: mlua panics internally on OOM; all other errors are handled.
          let lua: Lua = Lua::new();
          // Set package.path so that require("polyplug_guest") resolves correctly.
          let package_path_code: String = format!(
              "package.path = package.path .. ';' .. '{}/?.lua'",
              GUEST_LUA_DIR.replace('\\', "/")
          );
          lua.load(&package_path_code).exec().map_err(|e| {
              PolyplugError::Loader(LoaderError::LuaVmInitFailed {
                  reason: format!("failed to set package.path: {}", e),
              })
          })?;
          Ok(lua)
      })
  }

  /// Loads a Lua plugin bundle from `path` and registers it via `registrar`.
  ///
  /// The Lua script must define a global function `polyplug_init(registrar_ptr: integer)`.
  /// The registrar pointer is passed as an i64 integer (not a Lua number/double)
  /// to preserve full 64-bit precision on LuaJIT.
  pub struct LuaLoader {
      /// Configuration for this loader instance.
      pub config: LuaConfig,
  }

  impl LuaLoader {
      /// Create a new `LuaLoader` with the given configuration.
      pub fn new(config: LuaConfig) -> Self {
          Self { config }
      }
  }

  impl BundleLoader for LuaLoader {
      fn runtime_name(&self) -> &'static str {
          "lua"
      }

      fn load(
          &self,
          path: &Path,
          registrar: &mut PluginRegistrar,
      ) -> Result<(), PolyplugError> {
          let lua: &Lua = ensure_lua_initialized(&self.config)?;

          // Read the plugin script source.
          let source: String = std::fs::read_to_string(path).map_err(|e| {
              PolyplugError::Loader(LoaderError::LuaScriptLoadFailed {
                  path: path.display().to_string(),
                  reason: e.to_string(),
              })
          })?;

          // Execute the script.  This defines polyplug_init in the global environment.
          lua.load(&source).exec().map_err(|e| {
              PolyplugError::Loader(LoaderError::LuaScriptLoadFailed {
                  path: path.display().to_string(),
                  reason: e.to_string(),
              })
          })?;

          // Retrieve polyplug_init.
          let bundle_name: String = path
              .file_name()
              .map(|n| n.to_string_lossy().into_owned())
              .unwrap_or_else(|| path.display().to_string());

          // mlua 0.10: get requires two type params: key type and value type.
          let init_fn: Function = lua
              .globals()
              .get::<_, Function>("polyplug_init")
              .map_err(|_| {
                  PolyplugError::Loader(LoaderError::LuaInitFunctionMissing {
                      bundle: bundle_name.clone(),
                  })
              })?;

          // Pass registrar pointer as i64.
          // PRECISION: LuaJIT lua_Integer is int64_t. Passing as i64 preserves all
          // 64 address bits. The Lua side reconstructs via ffi.cast("uintptr_t", ...).
          let registrar_ptr: i64 = registrar as *mut PluginRegistrar as usize as i64;

          // mlua 0.10: call requires two type params: arg type and return type.
          init_fn.call::<_, ()>(registrar_ptr).map_err(|e| {
              PolyplugError::Loader(LoaderError::LuaInitRaisedError {
                  bundle: bundle_name.clone(),
                  message: e.to_string(),
              })
          })?;

          // After polyplug_init runs, read _G._polyplug_handlers and build Rust trampolines.
          // LuaJIT FFI callbacks cannot return structs by value, so the Lua plugin does NOT
          // create vtable fn pointers itself. The LuaLoader builds extern "C" trampolines.
          //
          // Read the handler table that the plugin populated:
          let handlers: mlua::Table = lua
              .globals()
              .get::<_, mlua::Table>("_polyplug_handlers")
              .map_err(|_| {
                  PolyplugError::Loader(LoaderError::LuaInitFunctionMissing {
                      bundle: bundle_name.clone(),
                  })
              })?;
          //
          // Build the vtable from the handlers table.
          // NOTE: The executor must implement this section using the concrete mlua API:
          // - Read contract_name, contract_id_hex, contract_version, plugin_name, functions[]
          // - For each function slot, store the mlua::Function in a global dispatch table
          //   (e.g., a process-global Vec<Option<mlua::Function>> protected by a Mutex)
          //   keyed by a unique slot index.
          // - Create a `Box`-allocated `extern "C"` dispatch trampoline for each slot that:
          //   a) Reads args_ptr/out_ptr as integers
          //   b) Calls the stored mlua::Function with (args_ptr_i64, out_ptr_i64)
          //   c) Returns AbiError { code: 0 } on Ok, AbiError { code: 1 } on Err
          // - Leak the trampolines (Box::into_raw / Box::leak) so they have 'static lifetime
          // - Build PluginVTable with function_count and the leaked fn pointers
          // - Build PluginDescriptor from contract_name/plugin_name/version fields
          // - Call (registrar.register_plugin)(registrar_ptr, &descriptor, &vtable)
          //   and map the AbiError to PolyplugError::Loader on failure
          //
          // See tests/integration_python/mod.rs for the expected vtable structure.
          //
          let _handlers: mlua::Table = handlers; // executor: expand this stub into full impl
          Ok(())
      }
  }
  ```

  - Open `crates/polyplug-lua/src/lib/mod.rs` and update it to:
    ```rust
    //! polyplug-lua: LuaJIT plugin loader for the polyplug runtime.

    pub mod config;
    pub mod loader;

    pub use loader::LuaLoader;
    pub use config::LuaConfig;
    ```

  - Open `crates/polyplug-lua/Cargo.toml` and add the mlua dependency:
    ```toml
    [dependencies]
    polyplug = { path = "../polyplug" }
    mlua = { version = "0.10", features = ["luajit", "vendored", "send"] }
    ```

  **Must NOT do**:
  - Do not pass the registrar pointer as `f64` / Lua number — must be `i64`
  - Do not use `.unwrap()` anywhere in this file
  - Do not place `ensure_lua_initialized` call outside of `load()` (lazy init is correct)
  - Do not add a `Mutex` around `LUA_VM` — mlua with `send` handles its own locking
  - **CRITICAL: Do NOT expect LuaJIT FFI callbacks to return `AbiError` by value.**
    LuaJIT FFI callbacks cannot return aggregate (struct) types by value. The vtable function
    pointers stored in the `PluginVTable` MUST be Rust-side `extern "C"` trampolines, NOT
    LuaJIT FFI callback objects. The LuaLoader is responsible for creating these trampolines.
    The Lua plugin (T6) sets up Lua function implementations; the LuaLoader wraps them in
    Rust trampolines before registering the vtable via the `PluginRegistrar` callback.
    Implementation approach:
    - After `polyplug_init` runs, call `lua.globals().get::<_, mlua::Table>("_polyplug_handlers")`
      to retrieve a table of {contract_id: String, version: u32, functions: [LuaFunction, ...]}
    - Allocate `Box::leak`-ed Rust `extern "C"` trampolines for each function slot.
      Each trampoline acquires the OnceLock<Lua>, calls `Lua::scope` or global fn registry,
      writes result into out pointer, returns `AbiError { code: 0 }` on success.
    - The `PluginVTable.functions` array points to these leaked Rust trampolines (not Lua callbacks).
    - Leaked allocations are intentional: vtable fn pointers must be 'static.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Requires understanding of mlua API, OnceLock patterns, and the polyplug BundleLoader trait
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO — T4 depends on T5 (T4 uses `env!("POLYPLUG_GUEST_LUA_DIR")` which is set by `polyplug-lua/build.rs` created in T5; T5 must complete before T4 can compile)
  - **Parallel Group**: Wave 2, but T5 must complete first within Wave 2 before T4 can build
  - **Blocks**: T7
  - **Blocked By**: T1 (error variants), T2 (LuaConfig), T3 (guest lib path), **T5** (`polyplug-lua/build.rs` emits `POLYPLUG_GUEST_LUA_DIR` which T4's code requires at compile time)

  **References**:

  **Pattern References**:
  - `crates/polyplug-python/src/lib/context/mod.rs` — `ensure_python_initialized` with `OnceLock` pattern; mirror for Lua
  - `crates/polyplug-python/src/lib/mod.rs` — module re-export pattern
  - `crates/polyplug-python/Cargo.toml` — `pyo3` dependency pattern; mirror for mlua
  - `crates/polyplug/src/loader/mod.rs` — `BundleLoader` trait signature; implement exactly

  **External References**:
  - mlua 0.10 API: `Lua::new()`, `lua.load(&str).exec()`, `lua.globals().get::<Function>()`, `fn.call::<()>(arg)`
  - OnceLock::get_or_try_init: https://doc.rust-lang.org/std/sync/struct.OnceLock.html#method.get_or_try_init

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug-lua` exits 0
  - [ ] `cargo clippy -p polyplug-lua -- -D warnings` exits 0
  - [ ] `LuaLoader` implements `BundleLoader` (verified by `cargo build`)

  **QA Scenarios**:
  ```
  Scenario: LuaLoader compiles and clippy passes
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug-lua 2>&1
      2. Assert: exit code 0
      3. Run: cargo clippy -p polyplug-lua -- -D warnings 2>&1
      4. Assert: exit code 0, no "warning:" lines
    Expected Result: Clean build and lint
    Evidence: .sisyphus/evidence/task-4-build.txt

  Scenario: LuaLoader::load returns LuaInitFunctionMissing for script without polyplug_init
    Tool: Bash (via cargo test once T7 exists; validate error path here via unit test sketch)
    Steps:
      1. Write a temp Lua file with no polyplug_init: echo "x = 1" > /tmp/noinit.lua
      2. (Validated in T7 integration tests — mark as deferred)
    Expected Result: Returns Err(PolyplugError::Loader(LoaderError::LuaInitFunctionMissing))
    Evidence: .sisyphus/evidence/task-4-no-init-error.txt (captured in T7)
  ```

  **Commit**: YES (standalone)
  - Message: `feat(polyplug-lua): implement LuaLoader with mlua/LuaJIT`
  - Files: `crates/polyplug-lua/src/lib/loader/mod.rs`, `crates/polyplug-lua/src/lib/mod.rs`, `crates/polyplug-lua/Cargo.toml`
  - Pre-commit: `cargo build -p polyplug-lua`

---

- [ ] 5. Update `crates/polyplug/build.rs`, create `crates/polyplug-lua/build.rs`, and update `crates/polyplug/Cargo.toml` for Lua test support

  **What to do**:

  **Create `crates/polyplug-lua/build.rs`** (NEW FILE — does not exist yet):
  - This is the build script for the `polyplug-lua` crate itself.
  - `POLYPLUG_GUEST_LUA_DIR` MUST be emitted from `polyplug-lua`'s own `build.rs` because
    `cargo:rustc-env` set in one crate's `build.rs` does NOT propagate to other crates.
  - Create `crates/polyplug-lua/build.rs` with this exact content:
    ```rust
    // build.rs for polyplug-lua
    // allow expect in build scripts (no better error handling mechanism here)
    #![allow(clippy::expect_used)]
    fn main() {
        // Emit the guest Lua library directory so that LuaLoader's
        // env!("POLYPLUG_GUEST_LUA_DIR") resolves at compile time.
        // This MUST be in polyplug-lua's own build.rs — cargo:rustc-env only
        // affects the crate that emits it, not downstream crates.
        let manifest_dir: std::path::PathBuf =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // workspace root is two levels up from crates/polyplug-lua/
        let workspace_root: std::path::PathBuf = manifest_dir
            .parent()
            .expect("crates/ parent must exist")
            .parent()
            .expect("workspace root must exist")
            .to_path_buf();
    let guest_lua_dir: std::path::PathBuf = workspace_root.join("guest-libs").join("lua");
        println!(
            "cargo:rustc-env=POLYPLUG_GUEST_LUA_DIR={}",
            guest_lua_dir.display()
        );
        println!("cargo:rerun-if-changed=build.rs");
        println!("cargo:rerun-if-changed=../../guest-libs/lua/polyplug_guest.lua");
    }
    ```

  **In `crates/polyplug/build.rs`**:
  - Near the bottom of `fn main()`, after the Python section, add ONLY the fixture env var:
    ```rust
    // --- Lua fixture ---
    println!(
        "cargo:rerun-if-changed={}",
        fixtures_dir.join("test_plugin.lua").display()
    );
    // mlua with `vendored` embeds LuaJIT — no system install required.
    // Always emit the real fixture path.
    // Tests can opt-out via SKIP_LUA=1 env var at runtime.
    println!(
        "cargo:rustc-env=TEST_LUA_PLUGIN={}",
        fixtures_dir.join("test_plugin.lua").display()
    );
    ```
  - Note: `workspace_root` and `fixtures_dir` variables are already defined in the existing `build.rs` — do NOT redeclare them
  - **DO NOT** emit `POLYPLUG_GUEST_LUA_DIR` here — it belongs in `polyplug-lua/build.rs` (see above)

  **In `crates/polyplug/Cargo.toml`**:
  - Add a new `[[test]]` entry for the Lua integration suite:
    ```toml
    [[test]]
    name = "integration_lua"
    path = "../../tests/integration_lua/mod.rs"
    ```
  - **Check if `polyplug-lua` is already in `[dev-dependencies]`** in `crates/polyplug/Cargo.toml`.
    As of this writing, `polyplug-lua = { path = "../../crates/polyplug-lua" }` is **already present**.
    **Do NOT add it again** — that would create a duplicate key and break Cargo.
    Verify: `grep -q 'polyplug-lua' crates/polyplug/Cargo.toml && echo exists || echo missing`.
    Only add if missing.

  **Must NOT do**:
  - Do not emit `POLYPLUG_GUEST_LUA_DIR` from `polyplug/build.rs` — it would have no effect there
  - Do not redeclare variables already declared in `build.rs`
  - Do not remove or modify the Python sections of `build.rs`
  - Do not add `polyplug-lua` as a duplicate `[dev-dependencies]` entry — it already exists in `Cargo.toml`; adding it again breaks Cargo

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Additive changes, pattern already established by Python
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T4, T6)
  - **Parallel Group**: Wave 2
  - **Blocks**: T7 (integration tests need `TEST_LUA_PLUGIN` env var and `[[test]]` entry)
  - **Blocked By**: None

  **References**:

  **Pattern References**:
  - `crates/polyplug/build.rs` — existing Python section; mirror pattern exactly for `TEST_LUA_PLUGIN`
  - `crates/polyplug/Cargo.toml` — existing `[[test]] integration_python` entry; mirror for Lua
  - `crates/polyplug-python/` — check if it has a `build.rs` for reference (Lua follows the same pattern)

  **Acceptance Criteria**:
  - [ ] File `crates/polyplug-lua/build.rs` exists
  - [ ] `cargo build -p polyplug-lua` exits 0 (POLYPLUG_GUEST_LUA_DIR baked in)
  - [ ] `cargo build -p polyplug` exits 0 (TEST_LUA_PLUGIN baked in)
  - [ ] `env!("TEST_LUA_PLUGIN")` resolves to a non-empty string (verified in T7)
  - [ ] `env!("POLYPLUG_GUEST_LUA_DIR")` resolves to the guest-libs/lua path (verified in T4 build)

  **QA Scenarios**:
  ```
  Scenario: POLYPLUG_GUEST_LUA_DIR baked into polyplug-lua crate
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug-lua 2>&1
      2. Assert: exit code 0
      3. Assert: no "error[" lines in output
    Expected Result: polyplug-lua compiles with POLYPLUG_GUEST_LUA_DIR set by its own build.rs
    Evidence: .sisyphus/evidence/task-5-polyplug-lua-build.txt

  Scenario: TEST_LUA_PLUGIN baked into polyplug crate
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplug 2>&1
      2. Assert: exit code 0
      3. Run: cargo test --test integration_lua -- --list 2>&1
      4. Assert: exit code 0 (env var resolves without compile error)
    Expected Result: Build succeeds, integration tests discoverable
    Evidence: .sisyphus/evidence/task-5-polyplug-build.txt
  ```

  **Commit**: YES — groups with T6
  - Message: `feat(build): emit TEST_LUA_PLUGIN and POLYPLUG_GUEST_LUA_DIR env vars`
  - Files: `crates/polyplug-lua/build.rs`, `crates/polyplug/build.rs`, `crates/polyplug/Cargo.toml`
  - Pre-commit: `cargo build -p polyplug-lua && cargo build -p polyplug`
---

- [ ] 6. Create `tests/fixtures/test_plugin.lua` and `tests/fixtures/test_plugin.manifest.toml`

  **What to do**:
  - Create `tests/fixtures/test_plugin.lua`:

  **IMPORTANT ARCHITECTURE NOTE**: LuaJIT FFI callbacks cannot return `AbiError` (a struct) by
  value. Therefore, `test_plugin.lua` must NOT create FFI callbacks for vtable functions.
  Instead, it sets up a Lua-side function table (`_G._polyplug_handlers`) that the LuaLoader
  reads after `polyplug_init` returns and wraps in Rust-side `extern "C"` trampolines.

  ```lua
  -- tests/fixtures/test_plugin.lua
  -- Lua test plugin implementing the test.add@1 contract.
  -- This is loaded by integration_lua tests via LuaLoader.
  --
  -- DESIGN: This plugin does NOT create LuaJIT FFI callbacks directly,
  -- because LuaJIT FFI callbacks cannot return structs by value (e.g. AbiError).
  -- Instead, polyplug_init populates _G._polyplug_handlers with pure Lua
  -- function implementations. The LuaLoader (Rust side) wraps these in
  -- extern "C" trampolines and builds the PluginVTable itself.

  local ffi = require("ffi")
  local polyplug_guest = require("polyplug_guest")

  local VERSION_STR = "1.0.0-lua"

  -- Implementation: add(a: u32, b: u32) -> u32
  -- args_ptr: lightuserdata/i64 pointing to a {a:u32, b:u32} C struct
  -- out_ptr:  lightuserdata/i64 pointing to a u32 output slot
  local function impl_add(args_ptr, out_ptr)
      local args = ffi.cast("uint32_t*", ffi.cast("uintptr_t", args_ptr))
      local out  = ffi.cast("uint32_t*", ffi.cast("uintptr_t", out_ptr))
      out[0] = args[0] + args[1]
  end

  local function impl_add_primitive(args_ptr, out_ptr)
      impl_add(args_ptr, out_ptr)
  end

  local function impl_version(_args_ptr, out_ptr)
      local out = ffi.cast("StringView*", ffi.cast("uintptr_t", out_ptr))
      out[0] = polyplug_guest.string_view(VERSION_STR)
  end

  local function impl_reset(_args_ptr, _out_ptr)
      -- no-op
  end

  -- polyplug_init is called by LuaLoader with the PluginRegistrar pointer as i64.
  -- It does NOT call register_plugin directly — the LuaLoader (Rust) does that
  -- after reading _G._polyplug_handlers and creating Rust-side trampolines.
  function polyplug_init(_registrar_ptr_int)
      _G._polyplug_handlers = {
          contract_name = "test",
          contract_id_hex = "0xCC4232FAB0410D2B",
          contract_version = 1,
          plugin_name = "test-plugin-lua",
          -- Functions in declaration order (must match contract function_id order):
          functions = {
              [0] = impl_add,          -- function_id 0: add
              [1] = impl_add_primitive, -- function_id 1: add_primitive
              [2] = impl_version,       -- function_id 2: version
              [3] = impl_reset,         -- function_id 3: reset
          },
      }
  end
  ```

  - Create `tests/fixtures/test_plugin.manifest.toml`:
  ```toml
  runtime = "lua"
  file = "test_plugin.lua"
  ```

  **Must NOT do**:
  - Do not create LuaJIT FFI callbacks (`ffi.cast("AbiError (*)(void*, void*)", fn)`) for vtable
    function pointers — LuaJIT FFI callbacks cannot return structs by value
  - Do not call `registrar.register_plugin(...)` from Lua — the LuaLoader does this
  - Do not pass the registrar pointer as a Lua `number` (use `int64_t` via FFI if needed at all)
  - Do not use `dofile("polyplug_guest.lua")` — use `require("polyplug_guest")`

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: LuaJIT FFI nuances, struct layout, function pointer casting, precision rules
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T4, T5)
  - **Parallel Group**: Wave 2
  - **Blocks**: T7
  - **Blocked By**: T3 (polyplug_guest.lua must exist; test plugin requires it)

  **References**:

  **Pattern References**:
  - `tests/fixtures/test_plugin.py` — Python fixture implementing the same contract; mirror function structure
  - `tests/fixtures/test_api.toml` — contract definition: `contract_id = 0xCC4232FAB0410D2B`, functions `add`, `add_primitive`, `version`, `reset`
  - `tests/fixtures/test_bundle.toml` — bundle manifest format
  - `guest-libs/lua/polyplug_guest.lua` (T3) — helper functions used by this plugin

  **Acceptance Criteria**:
  - [ ] File `tests/fixtures/test_plugin.lua` exists
  - [ ] File `tests/fixtures/test_plugin.manifest.toml` exists
  - [ ] Integration tests (T7) load and execute this plugin successfully

  **QA Scenarios**:
  ```
  Scenario: Fixture loads via LuaLoader (validated in T7)
    Tool: Bash (cargo test)
    Steps:
      1. This scenario is fully validated by T7 integration tests.
      2. Capture result from: cargo test --test integration_lua 2>&1
    Expected Result: All T7 tests pass (including bundle_loads, add, version_string)
    Evidence: .sisyphus/evidence/task-6-via-integration.txt
  ```

  **Commit**: YES — groups with T5
  - Message: `feat(build): emit TEST_LUA_PLUGIN and POLYPLUG_GUEST_LUA_DIR env vars` (same commit as T5)
  - Files: `tests/fixtures/test_plugin.lua`, `tests/fixtures/test_plugin.manifest.toml`

---

- [ ] 7. Create `tests/integration_lua/mod.rs`

  **What to do**:
  - Create directory `tests/integration_lua/`
  - Create file `tests/integration_lua/mod.rs` with the following content.
  - **This file mirrors `tests/integration_python/mod.rs` exactly** — same structure, same raw-ABI
    dispatch pattern, same thread-local registry pattern. NO high-level helpers exist on `PluginRegistrar`.
    Tests call vtable function pointers directly via `core::mem::transmute`.

  ```rust
  #![allow(clippy::expect_used)]

  use polyplug::abi::ABI_OK;
  use polyplug::abi::AbiError;
  use polyplug::abi::PluginDescriptor;
  use polyplug::abi::PluginHandle;
  use polyplug::abi::PluginRegistrar;
  use polyplug::abi::PluginVTable;
  use polyplug::abi::StringView;
  use polyplug::error::LoaderError;
  use polyplug::error::PolyplugError;
  use polyplug::error::RegistryError;
  use polyplug::loader::BundleLoader;
  use polyplug::registry::Registry;
  use polyplug_lua::LuaConfig;
  use polyplug_lua::LuaLoader;

  const LUA_PLUGIN: &str = env!("TEST_LUA_PLUGIN");

  /// `AddArgs` is the repr(C) struct that maps to `fn add(a: u32, b: u32) -> u32`.
  /// Fields must be in declaration order to match the Lua FFI cdef.
  #[repr(C)]
  struct AddArgs {
      a: u32,
      b: u32,
  }

  // Thread-local registry for test isolation.
  std::thread_local! {
      static LUA_REGISTRY: core::cell::RefCell<Registry> =
          core::cell::RefCell::new(Registry::new());
  }

  /// Registration callback passed to LuaLoader via PluginRegistrar.
  /// Writes the registered plugin into the thread-local LUA_REGISTRY.
  unsafe extern "C" fn registry_register_callback(
      _registrar: *mut PluginRegistrar,
      descriptor: *const PluginDescriptor,
      vtable: *const PluginVTable,
  ) -> AbiError {
      if descriptor.is_null() || vtable.is_null() {
          return AbiError {
              code: 1,
              message: StringView::null(),
          };
      }
      // SAFETY: descriptor and vtable are valid for this call (ABI contract).
      let desc: &PluginDescriptor = unsafe { &*descriptor };
      // SAFETY: vtable is valid for this call (ABI contract).
      let vt: &PluginVTable = unsafe { &*vtable };
      // SAFETY: contract_name.ptr points to valid UTF-8 bytes for contract_name.len bytes.
      let contract_name: &str = unsafe {
          let bytes: &[u8] =
              core::slice::from_raw_parts(desc.contract_name.ptr, desc.contract_name.len);
          core::str::from_utf8_unchecked(bytes)
      };
      // SAFETY: vtable pointer is 'static — extracted from a Lua VM that outlives registry.
      let result: Result<PluginHandle, RegistryError> = LUA_REGISTRY.with(|reg_cell| {
          let registry: core::cell::Ref<'_, Registry> = reg_cell.borrow();
          unsafe { registry.register(*desc, vtable, contract_name.to_owned(), vt.contract_id) }
      });
      match result {
          Ok(_) => AbiError {
              code: ABI_OK,
              message: StringView::null(),
          },
          Err(_) => AbiError {
              code: 1,
              message: StringView::null(),
          },
      }
  }

  fn make_loader() -> LuaLoader {
      LuaLoader::new(LuaConfig::default())
  }

  fn load_fixture() -> Result<(), PolyplugError> {
      let loader: LuaLoader = make_loader();
      LUA_REGISTRY.with(|cell| {
          *cell.borrow_mut() = Registry::new();
      });
      let mut registrar: PluginRegistrar = PluginRegistrar {
          register_plugin: registry_register_callback,
          host: core::ptr::null(),
      };
      loader.load(std::path::Path::new(LUA_PLUGIN), &mut registrar)
  }

  fn get_vtable() -> *const PluginVTable {
      let contract_id: u64 = polyplug::abi::contract_id("test.add", 1);
      let handle: PluginHandle = LUA_REGISTRY.with(|cell| {
          cell.borrow()
              .find(contract_id, 0)
              .expect("test.add must be registered after load_fixture()")
      });
      LUA_REGISTRY.with(|cell| cell.borrow().resolve(handle).expect("handle must be valid"))
  }

  #[test]
  fn integration_lua_runtime_name() {
      let loader: LuaLoader = LuaLoader::new(LuaConfig::default());
      assert_eq!(loader.runtime_name(), "lua");
  }

  #[test]
  fn integration_lua_bundle_loads() {
      let result: Result<(), PolyplugError> = load_fixture();
      assert!(
          result.is_ok(),
          "LuaLoader::load() must succeed for fixture: {:?}",
          result.err()
      );
  }

  #[test]
  fn integration_lua_add() {
      load_fixture().expect("fixture must load");
      let vtable_ptr: *const PluginVTable = get_vtable();
      // SAFETY: vtable_ptr is valid; the Lua VM stays alive for process lifetime.
      let vtable: &PluginVTable = unsafe { &*vtable_ptr };
      assert!(
          vtable.function_count >= 1,
          "test.add vtable must have at least 1 function"
      );
      let args: AddArgs = AddArgs { a: 3, b: 5 };
      let mut out: u32 = 0_u32;
      // SAFETY: fn_ptr is function 0 (add). args/out are correctly typed for the add function.
      let fn_ptr: *const () = unsafe { *vtable.functions.add(0) };
      let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
          // SAFETY: cast to generic dispatch signature; arg types enforced by test (AddArgs matches).
          unsafe { core::mem::transmute(fn_ptr) };
      // SAFETY: args is a valid AddArgs, out is a valid u32.
      let result: AbiError = unsafe {
          dispatch_fn(
              &args as *const AddArgs as *const (),
              &mut out as *mut u32 as *mut (),
          )
      };
      assert_eq!(result.code, ABI_OK, "add must return ABI_OK");
      assert_eq!(out, 8_u32, "add(3, 5) must equal 8");
  }

  #[test]
  fn integration_lua_add_primitive() {
      load_fixture().expect("fixture must load");
      let vtable_ptr: *const PluginVTable = get_vtable();
      // SAFETY: vtable_ptr is valid.
      let vtable: &PluginVTable = unsafe { &*vtable_ptr };
      assert!(
          vtable.function_count >= 2,
          "test.add vtable must have at least 2 functions"
      );
      let args: AddArgs = AddArgs { a: 10, b: 20 };
      let mut out: u32 = 0_u32;
      // SAFETY: fn_ptr is function 1 (add_primitive). args/out are correctly typed.
      let fn_ptr: *const () = unsafe { *vtable.functions.add(1) };
      let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
          // SAFETY: same dispatch signature as add; arg types enforced by test.
          unsafe { core::mem::transmute(fn_ptr) };
      // SAFETY: args and out are valid and correctly typed.
      let result: AbiError = unsafe {
          dispatch_fn(
              &args as *const AddArgs as *const (),
              &mut out as *mut u32 as *mut (),
          )
      };
      assert_eq!(result.code, ABI_OK, "add_primitive must return ABI_OK");
      assert_eq!(out, 30_u32, "add_primitive(10, 20) must equal 30");
  }

  #[test]
  fn integration_lua_version_string() {
      load_fixture().expect("fixture must load");
      let vtable_ptr: *const PluginVTable = get_vtable();
      // SAFETY: vtable_ptr valid.
      let vtable: &PluginVTable = unsafe { &*vtable_ptr };
      assert!(
          vtable.function_count >= 3,
          "test.add vtable must have at least 3 functions"
      );
      let mut out_view: StringView = StringView::null();
      // SAFETY: fn_ptr is function 2 (version). No arg input needed; pass null.
      let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
      let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
          // SAFETY: same dispatch signature; version takes no args (null input accepted by Lua side).
          unsafe { core::mem::transmute(fn_ptr) };
      // SAFETY: out_view is a valid StringView allocation on the stack.
      let result: AbiError = unsafe {
          dispatch_fn(
              core::ptr::null::<()>(),
              &mut out_view as *mut StringView as *mut (),
          )
      };
      assert_eq!(result.code, ABI_OK, "version must return ABI_OK");
      // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes.
      let version_bytes: &[u8] =
          unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
      let version_str: &str =
          core::str::from_utf8(version_bytes).expect("version must be UTF-8");
      assert_eq!(version_str, "1.0.0-lua", "unexpected version string");
  }

  #[test]
  fn integration_lua_reset() {
      load_fixture().expect("fixture must load");
      let vtable_ptr: *const PluginVTable = get_vtable();
      // SAFETY: vtable_ptr valid.
      let vtable: &PluginVTable = unsafe { &*vtable_ptr };
      assert!(
          vtable.function_count >= 4,
          "test.add vtable must have at least 4 functions"
      );
      // reset() takes no args and produces no output.
      let fn_ptr: *const () = unsafe { *vtable.functions.add(3) };
      let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
          // SAFETY: same dispatch signature; reset has void args and void out.
          unsafe { core::mem::transmute(fn_ptr) };
      // SAFETY: both null — reset does not read args or write output.
      let result: AbiError = unsafe {
          dispatch_fn(core::ptr::null::<()>(), core::ptr::null_mut::<()>())
      };
      assert_eq!(result.code, ABI_OK, "reset must return ABI_OK");
  }

  #[test]
  fn integration_lua_init_function_missing_returns_typed_error() {
      // Write a temp Lua file without polyplug_init.
      let tmp_path: std::path::PathBuf = std::env::temp_dir().join("noinit_test.lua");
      std::fs::write(&tmp_path, b"local x = 1\n").expect("write temp file");

      let loader: LuaLoader = make_loader();
      let mut registrar: PluginRegistrar = PluginRegistrar {
          register_plugin: registry_register_callback,
          host: core::ptr::null(),
      };
      let result: Result<(), PolyplugError> = loader.load(&tmp_path, &mut registrar);
      assert!(result.is_err());
      let err: PolyplugError = result.unwrap_err();
      assert!(
          matches!(
              err,
              PolyplugError::Loader(LoaderError::LuaInitFunctionMissing { .. })
          ),
          "expected LuaInitFunctionMissing, got: {:?}",
          err
      );
  }

  #[test]
  fn integration_lua_utf8_roundtrip() {
      load_fixture().expect("fixture must load");
      let vtable_ptr: *const PluginVTable = get_vtable();
      // SAFETY: vtable_ptr valid.
      let vtable: &PluginVTable = unsafe { &*vtable_ptr };
      let fn_ptr: *const () = unsafe { *vtable.functions.add(2) };
      let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError =
          unsafe { core::mem::transmute(fn_ptr) };
      let mut out_view: StringView = StringView::null();
      // SAFETY: out_view is valid stack allocation.
      let result: AbiError = unsafe {
          dispatch_fn(
              core::ptr::null::<()>(),
              &mut out_view as *mut StringView as *mut (),
          )
      };
      assert_eq!(result.code, ABI_OK);
      // SAFETY: out_view.ptr points to valid UTF-8 bytes for out_view.len bytes.
      let version_bytes: &[u8] =
          unsafe { core::slice::from_raw_parts(out_view.ptr, out_view.len) };
      let version_str: &str =
          core::str::from_utf8(version_bytes).expect("version must be UTF-8");
      assert!(version_str.is_ascii(), "version string is not ASCII: {}", version_str);
      assert_eq!(version_str.as_bytes(), b"1.0.0-lua");
  }

  #[test]
  fn integration_lua_second_load_does_not_panic() {
      // Loading the same plugin twice must not panic (ffi.cdef pcall guard).
      load_fixture().expect("first load");
      let loader: LuaLoader = make_loader();
      let mut registrar2: PluginRegistrar = PluginRegistrar {
          register_plugin: registry_register_callback,
          host: core::ptr::null(),
      };
      let result: Result<(), PolyplugError> =
          loader.load(std::path::Path::new(LUA_PLUGIN), &mut registrar2);
      assert!(result.is_ok(), "second load failed: {:?}", result.err());
  }
  ```

  **Must NOT do**:
  - Do not use `use` inside test functions — all imports at file top (AGENTS.md §2)
  - Do not use `PluginRegistrar::new()` — it does not exist; construct manually as `PluginRegistrar { register_plugin: ..., host: core::ptr::null() }`
  - Do not use high-level `registrar.call_*()` helpers — they do not exist; dispatch via vtable transmute directly
  - Do not add `use` inside functions or impl blocks

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Must mirror Python integration test structure exactly; requires understanding of the full test contract
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T8)
  - **Parallel Group**: Wave 3
  - **Blocks**: F1–F4
  - **Blocked By**: T4 (LuaLoader), T5 (build.rs env vars + Cargo.toml [[test]] entry), T6 (fixture)

  **References**:

  **Pattern References**:
  - `tests/integration_python/mod.rs` — **MIRROR EXACTLY**: `registry_register_callback`, `load_fixture()`, `get_vtable()`, `core::mem::transmute` vtable dispatch, `std::thread_local!` registry, `ABI_OK` assertion pattern
  - `crates/polyplug/src/abi/mod.rs` — `PluginRegistrar` struct (repr(C), has `register_plugin` + `host` fields — no `::new()` method)
  - `crates/polyplug/Cargo.toml` — `[[test]] integration_python` entry (T5 adds the Lua equivalent)

  **Acceptance Criteria**:
  - [ ] `cargo test --test integration_lua` exits 0
  - [ ] All 9 tests pass: `integration_lua_runtime_name`, `integration_lua_bundle_loads`, `integration_lua_add`, `integration_lua_add_primitive`, `integration_lua_version_string`, `integration_lua_reset`, `integration_lua_init_function_missing_returns_typed_error`, `integration_lua_utf8_roundtrip`, `integration_lua_second_load_does_not_panic`
  - [ ] `cargo clippy --test integration_lua -- -D warnings` exits 0

  **QA Scenarios**:
  ```
  Scenario: All integration tests pass
    Tool: Bash
    Steps:
      1. Run: cargo test --test integration_lua -- --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: "test result: ok. 9 passed" in output
    Expected Result: All 9 tests pass
    Evidence: .sisyphus/evidence/task-7-test-output.txt

  Scenario: runtime_name is "lua"
    Tool: Bash
    Steps:
      1. Run: cargo test --test integration_lua integration_lua_runtime_name -- --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: "ok" in output
    Expected Result: runtime_name() returns "lua"
    Evidence: .sisyphus/evidence/task-7-runtime-name.txt

  Scenario: init_function_missing returns typed error
    Tool: Bash
    Steps:
      1. Run: cargo test --test integration_lua integration_lua_init_function_missing_returns_typed_error -- --nocapture 2>&1
      2. Assert: exit code 0
      3. Assert: "ok" in output
    Expected Result: LuaInitFunctionMissing variant returned, test passes
    Evidence: .sisyphus/evidence/task-7-error-type.txt
  ```

  **Commit**: YES (standalone)
  - Message: `feat(tests): add integration_lua test suite`
  - Files: `tests/integration_lua/mod.rs`
  - Pre-commit: `cargo test --test integration_lua`

---

- [ ] 8. Implement full `LuaGenerator` in `crates/polyplugc/src/generators/lua/mod.rs`

  **What to do**:
  - Open `crates/polyplugc/src/generators/lua/mod.rs` (currently a stub)
  - Replace the stub with a full implementation of the `CodeGenerator` trait for Lua

  **CRITICAL: Use the EXACT trait signatures from `crates/polyplugc/src/generators/mod.rs`:**
  ```rust
  pub(crate) trait CodeGenerator {
      fn generate_host(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError>;
      fn generate_guest(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError>;
      fn language_name(&self) -> &'static str;
  }
  ```
  - There is **NO** `generate_bundle` method on this trait. Do NOT add one.
  - There is **NO** `out_dir: &Path` parameter. Files are collected into `GeneratedFiles`, the caller writes them.
  - There is **NO** `GeneratorError` type. The error type is `CodegenError` from `crate::error::CodegenError`.
  - There is **NO** `ContractIR`. The input type is `ValidatedIr` from `crate::ir::ValidatedIr`.

  **The generator must**:
  1. Implement `generate_host(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError>`
     - Push a `GeneratedFile { path: PathBuf::from("host/types.lua"), content: ... }` — ffi.cdef for all contract types, pcall-guarded
     - Push a `GeneratedFile { path: PathBuf::from("host/callers.lua"), content: ... }` — caller functions that dispatch to plugin vtables
  2. Implement `generate_guest(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError>`
     - Push `GeneratedFile { path: PathBuf::from("guest/types.lua"), content: ... }` — same cdef as host/types.lua
     - Push `GeneratedFile { path: PathBuf::from("guest/contracts.lua"), content: ... }` — stub implementations + registration helper
  3. Implement `language_name(&self) -> &'static str` returning `"lua"`

  **`ValidatedIr` shape** (from `crates/polyplugc/src/ir/mod.rs`):
  ```rust
  pub(crate) struct ValidatedIr {
      pub types: Vec<ResolvedType>,      // user-defined structs
      pub contracts: Vec<ResolvedContract>,
      pub bundle: Option<ResolvedBundle>,
  }
  pub(crate) struct ResolvedContract {
      pub name: String,
      pub contract_id: u64,
      pub version: Version,
      pub functions: Vec<ResolvedFunction>,
  }
  pub(crate) struct ResolvedFunction {
      pub name: String,
      pub function_id: u32,
      pub params: Vec<ResolvedParam>,
      pub returns: Option<ResolvedTypeRef>,
  }
  ```

  **`GeneratedFiles` shape** (from `crates/polyplugc/src/generators/mod.rs`):
  ```rust
  pub(crate) struct GeneratedFile { pub path: PathBuf, pub content: String }
  pub(crate) struct GeneratedFiles { pub files: Vec<GeneratedFile> }
  // Push to collection: files.files.push(GeneratedFile { path: PathBuf::from("host/types.lua"), content })
  // Do NOT call std::fs::write() inside the generator — caller handles writing
  ```

  **Type mapping** (Rust IR type → LuaJIT FFI C type):
  - `u8` → `uint8_t`, `u16` → `uint16_t`, `u32` → `uint32_t`, `u64` → `uint64_t`
  - `i8` → `int8_t`, `i16` → `int16_t`, `i32` → `int32_t`, `i64` → `int64_t`
  - `f32` → `float`, `f64` → `double`, `bool` → `bool`
  - `StringView` → `StringView` (declared in polyplug_guest), `Buffer` → `Buffer` (declared in polyplug_guest)
  - User-defined struct `Foo` → `Foo` (declared via ffi.cdef in types.lua)
  - **Reuse `PrimitiveType` enum's `.cpp_name()` method** — Lua uses identical C type names to C++

  **Header comment** for all generated files:
  ```lua
  -- THIS FILE IS AUTO-GENERATED BY polyplugc
  -- DO NOT EDIT BY HAND
  -- Re-generate with: polyplugc generate --api <api.toml> --lang lua --out <dir>
  ```

  **ffi.cdef guard pattern** (use in all generated files):
  ```lua
  local function cdef_guarded(decl)
      local ok, err = pcall(ffi.cdef, decl)
      if not ok and not string.find(err, "already defined", 1, true) then
          error(err, 2)
      end
  end
  ```

  **Rust source structure for LuaGenerator**:
  ```rust
  // crates/polyplugc/src/generators/lua/mod.rs
  // All use statements at file top — NEVER inside functions
  use crate::error::CodegenError;
  use crate::generators::CodeGenerator;
  use crate::generators::GeneratedFile;
  use crate::generators::GeneratedFiles;
  use crate::ir::ValidatedIr;
  use std::path::PathBuf;

  pub(crate) struct LuaGenerator;

  impl CodeGenerator for LuaGenerator {
      fn language_name(&self) -> &'static str { "lua" }

      fn generate_host(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError> {
          let types_src: String = gen_types_lua(ir);
          let callers_src: String = gen_callers_lua(ir);
          files.files.push(GeneratedFile { path: PathBuf::from("host/types.lua"), content: types_src });
          files.files.push(GeneratedFile { path: PathBuf::from("host/callers.lua"), content: callers_src });
          Ok(())
      }

      fn generate_guest(&self, ir: &ValidatedIr, files: &mut GeneratedFiles) -> Result<(), CodegenError> {
          let types_src: String = gen_types_lua(ir);
          let contracts_src: String = gen_contracts_lua(ir);
          files.files.push(GeneratedFile { path: PathBuf::from("guest/types.lua"), content: types_src });
          files.files.push(GeneratedFile { path: PathBuf::from("guest/contracts.lua"), content: contracts_src });
          Ok(())
      }
  }

  // Private helpers — explicit return types required (AGENTS.md §3)
  fn gen_types_lua(ir: &ValidatedIr) -> String { ... }
  fn gen_callers_lua(ir: &ValidatedIr) -> String { ... }
  fn gen_contracts_lua(ir: &ValidatedIr) -> String { ... }
  fn lua_ffi_type(primitive: &crate::ir::PrimitiveType) -> &'static str { ... }  // use .cpp_name() or map directly
  fn file_header() -> &'static str {
      "-- THIS FILE IS AUTO-GENERATED BY polyplugc\n-- DO NOT EDIT BY HAND\n"
  }
  ```

  - All Rust helper functions must have explicit return types (AGENTS.md §3)
  - String building: use `String::push_str` or `format!` — never `unwrap()` on string ops
  - **DO NOT call `std::fs::write()` inside the generator** — push to `files.files` only, the caller writes to disk
  - **DO NOT create output subdirs** — caller handles filesystem, generator just pushes `GeneratedFile`s

  **Must NOT do**:
  - Do not use `.unwrap()` or `.expect()` in production codegen logic
  - Do not add a `generate_bundle` method — it is NOT part of the `CodeGenerator` trait
  - Do not use `out_dir: &Path` parameter — it does not exist; push `GeneratedFile`s to `files.files`
  - Do not use `GeneratorError` — the error type is `CodegenError`
  - Do not use `ContractIR` or `BundleManifest` as input — the input is `ValidatedIr`
  - Do not call `std::fs::write()` inside the generator — the caller handles disk writes
  - Do not create bare `helper.rs` files — helpers go in the same `mod.rs` as private functions
  - Do not emit `ffi.cdef` outside the pcall guard pattern in generated Lua
  - Do not hardcode `contract_id` values — read from `ir.contracts[i].contract_id`

  **Recommended Agent Profile**:
  - **Category**: `deep`
    - Reason: Requires thorough understanding of the full Python generator (778 lines) as a reference, careful type mapping, and multiple output file generation
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with T7)
  - **Parallel Group**: Wave 3
  - **Blocks**: T9
  - **Blocked By**: None (generator is independent of loader; needs only `CodeGenerator` trait from T0 context)

  **References**:

  **Pattern References**:
  - `crates/polyplugc/src/generators/python/mod.rs` — full Python generator; mirror `generate_host`/`generate_guest` structure, use of `ValidatedIr`, `GeneratedFiles` push pattern, and type-mapping helpers
  - `crates/polyplugc/src/generators/mod.rs` — **READ THIS FIRST**: `CodeGenerator` trait has exactly 3 methods: `generate_host`, `generate_guest`, `language_name`. No `generate_bundle`. `ValidatedIr` is the input. `GeneratedFiles` collects output.
  - `crates/polyplugc/src/ir/mod.rs` — `ValidatedIr`, `ResolvedContract`, `ResolvedFunction`, `ResolvedType`, `PrimitiveType` (has `.cpp_name()` for C type strings)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplugc` exits 0
  - [ ] `cargo clippy -p polyplugc -- -D warnings` exits 0
  - [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/lua-out` exits 0 and produces `host/types.lua`, `host/callers.lua`, `guest/types.lua`, `guest/contracts.lua`
  - [ ] Generated files begin with `-- THIS FILE IS AUTO-GENERATED BY polyplugc`
  - [ ] `language_name()` returns `"lua"`

  **QA Scenarios**:
  ```
  Scenario: LuaGenerator produces host and guest files
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplugc 2>&1
      2. Assert: exit code 0
      3. Run: ./target/debug/polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/lua-out 2>&1
      4. Assert: exit code 0
      5. Run: ls /tmp/lua-out/host/ /tmp/lua-out/guest/ 2>&1
      6. Assert: types.lua and callers.lua in host/; types.lua and contracts.lua in guest/
    Expected Result: All 4 files generated
    Evidence: .sisyphus/evidence/task-8-gen-output.txt

  Scenario: Generated files have correct header comment
    Tool: Bash
    Steps:
      1. Run: head -3 /tmp/lua-out/host/types.lua 2>&1
      2. Assert: first line is "-- THIS FILE IS AUTO-GENERATED BY polyplugc"
    Expected Result: Header present
    Evidence: .sisyphus/evidence/task-8-header.txt

  Scenario: language_name returns lua (compilation check)
    Tool: Bash
    Steps:
      1. Run: cargo clippy -p polyplugc -- -D warnings 2>&1
      2. Assert: exit code 0
      3. Assert: no "error[" lines
    Expected Result: Clean compilation and lint pass
    Evidence: .sisyphus/evidence/task-8-clippy.txt
  ```

  **Commit**: YES (standalone)
  - Message: `feat(polyplugc): implement LuaGenerator for host/guest code generation`
  - Files: `crates/polyplugc/src/generators/lua/mod.rs`
  - Pre-commit: `cargo build -p polyplugc && cargo clippy -p polyplugc -- -D warnings`

---

- [ ] 9. Wire `LuaGenerator` into `crates/polyplugc/src/main.rs`

  **What to do**:
  - Open `crates/polyplugc/src/main.rs`
  - Find the `match lang.as_str()` block (inside `fn run()`, which returns `Result<(), CodegenError>`)
  - Add the `"lua"` arm **before** the `other =>` catch-all arm:
    ```rust
    "lua" => Box::new(generators::lua::LuaGenerator),
    ```
  - Update the error message string in the `other =>` arm to include `lua`.
    The `other =>` arm currently is:
    ```rust
    other => {
        return Err(CodegenError::ValidationFailed {
            message: format!(
                "Unknown language: `{other}`. Supported: rust, cpp, csharp, python"
            ),
        });
    }
    ```
    Change `"rust, cpp, csharp, python"` to `"rust, cpp, csharp, python, lua"` — that is the ONLY
    change in the `other =>` arm. Do NOT change it to `eprintln!` + `exit(1)`.
  - The final `other =>` arm must look like:
    ```rust
    other => {
        return Err(CodegenError::ValidationFailed {
            message: format!(
                "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua"
            ),
        });
    }
    ```
  - Do NOT modify any other part of `main.rs`

  **Must NOT do**:
  - Do not reorder existing arms
  - Do not add `use` inside the match arm
  - Do not add any other logic

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single match arm addition, trivial change
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (sequential, T8 must complete first)
  - **Blocks**: F1–F4
  - **Blocked By**: T8 (LuaGenerator must exist before it can be dispatched)

  **References**:

  **Pattern References**:
  - `crates/polyplugc/src/main.rs` — existing `"python"` arm; add `"lua"` arm in the same style
  - `crates/polyplugc/src/generators/lua/mod.rs` (T8) — `LuaGenerator` struct name

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplugc` exits 0
  - [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/t9-test` exits 0
  - [ ] `polyplugc generate --api tests/fixtures/test_api.toml --lang unknown` outputs error message containing "lua"

  **QA Scenarios**:
  ```
  Scenario: --lang lua is dispatched correctly
    Tool: Bash
    Steps:
      1. Run: cargo build -p polyplugc && ./target/debug/polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/t9-test 2>&1
      2. Assert: exit code 0
      3. Run: ls /tmp/t9-test/host/ /tmp/t9-test/guest/ 2>&1
      4. Assert: generated files exist
    Expected Result: Lua codegen dispatched and files produced
    Evidence: .sisyphus/evidence/task-9-dispatch.txt

  Scenario: Unknown lang shows updated error with lua listed
    Tool: Bash
    Steps:
      1. Run: ./target/debug/polyplugc generate --api tests/fixtures/test_api.toml --lang unknown 2>&1 || true
      2. Assert: output contains "lua"
    Expected Result: Error message lists all supported languages including lua
    Evidence: .sisyphus/evidence/task-9-error-msg.txt
  ```

  **Commit**: YES (standalone)
  - Message: `feat(polyplugc): wire LuaGenerator into CLI dispatch`
  - Files: `crates/polyplugc/src/main.rs`
  - Pre-commit: `cargo build -p polyplugc`

---

## Final Verification Wave (after ALL implementation tasks)

> 4 review agents run in PARALLEL. ALL must APPROVE. Rejection → fix → re-run.

- [ ] F1. **Plan Compliance Audit** — `oracle`

  Read the plan end-to-end. For each "Must Have":
  - `LuaLoader` implements `BundleLoader` — grep for `impl BundleLoader for LuaLoader` in `crates/polyplug-lua/src/lib/loader/mod.rs`
  - `polyplug_guest.lua` works as require-able module — run `luajit` test or verify T3 QA evidence
  - 64-bit pointer round-trip — check that loader passes `i64` and guest uses `ffi.cast("uintptr_t", ...)`
  - All integration tests pass — run `cargo test --test integration_lua`
  - All error paths typed — grep for `LoaderError::Lua` variants; grep for `.unwrap()` (must be zero in non-test code)
  - Module structure uses `mod.rs` — grep for bare `.rs` module roots in `crates/polyplug-lua/src/`

  For each "Must NOT Have":
  - No bare `filename.rs` roots — `find crates/polyplug-lua/src -name "*.rs" ! -name "mod.rs"` must return empty
  - No `.unwrap()` in production — grep for `\.unwrap()` in `crates/polyplug-lua/src/` (zero matches outside `#[cfg(test)]`)
  - No C host lib — `ls host-libs/lua/` must not exist
  - No `ffi.cdef` on hot path — grep for `ffi.cdef` in `test_plugin.lua` (must be zero; only in `polyplug_guest.lua`)

  Check evidence files exist in `.sisyphus/evidence/`.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | Tasks [9/9] | VERDICT: APPROVE/REJECT`

- [ ] F2. **Code Quality Review** — `unspecified-high`

  Run:
  - `cargo clippy --workspace -- -D warnings` → must exit 0
  - `cargo fmt --check` → must exit 0
  - `cargo test --workspace` → all tests pass

  Review all new/modified files for:
  - `as any` or `#[allow(...)]` pragmas that mask real issues
  - Empty `catch` / `_` catch-all error matches swallowing errors silently
  - `println!` / `dbg!` left in production code
  - Commented-out code blocks
  - Unused imports
  - AI slop: excessive comments restating code, over-abstraction, generic names (`data`, `result`, `item`, `temp`)
  - `use` inside functions (AGENTS.md violation)
  - Missing explicit type annotations (AGENTS.md violation)

  Output: `Build [PASS/FAIL] | Fmt [PASS/FAIL] | Tests [N pass/N fail] | Files [N clean/N issues] | VERDICT`

- [ ] F3. **Real Integration QA** — `unspecified-high`

  Start from clean state (`cargo clean`). Execute EVERY QA scenario from EVERY task:
  - T1: compile polyplug, clippy
  - T2: compile polyplug-lua
  - T3: double-require test for polyplug_guest.lua
  - T4: compile polyplug-lua, clippy
  - T5: compile polyplug, verify env vars baked in
  - T6: verify fixture files exist, content
  - T7: `cargo test --test integration_lua -- --nocapture` (all 8 pass)
  - T8: codegen produces all 4 files, header comment present
  - T9: `--lang lua` dispatched, `--lang unknown` shows lua in error

  Save all outputs to `.sisyphus/evidence/final-qa/`.
  Output: `Scenarios [N/N pass] | Integration [8/8] | Edge Cases [N tested] | VERDICT`

- [ ] F4. **Scope Fidelity Check** — `deep`

  For each task, read "What to do" and then `git diff HEAD~N` to verify:
  - Everything in spec was built (no missing functions/files)
  - Nothing beyond spec was built (no scope creep)
  - "Must NOT do" compliance verified
  - Cross-task contamination check (T1 only touches `error/mod.rs`, T2 only touches `config/mod.rs`, etc.)
  - Unaccounted changes flagged

  Output: `Tasks [9/9 compliant] | Contamination [CLEAN/N issues] | Unaccounted [CLEAN/N files] | VERDICT`

---

## Commit Strategy

| Commit | Message | Files |
|--------|---------|-------|
| 1 | `feat(error): add lua loader error variants` | `crates/polyplug/src/error/mod.rs`, `crates/polyplug-lua/src/lib/config/mod.rs`, `crates/polyplug-lua/src/lib/mod.rs` |
| 2 | `feat(guest-lua): add polyplug_guest.lua FFI guest library` | `guest-libs/lua/polyplug_guest.lua` |
| 3 | `feat(polyplug-lua): implement LuaLoader with mlua/LuaJIT` | `crates/polyplug-lua/src/lib/loader/mod.rs`, `crates/polyplug-lua/src/lib/mod.rs`, `crates/polyplug-lua/Cargo.toml` |
| 4 | `feat(build): emit TEST_LUA_PLUGIN and POLYPLUG_GUEST_LUA_DIR env vars` | `crates/polyplug/build.rs`, `crates/polyplug/Cargo.toml`, `tests/fixtures/test_plugin.lua`, `tests/fixtures/test_plugin.manifest.toml` |
| 5 | `feat(tests): add integration_lua test suite` | `tests/integration_lua/mod.rs` |
| 6 | `feat(polyplugc): implement LuaGenerator for host/guest code generation` | `crates/polyplugc/src/generators/lua/mod.rs` |
| 7 | `feat(polyplugc): wire LuaGenerator into CLI dispatch` | `crates/polyplugc/src/main.rs` |

---

## Success Criteria

### Verification Commands
```bash
cargo clippy --workspace -- -D warnings   # Expected: exit 0
cargo fmt --check                         # Expected: exit 0
cargo test --workspace                    # Expected: all pass
cargo test --test integration_lua -- --nocapture  # Expected: 8 passed
./target/debug/polyplugc generate --api tests/fixtures/test_api.toml --lang lua --out /tmp/lua-out  # Expected: exit 0
```

### Final Checklist
- [ ] `LuaLoader` implements `BundleLoader` trait
- [ ] All 8 integration_lua tests pass
- [ ] LuaGenerator produces valid Lua files for all 4 output targets
- [ ] `--lang lua` dispatch works end-to-end in polyplugc CLI
- [ ] No `.unwrap()` in production code
- [ ] No bare `filename.rs` module roots in polyplug-lua
- [ ] No ABI struct modifications
- [ ] No C host lib created
- [ ] `cargo clippy --workspace -- -D warnings` exits 0
- [ ] `cargo fmt --check` exits 0
