# Rust Host Lib Fix Plan

## Status: MINOR IMPROVEMENTS NEEDED

## Summary

The Rust host lib is the reference implementation and is already well-optimized. The only recommended change is adding `PluginGuard` for consistency with other languages and RAII safety.

---

## [OPTIONAL] Phase 1: Add PluginGuard Wrapper

**Blockers:** None  
**Parallel:** No

**Decision Required:** Should we add `PluginGuard` to Rust host lib?

**Pros:**
- Consistency with C#, Python, JS, Lua
- RAII safety (automatic guard release via `Drop`)
- Cached vtable pointer

**Cons:**
- Raw pointers are idiomatic Rust for FFI
- Adds abstraction layer
- Not strictly necessary for correctness

---

### If Proceeding:

- [ ] Add `PluginGuard` struct to `host-libs/rust/src/lib.rs` with vtable caching
  - **Verification:** `PluginGuard` has `guard: *mut OpaqueGuard` and `vtable: *const PluginVTable` fields; vtable cached in constructor

- [ ] Implement `Drop` for `PluginGuard` to release guard
  - **Verification:** `Drop::drop` calls `polyplug_runtime_plugin_release(self.guard)`; no double-free

- [ ] Add `PluginGuard::vtable()` method returning cached pointer
  - **Verification:** Method returns `self.vtable` with no FFI call

- [ ] Add `Runtime::resolve_plugin(&self, handle: PluginHandle) -> Option<PluginGuard>` method
  - **Verification:** Method calls `polyplug_runtime_resolve_plugin`, caches vtable, returns `Some(PluginGuard)`

- [ ] Add SAFETY comments to all `unsafe` blocks in `PluginGuard` implementation
  - **Verification:** Every `unsafe` block has `// SAFETY:` comment explaining why operation is sound

- [ ] Run `cargo test` to verify all tests pass
  - **Verification:** All Rust tests pass with exit code 0

- [ ] Run `cargo clippy -- -D warnings` to verify no lints
  - **Verification:** Clippy exits with code 0; no warnings

---

## Phase 2: No Changes Required

**Blockers:** N/A  
**Parallel:** N/A

- [ ] **No action needed** - Rust loaders are already separate adapter crates (`polyplug-dotnet`, `polyplug-python`, etc.)
  - **Verification:** Confirmed loaders are in `crates/polyplug-dotnet/`, `crates/polyplug-python/`, etc.

- [ ] **No action needed** - Rust codegen already generates optimal vtable dispatch
  - **Verification:** Generated code uses direct `vtable.functions.add(fn_id)` with one indirect call

---

## Recommendation

**Proceed with Phase 1** for consistency across all host libs, even though the current raw pointer approach is idiomatic Rust. The benefits are:

1. **Consistency**: All host libs (C#, Python, JS, Lua, Rust) have the same `PluginGuard` pattern
2. **RAII Safety**: Automatic cleanup via `Drop` trait
3. **Performance**: Cached vtable eliminates one FFI call per plugin interaction

---

## Estimated Effort (If Proceeding)

- Phase 1: 1 hour
- Testing: 30 minutes

**Total: ~1.5 hours (optional)**

---

## PRD References

- PRD §8: "polyplug crate (crates.io) — PluginRuntime builder, type-safe ABI wrappers"