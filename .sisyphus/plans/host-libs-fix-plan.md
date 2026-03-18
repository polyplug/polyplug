# Unified Host Libs Zero-Overhead Fix Plan

## Goal

Fix all host libs (C++, Python, JavaScript, Lua, Rust) for zero-overhead hot path: one indirect function call, nothing more.

---

## Phase 0: Critical Bug Fix

### [BLOCKING - Must Complete First]

- [ ] Fix Lua `find_by_bundle` to call actual FFI function instead of returning `1`
  - **File:** `host-libs/lua/polyplug.lua` lines 87-91
  - **Change:** Replace `return ffi.cast("uint64_t", 1)` with actual `lib.polyplug_runtime_find_by_bundle()` call
  - **Verification:** Run `tests/lua/` tests, verify `find_by_bundle` returns real handles
  - **Blocker:** None

---

## Phase 1: Core Infrastructure (PluginGuard + Type Caching)

### [PARALLEL GROUP: ALL HOST LIBS]

#### C++

- [ ] Add `PluginGuard` class to `host-libs/cpp/polyplug/runtime.hpp` with cached vtable
  - **Verification:** Class compiles, has `vtable()` method returning cached pointer
  - **Blocker:** None

#### Python

- [ ] Add module-level `_DISPATCH_FN_TYPE` and `_VTableStruct` in `host-libs/python/polyplug/runtime.py`
  - **Verification:** Types defined at module top, not inside functions
  - **Blocker:** None

- [ ] Add module-level `_init_lib_bindings()` function with initialized flag
  - **Verification:** Function sets `argtypes`/`restype` once, guarded by flag
  - **Blocker:** None

#### JavaScript

- [ ] Add module-level `_DISPATCH_FN_TYPE` and `_funcCache` in `host-libs/js/polyplug.js`
  - **Verification:** Variables defined at module level
  - **Blocker:** None

#### Lua

- [ ] Add module-level `VTableType`, `DispatchFnType`, and `func_cache` in `host-libs/lua/polyplug.lua`
  - **Verification:** Variables defined at module level before any functions
  - **Blocker:** None

- [ ] Create `Guard` metatable with cached vtable in `host-libs/lua/polyplug.lua`
  - **Verification:** `Guard.new()` caches vtable, `vtable()` returns cached value
  - **Blocker:** None

#### Rust

- [ ] Add `PluginGuard` struct with cached vtable in `host-libs/rust/src/lib.rs`
  - **Verification:** Struct stores guard pointer and cached vtable, implements `Drop`
  - **Blocker:** None

---

## Phase 2: VTable Caching Implementation

### [PARALLEL GROUP: ALL HOST LIBS - depends on Phase 1]

#### C++

- [ ] Update `Runtime::resolve_plugin` to return `PluginGuard` with cached vtable
  - **Verification:** Method returns `PluginGuard`, vtable cached at construction
  - **Blocker:** PluginGuard class exists

#### Python

- [ ] Update `PluginGuard.__init__` to cache vtable at construction
  - **Verification:** Constructor calls `polyplug_runtime_plugin_vtable` once
  - **Blocker:** Module-level types exist

- [ ] Change `get_vtable()` to return cached `_vtable` property
  - **Verification:** No FFI call in `get_vtable()`
  - **Blocker:** VTable cached in init

#### JavaScript

- [ ] Update `Guard` class to cache vtable in constructor
  - **Verification:** `#vtable` field populated by calling `polyplug_runtime_plugin_vtable` once
  - **Blocker:** Module-level types exist

- [ ] Change `vtable()` method to return cached `#vtable`
  - **Verification:** Method returns `this.#vtable` without FFI call
  - **Blocker:** VTable cached in constructor

#### Lua

- [ ] Update `Runtime:resolve_plugin` to return `Guard` instance
  - **Verification:** Returns `Guard.new(lib, guard_ptr)`, not raw pointer
  - **Blocker:** Guard metatable exists

#### Rust

- [ ] Add `Runtime::resolve_plugin` method returning `PluginGuard`
  - **Verification:** Method returns `Option<PluginGuard>`, vtable cached
  - **Blocker:** PluginGuard struct exists

---

## Phase 3: Function Pointer Caching

### [PARALLEL GROUP: PYTHON, JS, LUA - depends on Phase 1]

#### Python

- [ ] Add `_func_cache: dict[int, ctypes._CFuncPtr] = {}` module-level cache
  - **Verification:** Cache dict exists at module level
  - **Blocker:** Phase 1 types exist

- [ ] Rewrite `call_plugin_fn` in `host-libs/python/polyplug/helpers.py` to use cache
  - **Verification:** Function checks cache before creating new `CFUNCTYPE`
  - **Blocker:** Cache exists

