# Lua Host Lib Fix Plan

## Status: NEEDS FIXES - CRITICAL BUG

## Summary

Lua host lib has CRITICAL BUG: `find_by_bundle` returns dummy handle. Also has performance issues: no vtable caching, creates ffi.cast every call.

---

## Phase 0: Critical Bug Fix

### [IMMEDIATE - no blockers]

- [ ] Fix `find_by_bundle` to call actual `polyplug_runtime_find_by_bundle` function
  - **Verification:** Function calls `lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)` and returns result
  - **Blocker:** None

- [ ] Fix `find_by_bundle` signature to accept `bundle_id` as uint64, not `bundle_name` as string
  - **Verification:** Function signature matches FFI: `(bundle_id, contract_id, min_version)`
  - **Blocker:** None

---

## Phase 1: Core Infrastructure

### [PARALLEL GROUP: TYPE CACHING]

- [ ] Add `local VTableType = ffi.typeof("const PluginVTable*")` at module level
  - **Verification:** Type exists at module level, not inside function
  - **Blocker:** None

- [ ] Add `local DispatchFnType = ffi.typeof("uint32_t (*)(const void*, void*)")` at module level
  - **Verification:** Type exists at module level
  - **Blocker:** None

- [ ] Add `local func_cache = {}` module-level cache table
  - **Verification:** Cache table exists at module level
  - **Blocker:** None

---

## Phase 2: Guard Class Implementation

### [SEQUENTIAL - no blockers]

- [ ] Create `Guard` metatable with `__index`
  - **Verification:** Metatable exists, can create Guard instances
  - **Blocker:** None

- [ ] Add `Guard.new(lib, guard_ptr)` constructor that caches vtable
  - **Verification:** Constructor calls `polyplug_runtime_guard_vtable` once, stores in `_vtable`
  - **Blocker:** Metatable exists

- [ ] Add `Guard:vtable()` method returning cached `_vtable`
  - **Verification:** Method returns `self._vtable`, no FFI call
  - **Blocker:** Constructor caches vtable

- [ ] Add `Guard:destroy()` method for explicit cleanup
  - **Verification:** Method calls `polyplug_runtime_guard_destroy` if guard exists
  - **Blocker:** None

- [ ] Update `Runtime:resolve_plugin` to return `Guard` instance
  - **Verification:** Method returns `Guard.new(lib, guard_ptr)`, not raw pointer
  - **Blocker:** Guard class exists

---

## Phase 3: call_plugin_fn Optimization

### [SEQUENTIAL - depends on Phase 1]

- [ ] Rewrite `call_plugin_fn` to accept `vtable_ptr` instead of resolving inside
  - **Verification:** Function signature is `(vtable_ptr, func_idx, input)`, no resolve call inside
  - **Blocker:** None

- [ ] Use `VTableType` cast instead of inline ffi.cast
  - **Verification:** `ffi.cast(VTableType, vtable_ptr)` used, no string type in cast
  - **Blocker:** Module-level type exists

- [ ] Add function pointer cache lookup in `call_plugin_fn`
  - **Verification:** `func_cache[func_ptr]` checked before creating new cast
  - **Blocker:** Cache table exists

- [ ] Cache cast function pointer in `func_cache`
  - **Verification:** After cast, `func_cache[func_ptr] = func` executed
  - **Blocker:** Cache lookup implemented

- [ ] Remove guard creation/destruction from `call_plugin_fn`
  - **Verification:** No `polyplug_runtime_resolve_plugin` or `polyplug_runtime_guard_destroy` calls
  - **Blocker:** Caller provides vtable

---

## Phase 4: Loader Restructuring

### [PARALLEL GROUP: LOADER PACKAGES - no blockers]

- [ ] Create `loaders/polyplug-loaders-native/` with rockspec and `polyplug/loaders/native.lua`
  - **Verification:** `luarocks install` works, `require("polyplug.loaders.native")` succeeds
  - **Blocker:** None

- [ ] Move `loaders/native.lua` to `polyplug-loaders-native/polyplug/loaders/native.lua`
  - **Verification:** File moved, old file deleted
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

- [ ] Remove `loaders/*.lua` from main `polyplug` module
  - **Verification:** Old loader files deleted
  - **Blocker:** All loader packages created

---

## Phase 5: Testing

### [SEQUENTIAL - depends on all phases]

- [ ] Write test for `find_by_bundle` fix
  - **Verification:** Test calls `find_by_bundle` and gets real handle, not `1`
  - **Blocker:** Bug fix implemented

- [ ] Write test for Guard vtable caching
  - **Verification:** Test calls `vtable()` twice, verifies no second FFI call
  - **Blocker:** Guard class implemented

- [ ] Write test for function pointer cache
  - **Verification:** Test calls same function twice, verifies cache hit
  - **Blocker:** Cache implemented

- [ ] Write performance benchmark
  - **Verification:** Benchmark shows < 100ns per call after optimization
  - **Blocker:** All phases complete

---

## Self-Review

| Aspect | Status | Notes |
|--------|--------|-------|
| Tasks are atomic | ✅ | Each task is one action with one verification |
| Verifications are concrete | ✅ | All verifications are testable |
| Parallel groups marked | ✅ | Type caching and loader packages are parallelizable |
| Blockers identified | ✅ | Sequential dependencies clearly marked |
| Critical bug addressed | ✅ | `find_by_bundle` fix is Phase 0, immediate |
| Covers all issues | ✅ | Bug fix, vtable caching, function caching, loaders |

---

## Estimated Effort

| Phase | Time |
|-------|------|
| Phase 0 (Critical Bug) | 0.25h |
| Phase 1 (Types) | 0.5h |
| Phase 2 (Guard) | 1h |
| Phase 3 (call_plugin_fn) | 1h |
| Phase 4 (Loaders) | 2h |
| Phase 5 (Testing) | 1h |
| **Total** | **~5.75h** |