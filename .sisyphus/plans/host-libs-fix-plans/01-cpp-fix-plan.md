# C++ Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

C++ host lib has performance issues: loaders embedded, no vtable caching, generated code resolves plugin every call.

---

## Phase 1: Core Infrastructure

### [PARALLEL GROUP: CORE TYPES]

- [ ] Add `PluginGuard` class to `runtime.hpp` that caches vtable at construction
  - **Verification:** `PluginGuard` compiles, stores `guard_` and `vtable_`, returns cached vtable in `vtable()` method
  - **Blocker:** None

- [ ] Add `resolve_plugin()` method to `Runtime` class that returns `PluginGuard`
  - **Verification:** Method calls `polyplug_runtime_resolve_plugin`, constructs `PluginGuard` with cached vtable
  - **Blocker:** PluginGuard class must exist

- [ ] Update `runtime.hpp` error handling to use proper error types
  - **Verification:** Errors throw typed exceptions with ABI error codes
  - **Blocker:** None

---

## Phase 2: Loader Restructuring

### [SEQUENTIAL - depends on Phase 1]

- [ ] Create `loaders/native/` directory structure
  - **Verification:** Directory exists with `CMakeLists.txt` and `polyplug_loaders_native.hpp`
  - **Blocker:** Phase 1 complete

- [ ] Move `polyplug/loaders/native.hpp` to `loaders/native/polyplug_loaders_native.hpp`
  - **Verification:** File moved, includes updated, old file removed
  - **Blocker:** loaders/native directory exists

- [ ] Create `loaders/python/` package with `CMakeLists.txt` and header
  - **Verification:** Package builds standalone, exports `register_python_loader`
  - **Blocker:** None (parallel with other loaders)

- [ ] Create `loaders/lua/` package with `CMakeLists.txt` and header
  - **Verification:** Package builds standalone, exports `register_lua_loader`
  - **Blocker:** None (parallel with other loaders)

- [ ] Create `loaders/js/` package with `CMakeLists.txt` and header
  - **Verification:** Package builds standalone, exports `register_js_loader`
  - **Blocker:** None (parallel with other loaders)

- [ ] Create `loaders/js_deno/` package with `CMakeLists.txt` and header
  - **Verification:** Package builds standalone, exports `register_js_deno_loader`
  - **Blocker:** None (parallel with other loaders)

- [ ] Create `loaders/dotnet/` package with `CMakeLists.txt` and header
  - **Verification:** Package builds standalone, exports `register_dotnet_loader`
  - **Blocker:** None (parallel with other loaders)

- [ ] Remove loader headers from main `polyplug/` package
  - **Verification:** `polyplug/loaders/*.hpp` files deleted, main package no longer contains loaders
  - **Blocker:** All loader packages created

---

## Phase 3: Codegen Updates

### [SEQUENTIAL - depends on Phase 1]

- [ ] Update `generate_cpp_host_contract` to accept `const PluginVTable*` in constructor
  - **Verification:** Generated class stores vtable pointer, no `HostVTable*` member
  - **Blocker:** PluginGuard must cache vtable

- [ ] Update `generate_cpp_host_function` to use cached vtable for dispatch
  - **Verification:** Generated code reads vtable directly from member, no `resolve_plugin` call
  - **Blocker:** Constructor change complete

- [ ] Add caller-provides-buffer pattern for non-primitive returns
  - **Verification:** Non-primitive returns take output pointer parameter
  - **Blocker:** None

---

## Phase 4: Build System

### [SEQUENTIAL - depends on Phase 2]

- [ ] Create workspace `CMakeLists.txt` that builds all packages
  - **Verification:** `cmake --build .` builds polyplug + all loaders
  - **Blocker:** All loader directories exist

- [ ] Create pkg-config files for each package
  - **Verification:** `pkg-config --cflags polyplug_loaders_python` returns correct flags
  - **Blocker:** Packages build

- [ ] Update README with new package structure
  - **Verification:** README documents separate loader packages
  - **Blocker:** Build system complete

---

## Phase 5: Testing

### [SEQUENTIAL - depends on all phases]

- [ ] Write unit tests for `PluginGuard` vtable caching
  - **Verification:** Test verifies vtable() returns cached value without FFI call
  - **Blocker:** PluginGuard implemented

- [ ] Write integration tests for loader registration
  - **Verification:** Each loader can register and load its plugin type
  - **Blocker:** All loaders in separate packages

- [ ] Performance benchmark: verify hot path is one indirect call
  - **Verification:** Benchmark shows < 10ns per call after vtable cache
  - **Blocker:** All phases complete

---

## Self-Review

| Aspect | Status | Notes |
|--------|--------|-------|
| Tasks are atomic | ✅ | Each task is one action with one verification |
| Verifications are concrete | ✅ | All verifications are testable/measurable |
| Parallel groups marked | ✅ | Core types and loader packages are parallelizable |
| Blockers identified | ✅ | Sequential dependencies clearly marked |
| Covers all issues | ✅ | Loaders, vtable caching, codegen all addressed |

---

## Estimated Effort

| Phase | Time |
|-------|------|
| Phase 1 (Core) | 1.5h |
| Phase 2 (Loaders) | 3h |
| Phase 3 (Codegen) | 2h |
| Phase 4 (Build) | 1h |
| Phase 5 (Testing) | 1.5h |
| **Total** | **~9h** |