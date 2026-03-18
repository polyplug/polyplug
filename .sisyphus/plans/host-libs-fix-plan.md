# Host Libs Zero-Overhead Fix Plan

## Goal

Fix all host libs (C++, Python, JavaScript, Lua, Rust) for zero-overhead hot path: one indirect function call, nothing more.

---

## Phase 0: Critical Bug Fix

- [ ] Fix Lua `find_by_bundle` to call actual FFI function instead of returning dummy handle `1`
  - **Verification:** Test calls `find_by_bundle` and receives real plugin handle, not `1`
  - **Blocker:** None

---

## Phase 1: Core Infrastructure (PluginGuard + Type Caching)

### [PARALLEL GROUP: ALL HOST LIBS]

- [ ] Add `PluginGuard` class to C++ with cached vtable field
  - **Verification:** Class compiles, has `vtable()` method returning cached pointer
  - **Blocker:** None

- [ ] Add `PluginGuard` struct to Rust with cached vtable field
  - **Verification:** Struct compiles, has `vtable()` method, implements `Drop`
  - **Blocker:** None

- [ ] Add module-level dispatch function type to Python
  - **Verification:** Type defined at module top, not inside functions
  - **Blocker:** None

- [ ] Add module-level VTable struct to Python
  - **Verification:** Struct defined at module top, reused by callers
  - **Blocker:** None

- [ ] Add module-level lib bindings initializer to Python with initialized flag
  - **Verification:** Function sets `argtypes`/`restype` once, guarded by flag
  - **Blocker:** None

- [ ] Add module-level dispatch function type to JavaScript
  - **Verification:** Type defined at module level
  - **Blocker:** None

- [ ] Add module-level function pointer cache to JavaScript
  - **Verification:** Map exists at module level, properly typed
  - **Blocker:** None

- [ ] Add module-level VTable type to Lua
  - **Verification:** `ffi.typeof` called once at module level
  - **Blocker:** None

- [ ] Add module-level dispatch function type to Lua
  - **Verification:** `ffi.typeof` called once at module level
  - **Blocker:** None

- [ ] Add module-level function pointer cache to Lua
  - **Verification:** Cache table exists at module level
  - **Blocker:** None

- [ ] Create `Guard` metatable in Lua with cached vtable
  - **Verification:** `Guard.new()` caches vtable, `vtable()` method returns cached value
  - **Blocker:** None

---

## Phase 2: VTable Caching Implementation

### [PARALLEL GROUP: ALL HOST LIBS - depends on Phase 1]

- [ ] Update C++ `Runtime::resolve_plugin` to return `PluginGuard` with cached vtable
  - **Verification:** Method returns `PluginGuard`, vtable cached at construction
  - **Blocker:** C++ PluginGuard exists

- [ ] Update Rust `Runtime::resolve_plugin` to return `PluginGuard`
  - **Verification:** Method returns `Option<PluginGuard>`, vtable cached
  - **Blocker:** Rust PluginGuard exists

- [ ] Update Python `PluginGuard` to cache vtable at construction
  - **Verification:** Constructor calls `polyplug_runtime_plugin_vtable` once
  - **Blocker:** Python types exist

- [ ] Change Python `get_vtable()` to return cached vtable
  - **Verification:** No FFI call in method body
  - **Blocker:** VTable cached in init

- [ ] Update JavaScript `Guard` to cache vtable in constructor
  - **Verification:** Field populated by calling `polyplug_runtime_plugin_vtable` once
  - **Blocker:** JS types exist

- [ ] Change JavaScript `vtable()` to return cached value
  - **Verification:** Method returns cached field without FFI call
  - **Blocker:** VTable cached in constructor

- [ ] Update Lua `Runtime:resolve_plugin` to return `Guard` instance
  - **Verification:** Returns `Guard.new()`, not raw pointer
  - **Blocker:** Lua Guard exists

---

## Phase 3: Function Pointer Caching

### [PARALLEL GROUP: PYTHON, JS, LUA - depends on Phase 1]

- [ ] Add function pointer cache dict to Python module
  - **Verification:** Cache dict exists at module level
  - **Blocker:** Phase 1 types exist

- [ ] Rewrite Python `call_plugin_fn` to check cache before creating CFUNCTYPE
  - **Verification:** Function checks cache dict, creates only if missing
  - **Blocker:** Cache exists

- [ ] Use module-level VTable struct in Python `call_plugin_fn`
  - **Verification:** No inline `class VTableStruct` inside function
  - **Blocker:** Module-level struct exists

- [ ] Rewrite JavaScript `callPluginFn` to check cache before creating UnsafeFnPointer
  - **Verification:** Function checks cache map, creates only if missing
  - **Blocker:** Cache exists

- [ ] Rewrite Lua `call_plugin_fn` to accept vtable_ptr parameter
  - **Verification:** Function signature changed, no resolve call inside
  - **Blocker:** None

- [ ] Add cache lookup for function pointer in Lua `call_plugin_fn`
  - **Verification:** Cache checked before `ffi.cast`
  - **Blocker:** Cache exists

- [ ] Use module-level type in Lua `call_plugin_fn` instead of inline cast
  - **Verification:** No string type in `ffi.cast` call
  - **Blocker:** Module-level type exists

---

## Phase 4: Loader Restructuring

### [PARALLEL GROUP: ALL LOADERS - no blockers]

- [ ] Create C++ native loader package with CMakeLists.txt
  - **Verification:** Package builds standalone with `cmake --build .`
  - **Blocker:** None

- [ ] Create C++ python loader package with CMakeLists.txt
  - **Verification:** Package builds standalone
  - **Blocker:** None

- [ ] Create C++ lua loader package with CMakeLists.txt
  - **Verification:** Package builds standalone
  - **Blocker:** None