#### JavaScript

- [ ] Rewrite `callPluginFn` to check `_funcCache` before creating `UnsafeFnPointer`
  - **Verification:** Function checks cache, creates only if missing
  - **Blocker:** Cache exists

#### Lua

- [ ] Rewrite `call_plugin_fn` to accept `vtable_ptr` parameter (no resolve inside)
  - **Verification:** Function signature changed, no `resolve_plugin` call inside
  - **Blocker:** None

- [ ] Add cache lookup for function pointer in `call_plugin_fn`
  - **Verification:** `func_cache[func_ptr]` checked before `ffi.cast`
  - **Blocker:** Cache exists

#### C++

- [ ] No function pointer caching needed - direct vtable access is optimal
  - **Verification:** N/A
  - **Blocker:** None

#### Rust

- [ ] No function pointer caching needed - transmute is zero-cost
  - **Verification:** N/A
  - **Blocker:** None

---

## Phase 4: Loader Restructuring (Separate Packages)

### [PARALLEL GROUP: ALL LOADERS - no blockers]

### C++ Loaders

- [ ] Create `host-libs/cpp/loaders/native/` with `CMakeLists.txt` and header
- [ ] Create `host-libs/cpp/loaders/python/` package
- [ ] Create `host-libs/cpp/loaders/lua/` package
- [ ] Create `host-libs/cpp/loaders/js/` package
- [ ] Create `host-libs/cpp/loaders/js_deno/` package
- [ ] Create `host-libs/cpp/loaders/dotnet/` package
- [ ] Remove `polyplug/loaders/*.hpp` from main package
  - **Verification:** Each loader builds standalone with `cmake --build .`
  - **Blocker:** None

### Python Loaders

- [ ] Create `host-libs/python/loaders/polyplug-loaders-native/` with `pyproject.toml`
- [ ] Create `host-libs/python/loaders/polyplug-loaders-python/` package
- [ ] Create `host-libs/python/loaders/polyplug-loaders-lua/` package
- [ ] Create `host-libs/python/loaders/polyplug-loaders-js/` package
- [ ] Create `host-libs/python/loaders/polyplug-loaders-js-deno/` package
- [ ] Create `host-libs/python/loaders/polyplug-loaders-dotnet/` package
- [ ] Remove `polyplug/loaders/` from main package
  - **Verification:** `pip install .` works for each loader, imports succeed
  - **Blocker:** None

