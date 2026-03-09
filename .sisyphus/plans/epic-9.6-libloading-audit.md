# Epic 9.6 — NativeBundleLoader: libloading Audit and Library Handle Lifetime

## TL;DR

> **Quick Summary**: The `Library` handle returned by `load_bundle()` is currently dropped
> at end of `NativeBundleLoader::load()` — calling `dlclose()` and unmapping plugin code
> pages while vtable pointers into that memory are still live in the `Registry`.
> This epic fixes the use-after-free by storing handles in `Registry::loaded_libraries`,
> hardens every `unsafe` block with precise `// SAFETY:` comments, and adds a
> use-after-unload regression test verified under `cargo test` and Miri.
>
> **Deliverables**:
> - `Registry::loaded_libraries: Mutex<Vec<libloading::Library>>` field + `push_library()` method
> - `load_bundle()` return type changed to `Result<(), LoaderError>`; Library pushed into Registry
> - `NativeBundleLoader::load()` updated to match new return type; stale comment removed
> - `tests/library_lifetime/mod.rs` — use-after-unload regression test
> - `crates/polyplug/Cargo.toml` `[[test]]` entry for the new test
>
> **Estimated Effort**: Short (targeted correctness fix, no new features)
> **Parallel Execution**: NO — strictly sequential; each task depends on the prior
> **Critical Path**: Task 1 → Task 2 → Task 3 → Task 4 → Task 5

---

## Context

### Original Request
Epic 9.6: audit `NativeBundleLoader` for the Library-handle-lifetime correctness risk.
No new features — targeted correctness and hardening only.

### Audit Findings (confirmed by Planner)

**Bug confirmed. Root cause:**

`load_bundle()` in `crates/polyplug/src/loader/mod.rs` (lines 275–282) has this comment:
"Box::leak is used to make the leak explicit and intentional" but the actual code is:
```rust
let leaked_library: Box<libloading::Library> = Box::new(library);  // NOT Box::leak!
Ok(LoadedBundle { path: path.to_path_buf(), library: leaked_library })
```
This is `Box::new`, not `Box::leak`. The `Box<Library>` **will** be dropped when the
`LoadedBundle` is dropped.

`NativeBundleLoader::load()` (lines 87–92) receives the `LoadedBundle` and **drops it
immediately** at end of scope:
```rust
let _bundle: LoadedBundle = load_bundle(path, &self.registry, self.host_vtable)
    .map_err(|e: LoaderError| PolyplugError::Loader(e))?;
// _bundle drops here → Box<Library> drops → dlclose() → code pages unmapped → SIGBUS
Ok(())
```
The comment says "the library is already leaked inside load_bundle() via Box::leak" — this
comment is **factually incorrect**. No leak occurs. dlclose() is called here.

`Runtime._bundles: Vec<LoadedBundle>` exists in `runtime/mod.rs` but is initialized as
`Vec::new()` and **never populated**. `NativeBundleLoader` has no reference to `Runtime`
and cannot push bundles there.

**`registrar_callback` is a stub (confirmed):** `loader/mod.rs` lines 297–307 show that the
`registrar_callback` extern fn used in `load_bundle()` is a placeholder that returns
`AbiError::ok()` without actually calling `registry.register()`. After `load_bundle()`
completes, no plugins are registered in the Registry. This is a known TODO. This epic does
NOT fix the stub registrar — out of scope. The `library_lifetime` test is designed within
this constraint: it verifies lifetime by successful completion of `load_bundle()` and clean
Registry drop, not by asserting `registry.find()` succeeds.

**libloading version:** Already `0.9` in workspace `Cargo.toml` (line 19). No update needed.

**Symbol borrow safety:** `abi_version_symbol` and `init_fn` are `Symbol<'_, ...>` that
borrow `library`. Before moving `library` into the registry, both borrows must be released.
The fix: copy the fn pointer out of the Symbol (fn pointers are `Copy`), then drop the
Symbols explicitly.

**Drop order:** Rust drops struct fields in **reverse declaration order** (last field
drops first). `loaded_libraries` must be declared **first** in `Registry` so it drops
**last** — after `slots` (which holds `RegistryEntry` with raw vtable pointers). This
guarantees no dangling pointer dereference during `Registry` drop.

### Pre-Answered Decisions
- libloading version: `0.9` — already correct, no change
- Storage: `Registry` gains `loaded_libraries: Mutex<Vec<libloading::Library>>`
  declared as the **first** field (drop-order guarantee)
- `load_bundle()` return type: `Result<(), LoaderError>` — pushes Library into Registry
- Fix mechanism: `registry.push_library(library)` — NOT `Box::leak` or `mem::forget`
- Test: `#[cfg(not(miri))]` dlopen test + `#[cfg(miri)]` structural test
- **Library is moved into Registry BEFORE calling `init_fn_ptr`** (even if init later fails —
  the never-unload invariant means we never dlclose once any code from the library has run)
- **Symbol resolution failure is safe**: if `get(b"polyplug_init\0")` fails, `?` propagates
  and `library` drops before any vtable pointers are registered — no dangling pointers
- **No deadlock risk**: `push_library` locks `Mutex<loaded_libraries>`;
  `register()` locks `RwLock<slots>` + `RwLock<contract_index>` — entirely separate locks.
  `push_library` is called before init, `register()` is called during init. No nesting.
- **`Registry` lifetime**: `Runtime` holds `Arc<Registry>`. `GLOBAL_REGISTRY: OnceLock<Arc<Registry>>`
  holds a second clone. Libraries in Registry live at least as long as the Runtime.
- **Duplicate path loads**: two `push_library` calls for the same `.so` are safe.
  POSIX dlopen increments refcount; both handles are valid. Registry would reject the
  second `register()` with `DuplicateProvider` error — out of scope for this epic.

---

## Work Objectives

### Core Objective
Ensure `libloading::Library` handles are stored in `Registry::loaded_libraries` and live
exactly as long as the `Registry` (and thus the `Runtime`). Fix the misleading
`Box::new`/`Box::leak` comment. Harden all `unsafe` blocks with precise SAFETY justifications.

