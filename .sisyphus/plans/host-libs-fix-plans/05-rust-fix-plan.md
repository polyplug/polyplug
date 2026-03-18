# Rust Host Lib Fix Plan

## Status: OPTIONAL IMPROVEMENTS

## Summary

Rust host lib is the reference implementation and already optimal. Optional improvement: add `PluginGuard` for RAII consistency.

---

## Phase 1: Optional PluginGuard Implementation

### [SEQUENTIAL]

- [ ] Add `PluginGuard` struct with `guard` and `vtable` fields
  - **Verification:** Struct compiles, stores `*mut OpaqueGuard` and `*const PluginVTable`
  - **Blocker:** None

- [ ] Implement `PluginGuard::new(lib, guard_ptr)` that caches vtable
  - **Verification:** Constructor calls `polyplug_runtime_plugin_vtable` once, stores result
  - **Blocker:** Struct exists

- [ ] Add `PluginGuard::vtable(&self) -> *const PluginVTable` method
  - **Verification:** Method returns `self.vtable`, no FFI call
  - **Blocker:** Field exists

- [ ] Implement `Drop` for `PluginGuard` that calls `polyplug_runtime_plugin_release`
  - **Verification:** When guard goes out of scope, release is called
  - **Blocker:** Struct exists

- [ ] Add `Runtime::resolve_plugin(&self, handle) -> Option<PluginGuard>`
  - **Verification:** Method returns `PluginGuard` instead of raw pointer
  - **Blocker:** PluginGuard implemented

- [ ] Update `resolve_plugin` unsafe function to use `PluginGuard` internally
  - **Verification:** Old function still works, new method returns guard
  - **Blocker:** None

---

## Phase 2: Documentation

### [SEQUENTIAL - depends on Phase 1]

- [ ] Add documentation comments for `PluginGuard`
  - **Verification:** `cargo doc` generates documentation for PluginGuard
  - **Blocker:** PluginGuard exists

- [ ] Add example usage in lib.rs doc comment
  - **Verification:** Example shows resolving plugin and calling function
  - **Blocker:** PluginGuard documented

---

## Phase 3: Testing (Optional)

### [SEQUENTIAL - depends on Phase 1]

- [ ] Write unit test for PluginGuard vtable caching
  - **Verification:** Test verifies vtable() returns cached value
  - **Blocker:** PluginGuard implemented

- [ ] Write unit test for PluginGuard Drop behavior
  - **Verification:** Test verifies release called when guard dropped
  - **Blocker:** Drop implemented

---

## Self-Review

| Aspect | Status | Notes |
|--------|--------|-------|
| Tasks are atomic | ✅ | Each task is one action with one verification |
| Verifications are concrete | ✅ | All verifications are testable |
| Parallel groups marked | ✅ | No parallelization needed (optional feature) |
| Blockers identified | ✅ | Sequential dependencies |
| Marked as optional | ✅ | Clear this is optional, not required |

---

## Decision Required

This plan is **OPTIONAL**. The Rust host lib is already optimal:
- Raw pointer approach is idiomatic Rust for FFI
- Already has zero-overhead vtable dispatch
- Loaders are correctly separate adapter crates

**Recommendation:** Implement only if consistency with other host libs is desired.

---

## Estimated Effort

| Phase | Time |
|-------|------|
| Phase 1 (PluginGuard) | 1h |
| Phase 2 (Docs) | 0.25h |
| Phase 3 (Testing) | 0.5h |
| **Total** | **~1.75h (optional)** |