### JavaScript Loaders

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-native/` with `deno.json`
- [ ] Create `host-libs/js/loaders/@polyplug/loaders-python/` package
- [ ] Create `host-libs/js/loaders/@polyplug/loaders-lua/` package
- [ ] Create `host-libs/js/loaders/@polyplug/loaders-js/` package
- [ ] Create `host-libs/js/loaders/@polyplug/loaders-js-deno/` package
- [ ] Create `host-libs/js/loaders/@polyplug/loaders-dotnet/` package
- [ ] Remove `loaders/*.ts` from main module
  - **Verification:** `deno test` passes for each loader, imports succeed
  - **Blocker:** None

### Lua Loaders

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-native/` with rockspec
- [ ] Create `host-libs/lua/loaders/polyplug-loaders-python/` package
- [ ] Create `host-libs/lua/loaders/polyplug-loaders-lua/` package
- [ ] Create `host-libs/lua/loaders/polyplug-loaders-js/` package
- [ ] Create `host-libs/lua/loaders/polyplug-loaders-js-deno/` package
- [ ] Create `host-libs/lua/loaders/polyplug-loaders-dotnet/` package
- [ ] Remove `loaders/*.lua` from main module
  - **Verification:** `luarocks install` works for each loader, `require()` succeeds
  - **Blocker:** None

### Rust Loaders

- [ ] Already correct - loaders are separate adapter crates
  - **Verification:** N/A
  - **Blocker:** None

---

## Phase 5: Codegen Updates

### [PARALLEL GROUP: CODEGEN - depends on Phase 2]

#### C++ Codegen

- [ ] Update `generate_cpp_host_contract` in `crates/polyplug_codegen/src/generators/cpp.rs` to accept vtable in constructor
  - **Verification:** Generated class has `const PluginVTable* vtable_` member
  - **Blocker:** PluginGuard implemented

- [ ] Update `generate_cpp_host_function` to use cached vtable for dispatch
  - **Verification:** Generated code reads `vtable_->functions[idx]`, no `resolve_plugin` call
  - **Blocker:** Constructor change complete

#### Python Codegen

- [ ] Update `generate_host_caller_method` in `crates/polyplug_codegen/src/generators/python.rs` to use cached dispatch function
  - **Verification:** Generated code imports `_DISPATCH_FN_TYPE`, no inline `CFUNCTYPE`
  - **Blocker:** Module-level type exists

#### JavaScript Codegen

- [ ] Verify `js_quickjs.rs` generates optimal callers (check if updates needed)
  - **Verification:** Review generated `host/callers.ts`, ensure no per-call object creation
  - **Blocker:** None

#### Lua Codegen

- [ ] Verify `lua.rs` generates optimal callers (check if updates needed)
  - **Verification:** Review generated `host/callers.lua`, ensure no per-call `ffi.cast`
  - **Blocker:** None

#### Rust Codegen

- [ ] Already optimal - verify no changes needed
  - **Verification:** Review `rust.rs` generates direct vtable dispatch
  - **Blocker:** None

---

## Phase 6: Testing & Verification

### [SEQUENTIAL - depends on all phases]

### Run Existing Tests

- [ ] Run `cargo test` in workspace root
  - **Command:** `cargo test`
  - **Expected:** All tests pass
  - **On Failure:** Check error output, fix issues

- [ ] Run C# tests to verify no regressions
  - **Command:** `cd host-libs/csharp && dotnet test`
  - **Expected:** All tests pass
  - **On Failure:** Check if changes affected C#

- [ ] Run codegen unit tests
  - **Command:** `cargo test --lib -- csharp cpp python lua js`
  - **Expected:** All generator tests pass
  - **On Failure:** Fix codegen issues

### Verify Codegen Output

- [ ] Generate test output for each language and inspect
  - **Command:**
    ```bash
    cargo run -- generate --bundle tests/fixtures/test_bundle.toml --lang cpp --out /tmp/test-cpp
    cargo run -- generate --bundle tests/fixtures/test_bundle.toml --lang python --out /tmp/test-python
    cargo run -- generate --bundle tests/fixtures/test_bundle.toml --lang lua --out /tmp/test-lua
    cargo run -- generate --bundle tests/fixtures/test_bundle.toml --lang js-quickjs --out /tmp/test-js
    ```
  - **Verification Checklist:**
    - [ ] C++: No `resolve_plugin` call in generated methods
    - [ ] Python: No inline `CFUNCTYPE` or class definitions
    - [ ] JS: No `new Deno.UnsafeFunctionPrototype()` in generated callers
    - [ ] Lua: No `ffi.cast` with string type in generated callers
  - **On Failure:** Fix codegen, regenerate, re-verify

### Performance Verification

- [ ] Run hot path benchmark for C++ (if benchmark exists)
  - **Expected:** < 20ns per call after caching
  - **On Failure:** Check vtable caching is working

- [ ] Verify no P/Invoke/FFI calls on hot path by adding debug prints
  - **Method:** Add `print()` in vtable getter, verify not called during hot path
  - **Expected:** Only one call during Guard construction
  - **On Failure:** Check caching logic

### Integration Test

- [ ] Test each loader package can load its plugin type
  - **C++:** Compile example, load native plugin
  - **Python:** `import polyplug_loaders_python`, register loader, load plugin
  - **JS:** Import loader, register, load plugin
  - **Lua:** `require("polyplug.loaders.python")`, register, load plugin
  - **On Failure:** Check package exports correct functions

---

## Summary

| Phase | Parallelizable | Blockers | Est. Time |
|-------|----------------|----------|-----------|
| Phase 0 (Bug Fix) | No | None | 0.25h |
| Phase 1 (Core) | ✅ All hosts | None | 2h |
| Phase 2 (VTable) | ✅ All hosts | Phase 1 | 2h |
| Phase 3 (Func Cache) | ✅ Python/JS/Lua | Phase 1 | 2h |
| Phase 4 (Loaders) | ✅ All loaders | None | 6h |
| Phase 5 (Codegen) | ✅ All langs | Phase 2 | 2h |
| Phase 6 (Testing) | No | All phases | 3h |
| **Total** | | | **~17h** |

---

## Agent Testing Checklist

After completing each phase, run these checks:

```bash
# 1. Compilation check
cargo build --all
cargo test --no-run

# 2. Unit tests
cargo test

# 3. Codegen output inspection
cargo run -- generate --bundle tests/fixtures/test_bundle.toml --lang <LANG> --out /tmp/test-<LANG>
ls -la /tmp/test-<LANG>/host/

# 4. Verify generated code patterns
# C++: grep -v "resolve_plugin" /tmp/test-cpp/host/host_callers.hpp
# Python: grep -v "CFUNCTYPE" /tmp/test-python/host/callers.py
# JS: grep -v "UnsafeFunctionPrototype" /tmp/test-js/host/callers.ts
# Lua: grep -v 'ffi.cast.*"' /tmp/test-lua/host/callers.lua
```