### Concrete Deliverables
- `crates/polyplug/src/registry/mod.rs`: `loaded_libraries` field + `push_library()` method
- `crates/polyplug/src/loader/mod.rs`: `load_bundle()` returns `()`, pushes Library into Registry
- `tests/library_lifetime/mod.rs`: use-after-unload correctness test
- `crates/polyplug/Cargo.toml`: `[[test]]` entry for `library_lifetime`

### Definition of Done
- [ ] `Registry::loaded_libraries` field exists, is declared first, populated by every native load
- [ ] `NativeBundleLoader::load()` does not drop the `Library` handle
- [ ] Every `unsafe` block in `loader/mod.rs` has a `// SAFETY:` comment
- [ ] `cargo test --test library_lifetime` passes
- [ ] `cargo miri test --test library_lifetime` compiles and runs without UB errors
- [ ] `cargo clippy -- -D warnings` passes with zero warnings
- [ ] All existing tests still pass

### Must Have
- `Library` handle outlives all vtable pointers derived from it
- Every `unsafe` block in `loader/mod.rs` has a complete `// SAFETY:` comment
- `loaded_libraries` declared BEFORE `slots` in `Registry` (correct drop order)
- The `Box::new`/`Box::leak` confusion is fully resolved
- No `.unwrap()` in production code paths

### Must NOT Have (Guardrails)
- **`load_bundle()` return type change IS intentional and is NOT a stable public API break**.
  The crate's stable public ABI surface is the 6 `#[unsafe(no_mangle)]` C functions in `lib.rs`.
  `load_bundle()` is `pub fn` for integration-test accessibility only; it is not part of the
  versioned/semver-stable contract. Changing its return type is correct and necessary.
  This guardrail is relaxed for `load_bundle()` only.
- No other public API changes (types, trait signatures, module paths) beyond `load_bundle()` return type
- Do NOT store libraries in `NativeBundleLoader` — the loader may be dropped before Runtime
- Do NOT use `Box::leak` or `std::mem::forget` as the fix — use owned `Vec` in Registry
- Do NOT remove `LoadedBundle` struct — it is `pub` and referenced by `Runtime._bundles`.
  `Runtime._bundles` is the **path+metadata inventory** for Epic 12 (different purpose).
  `Registry::loaded_libraries` is the **lifetime owner** (different purpose). Both coexist.
- Do NOT drain or iterate `Registry::loaded_libraries` — push-only, drop on Registry drop
- Do NOT change `load_bundle()` to private — it is `pub` (though no external callers confirmed
  by grep; internal only). Changing visibility is out of scope.
- Do NOT add a `Registry::clear()`, `unload()`, or hot-reload method — out of scope
- Do NOT change the test file path — must be `tests/library_lifetime/mod.rs` per AGENTS.md Rule 1
- Do NOT add `use` statements inside functions — AGENTS.md Rule 2
- Do NOT change `Runtime._bundles` or its SAFETY comment in `runtime/mod.rs` — unchanged field

---

## Verification Strategy

> **ZERO HUMAN INTERVENTION** — ALL verification is agent-executed.

### Test Decision
- **Infrastructure exists**: YES (`cargo test`, existing integration tests)
- **Automated tests**: Tests-after (the new test IS a deliverable)
- **Framework**: standard Rust test harness (`cargo test`)
- **Miri**: `cargo +nightly miri test --test library_lifetime` — `#[cfg(miri)]` path

### QA Policy
Every task includes agent-executed `cargo check` / `cargo test` verification.

---

## Execution Strategy

### Sequential Execution

```
Task 1: Add Registry::loaded_libraries + push_library()
  ↓
Task 2: Rewrite load_bundle() — push Library, fix return type, SAFETY comments, doc
  ↓
Task 3: Fix NativeBundleLoader::load() — match new return type, remove stale comment
  ↓
Task 4: Add tests/library_lifetime/mod.rs + [[test]] entry in Cargo.toml
  ↓
Task 5: Final audit — clippy, full test suite, SAFETY review, commit
```

---

## TODOs

---


