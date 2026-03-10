# Epic 11.6 — Patch: Fix JS Architecture

## TL;DR

> **Quick Summary**: Gut the wrong N-API/subprocess polyplug-js implementation and replace it with correct in-process JS embedding — QuickJS via rquickjs (polyplug-js) and V8 via deno_core + tokio (polyplug-js-deno). Create the polyplug-js-deno crate from scratch.
>
> **Deliverables**:
> - `crates/polyplug-js/` — completely rewritten, QuickJS in-process, `runtime_name() = "js-quickjs"`
> - `crates/polyplug-js-deno/` — new crate, V8 in-process via deno_core + tokio, `runtime_name() = "js-deno"`
> - `crates/polyplugc/src/generators/js_quickjs/` and `js_deno/` — correct generator stubs
> - `guest-libs/js/polyplug-guest.ts` — rewritten with lo/hi u32 types, no N-API bigint
> - `tests/integration_js/mod.rs` — rewritten for js-quickjs + js-deno
> - Error variants updated, workspace cleaned
>
> **Estimated Effort**: Large
> **Parallel Execution**: YES — 5 waves
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 5 → Task 6 → Final wave

---

## Context

### Original Request

Epic 11.5 implemented polyplug-js using Node.js subprocess spawning and N-API/libloading to load compiled `.node` files. This is fundamentally wrong. Subprocesses create process boundaries; raw pointers cannot cross them. Any pointer-passing scheme across processes violates polyplug's core design of zero overhead, in-process only.

Epic 11.6 replaces the entire JS adapter implementation with the correct in-process embedding model.

### Architecture Pre-Answers (from epics.md §11.6 + §11.5)

**polyplug-js (QuickJS)**:
- One shared `rquickjs::Runtime` per process in `OnceLock<Mutex<Runtime>>`
- `HostVTable*` in `OnceLock<*const HostVTable>` — set from `registrar.host` on first `load()` call
- 8 wrapper functions on `polyplug` JS global with lo/hi u32 split for all u64 values
- `registerVtable(contract_lo, contract_hi, vtable_lo, vtable_hi)` — vtable ptr split as lo/hi
- After `ctx.eval(bundle_js)`: extract registered vtable, call `(registrar.register_plugin)(...)`
- `bundle.js` read from the path passed to `load()` + `/bundle.js`

**polyplug-js-deno (V8)**:
- One `std::thread::spawn` per bundle; V8 isolate is `!Send` (thread-pinned)
- `tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(...)` inside spawned thread  
  ⚠️ **NOT smol** — deno_core 0.311.0 is built on tokio; smol will cause runtime panics
- `HostVTable*` in `thread_local! Cell<*const HostVTable>` — set before JsRuntime creation
- 8 `#[op2(fast)]` ops in `deno_core::extension!`
- vtable received via `op_register_vtable` → sent to `load()` caller via `std::sync::mpsc` oneshot pattern
- Thread parks on `mpsc::Receiver` after load for vtable call dispatch (same trampoline pattern as Lua)
- Load `bundle.js` (or `index.ts`) via `runtime.load_main_es_module()`

**Trampoline pattern**: Follow Lua loader exactly — `make_trampoline!` macro, `TRAMPOLINES[64]` static array, `FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<...>>>>`, `dispatch_*_call(slot, args, out)` function.

### Research Findings (Metis review)

- **Contradiction resolved**: `deno_core 0.311.0` requires tokio, not smol. Use `tokio = { version = "1", features = ["rt"] }` inside the spawned thread only. Do NOT add tokio to workspace-level deps.
- **`make_registrar_context` is `pub(crate)`** in polyplug — JS loaders cannot call it. Loader receives a pre-built `&mut PluginRegistrar` and calls `(registrar.register_plugin)(registrar, &descriptor, vtable_ptr)` directly (same as Lua loader).
- **rquickjs feature flag needed**: `features = ["parallel"]` for `Send + Sync` on `Runtime`.
- **Lua loader is the gold reference** — study `crates/polyplug-lua/src/lib/loader/mod.rs` before implementing.
- **build.rs**: Section to delete is lines 502–562 of `crates/polyplug/build.rs` (the `// ─── ts-node fixture ───` section).
- **integration_js tests are wired through `crates/polyplug/Cargo.toml`** via `[[test]]` entry and `dev-dependencies`.

---

## Work Objectives

### Core Objective

Replace the wrong N-API/subprocess polyplug-js with correct in-process QuickJS and V8 adapters, matching the architecture of the existing Lua adapter.

### Concrete Deliverables

- `crates/polyplug-js/` — complete QuickJS in-process loader, `JsConfig {}`, `JsLoader`
- `crates/polyplug-js-deno/` — complete V8 in-process loader, `JsDenoConfig {}`, `JsDenoLoader`
- `crates/polyplugc/src/generators/js_quickjs/mod.rs` — working JsQuickjsGenerator (types.ts, contracts.ts, vtable.ts, init.ts, manifest.toml, README.md)
- `crates/polyplugc/src/generators/js_deno/mod.rs` — rewritten JsDenoGenerator (same structure, BigInt, Deno.core.ops API)
- `guest-libs/js/polyplug-guest.ts` — AbiError, StringView/Buffer with lo/hi ptrs, DependencyNotFoundError, EXT_TRACE_ID, TraceVTable
- `tests/integration_js/mod.rs` — loader tests, runtime name tests, no broken imports
- All old files deleted, all old error variants removed

### Definition of Done

- [ ] `cargo clippy --workspace -- -D warnings` — zero warnings
- [ ] `cargo test --workspace` — all tests pass (non-ignored)
- [ ] `grep -r "ts-node\|js-node\|ts-bun\|js-bun\|JsNodeNotFound\|JsBinaryNotConfigured\|JsConfigEmpty\|RuntimeNotImplemented" crates/ tests/ --include="*.rs"` — zero matches
- [ ] `JsLoader::new(JsConfig{}).runtime_name() == "js-quickjs"` — verified by test
- [ ] `JsDenoLoader::new(JsDenoConfig{}).runtime_name() == "js-deno"` — verified by test
- [ ] `test ! -d tests/fixtures/test_plugin_ts_node` — true
- [ ] `test ! -f tests/fixtures/test_plugin_ts_node.node` — true
- [ ] `test ! -f host-libs/js/polyplug.ts` — true

### Must Have

- polyplug-js uses rquickjs in-process, no subprocess, no IPC
- polyplug-js-deno uses deno_core + tokio in-process, no subprocess
- All 8 HostVTable wrapper functions accessible from JS in both variants
- u64 lo/hi split in js-quickjs, BigInt in js-deno
- EXT_TRACE_ID = 0xC4EB9AEE in polyplug-guest.ts
- `cargo test --workspace` passes after changes

### Must NOT Have (Guardrails)

- No subprocess code (std::process::Command to node/bun/deno) anywhere in JS crates
- No N-API, no `.node` files, no libloading in polyplug-js
- No ts-node, ts-bun, ts-deno, js-node, js-bun identifiers in any source file
- No `.unwrap()` or `.expect()` in production code (workspace lint: deny)
- No `use` inside function/impl bodies (AGENTS.md §2)
- No bare `filename.rs` as module roots (AGENTS.md §1)
- No tokio in workspace-level Cargo.toml — tokio only in polyplug-js-deno's own Cargo.toml
- No modification to frozen ABI types in `crates/polyplug/src/abi/`
- No modification to `crates/polyplug/src/loader/mod.rs` BundleLoader trait
- No modification to `crates/polyplug/src/runtime/mod.rs`

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (cargo test)
- **Automated tests**: Tests after (not TDD for this patch)
- **Framework**: cargo test (Rust built-in)

### QA Policy
Every task includes agent-executed QA scenarios. Evidence saved to `.sisyphus/evidence/task-{N}-{slug}.txt`.

- **Library/Loader**: Use Bash — `cargo test`, grep for forbidden patterns, check file existence
- **CLI/Tooling**: Use Bash — run polyplugc subcommands, check exit codes, verify output files

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 1 (Start Immediately — cleanup + foundation, all independent):
├── Task 1: Workspace cleanup — delete wrong files, fix Cargo.toml members, fix build.rs [quick]

Wave 2 (After Wave 1 — error variants + configs, all independent):
├── Task 2: Update error/mod.rs — remove old JS variants, add new ones [quick]
├── Task 3: Rewrite polyplug-js — QuickJS in-process loader [unspecified-high]
├── Task 4: Create polyplug-js-deno — V8 in-process loader [unspecified-high]

Wave 3 (After Wave 2 — wiring + generators):
├── Task 5: Update crates/polyplug/Cargo.toml dev-deps and rewrite integration_js tests [quick]
├── Task 6: Rewrite polyplugc generators (js_quickjs, js_deno, mod.rs, main.rs) [unspecified-high]
├── Task 7: Rewrite guest-libs/js/polyplug-guest.ts [quick]

Wave 4 (After Wave 3 — verification):
├── Task 8: Compile check + clippy pass + cargo test --workspace [unspecified-high]