- [ ] Create C++ js loader package with CMakeLists.txt
  - **Verification:** Package builds standalone
  - **Blocker:** None

- [ ] Create C++ js_deno loader package with CMakeLists.txt
  - **Verification:** Package builds standalone
  - **Blocker:** None

- [ ] Create C++ dotnet loader package with CMakeLists.txt
  - **Verification:** Package builds standalone
  - **Blocker:** None

- [ ] Remove C++ loader headers from main package
  - **Verification:** Main package no longer contains loaders
  - **Blocker:** All C++ loader packages created

- [ ] Create Python native loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Create Python python loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Create Python lua loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Create Python js loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Create Python js_deno loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Create Python dotnet loader package with pyproject.toml
  - **Verification:** `pip install .` works, package imports
  - **Blocker:** None

- [ ] Remove Python loaders from main package
  - **Verification:** Main package no longer contains loaders
  - **Blocker:** All Python loader packages created

- [ ] Create JavaScript native loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Create JavaScript python loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Create JavaScript lua loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Create JavaScript js loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Create JavaScript js_deno loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Create JavaScript dotnet loader package with deno.json
  - **Verification:** `deno test` passes, package imports
  - **Blocker:** None

- [ ] Remove JavaScript loaders from main module
  - **Verification:** Main module no longer contains loaders
  - **Blocker:** All JavaScript loader packages created

- [ ] Create Lua native loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Create Lua python loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Create Lua lua loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Create Lua js loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Create Lua js_deno loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Create Lua dotnet loader package with rockspec
  - **Verification:** `luarocks install` works, require succeeds
  - **Blocker:** None

- [ ] Remove Lua loaders from main module
  - **Verification:** Main module no longer contains loaders
  - **Blocker:** All Lua loader packages created

---

## Phase 5: Codegen Updates

### [PARALLEL GROUP: ALL LANGUAGES - depends on Phase 2]

- [ ] Update C++ codegen to accept vtable in constructor
  - **Verification:** Generated class has vtable member, no HostVTable member
  - **Blocker:** C++ PluginGuard implemented

- [ ] Update C++ codegen to use cached vtable for dispatch
  - **Verification:** Generated code reads vtable directly, no resolve call
  - **Blocker:** Constructor change complete

- [ ] Update Python codegen to use cached dispatch function type
  - **Verification:** Generated code imports type, no inline CFUNCTYPE
  - **Blocker:** Module-level type exists

- [ ] Update Python codegen to use module-level VTable struct
  - **Verification:** Generated code imports struct, no inline definition
  - **Blocker:** Module-level struct exists

- [ ] Verify JavaScript codegen generates optimal callers
  - **Verification:** Review generated code, no per-call object creation
  - **Blocker:** None

- [ ] Verify Lua codegen generates optimal callers
  - **Verification:** Review generated code, no per-call ffi.cast with string type
  - **Blocker:** None

- [ ] Verify Rust codegen is optimal (no changes needed expected)
  - **Verification:** Generated code uses direct vtable dispatch
  - **Blocker:** None

---

## Phase 6: Testing & Verification

### [SEQUENTIAL - depends on all phases]

- [ ] Run `cargo test` and verify all tests pass
  - **Verification:** All tests pass, no failures
  - **Blocker:** All phases complete

- [ ] Run C# tests to verify no regressions
  - **Verification:** `dotnet test` passes in host-libs/csharp
  - **Blocker:** All phases complete

- [ ] Run codegen unit tests
  - **Verification:** `cargo test --lib` passes all generator tests
  - **Blocker:** All phases complete

- [ ] Generate test output for C++ and inspect generated host callers
  - **Verification:** No `resolve_plugin` call in generated methods
  - **Blocker:** Codegen complete

- [ ] Generate test output for Python and inspect generated callers
  - **Verification:** No inline `CFUNCTYPE` or class definitions
  - **Blocker:** Codegen complete

- [ ] Generate test output for JavaScript and inspect generated callers
  - **Verification:** No `new Deno.UnsafeFunctionPrototype()` in generated code
  - **Blocker:** Codegen complete

- [ ] Generate test output for Lua and inspect generated callers
  - **Verification:** No `ffi.cast` with string type in generated code
  - **Blocker:** Codegen complete

- [ ] Verify no FFI calls on hot path by testing vtable caching
  - **Verification:** VTable getter called only once during Guard construction
  - **Blocker:** VTable caching implemented

- [ ] Test each C++ loader package can register and load plugins
  - **Verification:** Each loader compiles, registers, loads its plugin type
  - **Blocker:** Loader packages created

- [ ] Test each Python loader package can register and load plugins
  - **Verification:** Each loader imports, registers, loads its plugin type
  - **Blocker:** Loader packages created

- [ ] Test each JavaScript loader package can register and load plugins
  - **Verification:** Each loader imports, registers, loads its plugin type
  - **Blocker:** Loader packages created

- [ ] Test each Lua loader package can register and load plugins
  - **Verification:** Each loader requires, registers, loads its plugin type
  - **Blocker:** Loader packages created

---

## Summary

| Phase | Parallelizable | Est. Time |
|-------|----------------|-----------|
| Phase 0 (Bug Fix) | No | 0.25h |
| Phase 1 (Core) | ✅ All hosts | 2h |
| Phase 2 (VTable) | ✅ All hosts | 2h |
| Phase 3 (Func Cache) | ✅ Python/JS/Lua | 2h |
| Phase 4 (Loaders) | ✅ All loaders | 6h |
| Phase 5 (Codegen) | ✅ All languages | 2h |
| Phase 6 (Testing) | No | 3h |
| **Total** | | **~17h** |