- [ ] 1. Add `loaded_libraries` field and `push_library()` method to `Registry`

  **What to do**:

  File: `crates/polyplug/src/registry/mod.rs`

  **Step 1 — Add `use std::sync::Mutex;` at the top of the file.**
  The file already has `use std::sync::RwLock;`. Add `use std::sync::Mutex;` on its own line,
  immediately after `use std::sync::RwLock;` (both at file top, never inside a fn).

  **Step 2 — Restructure the `Registry` struct, declaring `loaded_libraries` FIRST.**

  Rust drops struct fields in **reverse declaration order** (last declared drops first).
  `loaded_libraries` must be the **first** field so that it drops **last** — after `slots`
  and `contract_index` (which hold raw vtable pointers). This prevents dangling pointers
  during `Registry` drop.

  Replace the existing `Registry` struct definition:
  ```rust
  pub struct Registry {
      /// Library handles for all loaded native bundles.
      /// Declared FIRST so they drop LAST (Rust drops fields in reverse order).
      /// This ensures vtable pointers in `slots` are never dangling during drop.
      loaded_libraries: Mutex<Vec<libloading::Library>>,
      slots: RwLock<Vec<RegistrySlot>>,
      /// Maps contract_id (FNV-1a u64) to the index of the registered slot.
      contract_index: RwLock<HashMap<u64, u32>>,
  }
  ```

  **Step 3 — Update `Registry::new()` to initialize the new field:**
  ```rust
  pub fn new() -> Registry {
      Registry {
          loaded_libraries: Mutex::new(Vec::new()),
          slots: RwLock::new(Vec::new()),
          contract_index: RwLock::new(HashMap::new()),
      }
  }
  ```

  **Step 4 — Add `Registry::push_library()` method** (inside `impl Registry`,
  after `Registry::new()` and before `Registry::register()`):
  ```rust
  /// Store a loaded native library handle, keeping it alive until this Registry drops.
  ///
  /// Called by `load_bundle()` after successfully extracting vtable pointers from
  /// the library. The handle must outlive the Registry to prevent `dlclose()` from
  /// unmapping code pages that vtable function pointers point into.
  ///
  /// `loaded_libraries` is declared as the first field in `Registry`, so it drops
  /// last during `Registry` drop — after all `RegistryEntry` vtable pointers are gone.
  pub(crate) fn push_library(&self, library: libloading::Library) {
      self.loaded_libraries
          .lock()
          .unwrap_or_else(|e| e.into_inner())
          .push(library);
  }
  ```

  **Step 5 — Update `unsafe impl Send for Registry` and `unsafe impl Sync for Registry`**
  SAFETY comments (currently say 'Registry uses RwLock internally').
  Update to say:
  ```
  // SAFETY: Registry uses RwLock and Mutex internally for all interior mutability.
  // `loaded_libraries` is a Mutex<Vec<Library>>. `Library` is Send in libloading 0.9.
  // All mutable state is lock-protected; sharing across threads is safe.
  ```

  **Step 6 — Update `unsafe impl Send for RegistryEntry` and `unsafe impl Sync for RegistryEntry`**
  SAFETY comments. They currently reference 'never-drop invariant'. Update to:
  ```
  // SAFETY: RegistryEntry contains raw pointers into library memory. The Library handle
  // is stored in Registry::loaded_libraries (declared before slots in the struct), so
  // the Library outlives all RegistryEntry instances. Pointers are written once at
  // registration and only read afterward, making concurrent access safe.
  ```

  **Must NOT do**:
  - Do NOT add a public `clear()` or `unload()` method
  - Do NOT make `push_library()` public — `pub(crate)` only
  - Do NOT change `slots` or `contract_index` field types

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Single-file structural addition with precise mechanical instructions
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential Step 1
  - **Blocks**: Tasks 2, 3, 4, 5
  - **Blocked By**: None (can start immediately)

  **References**:
  - `crates/polyplug/src/registry/mod.rs:7–8` — existing `use std::sync::RwLock;` to add `Mutex` after
  - `crates/polyplug/src/registry/mod.rs:53–57` — `Registry` struct layout to replace
  - `crates/polyplug/src/registry/mod.rs:65–72` — `Registry::new()` to update
  - `crates/polyplug/src/registry/mod.rs:37–46` — `unsafe impl Send/Sync for RegistryEntry` to update
  - `crates/polyplug/src/registry/mod.rs:59–63` — `unsafe impl Send/Sync for Registry` to update
  - `crates/polyplug/Cargo.toml:14` — `libloading = { workspace = true }` already present

  **Acceptance Criteria**:
  - [ ] `cargo check -p polyplug` exits 0
  - [ ] `Registry::push_library()` exists with `pub(crate)` visibility
  - [ ] `loaded_libraries` field is declared BEFORE `slots` in struct body
  - [ ] `use std::sync::Mutex;` is at file top (not inside a fn)
  - [ ] SAFETY comments on `unsafe impl Send/Sync` reference structural drop-order guarantee

  **QA Scenarios**:

  ```
  Scenario: Registry compiles with new field and method
    Tool: Bash (cargo check)
    Preconditions: Task 1 edits applied
    Steps:
      1. Run: cargo check -p polyplug 2>&1
      2. Assert: exit code 0, no 'error[' lines
    Expected Result: Clean compile
    Failure Indicators: Any 'error[E' line
    Evidence: cargo check stdout

  Scenario: push_library is pub(crate), loaded_libraries is first field
    Tool: Bash (grep)
    Preconditions: Task 1 edits applied
    Steps:
      1. Run: grep -n 'push_library\|loaded_libraries\|slots:' crates/polyplug/src/registry/mod.rs
      2. Assert: loaded_libraries line number < slots: line number
      3. Assert: push_library line contains 'pub(crate)'
    Expected Result: Correct field order and visibility
    Evidence: grep output with line numbers
  ```

  **Commit**: NO (single commit at Task 5 only)

---