Wave FINAL (After ALL tasks):
├── Task F1: Stale reference audit [deep]
├── Task F2: Scope fidelity check [deep]
```

### Dependency Matrix

- **1**: none → blocks 2, 3, 4
- **2**: 1 → blocks 3, 4 (new error variants needed)
- **3**: 1, 2 → blocks 5
- **4**: 1, 2 → blocks 5
- **5**: 3, 4 → blocks 8
- **6**: 1 → blocks 8
- **7**: 1 → blocks 8
- **8**: 5, 6, 7 → blocks F1, F2

---

## TODOs

---

## Final Verification Wave

- [ ] F1. **Stale Reference Audit** — `deep`
  Run: `grep -r "ts-node\|js-node\|ts-bun\|js-bun\|NodeConfig\|BunConfig\|DenoConfig\|JsNodeGenerator\|JsBunGenerator" crates/ tests/ --include="*.rs" --include="*.toml"`. Must be zero matches. Run: `grep -r "JsNodeNotFound\|JsNodeVersionTooOld\|JsBinaryNotConfigured\|JsInitRaisedError\|JsConfigEmpty\|RuntimeNotImplemented" crates/ tests/ --include="*.rs"`. Must be zero matches. Report file:line for any findings.
  Output: `CLEAN or [N] stale references: [list]`

- [ ] F2. **Scope Fidelity Check** — `deep`
  Read plan Must Have and Must NOT Have. For each item: verify codebase state matches. Check no modification was made to `crates/polyplug/src/abi/`, `crates/polyplug/src/loader/mod.rs`, `crates/polyplug/src/runtime/mod.rs`. Check deleted files are gone. Check new files exist. Run `cargo clippy --workspace -- -D warnings` and `cargo test --workspace`.
  Output: `Must Have [N/N] | Must NOT Have [N/N] | VERDICT: APPROVE/REJECT`

---

## Commit Strategy

- After Task 1: `chore(workspace): remove test_plugin_ts_node fixture and cleanup`
- After Task 2: `fix(error): remove old JS error variants, add JsRuntimePanic and RolldownNotFound`
- After Tasks 3+4: `feat(polyplug-js): implement QuickJS in-process loader; feat(polyplug-js-deno): implement V8 in-process loader`
- After Tasks 5-7: `fix(tests): rewrite integration_js for js-quickjs and js-deno; fix(generators): add js_quickjs, rewrite js_deno`
- After Task 8 passes: `chore: Epic 11.6 patch complete — fix JS architecture`

- [x] 1. Workspace Cleanup — Delete Wrong Files, Fix Cargo.toml, Fix build.rs

  **What to do**:

  **Delete these files/directories entirely** (use `rm -rf` or the delete tool):
  - `crates/polyplug-js/src/lib/loader/node/` (entire directory)
  - `crates/polyplug-js/src/lib/loader/bun/` (entire directory)
  - `crates/polyplug-js/src/lib/loader/deno/` (entire directory)
  - `crates/polyplug-js/build.rs`
  - `host-libs/js/polyplug.ts` (then delete `host-libs/js/` if empty)
  - `tests/fixtures/test_plugin_ts_node/` (entire directory)
  - `tests/fixtures/test_plugin_ts_node.node`
  - `crates/polyplugc/src/generators/js_node/` (entire directory)
  - `crates/polyplugc/src/generators/js_bun/` (entire directory)

  **Edit `Cargo.toml` (workspace root)** — line 3:
  - Remove `"tests/fixtures/test_plugin_ts_node"` from the `members` array
  - `crates/polyplug-js-deno` will be covered automatically by the `crates/*` glob

  **Edit `crates/polyplug/build.rs`** — delete lines 502–562:
  - Lines 502–562 are the `// ─── ts-node fixture ───` section
  - Delete from `// ─── ts-node fixture ─────` through the final closing `}`
  - The file should end at line 501 (after the Lua fixture section)
  - Do NOT touch lines 1–501 (test_plugin, memory_plugin, error_plugin, cpp plugins, csharp, python, lua builds)

  **Stub out `crates/polyplug-js/src/lib/loader/mod.rs`** temporarily:
  - After deleting node/bun/deno subdirectories, the loader/mod.rs still `pub(crate) mod node;` etc.
  - Replace the entire file with a minimal stub that compiles:
    ```rust
    //! JsLoader — placeholder until QuickJS impl (Task 3).
    use std::path::Path;
    use polyplug::abi::PluginRegistrar;
    use polyplug::error::PolyplugError;
    use polyplug::loader::BundleLoader;
    use crate::config::JsConfig;

    pub struct JsLoader {
        _config: JsConfig,
    }
    impl JsLoader {
        pub fn new(config: JsConfig) -> JsLoader {
            JsLoader { _config: config }
        }
    }
    impl BundleLoader for JsLoader {
        fn runtime_name(&self) -> &'static str { "js-quickjs" }
        fn load(&self, _path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
            todo!("QuickJS loader not yet implemented")
        }
    }
    ```

  **Run `cargo check -p polyplug-js`** — must compile after these changes.

  **Must NOT do**:
  - Do not touch `crates/polyplug/src/abi/`, `crates/polyplug/src/loader/mod.rs`, or `crates/polyplug/src/runtime/mod.rs`
  - Do not touch any test fixture other than test_plugin_ts_node
  - Do not remove any lines from build.rs above line 502

  **Recommended Agent Profile**:
  > File deletion and targeted edits across 3 files.
  - **Category**: `quick`
    - Reason: File deletions and small targeted edits — no logic to reason about
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 1 (must run first)
  - **Blocks**: Tasks 2, 3, 4, 6, 7
  - **Blocked By**: None (can start immediately)

  **References**:
  - `Cargo.toml` line 3 — workspace members list
  - `crates/polyplug/build.rs` lines 502–562 — the ts-node build section to delete
  - `crates/polyplug-lua/src/lib/loader/mod.rs` — pattern reference for how loader/mod.rs should look

  **Acceptance Criteria**:
  - [ ] `test ! -d crates/polyplug-js/src/lib/loader/node && echo PASS` → PASS
  - [ ] `test ! -d crates/polyplug-js/src/lib/loader/bun && echo PASS` → PASS
  - [ ] `test ! -d crates/polyplug-js/src/lib/loader/deno && echo PASS` → PASS
  - [ ] `test ! -f crates/polyplug-js/build.rs && echo PASS` → PASS
  - [ ] `test ! -f host-libs/js/polyplug.ts && echo PASS` → PASS
  - [ ] `test ! -d tests/fixtures/test_plugin_ts_node && echo PASS` → PASS
  - [ ] `test ! -f tests/fixtures/test_plugin_ts_node.node && echo PASS` → PASS
  - [ ] `test ! -d crates/polyplugc/src/generators/js_node && echo PASS` → PASS
  - [ ] `test ! -d crates/polyplugc/src/generators/js_bun && echo PASS` → PASS
  - [ ] `grep 'test_plugin_ts_node' Cargo.toml` → exit code 1 (no match)
  - [ ] `wc -l crates/polyplug/build.rs` → 501 lines
  - [ ] `cargo check -p polyplug-js 2>&1 | grep -c '^error'` → 0

  **QA Scenarios**:

  ```
  Scenario: File deletion verified
    Tool: Bash
    Steps:
      1. for f in node bun deno; do test ! -d crates/polyplug-js/src/lib/loader/$f && echo "$f DELETED" || echo "$f STILL EXISTS"; done
      2. test ! -f host-libs/js/polyplug.ts && echo "host-libs/js/polyplug.ts DELETED" || echo "STILL EXISTS"
      3. test ! -d tests/fixtures/test_plugin_ts_node && echo "fixture dir DELETED" || echo "STILL EXISTS"
      4. test ! -f tests/fixtures/test_plugin_ts_node.node && echo ".node DELETED" || echo "STILL EXISTS"
    Expected Result: All 5 lines say DELETED
    Evidence: .sisyphus/evidence/task-1-deletions.txt

  Scenario: workspace still compiles after cleanup
    Tool: Bash
    Steps:
      1. cargo check -p polyplug 2>&1
      2. cargo check -p polyplug-js 2>&1
      3. cargo check -p polyplugc 2>&1
    Expected Result: Exit code 0 for all three, no error lines
    Evidence: .sisyphus/evidence/task-1-cargo-check.txt
  ```

  **Commit**: YES — `chore(workspace): remove test_plugin_ts_node fixture and cleanup`

---

- [x] 2. Update `crates/polyplug/src/error/mod.rs` — Remove Old JS Variants, Add New Ones

  **What to do**:

  **REMOVE** these variants from `LoaderError` enum:
  - `RuntimeNotImplemented { runtime_name: String }` — lines 84–88
  - `JsNodeNotFound` — lines 128–129
  - `JsNodeVersionTooOld { found: String, required: String }` — lines 131–132
  - `JsBinaryNotConfigured { runtime_name, field_name, install_hint }` — lines 134–142
  - `JsInitRaisedError { bundle: String, message: String }` — lines 144–145
  - `JsConfigEmpty` — lines 147–148

  **Before removing**: grep for each variant name across the workspace to ensure zero references outside the files being deleted:
  ```
  grep -r 'RuntimeNotImplemented\|JsNodeNotFound\|JsNodeVersionTooOld\|JsBinaryNotConfigured\|JsInitRaisedError\|JsConfigEmpty' crates/ tests/ --include='*.rs'
  ```
  Expected: matches only in `crates/polyplug-js/src/lib/loader/mod.rs` (already stubbed in Task 1) and `tests/integration_js/mod.rs` (already broken — will be rewritten in Task 5).
  If other files reference these variants, fix them before removing.

  **ADD** these two variants to `LoaderError` after the existing Lua variants (after `LuaInitRaisedError`):
  ```rust
  #[error(
      "rolldown not found on PATH — js-quickjs pack requires rolldown. {hint}"
  )]
  RolldownNotFound { hint: String },

  #[error("JS runtime \"{runtime}\" panicked during bundle load: {message}")]
  JsRuntimePanic { runtime: String, message: String },
  ```

  **Run `cargo check --workspace`** — must compile after this change.

  **Must NOT do**:
  - Do not touch any other variant or any other error enum
  - Do not touch RegistryError, GraphError, AllocatorError, RuntimeError

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Targeted edit to a single file
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Task 3 and Task 4 (but Task 3 and 4 depend on this task)
  - **Parallel Group**: Wave 2 (after Task 1)
  - **Blocks**: Tasks 3, 4
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplug/src/error/mod.rs` — the complete file (198 lines)
  - `crates/polyplug-lua/src/lib/loader/mod.rs` — reference for how Lua variants are named (pattern: `LuaVmInitFailed`, `LuaScriptLoadFailed`, etc.)

  **Acceptance Criteria**:
  - [ ] `grep 'RuntimeNotImplemented\|JsNodeNotFound\|JsConfigEmpty\|JsBinaryNotConfigured\|JsInitRaisedError' crates/polyplug/src/error/mod.rs` → exit code 1 (no match)
  - [ ] `grep 'RolldownNotFound\|JsRuntimePanic' crates/polyplug/src/error/mod.rs` → matches found (both variants present)
  - [ ] `cargo check --workspace 2>&1 | grep -c '^error'` → 0

  **QA Scenarios**:

  ```
  Scenario: Old variants removed, new variants present
    Tool: Bash
    Steps:
      1. grep -c 'RuntimeNotImplemented\|JsNodeNotFound\|JsConfigEmpty' crates/polyplug/src/error/mod.rs
      2. grep -c 'RolldownNotFound' crates/polyplug/src/error/mod.rs
      3. grep -c 'JsRuntimePanic' crates/polyplug/src/error/mod.rs
    Expected Result: Step 1 returns 0; steps 2 and 3 return ≥1
    Evidence: .sisyphus/evidence/task-2-error-variants.txt

  Scenario: Workspace still compiles
    Tool: Bash
    Steps:
      1. cargo check --workspace 2>&1
    Expected Result: Exit code 0, no error lines
    Evidence: .sisyphus/evidence/task-2-cargo-check.txt
  ```

  **Commit**: YES — `fix(error): remove old JS error variants, add RolldownNotFound and JsRuntimePanic`
---

- [x] 3. Rewrite `crates/polyplug-js/` — QuickJS In-Process Loader

  **What to do**:

  Follow the Lua loader in `crates/polyplug-lua/src/lib/loader/mod.rs` as structural gold reference.

  **Step 3.1 — Rewrite `crates/polyplug-js/Cargo.toml`**:
  Replace `libloading` with `rquickjs = { version = "0.11.0", features = ["parallel"] }`. Keep `thiserror` and `polyplug`. Remove `build = "build.rs"` if present.
  ```toml
  [package]
  name             = "polyplug-js"
  version          = "0.1.0"
  edition.workspace     = true
  license.workspace     = true
  rust-version.workspace = true

  [lib]
  name       = "polyplug_js"
  path       = "src/lib/mod.rs"
  crate-type = ["rlib"]

  [dependencies]
  polyplug   = { path = "../polyplug" }
  rquickjs   = { version = "0.11.0", features = ["parallel"] }
  thiserror  = { workspace = true }

  [lints]
  workspace = true
  ```

  **Step 3.2 — Rewrite `crates/polyplug-js/src/lib/config/mod.rs`**:
  ```rust
  //! JsConfig — configuration for the QuickJS JS adapter.
  //!
  //! No fields — QuickJS is fully embedded, no system dependencies.

  /// Configuration for the QuickJS JavaScript plugin loader.
  ///
  /// No fields required — QuickJS is embedded in-process via rquickjs.
  ///
  /// # Example
  /// ```rust,ignore
  /// use polyplug_js::{JsConfig, JsLoader};
  /// let loader = JsLoader::new(JsConfig {});
  /// ```
  #[derive(Debug, Clone)]
  pub struct JsConfig {}
  ```

  **Step 3.3 — Rewrite `crates/polyplug-js/src/lib/loader/mod.rs`** (the core implementation):

  Full structure with these components:

  **(a) Global state** (at file top, after imports):
  ```rust
  // One shared QuickJS runtime per process (rquickjs parallel feature makes Runtime: Send+Sync)
  static QJS_RUNTIME: OnceLock<Mutex<rquickjs::Runtime>> = OnceLock::new();

  // Lazy-init guard for the runtime (double-checked locking)
  static QJS_RUNTIME_INIT: Mutex<()> = Mutex::new(());

  // HostVTable* stored once — valid for 'static (Box::leak'd by RuntimeBuilder)
  static HOST_VTABLE: OnceLock<*const HostVTable> = OnceLock::new();

  // Thread-safe wrapper for the raw pointer
  struct HostVtablePtr(*const HostVTable);
  // SAFETY: HostVTable* points to 'static data (Box::leak). Only read after set.
  unsafe impl Send for HostVtablePtr {}
  unsafe impl Sync for HostVtablePtr {}
  ```

  **(b) VTable registration state** (per-load, passed into JS closure via RefCell):
  ```rust
  // Stores vtable ptr received from plugin's registerVtable() call during ctx.eval()
  struct VtableRegistration {
      contract_id: u64,
      vtable_ptr: *const PluginVTable,  // 'static from plugin binary
  }
  // SAFETY: Only accessed synchronously within a single ctx.eval() call.
  unsafe impl Send for VtableRegistration {}
  ```

  **(c) Trampoline machinery** (64 static extern "C" trampolines, same as Lua):
  ```rust
  static FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<rquickjs::Persistent<rquickjs::Function>>>>> =
      OnceLock::new();

  fn function_registry() -> &'static Mutex<...> { FUNCTION_REGISTRY.get_or_init(|| ...) }

  fn dispatch_quickjs_call(slot: usize, args_ptr: *const (), out_ptr: *mut ()) -> AbiError { ... }

  macro_rules! make_trampoline { ... }  // same pattern as Lua
  make_trampoline!(trampoline_0, 0); ... make_trampoline!(trampoline_63, 63);
  static TRAMPOLINES: [...] = [trampoline_0, ..., trampoline_63];
  const MAX_TRAMPOLINES: usize = 64;
  ```

  **(d) `JsLoader` struct and `BundleLoader` impl**:
  ```rust
  pub struct JsLoader { _config: JsConfig }

  impl JsLoader {
      pub fn new(config: JsConfig) -> JsLoader { JsLoader { _config: config } }
  }

  impl BundleLoader for JsLoader {
      fn runtime_name(&self) -> &'static str { "js-quickjs" }

      fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
          // 1. Set HOST_VTABLE once from registrar.host
          // SAFETY: registrar.host is a Box::leak'd HostVTable — valid 'static.
          let _ = HOST_VTABLE.get_or_init(|| registrar.host);

          // 2. Read bundle.js
          let bundle_js_path: PathBuf = path.join("bundle.js");
          let bundle_js: String = std::fs::read_to_string(&bundle_js_path).map_err(|e| { ... })?;

          // 3. Ensure QuickJS runtime initialized
          let runtime: &Mutex<rquickjs::Runtime> = ensure_runtime();
          let rt_guard = runtime.lock().unwrap_or_else(|e| e.into_inner());

          // 4. Create fresh Context for this bundle
          let ctx: rquickjs::Context = rquickjs::Context::full(&rt_guard).map_err(|e| { ... })?;

          // 5. Shared cell to receive vtable from registerVtable callback
          let registered: std::cell::RefCell<Option<VtableRegistration>> = RefCell::new(None);

          // 6. Register polyplug global + eval bundle
          let eval_result: Result<(), PolyplugError> = ctx.with(|ctx_ref| {
              let globals = ctx_ref.globals();
              let polyplug_obj = rquickjs::Object::new(ctx_ref.clone()).map_err(...)?
              register_host_functions(&ctx_ref, &polyplug_obj, &registered)?;
              globals.set("polyplug", polyplug_obj).map_err(...)?
              ctx_ref.eval::<rquickjs::Value, _>(bundle_js.as_str()).map_err(...)?
              Ok(())
          });
          eval_result?;

          // 7. Extract registered vtable
          let reg_borrow = registered.borrow();
          let reg: &VtableRegistration = reg_borrow.as_ref().ok_or_else(|| { ... })?;

          // 8. Allocate function slot range in TRAMPOLINES for this bundle
          // SAFETY: vtable_ptr is 'static from the JS plugin bundle (never unloaded)
          let vtable_ref: &PluginVTable = unsafe { &*reg.vtable_ptr };
          let fn_count: usize = vtable_ref.function_count as usize;
          let slot_base: usize = allocate_slots(fn_count)?;

          // 9. Register each JS function in FUNCTION_REGISTRY at slot_base..slot_base+fn_count
          // (JS plugin must call polyplug.registerFunction(fn_index, jsFunction) OR
          //  store functions in a JS-accessible array — design: polyplug.registerVtable
          //  receives the contract_id + vtable_ptr where vtable.functions is a raw array
          //  of (ptr_lo, ptr_hi) pairs pointing to host-alloc'd PluginVTable)

          // 10. Build PluginVTable with TRAMPOLINES[slot_base..]
          let trampoline_ptrs: Vec<*const ()> = (slot_base..slot_base+fn_count)
              .map(|s| TRAMPOLINES[s] as *const ())
              .collect();
          // Leak the Vec<*const ()> — it must outlive the runtime
          let fn_array: &'static [*const ()] = Box::leak(trampoline_ptrs.into_boxed_slice());

          let vtable_box: Box<PluginVTable> = Box::new(PluginVTable {
              contract_id: reg.contract_id,
              contract_version: vtable_ref.contract_version,
              function_count: fn_count as u32,
              functions: fn_array.as_ptr() as *const *const (),
          });
          let static_vtable: &'static PluginVTable = Box::leak(vtable_box);

          // 11. Build descriptor (name from contract_id)
          let contract_name_bytes: Vec<u8> = format!("js_contract_{:#x}", reg.contract_id).into_bytes();
          let contract_name_static: &'static [u8] = Box::leak(contract_name_bytes.into_boxed_slice());
          let descriptor: PluginDescriptor = PluginDescriptor {
              name: StringView::from_static(b"js-quickjs-plugin"),
              contract_name: StringView { ptr: contract_name_static.as_ptr(), len: contract_name_static.len() },
              version_major: vtable_ref.contract_version >> 16,
              version_minor: vtable_ref.contract_version & 0xFFFF,
              version_patch: 0,
          };

          // 12. Register with host
          // SAFETY: registrar, descriptor, and static_vtable are all valid for this call.
          let abi_result: AbiError = unsafe {
              (registrar.register_plugin)(registrar as *mut PluginRegistrar, &descriptor, static_vtable)
          };
          if abi_result.code != ABI_OK {
              return Err(PolyplugError::Loader(LoaderError::JsRuntimePanic { ... }));
          }
          Ok(())
      }
  }
  ```

  **(e) `register_host_functions` helper** — the 8 HostVTable wrappers on the JS `polyplug` object:

  All u64 values split into lo/hi u32. Use `rquickjs::Function::new(ctx, closure)`. Each closure reads `HOST_VTABLE`.

  NOTE: `rquickjs::Function::new` requires the closure to be `'static`. Since `HOST_VTABLE` is a `OnceLock` static, reading it in a `'static` closure is safe.

  The 8 functions:

  1. **`findByContract(lo: u32, hi: u32, min_ver: u32)`** → `{index, generation} | null`
     - `contract_id = (hi as u64) << 32 | (lo as u64)`
     - Calls `((*vtable).find_by_contract)(contract_id, min_ver)`
     - Returns null if `handle.is_null()`, else `Object{index, generation}`

  2. **`findByBundle(blo: u32, bhi: u32, clo: u32, chi: u32, min_ver: u32)`** → `{index, generation} | null`
     - Calls `((*vtable).find_by_bundle)(bundle_id, contract_id, min_ver)`

  3. **`findAllByContract(lo: u32, hi: u32, min_ver: u32)`** → `Array<{index, generation}>`
     - Uses a fixed 64-element stack buffer `[PluginHandle; 64]`
     - Calls `((*vtable).find_all_by_contract)(contract_id, min_ver, buf.as_mut_ptr(), 64)`
     - Returns JS array of `{index, generation}` objects

  4. **`resolvePlugin(index: u32, generation: u32)`** → `u32 | null` (guard token)
     - Calls `((*vtable).resolve_plugin)(PluginHandle{index, generation})`
     - If result is null ptr: return JS null
     - Otherwise: store in global slab, return slab index as u32

  5. **`getExtension(extension_id: u32)`** → `{lo: u32, hi: u32} | null`
     - Calls `((*vtable).get_extension)(extension_id)`
     - Splits returned `*const ()` into lo/hi u32
     - Returns null if ptr is null

  6. **`registerVtable(contract_lo: u32, contract_hi: u32, vtable_lo: u32, vtable_hi: u32)`** → void
     - Reassembles contract_id and vtable_ptr from lo/hi pairs
     - Stores `VtableRegistration { contract_id, vtable_ptr }` in the `registered` RefCell
     - The `registered` RefCell is passed in via the closure — use `Arc<RefCell<...>>` to share with the closure since the closure must be `'static` OR use thread-local storage for the registration state
     - **Design decision**: Use a `thread_local! { static PENDING_VTABLE: RefCell<Option<VtableRegistration>> }` set before `ctx.eval()` and cleared after. This avoids lifetime issues with closure captures.

  7. **`alloc(size: u32)`** → `{lo: u32, hi: u32}`
     - Calls `((*vtable).alloc)(size as usize, 8)`
     - Returns lo/hi split of returned pointer

  8. **`free(lo: u32, hi: u32)`** → void
     - Calls `((*vtable).free)((hi as usize) << 32 | lo as usize) as *mut u8, 0, 8)`

  **SAFETY comment required on each HostVTable access**:
  ```rust
  // SAFETY: HOST_VTABLE is set from Box::leak'd HostVTable before any JS runs.
  // OnceLock ensures single-writer, all-reader semantics. The pointer is 'static.
  let vtable: *const HostVTable = *HOST_VTABLE.get()
      .ok_or_else(|| PolyplugError::Loader(LoaderError::JsRuntimePanic { ... }))?;
  ```

  **Step 3.4 — Rewrite `crates/polyplug-js/src/lib/mod.rs`**:
  ```rust
  //! polyplug-js — QuickJS in-process JS adapter for polyplug.
  //!
  //! Implements BundleLoader for js-quickjs plugin bundles.
  //! One shared QuickJS VM per process. No subprocess. No IPC.

  pub mod config;
  pub(crate) mod loader;

  pub use config::JsConfig;
  pub use loader::JsLoader;
  ```

  **Must NOT do**:
  - No subprocess code (`std::process::Command`)
  - No `libloading` usage
  - No `unsafe` block without `// SAFETY:` comment
  - No `.unwrap()` or `.expect()` (workspace deny lint)
  - No `use` inside function bodies

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Complex Rust code with unsafe, OnceLock, and rquickjs API — requires careful implementation
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Task 4 (independent crates)
  - **Parallel Group**: Wave 2 (with Task 4, after Tasks 1+2)
  - **Blocks**: Task 5
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug-lua/src/lib/loader/mod.rs` — Gold reference: trampoline pattern, global registry, BundleLoader impl (517 lines)
  - `crates/polyplug/src/abi/mod.rs` — ABI types: HostVTable, PluginVTable, PluginDescriptor, PluginRegistrar, PluginHandle, StringView
  - `crates/polyplug/src/error/mod.rs` — Error variants: `JsRuntimePanic`, `RolldownNotFound`, `LoadFailed`, `ManifestParse`
  - `rquickjs 0.11.0` docs: `Runtime::new()`, `Context::full()`, `ctx.with()`, `ctx.globals()`, `Object::new()`, `Function::new()`, `ctx.eval::<Value, _>()`
  - `epics.md` lines 1705–1727 — QuickJS loading model spec

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug-js 2>&1 | grep -c '^error'` → 0
  - [ ] `cargo clippy -p polyplug-js -- -D warnings 2>&1 | grep -c '^error'` → 0
  - [ ] `JsLoader::new(JsConfig{}).runtime_name()` returns `"js-quickjs"` — verified by unit test in loader/mod.rs
  - [ ] No `unsafe` block without `// SAFETY:` comment in new code

  **QA Scenarios**:

  ```
  Scenario: polyplug-js compiles and clippy passes
    Tool: Bash
    Steps:
      1. cargo build -p polyplug-js 2>&1 | tail -3
      2. cargo clippy -p polyplug-js -- -D warnings 2>&1 | grep '^error' || echo CLEAN
    Expected Result: Build succeeds; clippy shows CLEAN
    Evidence: .sisyphus/evidence/task-3-build.txt

  Scenario: runtime_name returns js-quickjs
    Tool: Bash
    Steps:
      1. cargo test -p polyplug-js 2>&1
    Expected Result: Any runtime_name test passes; exit code 0
    Evidence: .sisyphus/evidence/task-3-unit-test.txt

  Scenario: No forbidden patterns in new code
    Tool: Bash
    Steps:
      1. grep -n 'subprocess\|\.node\|ts-node\|js-node\|\.unwrap()' crates/polyplug-js/src/ -r
    Expected Result: Zero matches
    Evidence: .sisyphus/evidence/task-3-forbidden-check.txt
  ```

  **Commit**: NO (groups with Task 4)

---

- [x] 4. Create `crates/polyplug-js-deno/` — V8 In-Process Loader (New Crate)

  **What to do**:

  Follow the same structural pattern as Task 3 / Lua loader. Create the crate from scratch.

  **Step 4.1 — Create directory structure**:
  ```
  crates/polyplug-js-deno/
  crates/polyplug-js-deno/Cargo.toml
  crates/polyplug-js-deno/src/
  crates/polyplug-js-deno/src/lib/
  crates/polyplug-js-deno/src/lib/mod.rs
  crates/polyplug-js-deno/src/lib/config/
  crates/polyplug-js-deno/src/lib/config/mod.rs
  crates/polyplug-js-deno/src/lib/loader/
  crates/polyplug-js-deno/src/lib/loader/mod.rs
  ```

  **Step 4.2 — `crates/polyplug-js-deno/Cargo.toml`**:
  ```toml
  [package]
  name             = "polyplug-js-deno"
  version          = "0.1.0"
  edition.workspace     = true
  license.workspace     = true
  rust-version.workspace = true

  [lib]
  name       = "polyplug_js_deno"
  path       = "src/lib/mod.rs"
  crate-type = ["rlib"]

  [dependencies]
  polyplug     = { path = "../polyplug" }
  deno_core    = "0.311.0"
  tokio        = { version = "1", features = ["rt"] }
  thiserror    = { workspace = true }

  [lints]
  workspace = true
  ```

  ⚠️ **tokio is ONLY in polyplug-js-deno's own Cargo.toml** — NOT in workspace deps.

  **Step 4.3 — `src/lib/config/mod.rs`**:
  ```rust
  //! JsDenoConfig — configuration for the V8/deno_core JS adapter.

  /// Configuration for the V8/deno_core JavaScript plugin loader.
  ///
  /// No fields required — V8 is embedded in-process via deno_core.
  #[derive(Debug, Clone)]
  pub struct JsDenoConfig {}
  ```

  **Step 4.4 — `src/lib/loader/mod.rs`** (the core implementation):

  **(a) Global state and channel types**:
  ```rust
  // HostVTable* thread-local — set before JsRuntime creation on the bundle thread
  thread_local! {
      static DENO_HOST_VTABLE: core::cell::Cell<*const HostVTable> =
          const { core::cell::Cell::new(core::ptr::null()) };
  }
  // SAFETY: DENO_HOST_VTABLE is only accessed from the bundle's dedicated thread,
  // which is pinned to a single V8 isolate. thread_local ensures thread isolation.
  ```

  **(b) Trampoline machinery** — same `make_trampoline!` pattern as Lua/QuickJS:
  ```rust
  static DENO_FUNCTION_REGISTRY: OnceLock<Mutex<Vec<Option<DenoFunctionSlot>>>> = OnceLock::new();

  struct DenoFunctionSlot {
      call_tx: std::sync::mpsc::SyncSender<JsCallRequest>,
      fn_index: u32,
  }

  pub(crate) struct JsCallRequest {
      pub fn_name: String,        // JS function name to call
      pub args_ptr: usize,        // *const () as usize
      pub out_ptr: usize,         // *mut () as usize
      pub result_tx: std::sync::mpsc::SyncSender<AbiError>,
  }

  fn dispatch_deno_call(slot: usize, args_ptr: *const (), out_ptr: *mut ()) -> AbiError {
      // Get SyncSender from DENO_FUNCTION_REGISTRY[slot]
      // Send JsCallRequest
      // Block on result_rx.recv()
  }

  macro_rules! make_trampoline { ... }  // same pattern
  make_trampoline!(trampoline_0, 0); ... make_trampoline!(trampoline_63, 63);
  static TRAMPOLINES: [...] = [trampoline_0, ..., trampoline_63];
  ```

  **(c) The 8 deno_core ops**:
  ```rust
  use deno_core::{op2, extension};

  #[op2(fast)]
  fn op_find_by_contract(contract_id: u64, min_ver: u32) -> u64 {
      // SAFETY: DENO_HOST_VTABLE set before JsRuntime creation on this thread.
      // V8 isolate is thread-pinned — ops always run on the same thread.
      let vtable: *const HostVTable = DENO_HOST_VTABLE.with(|c| c.get());
      if vtable.is_null() { return u64::MAX; }  // null handle encoded as u64::MAX
      let handle: PluginHandle = unsafe { ((*vtable).find_by_contract)(contract_id, min_ver) };
      if handle.is_null() { return u64::MAX; }
      (handle.generation as u64) << 32 | handle.index as u64
  }

  // op_find_by_bundle(bundle_id: u64, contract_id: u64, min_ver: u32) -> u64
  // op_find_all_by_contract(contract_id: u64, min_ver: u32) -> v8::Local<v8::Array>  [or Vec<u64>]
  // op_resolve_plugin(handle: u64) -> u64  (slab index as u64 or u64::MAX for null)
  // op_get_extension(extension_id: u32) -> u64  (ptr as u64 or 0 for null)
  // op_register_vtable(contract_id: u64, vtable_ptr: u64) -> void
  // op_alloc(size: u32) -> u64  (ptr as u64)
  // op_free(ptr: u64) -> void

  extension!(
      polyplug_ops,
      ops = [
          op_find_by_contract, op_find_by_bundle, op_find_all_by_contract,
          op_resolve_plugin, op_get_extension, op_register_vtable,
          op_alloc, op_free,
      ]
  );
  ```

  **Note on `op_register_vtable`**: needs a way to communicate the vtable back to the `load()` caller. Use a thread-local `SyncSender` set before the JsRuntime is created:
  ```rust
  thread_local! {
      static VTABLE_SENDER: RefCell<Option<std::sync::mpsc::SyncSender<*const PluginVTable>>> =
          RefCell::new(None);
  }
  // Set before JsRuntime::new(), clear after
  ```

  **(d) `JsDenoLoader` struct and `BundleLoader` impl**:
  ```rust
  pub struct JsDenoLoader { _config: JsDenoConfig }

  impl JsDenoLoader {
      pub fn new(config: JsDenoConfig) -> JsDenoLoader { JsDenoLoader { _config: config } }
  }

  impl BundleLoader for JsDenoLoader {
      fn runtime_name(&self) -> &'static str { "js-deno" }

      fn load(&self, path: &Path, registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
          // 1. Capture host vtable ptr (to send to thread)
          let host_vtable_ptr: *const HostVTable = registrar.host;

          // 2. Create channel for vtable registration result (oneshot via bounded SyncChannel(1))
          let (vtable_tx, vtable_rx) =
              std::sync::mpsc::sync_channel::<*const PluginVTable>(1);

          // 3. Create channel for vtable call dispatch (per bundle)
          let (call_tx, call_rx) =
              std::sync::mpsc::sync_channel::<JsCallRequest>(16);

          // 4. Clone path for thread
          let bundle_path: std::path::PathBuf = path.to_owned();

          // 5. Spawn dedicated thread for this bundle's V8 isolate
          let _thread: std::thread::JoinHandle<()> = std::thread::spawn(move || {
              // Set thread-local HostVTable*
              // SAFETY: host_vtable_ptr is Box::leak'd by RuntimeBuilder — valid 'static.
              DENO_HOST_VTABLE.with(|c| c.set(host_vtable_ptr));

              // Set thread-local vtable sender
              VTABLE_SENDER.with(|c| {
                  *c.borrow_mut() = Some(vtable_tx);
              });

              // Build tokio single-thread runtime
              let tokio_rt = tokio::runtime::Builder::new_current_thread()
                  .enable_all()
                  .build()
                  .unwrap_or_else(|e| panic!("failed to build tokio runtime: {e}"));

              tokio_rt.block_on(async move {
                  // Create deno_core JsRuntime with polyplug_ops extension
                  let mut runtime: deno_core::JsRuntime = deno_core::JsRuntime::new(
                      deno_core::RuntimeOptions {
                          extensions: vec![polyplug_ops::init_ops()],
                          ..Default::default()
                      }
                  );

                  // Determine module file: prefer bundle.js, fallback to index.ts
                  let bundle_js: PathBuf = bundle_path.join("bundle.js");
                  let index_ts: PathBuf = bundle_path.join("index.ts");
                  let module_path: PathBuf = if bundle_js.exists() { bundle_js } else { index_ts };

                  let module_url: deno_core::ModuleSpecifier =
                      deno_core::resolve_path(module_path.to_str().unwrap_or(""), ...);

                  let mod_id = runtime.load_main_es_module(&module_url).await
                      .unwrap_or_else(|e| panic!("failed to load module: {e}"));
                  runtime.run_event_loop(Default::default()).await
                      .unwrap_or_else(|e| panic!("event loop failed: {e}"));

                  // op_register_vtable has sent vtable_ptr via VTABLE_SENDER by now
                  // Clean up thread-local sender
                  VTABLE_SENDER.with(|c| { *c.borrow_mut() = None; });

                  // Park on call_rx loop
                  while let Ok(req) = call_rx.recv() {
                      // Execute JS vtable function via deno_core
                      // For each request: call the JS handler fn and return result
                      // Implementation: pass args_ptr and out_ptr as numbers to JS
                      // For now: return ABI_OK (stub, full impl in later iteration)
                      let _ = req.result_tx.send(AbiError::ok());
                  }
              });
          });

          // 6. Receive the registered vtable ptr from the bundle thread
          let raw_vtable: *const PluginVTable = vtable_rx
              .recv_timeout(std::time::Duration::from_secs(30))
              .map_err(|_| PolyplugError::Loader(LoaderError::JsRuntimePanic {
                  runtime: "js-deno".to_owned(),
                  message: "vtable registration timed out after 30s".to_owned(),
              }))?;

          // 7. Same vtable construction as QuickJS (Step 3.3 items 8-12)
          // Allocate trampolines, build static PluginVTable, call registrar.register_plugin
          // ...

          Ok(())
      }
  }
  ```

  **Note on `_thread` handle**: It must be stored somewhere so the thread isn't dropped. Store it in a process-global `OnceLock<Mutex<Vec<JoinHandle<()>>>>` or simply leak it with `Box::leak(Box::new(_thread))` since bundle threads live for the process lifetime.

  **Must NOT do**:
  - No subprocess (std::process::Command)
  - No tokio in workspace deps — only in this crate's Cargo.toml
  - No `.unwrap()` in production code (note: tokio rt build inside a spawned thread panicking is acceptable since the panic is caught by thread boundary — but use `unwrap_or_else(|e| panic!(...))` for explicit messages)
  - No `use` inside function bodies

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: New crate, complex async + threading + deno_core API, trampoline machinery
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Task 3 (independent crates)
  - **Parallel Group**: Wave 2 (with Task 3, after Tasks 1+2)
  - **Blocks**: Task 5
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug-lua/src/lib/loader/mod.rs` — Gold reference: trampoline pattern, dispatch function
  - `crates/polyplug/src/abi/mod.rs` — ABI types
  - `deno_core 0.311.0` — `JsRuntime::new()`, `RuntimeOptions`, `extension!`, `#[op2(fast)]`, `load_main_es_module`, `run_event_loop`
  - `epics.md` lines 1729–1770 — V8 loading model spec

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplug-js-deno 2>&1 | grep -c '^error'` → 0
  - [ ] `cargo clippy -p polyplug-js-deno -- -D warnings 2>&1 | grep '^error' || echo CLEAN` → CLEAN
  - [ ] `JsDenoLoader::new(JsDenoConfig{}).runtime_name()` returns `"js-deno"`
  - [ ] No tokio in workspace Cargo.toml: `grep 'tokio' Cargo.toml` → exit code 1

  **QA Scenarios**:

  ```
  Scenario: polyplug-js-deno compiles and clippy passes
    Tool: Bash
    Steps:
      1. cargo build -p polyplug-js-deno 2>&1 | tail -3
      2. cargo clippy -p polyplug-js-deno -- -D warnings 2>&1 | grep '^error' || echo CLEAN
    Expected Result: Build succeeds; clippy shows CLEAN
    Evidence: .sisyphus/evidence/task-4-build.txt

  Scenario: tokio not in workspace deps
    Tool: Bash
    Steps:
      1. grep 'tokio' Cargo.toml
    Expected Result: Exit code 1 (no match in workspace Cargo.toml)
    Evidence: .sisyphus/evidence/task-4-tokio-check.txt

  Scenario: New crate registered in workspace
    Tool: Bash
    Steps:
      1. cargo metadata --no-deps --format-version 1 | grep 'polyplug-js-deno'
    Expected Result: Match found (crate recognized by workspace)
    Evidence: .sisyphus/evidence/task-4-workspace-check.txt
  ```

  **Commit**: YES (with Task 3) — `feat(polyplug-js): implement QuickJS in-process loader; feat(polyplug-js-deno): implement V8 in-process loader`

---

- [ ] 5. Update `crates/polyplug/Cargo.toml` Dev-Deps and Rewrite `tests/integration_js/mod.rs`

  **What to do**:

  **Step 5.1 — Edit `crates/polyplug/Cargo.toml`**:
  - In `[dev-dependencies]` section: add `polyplug-js-deno = { path = "../../crates/polyplug-js-deno" }`
  - The `polyplug-js` entry already exists: `polyplug-js = { path = "../../crates/polyplug-js" }` (line 76) — keep it

  **Step 5.2 — Rewrite `tests/integration_js/mod.rs`** completely:

  The new file must:
  - Import `polyplug_js::{JsConfig, JsLoader}` and `polyplug_js_deno::{JsDenoConfig, JsDenoLoader}`
  - NOT reference `JsConfig::node_only()`, `BunConfig`, `DenoConfig`, `NodeConfig`
  - NOT have `const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN")`
  - NOT have tests that use `registry_register_callback` directly (this was testing the old .node fixture load path)

  **Tests to include** (all should pass without any JS fixtures):

  ```rust
  #![allow(clippy::expect_used)]

  use polyplug::error::LoaderError;
  use polyplug::error::RuntimeError;
  use polyplug::loader::BundleLoader;
  use polyplug_js::{JsConfig, JsLoader};
  use polyplug_js_deno::{JsDenoConfig, JsDenoLoader};
  #[test]
  fn js_quickjs_loader_runtime_name() {
      let loader: JsLoader = JsLoader::new(JsConfig {});
      assert_eq!(loader.runtime_name(), "js-quickjs");
      assert_eq!(loader.runtime_names(), vec!["js-quickjs".to_owned()]);
  }

  #[test]
  fn js_deno_loader_runtime_name() {
      let loader: JsDenoLoader = JsDenoLoader::new(JsDenoConfig {});
      assert_eq!(loader.runtime_name(), "js-deno");
      assert_eq!(loader.runtime_names(), vec!["js-deno".to_owned()]);
  }

  #[test]
  fn js_quickjs_registered_in_runtime_builder() {
      let result = polyplug::runtime::Runtime::builder()
          .loader(JsLoader::new(JsConfig {}))
          .build();
      assert!(result.is_ok(), "RuntimeBuilder with JsLoader must succeed: {:?}", result.err());
  }

  #[test]
  fn js_deno_registered_in_runtime_builder() {
      let result = polyplug::runtime::Runtime::builder()
          .loader(JsDenoLoader::new(JsDenoConfig {}))
          .build();
      assert!(result.is_ok(), "RuntimeBuilder with JsDenoLoader must succeed: {:?}", result.err());
  }

  #[test]
  fn js_quickjs_duplicate_runtime_name_is_rejected() {
      let result = polyplug::runtime::Runtime::builder()
          .loader(JsLoader::new(JsConfig {}))
          .loader(JsLoader::new(JsConfig {}))
          .build();
      assert!(
          matches!(result, Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))),
          "Duplicate js-quickjs registration must return DuplicateLoader"
      );
  }

  #[test]
  fn js_deno_duplicate_runtime_name_is_rejected() {
      let result = polyplug::runtime::Runtime::builder()
          .loader(JsDenoLoader::new(JsDenoConfig {}))
          .loader(JsDenoLoader::new(JsDenoConfig {}))
          .build();
      assert!(
          matches!(result, Err(RuntimeError::Loader(LoaderError::DuplicateLoader { .. }))),
          "Duplicate js-deno registration must return DuplicateLoader"
      );
  }

  #[test]
  #[ignore = "requires pre-built bundle.js fixture"]
  fn js_quickjs_load_bundle_and_call() {
      // TODO: Implement once a test bundle.js fixture exists
  }

  #[test]
  #[ignore = "requires pre-built bundle.js or index.ts fixture"]
  fn js_deno_load_bundle_and_call() {
      // TODO: Implement once a test bundle fixture exists
  }
  ```

  **Must NOT do**:
  - Do not reference old error variants (`RuntimeNotImplemented`, `JsBinaryNotConfigured`)
  - Do not keep `const JS_PLUGIN: &str = env!("TEST_JS_PLUGIN")`
  - Do not keep `registry_register_callback` from old file

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Simple test file rewrite with known imports and test patterns
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Tasks 6 and 7
  - **Parallel Group**: Wave 3 (after Tasks 3+4)
  - **Blocks**: Task 8
  - **Blocked By**: Tasks 3, 4

  **References**:
  - `crates/polyplug/Cargo.toml` lines 71–80 — dev-dependencies section
  - `tests/integration_lua/mod.rs` — reference for test structure pattern
  - `tests/integration_js/mod.rs` — OLD file to be replaced

  **Acceptance Criteria**:
  - [ ] `cargo test --test integration_js -- js_quickjs_loader_runtime_name` → PASS
  - [ ] `cargo test --test integration_js -- js_deno_loader_runtime_name` → PASS
  - [ ] `cargo test --test integration_js -- js_quickjs_registered_in_runtime_builder` → PASS
  - [ ] `cargo test --test integration_js -- js_deno_registered_in_runtime_builder` → PASS
  - [ ] `grep 'TEST_JS_PLUGIN\|node_only\|BunConfig\|DenoConfig' tests/integration_js/mod.rs` → exit code 1 (no match)

  **QA Scenarios**:

  ```
  Scenario: All non-ignored integration_js tests pass
    Tool: Bash
    Steps:
      1. cargo test --test integration_js 2>&1
    Expected Result: All non-ignored tests pass; exit code 0
    Evidence: .sisyphus/evidence/task-5-integration-tests.txt

  Scenario: No old references in test file
    Tool: Bash
    Steps:
      1. grep -n 'TEST_JS_PLUGIN\|node_only\|BunConfig\|DenoConfig\|JsBinaryNotConfigured\|RuntimeNotImplemented' tests/integration_js/mod.rs
    Expected Result: Zero matches
    Evidence: .sisyphus/evidence/task-5-old-refs.txt
  ```

  **Commit**: NO (groups with Tasks 6+7)

---

- [ ] 6. Rewrite polyplugc Generators (js_quickjs, js_deno, mod.rs, main.rs)

  **What to do**:

  **Step 6.1 — Update `crates/polyplugc/src/generators/mod.rs`**:
  Remove `pub(crate) mod js_node;` and `pub(crate) mod js_bun;`. Add `pub(crate) mod js_quickjs;`.
  ```rust
  pub(crate) mod cpp;
  pub(crate) mod csharp;
  pub(crate) mod js_deno;
  pub(crate) mod js_quickjs;  // NEW
  pub(crate) mod lua;
  pub(crate) mod python;
  pub(crate) mod rust;
  ```

  **Step 6.2 — Create `crates/polyplugc/src/generators/js_quickjs/mod.rs`**:

  Model after `js_node/mod.rs` (the old generator) but produce js-quickjs correct output.

  Generator struct: `pub(crate) struct JsQuickjsGenerator;` (no fields).

  `language_name()`: `"js-quickjs"`

  **`generate_host()`**: Produce `host/types.ts` and `host/callers.ts`.
  - `host/types.ts`: TypeScript interfaces with lo/hi ptr representation for u64/pointer fields.
    - `u8/u16/u32/f32/f64/bool` → `number`/`boolean`
    - `u64/i64` → `{ lo: number; hi: number }` (NOT bigint — QuickJS uses f64)
    - `StringView` → `{ ptr_lo: number; ptr_hi: number; len: number }`
    - `Buffer` → `{ ptr_lo: number; ptr_hi: number; len: number; cap: number }`
    - User-defined structs → TypeScript interfaces
  - `host/callers.ts`: host caller classes with `TODO: implement` stubs (host is always Rust, this file is informational)

  **`generate_guest()`**: Produce `guest/types.ts`, `guest/contracts.ts`, `guest/vtable.ts`, `guest/init.ts`, `manifest.toml`, `README.md`.

  - `guest/types.ts`: same type mapping as above
  - `guest/contracts.ts`: abstract classes per contract (same as old js_node generator)
  - `guest/vtable.ts`: helper that calls `polyplug.registerVtable(contract_lo, contract_hi, vtable_lo, vtable_hi)`
    ```typescript
    // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
    export function registerVtable(
        contractLo: number, contractHi: number,
        vtablePtrLo: number, vtablePtrHi: number
    ): void {
        polyplug.registerVtable(contractLo, contractHi, vtablePtrLo, vtablePtrHi);
    }
    ```
  - `guest/init.ts`: entry point with dependency resolution and vtable registration.
    Template (with generated values substituted by generator logic):
    ```typescript
    // THIS FILE IS AUTO-GENERATED BY polyplugc. DO NOT EDIT.
    // Runtime: js-quickjs
    import { DependencyNotFoundError } from '../polyplug-guest';
    // CONTRACT IDs (lo/hi split)
    const MY_BUNDLE_ID_LO: number = <lo>;
    const MY_BUNDLE_ID_HI: number = <hi>;
    // (for each declared dependency)
    const <CONTRACT>_LO: number = <lo>;
    const <CONTRACT>_HI: number = <hi>;

    function init(): void {
        // For each declared dependency:
        const <dep>Handle = polyplug.findByContract(<CONTRACT>_LO, <CONTRACT>_HI, 1);
        if (!<dep>Handle) throw new DependencyNotFoundError('<contract.name>');

        // Register this plugin's vtable:
        polyplug.registerVtable(<contract_lo>, <contract_hi>, <vtable_lo>, <vtable_hi>);
    }
    init();
    ```
    If no dependencies declared, omit the dependency block.
  - `manifest.toml`: `runtime = "js-quickjs"` plus bundle_name, bundle_id, version
  - `README.md`: rolldown instructions for js-quickjs
    ```markdown
    # js-quickjs Plugin Bundle
    ## Requirements
    - rolldown: `npm i -g rolldown`
    ## Build
    ```bash
    rolldown index.ts --format iife --platform neutral --file bundle.js
    ```
    ```

  **Step 6.3 — Rewrite `crates/polyplugc/src/generators/js_deno/mod.rs`**:

  Same structure as JsQuickjsGenerator but:
  - `pub(crate) struct JsDenoGenerator;` (no fields, no `typescript_mode`)
  - `language_name()`: `"js-deno"`
  - Type mapping: `u64/i64` → `bigint`, `StringView` → `{ ptr: bigint; len: number }`, `Buffer` → `{ ptr: bigint; len: number; cap: number }`
  - `init.ts` uses `Deno.core.ops.op_find_by_contract(CONTRACT_ID, 1)` etc. (BigInt args)
  - `manifest.toml`: `runtime = "js-deno"`
  - `README.md`: deno-specific (TypeScript loaded natively, rolldown optional for npm deps)

  **Step 6.4 — Update `crates/polyplugc/src/main.rs`**:

  **Replace generator dispatch block** (lines 89–120 of current main.rs):
  ```rust
  let generator: Box<dyn generators::CodeGenerator> = match lang.as_str() {
      "rust" => Box::new(generators::rust::RustGenerator),
      "cpp" | "c++" => Box::new(generators::cpp::CppGenerator),
      "csharp" | "c#" => Box::new(generators::csharp::CSharpGenerator),
      "python" | "py" => Box::new(generators::python::PythonGenerator),
      "lua" => Box::new(generators::lua::LuaGenerator),
      "js-quickjs" => Box::new(generators::js_quickjs::JsQuickjsGenerator),
      "js-deno" => Box::new(generators::js_deno::JsDenoGenerator),
      other => {
          return Err(CodegenError::ValidationFailed {
              message: format!(
                  "Unknown language: `{other}`. Supported: rust, cpp, csharp, python, lua, js-quickjs, js-deno"
              ),
          });
      }
  };
  ```
  Remove old `ts-node`, `js-node`, `ts-bun`, `js-bun`, `ts-deno`, `js-deno` match arms entirely.

  **Update `--lang` description in the `Generate` CLI variant** (line 41):
  ```rust
  /// Target language: rust, cpp, csharp, python, lua, js-quickjs, js-deno.
  ```

  **Must NOT do**:
  - Do not add a `pack` subcommand in this task (not in scope for this patch — see epics.md for future epic)
  - Do not reference old runtime names anywhere

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multiple file rewrites across generator system
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Tasks 5 and 7
  - **Parallel Group**: Wave 3 (after Task 1)
  - **Blocks**: Task 8
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplugc/src/generators/js_node/mod.rs` — reference for type mapping logic (476 lines)
  - `crates/polyplugc/src/generators/mod.rs` — CodeGenerator trait
  - `crates/polyplugc/src/main.rs` — CLI dispatch (current, lines 89–120)
  - `epics.md` lines 1797–1870 — full generator spec (files to produce, type mapping, init.ts templates)

  **Acceptance Criteria**:
  - [ ] `cargo build -p polyplugc 2>&1 | grep -c '^error'` → 0
  - [ ] `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang js-quickjs --out /tmp/qjs_out 2>&1` → exit code 0
  - [ ] `ls /tmp/qjs_out/guest/` → contains `types.ts`, `contracts.ts`
  - [ ] `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang js-deno --out /tmp/deno_out 2>&1` → exit code 0
  - [ ] `cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang ts-node --out /tmp/x 2>&1` → exit code 1 (unknown language error)

  **QA Scenarios**:

  ```
  Scenario: js-quickjs generator produces files
    Tool: Bash
    Preconditions: mkdir -p /tmp/qjs_out
    Steps:
      1. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang js-quickjs --out /tmp/qjs_out 2>&1
      2. ls /tmp/qjs_out/guest/
    Expected Result: Step 1 exits 0; step 2 lists types.ts and contracts.ts
    Evidence: .sisyphus/evidence/task-6-quickjs-gen.txt

  Scenario: Old lang names rejected
    Tool: Bash
    Steps:
      1. cargo run -p polyplugc -- generate --api tests/fixtures/test_api.toml --lang ts-node --out /tmp/x 2>&1
      2. echo "exit: $?"
    Expected Result: Error message mentioning "Unknown language"; exit code 1
    Evidence: .sisyphus/evidence/task-6-unknown-lang.txt
  ```

  **Commit**: NO (groups with Tasks 5+7)

---

- [ ] 7. Rewrite `guest-libs/js/polyplug-guest.ts`

  **What to do**:

  Replace the entire contents with the correct lo/hi pointer representation.

  The file must NOT be marked as auto-generated (it is a hand-written guest SDK).

  New contents:
  ```typescript
  // THIS FILE IS PART OF THE polyplug GUEST LIBRARY FOR JAVASCRIPT/TYPESCRIPT.
  // NOT auto-generated. Hand-written for the polyplug JS guest SDK.
  // Shared between js-quickjs and js-deno generators.

  /**
   * A read-only view into a UTF-8 string in the host's address space.
   *
   * ptr_lo/ptr_hi are the low and high 32-bit halves of the 64-bit pointer.
   * (QuickJS uses f64 internally — 64-bit integers must be split into lo/hi pairs.)
   */
  export interface StringView {
      readonly ptr_lo: number;
      readonly ptr_hi: number;
      readonly len: number;
  }

  /**
   * A byte buffer owned by the host allocator.
   */
  export interface Buffer {
      readonly ptr_lo: number;
      readonly ptr_hi: number;
      readonly len: number;
      readonly cap: number;
  }

  /**
   * ABI error code. code === 0 means ABI_OK (success).
   */
  export interface AbiError {
      readonly code: number;
      readonly message: StringView;
  }

  /** ABI_OK sentinel — code 0 means success. */
  export const ABI_OK: number = 0;

  /**
   * Error thrown when a declared dependency cannot be resolved at init time.
   */
  export class DependencyNotFoundError extends Error {
      constructor(public readonly contractName: string) {
          super(`dependency not found: ${contractName}`);
          this.name = 'DependencyNotFoundError';
      }
  }

  /**
   * Extension ID for the trace extension.
   * Value: fnv1a_32("trace") = 0xC4EB9AEE
   */
  export const EXT_TRACE_ID: number = 0xC4EB9AEE;

  /**
   * VTable interface for the trace extension.
   * Obtain via polyplug.getExtension(EXT_TRACE_ID) -> {lo, hi} | null.
   * The {lo, hi} pair is a pointer to a TraceVTable in the host.
   */
  export interface TraceVTable {
      /** Emit a trace event. ptr_lo/ptr_hi/len describe a StringView of the message. */
      emit(ptr_lo: number, ptr_hi: number, len: number): void;
  }

  /**
   * Type mapping reference (for code generators):
   *
   * js-quickjs (QuickJS / f64 internal):   js-deno (V8 / BigInt native):
   *   u8/u16/u32   -> number               u8/u16/u32   -> number
   *   u64/i64      -> {lo:number,hi:number} u64/i64      -> bigint
   *   f32/f64      -> number               f32/f64      -> number
   *   bool         -> boolean              bool         -> boolean
   *   StringView   -> {ptr_lo,ptr_hi,len}  StringView   -> {ptr:bigint,len:number}
   *   Buffer       -> {ptr_lo,ptr_hi,len,cap} Buffer    -> {ptr:bigint,len:number,cap:number}
   *   void         -> void                 void         -> void
   */
  export {}; // treat this as a module
  ```

  **Must NOT do**:
  - Do not keep the old `bigint` types (`ptr: bigint`)
  - Do not keep `PolyplugInitFn`, `PluginRegistrar`, `PluginVTable`, `PluginDescriptor` N-API types
  - Do not mark this file as auto-generated

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single file rewrite, no logic
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES — with Tasks 5 and 6
  - **Parallel Group**: Wave 3 (after Task 1)
  - **Blocks**: Task 8
  - **Blocked By**: Task 1

  **References**:
  - `guest-libs/js/polyplug-guest.ts` — OLD file to be replaced
  - `epics.md` lines 1838–1847 — guest-libs/js/ spec
  - `epics.md` line 2206 — `EXT_TRACE_ID = 0xC4EB9AEE`

  **Acceptance Criteria**:
  - [ ] `grep 'EXT_TRACE_ID = 0xC4EB9AEE' guest-libs/js/polyplug-guest.ts` → match found
  - [ ] `grep 'DependencyNotFoundError' guest-libs/js/polyplug-guest.ts` → match found
  - [ ] `grep 'TraceVTable' guest-libs/js/polyplug-guest.ts` → match found
  - [ ] `grep 'ptr: bigint\|PolyplugInitFn\|PluginRegistrar' guest-libs/js/polyplug-guest.ts` → exit code 1 (no old N-API types)

  **QA Scenarios**:

  ```
  Scenario: New constants and types present, old types absent
    Tool: Bash
    Steps:
      1. grep -c 'EXT_TRACE_ID\|DependencyNotFoundError\|TraceVTable\|ptr_lo' guest-libs/js/polyplug-guest.ts
      2. grep -c 'ptr: bigint\|PolyplugInitFn\|PluginRegistrar' guest-libs/js/polyplug-guest.ts
    Expected Result: Step 1 >= 4; step 2 == 0
    Evidence: .sisyphus/evidence/task-7-guest-ts.txt
  ```

  **Commit**: YES (with Tasks 5+6) — `fix(tests,generators,guest): rewrite integration_js, js generators, and guest-libs/js`

---

- [ ] 8. Final Compile + Clippy + Full Test Suite

  **What to do**:

  Run the full verification suite and fix any issues found.

  ```bash
  cargo build --workspace 2>&1
  cargo clippy --workspace -- -D warnings 2>&1
  cargo fmt --check 2>&1
  cargo test --workspace 2>&1
  ```

  **Fix any failures before proceeding to the Final Verification Wave.** Common issues to watch for:
  - Unused imports in test files after rewrite
  - Type inference failures in new Rust code (AGENTS.md §3 requires explicit types)
  - Missing `// SAFETY:` comments on unsafe blocks
  - Old error variant references that were missed
  - Integration test failures in Epic 12 discovery tests (should be unaffected but verify)

  Run stale reference grep:
  ```bash
  grep -r "ts-node\|js-node\|ts-bun\|js-bun\|NodeConfig\|BunConfig\|DenoConfig" crates/ tests/ --include="*.rs" --include="*.toml"
  grep -r "JsNodeNotFound\|JsNodeVersionTooOld\|JsBinaryNotConfigured\|JsInitRaisedError\|JsConfigEmpty\|RuntimeNotImplemented" crates/ tests/ --include="*.rs"
  ```
  Both must return zero matches.

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Diagnostic + fix cycle across the full workspace
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Wave 4 (sequential, after all implementation tasks)
  - **Blocks**: F1, F2
  - **Blocked By**: Tasks 5, 6, 7

  **References**:
  - All previous task outputs

  **Acceptance Criteria**:
  - [ ] `cargo build --workspace` → exit code 0
  - [ ] `cargo clippy --workspace -- -D warnings` → exit code 0, zero error lines
  - [ ] `cargo fmt --check` → exit code 0
  - [ ] `cargo test --workspace` → all non-ignored tests pass
  - [ ] Stale grep: both commands return zero matches

  **QA Scenarios**:

  ```
  Scenario: Full workspace build and test pass
    Tool: Bash
    Steps:
      1. cargo build --workspace 2>&1 | tail -5
      2. cargo clippy --workspace -- -D warnings 2>&1 | grep '^error' | head -5
      3. cargo test --workspace 2>&1 | tail -10
    Expected Result: Step 1 shows "Finished"; step 2 empty; step 3 shows "test result: ok"
    Evidence: .sisyphus/evidence/task-8-full-suite.txt

  Scenario: Zero stale references
    Tool: Bash
    Steps:
      1. grep -r "ts-node\|js-node\|ts-bun\|js-bun" crates/ tests/ --include="*.rs" --include="*.toml" | wc -l
      2. grep -r "JsNodeNotFound\|JsBinaryNotConfigured\|JsConfigEmpty\|RuntimeNotImplemented" crates/ tests/ --include="*.rs" | wc -l
    Expected Result: Both steps return 0
    Evidence: .sisyphus/evidence/task-8-stale-refs.txt
  ```

  **Commit**: YES — `chore: Epic 11.6 patch complete — fix JS architecture`

