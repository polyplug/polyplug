# JavaScript (Deno) Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

JS host lib has FFI overhead: no vtable caching, creates UnsafeFnPointer every call, creates buffers every call.

---

## Phase 0: Critical Bug Fix

### [IMMEDIATE - no blockers]

- [ ] ~~No critical bugs found in JS host lib~~
  - **Verification:** N/A
  - **Blocker:** None

---

## Phase 1: Core Infrastructure

### [PARALLEL GROUP: TYPE CACHING]

- [ ] Add `_DISPATCH_FN_TYPE` module-level UnsafeFunctionPrototype
  - **Verification:** Type defined once, reused by all callers
  - **Blocker:** None

- [ ] Add `_funcCache: Map<bigint, Deno.UnsafeFnPointer>` module-level cache
  - **Verification:** Map exists at module level, properly typed
  - **Blocker:** None

- [ ] Move SYMBOLS definition to module level (already there, verify)
  - **Verification:** SYMBOLS is module-level const, not inside function
  - **Blocker:** None

---

## Phase 2: Guard VTable Caching

### [SEQUENTIAL - no blockers]

- [ ] Add `#vtable` private field to `Guard` class
  - **Verification:** Field exists, initialized in constructor
  - **Blocker:** None

- [ ] Cache vtable in `Guard` constructor by calling `polyplug_runtime_plugin_vtable`
  - **Verification:** Constructor calls FFI once, stores result in `#vtable`
  - **Blocker:** Field exists

- [ ] Change `vtable()` method to return cached `#vtable`
  - **Verification:** Method returns `this.#vtable`, no FFI call
  - **Blocker:** Caching implemented

---

## Phase 3: callPluginFn Optimization

### [SEQUENTIAL - depends on Phase 1]

- [ ] Rewrite `callPluginFn` to use cached `_DISPATCH_FN_TYPE`
  - **Verification:** No `new Deno.UnsafeFunctionPrototype()` inside function
  - **Blocker:** Module-level type exists

- [ ] Add function pointer cache lookup in `callPluginFn`
  - **Verification:** Function checks `_funcCache.get(funcPtr)`, only creates if missing
  - **Blocker:** Cache map exists

- [ ] Cache created function pointer in `_funcCache`
  - **Verification:** After creation, `_funcCache.set(funcPtr, func)` called
  - **Blocker:** Cache lookup implemented

- [ ] Reuse typed arrays for input/output buffers
  - **Verification:** Buffers allocated once and reused, not created every call
  - **Blocker:** None

---

## Phase 4: Loader Restructuring

### [PARALLEL GROUP: LOADER PACKAGES - no blockers]

- [ ] Create `loaders/@polyplug/loaders-native/` with `deno.json` and `mod.ts`
  - **Verification:** `deno test` works, package imports successfully
  - **Blocker:** None

- [ ] Move `loaders/native.ts` to `@polyplug/loaders-native/mod.ts`
  - **Verification:** `import { registerNativeLoader } from "@polyplug/loaders-native"` works
  - **Blocker:** Package directory exists

- [ ] Create `loaders/@polyplug/loaders-python/` package
  - **Verification:** Package imports and works
  - **Blocker:** None (parallel)

- [ ] Create `loaders/@polyplug/loaders-lua/` package
  - **Verification:** Package imports and works
  - **Blocker:** None (parallel)

- [ ] Create `loaders/@polyplug/loaders-js/` package
  - **Verification:** Package imports and works
  - **Blocker:** None (parallel)

- [ ] Create `loaders/@polyplug/loaders-js-deno/` package
  - **Verification:** Package imports and works
  - **Blocker:** None (parallel)

- [ ] Create `loaders/@polyplug/loaders-dotnet/` package
  - **Verification:** Package imports and works
  - **Blocker:** None (parallel)

- [ ] Remove `loaders/*.ts` from main `polyplug` module
  - **Verification:** Old loader files deleted
  - **Blocker:** All loader packages created

---

## Phase 5: Type Definitions

### [SEQUENTIAL - depends on Phase 2, 3]

- [ ] Update `polyplug.d.ts` with cached `Guard.vtable` property
  - **Verification:** Type definition shows `vtable(): Deno.PointerValue` returns cached value
  - **Blocker:** Implementation complete

- [ ] Add type definitions for loader packages
  - **Verification:** Each loader has `deno.d.ts` or `.d.ts` file
  - **Blocker:** Loader packages created

---

## Phase 6: Testing

### [SEQUENTIAL - depends on all phases]

- [ ] Write unit test for Guard vtable caching
  - **Verification:** Test verifies `vtable()` returns same value without FFI call
  - **Blocker:** Caching implemented

- [ ] Write unit test for function pointer cache
  - **Verification:** Test verifies cache hit on second call to same function
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
| Blockers identified | ✅ | Sequential dependencies for codegen, testing |
| Covers all issues | ✅ | VTable caching, function caching, loaders addressed |

---

## Estimated Effort

| Phase | Time |
|-------|------|
| Phase 0 (Bugs) | 0h |
| Phase 1 (Types) | 0.5h |
| Phase 2 (Guard) | 0.5h |
| Phase 3 (callPluginFn) | 1.5h |
| Phase 4 (Loaders) | 2h |
| Phase 5 (Types) | 0.5h |
| Phase 6 (Testing) | 1h |
| **Total** | **~6h** |