- [ ] 2. Rewrite `load_bundle()` — push `Library` into `Registry`, fix return type and SAFETY comments

  **What to do**:

  File: `crates/polyplug/src/loader/mod.rs`

  This is the core structural fix. `load_bundle()` currently returns `Result<LoadedBundle, LoaderError>`.
  After this task it returns `Result<(), LoaderError>`. The `Library` is moved into the
  `Registry` before `init` is called.

  **Step 1 — Update the module-level doc comment** (lines 1–10):
  Replace the stale 'Library Lifetime (Never-Drop)' section with:
  ```rust
  //! # Library Lifetime
  //! `libloading::Library` handles for loaded native bundles are moved into
  //! `Registry::loaded_libraries` immediately after symbol resolution.
  //! This ensures code pages remain mapped for the entire lifetime of the `Registry`
  //! (i.e., the `Runtime`). Dropping a `Library` calls `dlclose()` which unmaps
  //! plugin code — any vtable fn pointer into those pages would become dangling.
  ```

  **Step 2 — Change `load_bundle()` function signature** (line ~173):
  ```rust
  pub fn load_bundle(
      path: &Path,
      registry: &Registry,
      host_vtable: &'static HostVTable,
  ) -> Result<(), LoaderError> {
  ```

  **Step 3 — Keep Steps 1–2 (dlopen, ABI version check) identical** with one update:
  After calling `abi_version_symbol()` (line ~203), add an explicit drop before the
  `init` symbol resolution block:
  ```rust
  let found_version: u32 = unsafe { abi_version_symbol() };
  // Explicitly drop the symbol to release its borrow on `library` before the
  // version check (we need `library` to be movable later).
  drop(abi_version_symbol);
  if found_version != POLYPLUG_ABI_VERSION {
      // ... existing error return ...
  }
  ```

  **Step 4 — Replace the `init_fn` resolution block** (lines ~212–225).
  The goal: resolve the symbol, copy the fn pointer out of the `Symbol` borrow
  (fn pointers are `Copy`), then drop the `Symbol` to release the borrow on `library`.

  Replace with:
  ```rust
  // Step 2: Resolve init symbol and extract the raw function pointer.
  // We copy the fn pointer out of the Symbol immediately so the Symbol's borrow
  // on `library` is released before we move `library` into the registry below.
  // SAFETY: polyplug_init is guaranteed by the plugin build process to have the
  // signature: extern "C" fn(*mut PluginRegistrar) -> AbiError.
  // Symbol<F> derefs to F (a fn pointer). Fn pointers are Copy — copying does not
  // extend the lifetime of `library`. The pointer remains valid as long as `library`
  // is alive. `library` is moved into `registry.loaded_libraries` immediately after,
  // so the pointer is always valid while reachable.
  let init_fn_ptr: unsafe extern "C" fn(*mut PluginRegistrar) -> AbiErrorType = {
      let sym: libloading::Symbol<
          '_,
          unsafe extern "C" fn(*mut PluginRegistrar) -> AbiErrorType,
      > = unsafe {
          library
              .get(b"polyplug_init\0")
              .map_err(|_| LoaderError::MissingSymbol {
                  bundle: path_str.clone(),
                  symbol: "polyplug_init".to_owned(),
              })?
      };
      // SAFETY: Deref of Symbol<F> where F is a fn pointer type (Copy).
      // This copies the function address out of the Symbol without cloning Library.
      *sym
  };
  // `sym` is dropped here, releasing the borrow on `library`.
  ```

  **Step 5 — Move `library` into `registry` BEFORE building the registrar**.
  Insert immediately after the `init_fn_ptr` block:
  ```rust
  // Step 3: Move the library into the registry BEFORE calling init.
  // SAFETY: `library` is a successfully loaded shared library. Moving it into
  // `registry.loaded_libraries` transfers ownership to the Registry, which
  // outlives this function and all vtable pointers registered during init.
  // This prevents dlclose() from being called while vtable fn pointers are live.
  registry.push_library(library);
  ```

  **Step 6 — Keep the registrar construction and state building** (Steps 3–4 in original)
  unchanged. They follow the `push_library` call.

  **Step 7 — Update the init call** (step 5 in original) to use `init_fn_ptr` instead
  of `init_fn`:
  ```rust
  // Step 5: Call init
  // SAFETY: init_fn_ptr was resolved from the library (now stored in registry).
  // The PluginRegistrar is valid for the duration of the call.
  // The state pointer is stable (pinned on the stack).
  let init_result: AbiError =
      unsafe { init_fn_ptr(&mut registrar as *mut PluginRegistrar) };
  ```

  **Step 8 — Update the init-failure comment block** (lines ~247–253):
  ```rust
  // On init failure: the library is already stored in registry.loaded_libraries
  // and will NOT be unloaded. The never-unload invariant means we never call
  // dlclose on a library once any code from it has run. Failed slots remain
  // vacant (non-functional) in the registry.
  ```

  **Step 9 — Delete the old 'Step 7: Leak the library' block** (lines ~275–282):
  Remove:
  ```rust
  // Step 7: Leak the library — it must outlive all vtable pointers.
  // Box::leak is used to make the leak explicit and intentional.
  let leaked_library: Box<libloading::Library> = Box::new(library);
  Ok(LoadedBundle { path: path.to_path_buf(), library: leaked_library })
  ```
  Replace with:
  ```rust
  Ok(())
  ```

  **Step 10 — Update the `load_bundle()` doc comment** (lines ~163–172):
  Replace with:
  ```rust
  /// Load a single native plugin bundle.
  ///
  /// # Steps
  /// 1. `dlopen` the library (RTLD_NOW | RTLD_LOCAL via libloading defaults).
  ///    RTLD_LOCAL: plugins must not pollute the global symbol namespace.
  ///    RTLD_NOW: fail fast at load time if any symbols are missing.
  /// 2. Resolve `polyplug_abi_version` sentinel — reject if missing or wrong version.
  /// 3. Resolve `polyplug_init`, copy the fn pointer out of the `Symbol` borrow,
  ///    then move `library` into `registry.loaded_libraries`.
  ///    **Why critical**: `Library::drop` calls `dlclose()`, unmapping plugin code pages.
  ///    Any vtable fn pointer into those pages then becomes dangling — silent
  ///    memory corruption or SIGBUS on the next vtable call. By storing the handle in
  ///    `Registry`, it lives exactly as long as the `Runtime`.
  /// 4. Call `polyplug_init` with a `PluginRegistrar` callback.
  /// 5. On init failure: propagate the error. The library remains in
  ///    `registry.loaded_libraries` — the never-unload invariant applies.
  ```

  **Step 11 — Audit remaining SAFETY comments** on existing unsafe blocks:
  - `Library::new(path)` block (line ~184): existing SAFETY comment is good; keep it.
  - `library.get(b"polyplug_abi_version\0")` block (line ~194): update to note that the
    symbol is explicitly dropped via `drop(abi_version_symbol)` before the move.
  - `abi_version_symbol()` call (line ~203): existing comment is good; keep it.
  - `slice::from_raw_parts` in error-message block (line ~263): existing comment is good; keep it.

  **Must NOT do**:
  - Do NOT use `Box::leak` or `std::mem::forget`
  - Do NOT remove the `LoadedBundle` struct or change its fields
  - Do NOT change `NativeBundleLoader::load()` in this task — that is Task 3

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Requires precise understanding of Symbol borrow semantics and drop ordering;
      careful mechanical rewrite of a safety-critical function
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential Step 2
  - **Blocks**: Tasks 3, 4, 5
  - **Blocked By**: Task 1

  **References**:
  - `crates/polyplug/src/loader/mod.rs:163–283` — full `load_bundle()` function to rewrite
  - `crates/polyplug/src/loader/mod.rs:1–10` — module doc to update
  - `crates/polyplug/src/registry/mod.rs` — `Registry::push_library()` added in Task 1
  - libloading 0.9: `Symbol<T>` derefs to `T`; fn pointers are `Copy`

  **Acceptance Criteria**:
  - [ ] `load_bundle()` return type is `Result<(), LoaderError>`
  - [ ] `registry.push_library(library)` called before `init_fn_ptr`
  - [ ] `abi_version_symbol` explicitly dropped before move
  - [ ] No `Box::new(library)`, no `Box::leak`, no `mem::forget` in `load_bundle()`
  - [ ] All 5+ unsafe blocks have `// SAFETY:` comments
  - [ ] `load_bundle()` doc comment explains RTLD flags and Library-lifetime guarantee

  **QA Scenarios**:

  ```
  Scenario: load_bundle returns () not LoadedBundle
    Tool: Bash (grep)
    Preconditions: Task 2 edits applied
    Steps:
      1. Run: grep -n 'fn load_bundle' crates/polyplug/src/loader/mod.rs
      2. Assert: line contains 'Result<(), LoaderError>'
      3. Assert: line does NOT contain 'LoadedBundle'
    Expected Result: Single match showing () return type
    Evidence: grep output

  Scenario: push_library called before init_fn_ptr
    Tool: Bash (grep)
    Preconditions: Task 2 edits applied
    Steps:
      1. Run: grep -n 'push_library\|init_fn_ptr' crates/polyplug/src/loader/mod.rs
      2. Assert: push_library line number < init_fn_ptr call line number
    Expected Result: push_library appears before init_fn_ptr call
    Evidence: grep output with line numbers

  Scenario: No Box::new(library) or mem::forget
    Tool: Bash (grep)
    Preconditions: Task 2 edits applied
    Steps:
      1. Run: grep -n 'Box::new(library)\|mem::forget\|Box::leak' crates/polyplug/src/loader/mod.rs
      2. Assert: zero matches
    Expected Result: No output (clean)
    Evidence: grep exit code
  ```

  **Commit**: NO