---

## Success Criteria

### Verification Commands

```bash
# No stale JS runtime references
grep -r "ts-node\|js-node\|ts-bun\|js-bun" crates/ tests/ --include="*.rs" --include="*.toml"
# Expected: zero matches

# Runtime names correct
cargo test --test integration_js -- js_loader_runtime_name
# Expected: test passes

# Full test suite
cargo test --workspace 2>&1 | tail -5
# Expected: test result: ok

# Clippy clean
cargo clippy --workspace -- -D warnings 2>&1 | grep -c "^error"
# Expected: 0

# Deleted files gone
test ! -f tests/fixtures/test_plugin_ts_node.node && echo PASS || echo FAIL
test ! -d tests/fixtures/test_plugin_ts_node && echo PASS || echo FAIL
test ! -f host-libs/js/polyplug.ts && echo PASS || echo FAIL
```

### Final Checklist

- [ ] JsLoader::new(JsConfig{}).runtime_name() == "js-quickjs"
- [ ] JsDenoLoader::new(JsDenoConfig{}).runtime_name() == "js-deno"
- [ ] Both loaders register cleanly in RuntimeBuilder without error
- [ ] Duplicate loader registration returns DuplicateLoader error
- [ ] All old N-API/subprocess/ts-node references removed from source
- [ ] All tests pass (non-ignored)
- [ ] Zero clippy warnings
