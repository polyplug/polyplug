# Work Plan: Loader FFI Epic v10 — COMPLETE SINGLE-FILE PLAN

## TL;DR

> **Core Objective**: Enable non-Rust hosts (C++, Python, Lua, Deno/JS, C#) to load non-native guest plugins (Python, Lua, .NET, JS) via unified FFI surface from `libpolyplug.so`.
>
> **Problem Statement**: BundleLoader trait is Rust-only; non-Rust hosts calling `libpolyplug.so` via FFI only get NativeBundleLoader. They cannot load non-native guests, contradicting "any language can be a host that loads any guest."
>
> **Solution**: Add interior mutability to Runtime (RwLock, freeze semantics), create unified `polyplug_ffi` cdylib exporting loader registration functions from single `libpolyplug.so`.
>
> **Design**: Option A - abi.hpp canonical ABI (`polyplug_runtime_new/free`, `polyplug_rt_find_by_contract`, etc.)
>
> **Architecture**:
> - `polyplug` crate: rlib only (core runtime)
> - `polyplug_ffi` crate: cdylib producing `libpolyplug.so`
> - Loader crates: rlib, depend on polyplug
> - Hosts: Link single `libpolyplug.so`, call registration functions
>
> **Deliverables**:
> - Core runtime with interior mutability + freeze semantics
> - Unified FFI exports from `libpolyplug.so`
> - 5 loader registration functions
> - 5 host library updates
> - 6 real host examples (replacing fake Rust cdylib hosts)
> - 14 guest examples (2 per language × 7 languages)
> - Updated build.sh and README.md
>
> **Total**: 40 tasks + 4 quality checks across 10 waves
> **Estimated Effort**: Large (multi-crate refactor)
> **Parallel Execution**: YES — ~60% parallelizable
> **Critical Path**: Core runtime → FFI crate → Loader exports → Host libs → Examples → Integration
>
> **Execute**: `/start-work`

---

## TABLE OF CONTENTS

1. [Context](#context)
2. [Architecture](#architecture)
3. [Work Objectives](#work-objectives)
4. [Verification Strategy](#verification-strategy)
5. [Execution Waves](#execution-waves)
6. [TODOs](#todos)
7. [Commit Strategy](#commit-strategy)
8. [Success Criteria](#success-criteria)

---

## CONTEXT

### Original Request

This is a v1-blocking epic that fixes the architectural gap preventing non-Rust host applications from loading non-native guests. The BundleLoader trait is Rust-only; non-Rust hosts call `libpolyplug.so` via FFI which only contains NativeBundleLoader.

### Design Evolution

**v1 (REJECTED)**: `polyplug_runtime_register_loader(rt, loader_ptr)` — impossible because `dyn Trait` is a fat pointer (data + vtable) that cannot cross FFI.

**v2 (REJECTED)**: Per-loader cdylibs exporting registration — unsafe cross-dylib Rust ABI (different compilations = incompatible vtables).

**v3-v7 (REJECTED)**: Various fixes for Cargo cycles, symbol mismatches, C++ header inconsistencies, vague QA.

**v8 (REJECTED)**: Complete QA but some commands not copy-pasteable.

**v9-v10 (APPROVED)**: Momus-provided exact QA commands.

### Selected Design: Option A (Momus-approved)

**Canonical C ABI**: `host-libs/cpp/polyplug/abi.hpp`
- `polyplug_runtime_new/free`
- `polyplug_load_bundle/reload_bundle`
- `polyplug_rt_find_by_contract/find_by_bundle/find_all/resolve`
- `polyplug_guard_free/get_vtable`
- `polyplug_last_error/error_message_len`

**Rationale**:
- Already used by Python, Lua, JS, C# host libs
- Most complete surface (explicit handles, bundle operations)
- Fewest changes required

**Non-canonical (legacy)**: `crates/polyplug/src/lib.rs` exports
- `polyplug_runtime_init/destroy` — NOT used by host libs
- Keep for Rust-internal use only

---

## ARCHITECTURE

### Final Crate Structure

```
crates/
├── polyplug/              # Core runtime (rlib only)
│   ├── src/runtime.rs     # RwLock, freeze semantics, register_loader_boxed
│   ├── src/loader/mod.rs  # BundleLoader trait
│   └── Cargo.toml         # crate-type = ["rlib"]
├── polyplug_ffi/          # NEW: FFI cdylib
│   ├── src/lib.rs         # C ABI exports (from ffi.rs)
│   ├── src/ffi_core.rs    # Core FFI: runtime_new/free, load_bundle, etc.
│   ├── src/ffi_loaders.rs # Loader registration exports
│   └── Cargo.toml         # crate-type = ["cdylib"], name = "polyplug"
│                            # deps: polyplug + loader crates (optional)
└── polyplug_*/            # Loader crates (rlib)
    ├── polyplug_dotnet/
    ├── polyplug_python/
    ├── polyplug_lua/
    ├── polyplug_js/
    └── polyplug_js_deno/
```

### Dependencies (Cycle-Free)

```
polyplug_ffi (cdylib)
├── polyplug (rlib)
├── polyplug_dotnet (rlib, optional)
├── polyplug_python (rlib, optional)
├── polyplug_lua (rlib, optional)
├── polyplug_js (rlib, optional)
└── polyplug_js_deno (rlib, optional)

polyplug_dotnet (rlib)
└── polyplug (rlib)

polyplug_python (rlib)
└── polyplug (rlib)

... etc
```

### Export Strategy

`libpolyplug.so` exports from `polyplug_ffi`:

```
# Core FFI (from ffi.rs)
polyplug_runtime_new
polyplug_runtime_free
polyplug_load_bundle
polyplug_reload_bundle
polyplug_rt_find_by_contract
polyplug_rt_find_by_bundle
polyplug_rt_find_all_by_contract
polyplug_rt_resolve_plugin
polyplug_guard_free
polyplug_get_vtable
polyplug_last_error
polyplug_error_message_len

# Loader Registration (new)
polyplug_dotnet_runtime_register_loader
polyplug_python_runtime_register_loader
polyplug_lua_runtime_register_loader
polyplug_js_runtime_register_loader
polyplug_js_deno_runtime_register_loader
```

---

## WORK OBJECTIVES

### Core Objective

Enable non-Rust hosts to register and use non-native loaders via FFI from unified `libpolyplug.so`.

### Concrete Deliverables

- [ ] Core runtime with interior mutability (RwLock)
- [ ] Freeze semantics (AtomicBool, no registration after first load)
- [ ] `register_loader_boxed` method
- [ ] New `polyplug_ffi` cdylib crate
- [ ] Canonical ABI exports moved to `polyplug_ffi`
- [ ] 5 loader registration functions exported
- [ ] 5 host library loader registration wrappers
- [ ] 6 real host examples
- [ ] 14 guest examples
- [ ] Updated build script and documentation

### Definition of Done

- [ ] Non-Rust host (e.g., Python) can create runtime, register Python loader, load Python guest
- [ ] All 6 hosts produce identical output when run (14 lines each)
- [ ] `cargo build --release -p polyplug_ffi --features all-loaders` succeeds
- [ ] All existing tests pass (`cargo test --workspace`)
- [ ] No `.unwrap()` in production code
- [ ] All `unsafe` blocks have `// SAFETY:` comments

### Must Have

- Core runtime interior mutability
- Freeze semantics
- Duplicate loader name detection
- 5 per-loader FFI registration functions
- Loader registration in all 5 host libraries
- 6 real host examples
- 14 guest examples
- Working build script

### Must NOT Have (Guardrails)

- No `polyplug_runtime_register_loader` with void pointer (impossible FFI pattern)
- No separate loader cdylibs
- No cross-dylib Rust type passing
- No changes to existing FFI symbols used by host libs
- No changes to ABI structs
- No RuntimeBuilder exposed via FFI
- No fake Rust cdylib hosts remaining

---

## VERIFICATION STRATEGY

### Test Infrastructure

- **Framework**: Rust built-in test + bash scripts
- **Integration**: Existing `tests/integration` crate
- **QA Policy**: Every task has executable QA with concrete commands

### QA Pattern

Each task QA includes:
- **Tool**: Bash command(s)
- **Steps**: Exact commands to run
- **Expected Result**: Exit code or specific output
- **Evidence**: Observable result

---

## EXECUTION WAVES

### ONE-TIME SETUP (Before All QA)

```bash
# Build all example guest bundles
./examples/build.sh

# Build full-loader shared library (used by non-Rust hosts)
cargo build --manifest-path examples/hosts/js/Cargo.toml

# Build Lua companion cdylib
cargo build --manifest-path examples/hosts/lua/Cargo.toml
```

---

## TODOs

### WAVE 0: Foundation [PARALLEL GROUP: FOUNDATION]

> **Blockers**: None — can start immediately
> **Parallelism**: Tasks 0-3 sequential (build on each other)
> **Group Completes When**: Runtime supports post-initialization loader registration

- [ ] **Task 0: Change polyplug to rlib-only**

  **What to do**: Edit `crates/polyplug/Cargo.toml` to remove cdylib crate-type.

  **Implementation**:
  - Change: `crate-type = ["cdylib", "rlib"]`
  - To: `crate-type = ["rlib"]`

  **Must NOT do**: Leave cdylib (causes artifact conflict with polyplug_ffi)

  **Recommended Agent Profile**:
  - **Category**: `quick` (simple config change)

  **Acceptance Criteria**:
  - [ ] `crates/polyplug/Cargo.toml` has `crate-type = ["rlib"]`

  **QA Scenario**:
  ```bash
  cd /mnt/data/Projects/Utils/polyplug
  cargo build --release -p polyplug
  test ! -f target/release/libpolyplug.so
  echo $?
  # Expected: 0 (command succeeds, file does not exist)
  ```

  **Commit**: `build(polyplug): make rlib-only to avoid artifact conflict`

- [ ] **Task 1: Add RwLock to Runtime loaders**

  **What to do**: Change `loaders` field to use interior mutability.

  **Implementation**:
  - File: `crates/polyplug/src/runtime.rs`
  - Change: `loaders: HashMap<String, Box<dyn BundleLoader>>`
  - To: `loaders: std::sync::RwLock<HashMap<String, Box<dyn BundleLoader>>>`
  - Add: `has_loaded_any_bundle: std::sync::atomic::AtomicBool`

  **Must NOT do**: Change public API signatures

  **Acceptance Criteria**:
  - [ ] Compiles successfully
  - [ ] Existing tests pass

  **QA Scenario**:
  ```bash
  cargo build --release -p polyplug
  echo $?
  # Expected: 0
  ```

  **Commit**: `refactor(runtime): wrap loaders in RwLock for interior mutability`

- [ ] **Task 2: Add register_loader_boxed method**

  **What to do**: Add public method to register loaders at runtime with freeze protection.

  **Implementation**:
  ```rust
  impl Runtime {
      pub fn register_loader_boxed(
          &self,
          loader: Box<dyn BundleLoader>,
      ) -> Result<(), PolyplugError> {
          // Check freeze flag
          if self.has_loaded_any_bundle.load(Ordering::SeqCst) {
              return Err(PolyplugError::RuntimeFrozen);
          }
          let name = loader.runtime_name().to_string();
          let mut loaders = self.loaders.write()?;
          if loaders.contains_key(&name) {
              return Err(PolyplugError::LoaderAlreadyRegistered(name));
          }
          loaders.insert(name, loader);
          Ok(())
      }
  }
  ```

  **Acceptance Criteria**:
  - [ ] Method compiles
  - [ ] Rejects when frozen
  - [ ] Rejects duplicates

  **QA Scenario**:
  ```bash
  cargo test --package polyplug
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(runtime): add register_loader_boxed with freeze semantics`

- [ ] **Task 3: Set freeze flag on bundle load**

  **What to do**: Set AtomicBool at start of bundle loading.

  **Implementation**:
  - In `load_bundle_with()`: set flag before loading

  **Acceptance Criteria**:
  - [ ] Flag set on first load
  - [ ] Registration rejected after

  **QA Scenario**:
  ```bash
  cargo test --package polyplug
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(runtime): set freeze flag on first bundle load`

---

### WAVE 1: FFI Crate [PARALLEL GROUP: FFI_CRATE]

> **Blockers**: Tasks 0-3
> **Parallelism**: Tasks 4-5 sequential
> **Group Completes When**: polyplug_ffi crate exists and builds

- [ ] **Task 4: Create polyplug_ffi crate structure**

  **What to do**: Create new crate `crates/polyplug_ffi/`.

  **Implementation**:
  Create `crates/polyplug_ffi/Cargo.toml`:
  ```toml
  [package]
  name = "polyplug_ffi"
  version = "0.1.0"
  edition = "2021"
  
  [lib]
  name = "polyplug"
  crate-type = ["cdylib"]
  
  [dependencies]
  polyplug = { workspace = true }
  polyplug_dotnet = { workspace = true, optional = true }
  polyplug_python = { workspace = true, optional = true }
  polyplug_lua = { workspace = true, optional = true }
  polyplug_js = { workspace = true, optional = true }
  polyplug_js_deno = { workspace = true, optional = true }
  
  [features]
  default = []
  all-loaders = ["dotnet", "python", "lua", "js", "js-deno"]
  dotnet = ["dep:polyplug_dotnet"]
  python = ["dep:polyplug_python"]
  lua = ["dep:polyplug_lua"]
  js = ["dep:polyplug_js"]
  js-deno = ["dep:polyplug_js_deno"]
  ```

  **Acceptance Criteria**:
  - [ ] Crate structure created
  - [ ] Builds successfully

  **QA Scenario**:
  ```bash
  cargo build --release -p polyplug_ffi
  echo $?
  # Expected: 0
  ```

  **Commit**: `build(polyplug_ffi): create cdylib crate`

- [ ] **Task 5: Export canonical ABI**

  **What to do**: Move C ABI exports from `crates/polyplug/src/ffi.rs` to `polyplug_ffi`.

  **Exports to move**:
  - `polyplug_runtime_new`
  - `polyplug_runtime_free`
  - `polyplug_load_bundle`
  - `polyplug_reload_bundle`
  - `polyplug_rt_find_by_contract`
  - `polyplug_rt_find_by_bundle`
  - `polyplug_rt_find_all_by_contract`
  - `polyplug_rt_resolve_plugin`
  - `polyplug_guard_free`
  - `polyplug_get_vtable`
  - `polyplug_last_error`
  - `polyplug_error_message_len`

  **Acceptance Criteria**:
  - [ ] All exports present in libpolyplug.so

  **QA Scenario**:
  ```bash
  cargo build --release -p polyplug_ffi
  nm -D target/release/libpolyplug.so | grep polyplug_runtime_new
  # Expected: shows exported symbol
  ```

  **Commit**: `feat(ffi): export canonical ABI from polyplug_ffi`

---

### WAVE 2: Loader Registration [PARALLEL GROUP: LOADER_EXPORTS]

> **Blockers**: Task 5
> **Parallelism**: Tasks 6-10 can run in parallel
> **Group Completes When**: All loader registration functions exported

- [ ] **Task 6: Export dotnet loader registration**

  **Implementation**:
  ```rust
  #[cfg(feature = "dotnet")]
  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_dotnet_runtime_register_loader(
      rt: *mut OpaqueRuntime,
      cfg: *const PolyplugDotnetConfig,
  ) -> u32 {
      // Implementation
  }
  ```

  **QA Scenario**:
  ```bash
  cargo build --release -p polyplug_ffi --features dotnet
  nm -D target/release/libpolyplug.so | grep polyplug_dotnet_runtime_register_loader
  # Expected: shows symbol
  ```

  **Commit**: `feat(ffi): export dotnet loader registration`

- [ ] **Task 7: Export python loader registration**
  - Same pattern
  - **Commit**: `feat(ffi): export python loader registration`

- [ ] **Task 8: Export lua loader registration**
  - Same pattern
  - **Commit**: `feat(ffi): export lua loader registration`

- [ ] **Task 9: Export js loader registration**
  - Same pattern
  - **Commit**: `feat(ffi): export js loader registration`

- [ ] **Task 10: Export js_deno loader registration**
  - Same pattern
  - **Commit**: `feat(ffi): export js_deno loader registration`

---

### WAVE 3: Host Libraries [PARALLEL GROUP: HOSTS]

> **Blockers**: Tasks 6-10
> **Parallelism**: Tasks 11-15 parallel
> **Group Completes When**: All host libraries can register all 5 loaders

- [ ] **Task 11: Fix C++ host lib**

  **What to do**: Align `runtime.hpp` with `abi.hpp` canonical ABI.

  **Changes**:
  - Replace `polyplug_runtime_init/destroy` with `polyplug_runtime_new/free`
  - Replace `polyplug_find_plugin` with `polyplug_rt_find_by_contract`
  - Create `loaders.hpp`

  **QA Scenario**:
  ```bash
  echo '#include <polyplug/runtime.hpp>' | \
    g++ -c -I host-libs/cpp -x c++ - -o /tmp/test.o
  echo $?
  # Expected: 0
  ```

  **Commit**: `fix(cpp): align runtime.hpp with canonical ABI`

- [ ] **Task 12: Python host lib**

  **Implementation**: Create `host-libs/python/polyplug/loaders.py`

  **QA Scenario**:
  ```bash
  PYTHONPATH="host-libs/python" \
  POLYPLUG_LIB="target/release/libpolyplug.so" \
  python3 -c '
  from polyplug import Runtime
  from polyplug.loaders import register_all_loaders
  rt = Runtime()
  register_all_loaders(rt)
  rt.load_bundle("examples/guests/python/decoder")
  print("OK: python loader registered + bundle loaded")
  '
  # Expected: exits 0, prints OK message
  ```

  **Commit**: `feat(python): add loader registration`

- [ ] **Task 13: Lua host lib**

  **Implementation**: Add to `host-libs/lua/polyplug.lua`

  **QA Scenario**:
  ```bash
  POLYPLUG_SO="target/release/libpolyplug.so" \
  luajit -e '
  local polyplug = dofile("host-libs/lua/polyplug.lua")
  polyplug.load_lib(os.getenv("POLYPLUG_SO"))
  local rt = polyplug.Runtime.new()
  rt:load_bundle("examples/guests/lua/validator")
  print("OK: lua bundle loaded")
  '
  # Expected: exits 0, prints OK message
  ```

  **Commit**: `feat(lua): add loader registration`

- [ ] **Task 14: Deno/JS host lib**

  **Implementation**: Add to `host-libs/js/polyplug.ts`

  **QA Scenario**:
  ```bash
  export POLYPLUG_SO="$PWD/target/release/libpolyplug.so"
  export TEST_PLUGIN_DIR="$PWD/tests/fixtures/test_plugin_dir"
  deno test --allow-ffi --allow-env --allow-read host-libs/js/polyplug_test.ts
  # Expected: exits 0
  ```

  **Commit**: `feat(js): add loader registration`

- [ ] **Task 15: C# host lib**

  **Implementation**: Add to `host-libs/csharp/src/Runtime.cs`

  **QA Scenario**:
  ```bash
  dotnet build host-libs/csharp/Polyplug.csproj
  # Expected: exits 0
  ```

  **Commit**: `feat(csharp): add loader registration`

---

### WAVE 4: Examples - Hosts [PARALLEL GROUP: EXAMPLE_HOSTS]

> **Blockers**: Tasks 11-15
> **Parallelism**: Tasks 16-23 mostly parallel

- [ ] **Task 16: Delete fake hosts**

  **What to do**: Remove fake Rust cdylib hosts.

  **Files to delete**:
  - `examples/hosts/lua/Cargo.toml`
  - `examples/hosts/lua/Cargo.lock`
  - `examples/hosts/lua/src/lib.rs`
  - `examples/hosts/lua/polyplug_full.lua`
  - `examples/hosts/js/Cargo.toml`
  - `examples/hosts/js/Cargo.lock`
  - `examples/hosts/js/src/lib.rs`
  - `examples/hosts/js/polyplug_full.map`
  - `examples/hosts/js/build.rs`
  - Rename `examples/hosts/js/` to `examples/hosts/js_deno/`

  **QA Scenario**:
  ```bash
  find examples/hosts -name "Cargo.toml" -not -path "*/rust/*" | wc -l
  # Expected: 0
  ```

  **Commit**: `chore(examples): remove fake Rust cdylib hosts`

- [ ] **Task 17: Rust host example**

  **What**: Create `examples/hosts/rust/src/main.rs`

  **QA Scenario**:
  ```bash
  mkdir -p examples/_out
  cargo run --manifest-path examples/hosts/rust/Cargo.toml > examples/_out/rust.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add Rust host`

- [ ] **Task 18: C++ host example**

  **QA Scenario**:
  ```bash
  make -C examples/hosts/cpp
  mkdir -p examples/_out
  ./examples/hosts/cpp/polyplug_host_cpp > examples/_out/cpp.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add C++ host`

- [ ] **Task 19: C# host example**

  **QA Scenario**:
  ```bash
  export POLYPLUG_SO="$PWD/target/release/libpolyplug.so"
  mkdir -p examples/_out
  dotnet run --project examples/hosts/csharp/PolyplugHost.csproj > examples/_out/csharp.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add C# host`

- [ ] **Task 20: Python host example**

  **QA Scenario**:
  ```bash
  mkdir -p examples/_out
  PYTHONPATH="host-libs/python" \
  POLYPLUG_LIB="$PWD/target/release/libpolyplug.so" \
  python3 examples/hosts/python/host.py > examples/_out/python.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add Python host`

- [ ] **Task 21: Lua host example**

  **QA Scenario**:
  ```bash
  mkdir -p examples/_out
  luajit examples/hosts/lua/host.lua > examples/_out/lua.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add Lua host`

- [ ] **Task 22: Deno host example**

  **QA Scenario**:
  ```bash
  export POLYPLUG_SO="$PWD/target/release/libpolyplug.so"
  mkdir -p examples/_out
  deno run --allow-ffi --allow-env --allow-read examples/hosts/js/host.ts > examples/_out/js.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples): add Deno host`

- [ ] **Task 23: Golden output**

  **What**: Save Rust host output as golden file.

  **QA Scenario**:
  ```bash
  cargo run --manifest-path examples/hosts/rust/Cargo.toml > examples/expected_output.txt
  test -f examples/expected_output.txt
  echo $?
  # Expected: 0
  ```

  **Commit**: `test(examples): add golden output file`

---

### WAVE 5: Examples - Diff Verification [PARALLEL GROUP: DIFF]

> **Blockers**: Task 23

- [ ] **Task 24: Verify all hosts match golden**

  **QA per host**:
  ```bash
  diff -u examples/expected_output.txt examples/_out/<host>.txt
  echo $?
  # Expected: 0 (no differences)
  ```

  **Commit**: `test(examples): verify all hosts match golden output`

---

### WAVE 6: Guest Examples [PARALLEL GROUP: GUESTS]

> **Blockers**: None (can parallel with Waves 4-5)

- [ ] **Task 25: Rust guests**

  **QA**: `cargo build --release` in `rust/decoder` and `rust/reporter`
  **Commit**: `feat(examples): add Rust guests`

- [ ] **Task 26: C++ guests**

  **QA**: `make` in `cpp/decoder` and `cpp/reporter`
  **Commit**: `feat(examples): add C++ guests`

- [ ] **Task 27: C# guests**

  **QA**: `dotnet build` in `csharp/encoder` and `csharp/reporter`
  **Commit**: `feat(examples): add C# guests`

- [ ] **Task 28: Python guests**

  **QA**: `python -m py_compile python/decoder/plugin.py`
  **Commit**: `feat(examples): add Python guests`

- [ ] **Task 29: Lua guests**

  **QA**: `luajit -b lua/transformer/plugin.lua /tmp/test.luac`
  **Commit**: `feat(examples): add Lua guests`

- [ ] **Task 30: JS QuickJS guests**

  **QA**: `test -f js_quickjs/transformer/bundle.js`
  **Commit**: `feat(examples): add JS QuickJS guests`

- [ ] **Task 31: JS Deno guests**

  **QA**: `deno check js_deno/transformer/plugin.ts`
  **Commit**: `feat(examples): add JS Deno guests`

---

### WAVE 7: Build & Docs [PARALLEL GROUP: BUILD]

> **Blockers**: Tasks 25-31

- [ ] **Task 32: Update api.toml**

  **QA**: `polyplugc validate --api examples/api.toml`
  **Commit**: `docs(examples): update api.toml`

- [ ] **Task 33: Update build.sh**

  **QA**: `./examples/build.sh`
  **Commit**: `build(examples): update build.sh`

- [ ] **Task 34: Update README.md**

  **QA**: `grep -q "hosts/rust" examples/README.md`
  **Commit**: `docs(examples): update README`

---

### WAVE 8: Integration Tests [PARALLEL GROUP: INTEGRATION]

> **Blockers**: Tasks 17-22, 32-34

- [ ] **Task 35-40: Integration tests**

  **Use existing harness**:
  ```bash
  cargo test -p integration
  cargo test -p integration --test integration_loader_dispatch
  cargo test -p integration --test integration_python
  cargo test -p integration --test integration_lua
  cargo test -p integration --test integration_js
  cargo test -p integration --test integration_dotnet
  cargo test -p integration --test cross_language
  ```
  **Expected**: each exits 0

  **Commit**: `test(integration): verify all integration tests pass`

---

### FINAL VERIFICATION WAVE

> **Blockers**: Tasks 35-40
> **Parallelism**: F1-F4 can run in parallel

- [ ] **F1: Clippy**

  **QA**:
  ```bash
  cargo clippy --workspace -- -D warnings
  echo $?
  # Expected: 0
  ```

- [ ] **F2: SAFETY comments**

  **QA**:
  ```bash
  grep -r "unsafe {" --include="*.rs" crates/ | grep -v "// SAFETY:" | wc -l
  # Expected: 0
  ```

- [ ] **F3: No unwrap**

  **QA**:
  ```bash
  grep -r "\.unwrap()" --include="*.rs" crates/*/src/ | grep -v "#\[cfg(test)\]" | wc -l
  # Expected: 0
  ```

- [ ] **F4: Full test suite**

  **QA**:
  ```bash
  cargo test --workspace
  echo $?
  # Expected: 0
  ```

---

## COMMIT STRATEGY

1. `build(polyplug): make rlib-only`
2. `refactor(runtime): interior mutability`
3. `feat(runtime): register_loader_boxed`
4. `feat(runtime): freeze semantics`
5. `build(polyplug_ffi): create cdylib crate`
6. `feat(ffi): export canonical ABI`
7-11. `feat(ffi): export <loader> registration` (5 commits)
12. `fix(cpp): align with canonical ABI`
13-16. `feat(host-libs): <lang> loader registration` (4 commits)
17. `chore(examples): remove fake hosts`
18-23. `feat(examples): <lang> host` (6 commits)
24. `test(examples): golden output`
25. `test(examples): verify hosts match golden`
26-31. `feat(examples): <lang> guests` (7 commits)
32. `docs(examples): update api.toml`
33. `build(examples): update build.sh`
34. `docs(examples): update README`
35. `test(integration): verify all tests`
36-39. Quality commits (if needed)

---

## SUCCESS CRITERIA

1. [ ] `cargo build --release -p polyplug_ffi --features all-loaders` succeeds
2. [ ] `nm -D target/release/libpolyplug.so` shows core + loader registration symbols
3. [ ] All 6 hosts produce output matching golden file (`diff exits 0`)
4. [ ] `cargo test -p integration` passes
5. [ ] `cargo clippy --workspace -- -D warnings` passes
6. [ ] `cargo test --workspace` passes
7. [ ] No `unsafe` blocks without `// SAFETY:` comments
8. [ ] No `.unwrap()` in production code