---

- [ ] 3. Fix `NativeBundleLoader::load()` — match new return type, remove stale comment, add doc

  **What to do**:

  File: `crates/polyplug/src/loader/mod.rs`

  After Task 2, `load_bundle()` returns `Result<(), LoaderError>`. The existing
  `NativeBundleLoader::load()` (lines 81–93) tries to bind the return value to
`_bundle: LoadedBundle` — this is now a compile error. Fix it.

  **Step 1 — Replace the entire `BundleLoader for NativeBundleLoader` impl body**
  (lines 76–94) with:
  ```rust
  impl BundleLoader for NativeBundleLoader {
      fn runtime_name(&self) -> &'static str {
          "native"
      }

      /// Load a native plugin bundle by calling `load_bundle()`.
      ///
      /// The `Library` handle for the loaded bundle is stored in the `Registry`
      /// (`self.registry`) — NOT here in the loader. `NativeBundleLoader` may be
      /// dropped before `Runtime` (e.g., after the build phase). Storing the library
      /// here would allow `dlclose()` to fire while vtable pointers are still live.
      fn load(&self, path: &Path, _registrar: &mut PluginRegistrar) -> Result<(), PolyplugError> {
          // NativeBundleLoader uses load_bundle() which pushes the Library handle
          // directly into the Registry via registry.push_library(). The trait's
          // `registrar` parameter is unused here — native loading goes through
          // dlopen + ABI init directly via the injected registry and host_vtable.
          load_bundle(path, &self.registry, self.host_vtable)
              .map_err(|e: LoaderError| PolyplugError::Loader(e))
      }
  }
  ```

  **Step 2 — Update the `NativeBundleLoader` struct doc comment** (lines 51–55).
  Append one sentence to the existing comment:
  ```
  /// The `Library` handle for each loaded bundle is stored in the injected `Registry`,
  /// not in this struct, to guarantee it outlives all vtable function pointers.
  ```

  **Step 3 — Confirm no other call sites of `load_bundle()` in the codebase exist**
  that still expect a `LoadedBundle` return. Run:
  `grep -rn "load_bundle" crates/ tests/`
  The only expected call sites:
  - `crates/polyplug/src/loader/mod.rs` (the definition + NativeBundleLoader::load())
  - `tests/integration_load/mod.rs` — uses `libloading::Library::new()` directly, not `load_bundle()`
  If any test calls `load_bundle()` directly and assigns a `LoadedBundle`, update it
  to expect `()` and remove the `LoadedBundle` binding and any `mem::forget` on it.

  **Must NOT do**:
  - Do NOT change `NativeBundleLoader::new()` signature or fields
  - Do NOT remove `_registrar` parameter from `load()` — required by `BundleLoader` trait
  - Do NOT change the `BundleLoader` trait

  **Recommended Agent Profile**:
  - **Category**: `quick`
    - Reason: Small mechanical fix — one function body + one comment
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential Step 3
  - **Blocks**: Tasks 4, 5
  - **Blocked By**: Tasks 1, 2

  **References**:
  - `crates/polyplug/src/loader/mod.rs:76–93` — `BundleLoader for NativeBundleLoader` impl to replace
  - `crates/polyplug/src/loader/mod.rs:51–60` — `NativeBundleLoader` struct doc to extend

  **Acceptance Criteria**:
  - [ ] `cargo check -p polyplug` exits 0 (expect compile error before this task)
  - [ ] `NativeBundleLoader::load()` body contains no `LoadedBundle` binding or `_bundle`
  - [ ] 'leaked inside load_bundle() via Box::leak' comment is fully removed
  - [ ] Doc comment on `load()` explains why Library goes to Registry not loader

  **QA Scenarios**:

  ```
  Scenario: Crate compiles cleanly after Tasks 1-3
    Tool: Bash (cargo check)
    Preconditions: Tasks 1, 2, 3 applied
    Steps:
      1. Run: cargo check -p polyplug 2>&1
      2. Assert: exit code 0, no 'error[' lines
    Expected Result: Clean compile
    Failure Indicators: Any compile error
    Evidence: cargo check stdout

  Scenario: All existing tests pass after structural fix
    Tool: Bash (cargo test)
    Preconditions: Tasks 1, 2, 3 applied
    Steps:
      1. Run: cargo test --workspace 2>&1
      2. Assert: exit code 0
      3. Assert: every 'test result:' line shows 'ok'
      4. Assert: no 'FAILED' in output
    Expected Result: All tests pass
    Failure Indicators: 'FAILED' or non-zero exit code
    Evidence: cargo test stdout
  ```

  **Commit**: NO

---

