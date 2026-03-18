# Python Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

Python host lib has ctypes overhead: no vtable caching, creates new types every call, function bindings set on every Runtime creation.

---

## Phase 1: Core Infrastructure

### [PARALLEL GROUP: TYPE CACHING]

- [ ] Move ctypes type definitions to module level in `runtime.py`
  - **Verification:** `StringView`, `PluginHandle`, `AbiError` defined at module top, not inside functions
  - **Blocker:** None

- [ ] Create module-level `_init_lib_bindings()` function that sets `argtypes`/`restype` once
  - **Verification:** Function sets all bindings, guarded by `_lib_bindings_initialized` flag
  - **Blocker:** None

- [ ] Add `_DISPATCH_FN_TYPE` module-level CFUNCTYPE definition
  - **Verification:** Type defined once at module level, reused by all callers
  - **Blocker:** None

- [ ] Add `_VTableStruct` module-level ctypes Structure
  - **Verification:** Structure defined once, imported by callers
  - **Blocker:** None

---

## Phase 2: PluginGuard VTable Caching

### [SEQUENTIAL - depends on Phase 1]

- [ ] Add `_vtable` attribute to `PluginGuard.__init__`
  - **Verification:** Constructor calls `polyplug_runtime_plugin_vtable` once and stores result
  - **Blocker:** None

- [ ] Change `get_vtable()` to return cached `_vtable`
  - **Verification:** Method returns `self._vtable` without FFI call
  - **Blocker:** `_vtable` attribute added

- [ ] Add `vtable` property for zero-overhead access
  - **Verification:** `guard.vtable` returns cached value, no method call overhead
  - **Blocker:** `_vtable` caching implemented

---

## Phase 3: Function Pointer Caching

### [SEQUENTIAL - depends on Phase 1]

- [ ] Add module-level `_func_cache: dict[int, ctypes._CFuncPtr] = {}`
  - **Verification:** Cache dict exists at module level
  - **Blocker:** None

- [ ] Rewrite `call_plugin_fn` to check cache before creating wrapper
  - **Verification:** Function checks `func_ptr in _func_cache`, creates only if missing
  - **Blocker:** Cache dict exists

- [ ] Use `_VTableStruct` instead of inline class definition in `call_plugin_fn`
  - **Verification:** No `class VTableStruct` inside function body
  - **Blocker:** Module-level struct exists

- [ ] Use `_DISPATCH_FN_TYPE` instead of inline CFUNCTYPE in `call_plugin_fn`
  - **Verification:** No `ctypes.CFUNCTYPE` inside function body
  - **Blocker:** Module-level type exists

---

## Phase 4: Loader Restructuring

### [PARALLEL GROUP: LOADER PACKAGES - no blockers]

- [ ] Create `loaders/polyplug-loaders-native/` with `pyproject.toml` and `__init__.py`
  - **Verification:** `pip install .` works, package imports successfully
  - **Blocker:** None

- [ ] Move `polyplug/loaders/native.py` to `polyplug-loaders-native/polyplug_loaders_native/__init__.py`
  - **Verification:** `from polyplug_loaders_native import register_native_loader` works
  - **Blocker:** Package directory exists

- [ ] Create `loaders/polyplug-loaders-python/` package
  - **Verification:** Package installs and imports
  - **Blocker:** None (parallel)

- [ ] Create `loaders/polyplug-loaders-lua/` package
  - **Verification:** Package installs and imports
  - **Blocker:** None (parallel)

- [ ] Create `loaders/polyplug-loaders-js/` package
  - **Verification:** Package installs and imports
  - **Blocker:** None (parallel)

- [ ] Create `loaders/polyplug-loaders-js-deno/` package
  - **Verification:** Package installs and imports
  - **Blocker:** None (parallel)

- [ ] Create `loaders/polyplug-loaders-dotnet/` package
  - **Verification:** Package installs and imports
  - **Blocker:** None (parallel)

- [ ] Remove `polyplug/loaders/` directory from main package
  - **Verification:** `import polyplug.loaders` raises ImportError
  - **Blocker:** All loader packages created

---

## Phase 5: Codegen Updates

### [SEQUENTIAL - depends on Phase 3]

- [ ] Update `generate_host_caller_method` to use pre-cached dispatch function type
  - **Verification:** Generated code imports `_DISPATCH_FN_TYPE`, doesn't create CFUNCTYPE inline
  - **Blocker:** Module-level type exists

- [ ] Update generated callers to use module-level `_VTableStruct`
  - **Verification:** Generated code imports struct, doesn't define inline
  - **Blocker:** Module-level struct exists

---

## Phase 6: Testing

### [SEQUENTIAL - depends on all phases]

- [ ] Write unit test for vtable caching in PluginGuard
  - **Verification:** Test calls `vtable` property twice, verifies no second FFI call
  - **Blocker:** Caching implemented

- [ ] Write unit test for function pointer cache hit
  - **Verification:** Test calls same function twice, verifies cache hit on second call
  - **Blocker:** Cache implemented

- [ ] Write performance benchmark comparing old vs new
  - **Verification:** Benchmark shows >5x improvement on hot path
  - **Blocker:** All phases complete

---

## Self-Review

| Aspect | Status | Notes |
|--------|--------|-------|
| Tasks are atomic | ✅ | Each task is one action with one verification |
| Verifications are concrete | ✅ | All verifications are testable |
| Parallel groups marked | ✅ | Type caching and loader packages are parallelizable |
| Blockers identified | ✅ | Sequential dependencies for codegen, testing |
| Covers all issues | ✅ | VTable caching, function caching, loaders addressed |

---

## Estimated Effort

| Phase | Time |
|-------|------|
| Phase 1 (Types) | 1h |
| Phase 2 (PluginGuard) | 0.5h |
| Phase 3 (Func Cache) | 1h |
| Phase 4 (Loaders) | 2h |
| Phase 5 (Codegen) | 1h |
| Phase 6 (Testing) | 1h |
| **Total** | **~6.5h** |