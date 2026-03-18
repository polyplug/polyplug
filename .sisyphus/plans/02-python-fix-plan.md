# Python Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The Python host lib has significant performance overhead due to ctypes patterns that create new objects on every call. Python's ctypes is inherently slower than native code, but we can still optimize for zero-overhead on the hot path.

---

## Phase 1: Add VTable Caching to PluginGuard

**Blockers:** None  
**Parallel:** No

- [ ] Modify `PluginGuard.__init__` in `host-libs/python/polyplug/runtime.py` to cache vtable pointer at construction
  - **Verification:** `PluginGuard` has `self._vtable` attribute set via one `polyplug_runtime_plugin_vtable()` call in constructor

- [ ] Add `vtable` property to `PluginGuard` that returns cached pointer
  - **Verification:** `guard.vtable` returns cached value; no FFI call on property access

---

## Phase 2: Module-Level Type Caching

**Blockers:** None  
**Parallel:** Yes - can be done independently of Phase 1

- [ ] Add module-level `_VTableStruct` type definition in `host-libs/python/polyplug/helpers.py`
  - **Verification:** `_VTableStruct` defined at module scope; not recreated in `call_plugin_fn`

- [ ] Add module-level `_DispatchFnType` CFUNCTYPE definition
  - **Verification:** `_DispatchFnType` defined at module scope; reused for all function pointer casts

- [ ] Add `_func_cache` dictionary for caching function pointer wrappers
  - **Verification:** `_func_cache: dict[int, ctypes._CFuncPtr]` exists; populated on first call, reused on subsequent calls

- [ ] Rewrite `call_plugin_fn` to use cached types and function pointers
  - **Verification:** Function uses `_VTableStruct.from_address()`, `_DispatchFnType()`, and `_func_cache`; no type creation inside function body

---

## Phase 3: Module-Level Function Bindings

**Blockers:** None  
**Parallel:** Yes

- [ ] Move `_bind_functions` logic to module-level initialization in `runtime.py`
  - **Verification:** `argtypes` and `restype` set once at module import; not per-`Runtime` instance

- [ ] Add `_lib_bindings_initialized` guard to prevent double-initialization
  - **Verification:** Bindings set exactly once; subsequent `Runtime` creations skip binding setup

---

## [PARALLEL GROUP: LOADER RESTRUCTURING]

**Blockers:** None  
**Parallel:** Yes - all 6 loaders can be restructured in parallel

- [ ] Create `host-libs/python/loaders/polyplug-loaders-native/` with `pyproject.toml` and package structure
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-native` succeeds

- [ ] Create `host-libs/python/loaders/polyplug-loaders-python/` with `pyproject.toml`
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-python` succeeds

- [ ] Create `host-libs/python/loaders/polyplug-loaders-lua/` with `pyproject.toml`
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-lua` succeeds

- [ ] Create `host-libs/python/loaders/polyplug-loaders-js/` with `pyproject.toml`
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-js` succeeds

- [ ] Create `host-libs/python/loaders/polyplug-loaders-js-deno/` with `pyproject.toml`
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-js-deno` succeeds

- [ ] Create `host-libs/python/loaders/polyplug-loaders-dotnet/` with `pyproject.toml`
  - **Verification:** `pip install -e host-libs/python/loaders/polyplug-loaders-dotnet` succeeds

- [ ] Remove old loader files from `host-libs/python/polyplug/loaders/`
  - **Verification:** `host-libs/python/polyplug/loaders/` directory is deleted; no imports reference old paths

- [ ] Create workspace `host-libs/python/pyproject.toml` with all loader subpackages
  - **Verification:** `pip install -e host-libs/python` installs all loaders

---

## Phase 5: Update Codegen for Python

**Blockers:** Phase 2 complete  
**Parallel:** No

- [ ] Update `generate_host_caller_method` in `crates/polyplug_codegen/src/generators/python.rs` to use cached dispatch types
  - **Verification:** Generated code imports and uses module-level `_DISPATCH_FN_TYPE` instead of creating new `CFUNCTYPE` per call

- [ ] Run `cargo test --lib python` to verify codegen tests pass
  - **Verification:** All Python codegen tests pass with exit code 0

---

## New Directory Structure

```
host-libs/python/
├── polyplug/
│   ├── __init__.py
│   ├── __init__.pyi
│   ├── abi.py
│   ├── abi.pyi
│   ├── runtime.py                 # PluginGuard with vtable caching
│   ├── runtime.pyi
│   ├── helpers.py                 # Module-level type caching
│   └── helpers.pyi
├── loaders/
│   ├── polyplug-loaders-native/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_native/__init__.py
│   ├── polyplug-loaders-python/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_python/__init__.py
│   ├── polyplug-loaders-lua/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_lua/__init__.py
│   ├── polyplug-loaders-js/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_js/__init__.py
│   ├── polyplug-loaders-js-deno/
│   │   ├── pyproject.toml
│   │   └── polyplug_loaders_js_deno/__init__.py
│   └── polyplug-loaders-dotnet/
│       ├── pyproject.toml
│       └── polyplug_loaders_dotnet/__init__.py
└── pyproject.toml
```

---

## Performance Expectations

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~200ns (P/Invoke) | ~0ns (cached) |
| Function pointer cast | ~100ns (new type) | ~0ns (cached) |
| Type creation | ~500ns (class def) | ~0ns (module-level) |
| **Hot path** | ~800ns | ~100-200ns |

---

## PRD References

- PRD §8: "loaders/python.py, loaders/lua.py, etc. (one per loader)" - separate packages
- PRD §10 (Python): "All ctypes function objects cached at module level — no per-call lookup"
- PRD §10 (Python): "All argtypes/restype set once at import time"

---

## Estimated Effort

- Phase 1: 30 minutes
- Phase 2: 1 hour
- Phase 3: 30 minutes
- Phase 4: 2 hours (parallel execution)
- Phase 5: 1 hour
- Testing: 1 hour

**Total: ~5.5 hours**