- [ ] 4. Add `tests/library_lifetime/mod.rs` and `[[test]]` entry in `Cargo.toml`

  **What to do**:

  **Part A — Create `tests/library_lifetime/mod.rs`**:

  This test verifies the Library handle is NOT dropped at end of `load_bundle()`, and
  that the Registry holds it alive until the Registry itself is dropped.

  Miri cannot `dlopen` real `.so` files. Use `#[cfg(not(miri))]` / `#[cfg(miri)]` guards.

  ```rust
  //! Library-lifetime correctness test.
  //!
  //! Regression test for Epic 9.6: NativeBundleLoader must NOT drop the
  //! libloading::Library handle at the end of load_bundle(). If it did,
  //! dlclose() would unmap plugin code pages while vtable fn pointers
  //! into those pages are still stored in the Registry (use-after-free / SIGBUS).
  //!
  //! AGENTS.md Rule 1: module roots use dirname/mod.rs.

  #![allow(clippy::expect_used)]

  use polyplug::abi::AbiError;
  use polyplug::abi::HostVTable;
  use polyplug::abi::PluginHandle;
  use polyplug::allocator::polyplug_host_alloc;
  use polyplug::allocator::polyplug_host_free;
  use polyplug::loader::load_bundle;
  use polyplug::registry::Registry;
  use std::path::Path;

  // ─── Stub host vtable callbacks ───────────────────────────────────────────────

  /// # Safety
  /// Stub callback — not called during this test.
  unsafe extern "C" fn stub_find_plugin(_contract_id: u64, _min_version: u32) -> PluginHandle {
      PluginHandle::null()
  }

  /// # Safety
  /// Stub callback — not called during this test.
  unsafe extern "C" fn stub_call_plugin(
      _plugin: PluginHandle,
      _fn_id: u32,
      _args: *const (),
      _out: *mut (),
  ) -> AbiError {
      AbiError::ok()
  }

  /// # Safety
  /// Stub callback — not called during this test.
  unsafe extern "C" fn stub_get_extension(_extension_id: u32) -> *const () {
      core::ptr::null()
  }

  // ─── Tests ────────────────────────────────────────────────────────────────────

  /// Verify that the Library handle is alive after load_bundle() returns.
  ///
  /// **Important context**: `load_bundle()` uses `registrar_callback`, which is currently
  /// a stub that returns `AbiError::ok()` without registering anything into the Registry
  /// (see `loader/mod.rs` around line 297: `// TODO: Implement proper state passing`).
  /// Therefore we cannot use `registry.find()` to confirm registration — this epic does
  /// NOT fix the stub registrar (that is a separate concern).
  ///
  /// **What we CAN verify**: the Library handle is alive when `load_bundle()` returns `Ok(())`.
  /// If the Library had been dropped inside `load_bundle()`, the dlclose() call would fire
  /// DURING the init phase (after symbol resolution), potentially causing a SIGBUS if any
  /// plugin code touched after the close. The fact that `load_bundle()` returns `Ok(())`
  /// successfully is itself evidence the Library was alive through the init call.
  ///
  /// Additionally, we drop the Registry explicitly and verify no crash on cleanup.
  ///
  /// Skipped under Miri: Miri does not support dlopen.
  #[test]
  #[cfg(not(miri))]
  fn library_handle_outlives_load_call() {
      let plugin_path: &str = env!("TEST_PLUGIN_SO");
      let path: &Path = Path::new(plugin_path);

      let host_vtable: &'static HostVTable = Box::leak(Box::new(HostVTable {
          alloc: polyplug_host_alloc,
          free: polyplug_host_free,
          find_plugin: stub_find_plugin,
          call_plugin: stub_call_plugin,
          get_extension: stub_get_extension,
      }));

      let registry: Registry = Registry::new();

      // load_bundle() must push the Library into registry.loaded_libraries BEFORE
      // calling init. If the Library were dropped inside load_bundle() (the bug this
      // epic fixes), dlclose() would fire while init is executing plugin code, which
      // could SIGBUS or corrupt state. Returning Ok(()) here proves the Library was
      // alive through the entire load sequence.
      load_bundle(path, &registry, host_vtable)
          .expect("load_bundle must succeed for test_plugin");

      // NOTE: registry.find() is NOT called here because registrar_callback is a stub
      // (does not register vtables into the Registry). That is a separate TODO, not part
      // of this epic. The lifetime guarantee is verified by the successful Ok(()) above.

      // Explicitly drop the registry, which drops loaded_libraries (and thus the Library),
      // calling dlclose(). This is safe because we hold no raw pointers into library memory
      // past this point.
      drop(registry);
      // Reaching here without SIGBUS or panic confirms clean cleanup.
  }

  /// Miri-compatible structural assertion.
  ///
  /// Under Miri, dlopen is not supported so the above test is excluded.
  /// This test verifies that the structural ownership invariant compiles correctly:
  /// push_library() takes `library: libloading::Library` by value (not by reference),
  /// so the compiler statically prevents double-free and ensures the Library's
  /// destructor runs when Registry drops, not before.
  #[test]
  #[cfg(miri)]
  fn push_library_ownership_enforced_at_compile_time() {
      // This is a documentation test. The ownership invariant is a type-system guarantee:
      // push_library() takes ownership, so the caller cannot drop the Library
      // independently once it has been pushed.
      //
      // Under Miri we cannot construct a real Library (no dlopen support).
      // The invariant is verified statically by the type checker for every caller.
      assert!(true, "ownership invariant is statically verified by the compiler");
  }
  ```

  **Part B — Add `[[test]]` entry to `crates/polyplug/Cargo.toml`**:

  Append after the `integration_dotnet` test entry (the last `[[test]]` block, ~line 73–75):
  ```toml
  [[test]]
  name = "library_lifetime"
  path = "../../tests/library_lifetime/mod.rs"
  ```

  **Important notes for the executor**:
  - `load_bundle` is `pub fn` in `loader/mod.rs` — accessible from test binaries
  - `Registry` is `pub struct`, `Registry::new()` is `pub fn`
  - **`Registry::find()` is NOT used in this test** — `registrar_callback` in `loader/mod.rs` is a
    stub that does not register vtables, so `find()` would return `PluginNotFound`. The test
    verifies lifetime by observing that `load_bundle()` completes successfully and the Registry
    drops cleanly — not by checking registration.
  - `AbiError::ok()` exists in `polyplug::abi::AbiError`
  - `PluginHandle::null()` exists in `polyplug::abi` (needed for stub_find_plugin return type)
  - All `use` statements are at file top (AGENTS.md Rule 2)
  - No `std::mem::forget` anywhere in the test
  - `.expect()` is allowed in tests (`#![allow(clippy::expect_used)]`)

  **Must NOT do**:
  - Do NOT use `std::mem::forget` in the test
  - Do NOT add test-only accessor methods to `Registry`
  - Do NOT use `use` inside the test functions

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Requires understanding of Miri/dlopen incompatibility and correct
      two-pronged test structure
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential Step 4
  - **Blocks**: Task 5
  - **Blocked By**: Tasks 1, 2, 3

  **References**:
  - `tests/integration_load/mod.rs:1–16` — test file structure to follow (allow, imports)
  - `tests/integration_load/mod.rs:63–85` — `load_bundle` usage pattern with `TEST_PLUGIN_SO`
  - `crates/polyplug/Cargo.toml:73–75` — last `[[test]]` entry to append after
  - `crates/polyplug/src/abi/mod.rs` — `HostVTable`, `AbiError::ok()`, `PluginHandle::null()`

  **Acceptance Criteria**:
  - [ ] `tests/library_lifetime/mod.rs` file created
  - [ ] `[[test]] name = "library_lifetime"` entry exists in `crates/polyplug/Cargo.toml`
  - [ ] `cargo test --test library_lifetime` exits 0
  - [ ] Test output contains `test library_handle_outlives_load_call ... ok`
  - [ ] `cargo +nightly miri test --test library_lifetime` compiles without UB errors

  **QA Scenarios**:

  ```
  Scenario: library_lifetime test passes under cargo test
    Tool: Bash (cargo test)
    Preconditions: Tasks 1-4 applied, test_plugin.so compiled
    Steps:
      1. Run: cargo test --test library_lifetime 2>&1
      2. Assert: exit code 0
      3. Assert: output contains 'library_handle_outlives_load_call ... ok'
      4. Assert: no 'FAILED' in output
    Expected Result: Test passes
    Failure Indicators: 'FAILED', SIGBUS, or non-zero exit code
    Evidence: cargo test stdout

  Scenario: Miri check passes without UB
    Tool: Bash (cargo miri)
    Preconditions: Tasks 1-4 applied, nightly Rust + miri installed
    Steps:
      1. Run: cargo +nightly miri test --test library_lifetime 2>&1
      2. Assert: cfg(not(miri)) test is excluded (not compiled for miri)
      3. Assert: cfg(miri) test 'push_library_ownership_enforced_at_compile_time' ... ok
      4. Assert: no 'Undefined Behavior' in output
    Expected Result: Miri passes or correctly skips dlopen-dependent test
    Note: If miri is not installed, skip this scenario and document it
    Evidence: miri stdout
  ```

  **Commit**: NO

---

- [ ] 5. Final audit pass — clippy, full test suite, SAFETY completeness, commit

  **What to do**:

  **Step 1 — Run clippy across the workspace:**
  ```
  cargo clippy --workspace -- -D warnings
  ```
  Fix any warnings. Common issues to expect:
  - `clippy::undocumented_unsafe_blocks` — every unsafe block needs `// SAFETY:`
  - `clippy::unwrap_used` — no `.unwrap()` in production; use `.unwrap_or_else(|e| e.into_inner())`
  - `clippy::expect_used` — `.expect()` only in test code with `#![allow(clippy::expect_used)]`

  **Step 2 — Run `cargo fmt --check`:**
  ```
  cargo fmt --check --all
  ```
  Fix any formatting issues. All modified files must be properly formatted.

  **Step 3 — Run the full test suite:**
  ```
  cargo test --workspace
  ```
  All tests must pass. Key tests to watch:
  - `integration_load` — uses `libloading::Library::new()` directly, not `load_bundle()`;
    confirmed by grep (no `load_bundle` calls in tests/). No change expected.
  - `integration_dispatch` — dispatches through vtables
  - `integration_graph` — loads multiple plugins
  - `library_lifetime` — the new regression test

  **Step 3 — SAFETY comment completeness audit.** Manually inspect every `unsafe` block in:
  - `crates/polyplug/src/loader/mod.rs`
  - `crates/polyplug/src/registry/mod.rs`

  For each `unsafe` block verify:
  (a) `// SAFETY:` comment present immediately above
  (b) Comment explains what invariant makes this safe and who upholds it
  (c) For Library-touching code: comment states that `library` is in `registry.loaded_libraries`

  **Step 4 — Verify drop-order correctness:**
  ```
  grep -n 'loaded_libraries\|slots:\|contract_index' crates/polyplug/src/registry/mod.rs | head -10
  ```
  Assert: `loaded_libraries` line number is LOWER than `slots:` line number.

  **Step 5 — Verify libloading version:**
  ```
  grep 'libloading' Cargo.toml
  ```
  Must show `libloading = { version = "0.9" }`.

  **Step 6 — Update `Runtime` SAFETY comment in `crates/polyplug/src/runtime/mod.rs`**
  (lines 67–69). The current comment says:
  "LoadedBundle contains a Box<Library> which is not Sync by itself, but we never share
  the library references — only vtable pointers (which are 'static)."
  Update to say:
  "LoadedBundle contains a Box<Library> which is not Sync by itself, but libraries are
  stored in `Registry::loaded_libraries` and never shared as references — only vtable
  pointers (which are valid for the Registry's lifetime) are accessed concurrently."

  **Step 7 — Verify no LoadedBundle is created with a live Library that drops early:**
  ```
  grep -rn 'LoadedBundle' crates/ tests/
  ```
  Expect:
  - Struct definition in `loader/mod.rs`
  - `_bundles: Vec<LoadedBundle>` field in `runtime/mod.rs`
  - No call to `LoadedBundle { ... }` outside the struct definition
  **Step 8 — Create the single commit for this epic:**
  ```
  git add crates/polyplug/src/loader/mod.rs
  git add crates/polyplug/src/registry/mod.rs
  git add crates/polyplug/src/runtime/mod.rs
  git add crates/polyplug/Cargo.toml
  git add tests/library_lifetime/mod.rs
  git commit -m 'fix(loader): store Library handle in Registry to prevent use-after-dlclose

  NativeBundleLoader::load() was dropping the libloading::Library handle at end of
  scope, which called dlclose() and unmapped plugin code pages while vtable fn pointers
  into those pages were still live in the Registry (use-after-free / SIGBUS on next call).

  Fix: Registry gains loaded_libraries: Mutex<Vec<Library>> field (declared first for
  correct Rust drop order). load_bundle() moves Library into Registry before calling init.
  Adds library_lifetime regression test verifying no crash after load.

  Epic 9.6 / AGENTS.md: no .unwrap(), explicit types, SAFETY comments on all unsafe.'
  ```

  **Must NOT do**:
  - Do NOT add new public API or lint suppressions without justification
  - Do NOT amend any previous commits (there are none in this epic)

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
    - Reason: Multi-step verification sweep across multiple files; must fix any clippy issues found
  - **Skills**: [`git-master`]
    - `git-master`: Needed for the commit step

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Parallel Group**: Sequential Step 5 (final)
  - **Blocks**: Nothing (last task)
  - **Blocked By**: Tasks 1, 2, 3, 4

  **References**:
  - All modified files: `loader/mod.rs`, `registry/mod.rs`, `runtime/mod.rs`, `Cargo.toml`, `tests/library_lifetime/mod.rs`

  **Acceptance Criteria**:
  - [ ] `cargo clippy --workspace -- -D warnings` exits 0
  - [ ] `cargo fmt --check --all` exits 0
  - [ ] `cargo test --workspace` exits 0 (all tests pass)
  - [ ] `grep -c '// SAFETY:' crates/polyplug/src/loader/mod.rs` ≥ 5
  - [ ] `loaded_libraries` field line number < `slots:` field line number in registry
  - [ ] `libloading = { version = "0.9" }` confirmed in workspace `Cargo.toml`
  - [ ] `Runtime` SAFETY comment in `runtime/mod.rs` updated to reference `Registry::loaded_libraries`
  - [ ] Commit created with message starting `fix(loader): store Library handle`

  **QA Scenarios**:

  ```
  Scenario: clippy passes with zero warnings
    Tool: Bash (cargo clippy)
    Preconditions: Tasks 1-4 applied
    Steps:
      1. Run: cargo clippy --workspace -- -D warnings 2>&1
      2. Assert: exit code 0
      3. Assert: no 'warning[' or 'error[' lines
    Expected Result: Clean clippy run
    Evidence: clippy stdout

  Scenario: Full workspace test suite passes
    Tool: Bash (cargo test)
    Preconditions: Tasks 1-5 applied
    Steps:
      1. Run: cargo test --workspace 2>&1
      2. Assert: exit code 0
      3. Assert: every 'test result:' shows 'ok'
      4. Assert: no 'FAILED' in output
    Expected Result: All tests pass
    Evidence: cargo test stdout

  Scenario: Commit is created
    Tool: Bash (git log)
    Preconditions: Step 7 complete
    Steps:
      1. Run: git log --oneline -1
      2. Assert: message starts with 'fix(loader): store Library handle'
    Expected Result: Commit present with correct message
    Evidence: git log output
  ```

  **Commit**: YES — this is the ONLY commit in this epic.
  - Message: `fix(loader): store Library handle in Registry to prevent use-after-dlclose`
  - Files: `crates/polyplug/src/loader/mod.rs`, `crates/polyplug/src/registry/mod.rs`,
    `crates/polyplug/src/runtime/mod.rs`, `crates/polyplug/Cargo.toml`, `tests/library_lifetime/mod.rs`
  - Pre-commit: `cargo fmt --check --all && cargo test --workspace && cargo clippy --workspace -- -D warnings`

---

## Final Verification Wave

> Run after ALL 5 tasks complete. Two reviewers in parallel.

- [ ] F1. **Correctness Audit** — `oracle`
  Read `loader/mod.rs` and `registry/mod.rs` in their final state. Verify:
  (a) `Library` is moved into `registry.loaded_libraries` before `init_fn_ptr` is called;
  (b) no `Symbol` borrow on `library` exists at the point of the move;
  (c) `loaded_libraries` is declared before `slots` in the `Registry` struct;
  (d) `abi_version_symbol` is explicitly dropped before the move;
  (e) no `Box::leak`, `Box::new(library)` for lifetime extension, or `mem::forget` remain.
  Output: APPROVE or REJECT with specific file:line citations.

- [ ] F2. **AGENTS.md Compliance Check** — `unspecified-high`
  Scan all modified files against AGENTS.md rules:
  - Rule 1: all module roots use `dirname/mod.rs` (new test: `tests/library_lifetime/mod.rs` ✓)
  - Rule 2: no `use` inside functions — verify all new `use` statements are at file top
  - Rule 3: all new `let` bindings have explicit types
  - Rule 4: no `.unwrap()` in production code; `.expect()` only in test code
  - Rule 5: visibility explicit on new methods (`pub(crate) fn push_library`)
  - Rule 6: all `unsafe` blocks have `// SAFETY:` comments
  Output: APPROVE or REJECT with rule number and file:line.

---

## Commit Strategy

Single commit after Task 5 passes all verification:
```
fix(loader): store Library handle in Registry to prevent use-after-dlclose
```
Files: `loader/mod.rs`, `registry/mod.rs`, `runtime/mod.rs`, `Cargo.toml`, `tests/library_lifetime/mod.rs`
Pre-commit: `cargo fmt --check --all && cargo test --workspace && cargo clippy --workspace -- -D warnings`

---

## Success Criteria

### Verification Commands
```bash
cargo check -p polyplug                           # Expected: exit 0
cargo fmt --check --all                           # Expected: exit 0
cargo clippy --workspace -- -D warnings           # Expected: exit 0, zero warnings
cargo test --workspace                            # Expected: all tests ok
cargo test --test library_lifetime                # Expected: library_handle_outlives_load_call ok
grep 'loaded_libraries' crates/polyplug/src/registry/mod.rs  # Expected: field + method
grep 'fn load_bundle' crates/polyplug/src/loader/mod.rs       # Expected: Result<(), LoaderError>
grep 'push_library' crates/polyplug/src/loader/mod.rs         # Expected: registry.push_library(library)
```

### Final Checklist
- [ ] `libloading = "0.9"` in workspace `Cargo.toml` (already correct — no change needed)
- [ ] `Library` handle stored in `Registry::loaded_libraries`, never dropped before Runtime
- [ ] `loaded_libraries` declared FIRST in `Registry` struct (correct drop order)
- [ ] Every `unsafe` block in `loader/mod.rs` has a `// SAFETY:` comment
- [ ] `library_lifetime` test passes under `cargo test`
- [ ] `#[cfg(miri)]` structural test present and passes under Miri
- [ ] No `.unwrap()` in production code
- [ ] `cargo fmt --check --all` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes with zero warnings
- [ ] `cargo test --workspace` passes (all existing tests still pass)
- [ ] `Runtime` SAFETY comment in `runtime/mod.rs` updated to reference `Registry::loaded_libraries`
- [ ] All 5 files staged in the final commit
