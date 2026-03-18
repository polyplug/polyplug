# Rust Host Lib Fix Plan

## Status: ✅ COMPLETE — NO CHANGES NEEDED

## Summary

After analysis, **Rust does not need a separate `host-libs/rust/` package** because:

1. **Rust hosts use `polyplug` crate directly** — no FFI needed
2. **FFI layer already exists** in `crates/polyplug/src/ffi.rs` with `OpaquePluginGuard`
3. **Guard pattern already exists** — `PluginVTableGuard` in registry.rs for Rust, `OpaquePluginGuard` in ffi.rs for C ABI

---

## Architectural Decision

### Deleted: `host-libs/rust/` (polyplug_host crate)

**Rationale:**

| Language | How it Accesses polyplug | Needs host-libs? |
|----------|-------------------------|------------------|
| C# | FFI → `libpolyplug.so` | ✅ Yes — P/Invoke bindings |
| Python | FFI → `libpolyplug.so` | ✅ Yes — ctypes bindings |
| Lua | FFI → `libpolyplug.so` | ✅ Yes — LuaJIT FFI |
| JS | FFI → `libpolyplug.so` | ✅ Yes — Deno.dlopen |
| **Rust** | **Direct crate dependency** | ❌ **No — same language!** |

**PRD Quote:**
> `Rust host-libs/rust/ → polyplug crate (crates.io)`

This means Rust uses `polyplug` directly, not a separate host lib.

### What Was Removed

- `host-libs/rust/` directory
- `polyplug_host` from workspace members
- `polyplug_host` from workspace.dependencies

---

## Existing Guard Patterns (No Changes Needed)

### For Rust Hosts (Internal API)

```rust
// crates/polyplug/src/registry.rs
pub struct PluginVTableGuard {
    slot: Arc<VTableSlot>,
    _not_send: PhantomData<Cell<()>>,  // NOT Send - must re-resolve per thread
}

// Usage:
let guard: PluginVTableGuard = runtime.resolve_guard(handle)?;
let vtable: *const PluginVTable = guard.vtable();
```

### For Non-Rust Hosts (C ABI)

```rust
// crates/polyplug/src/ffi.rs
pub struct OpaquePluginGuard(pub(crate) PluginVTableGuard);

// FFI functions:
polyplug_runtime_resolve_plugin(rt, handle) → *mut OpaquePluginGuard
polyplug_runtime_plugin_vtable(guard) → *const PluginVTable
polyplug_runtime_plugin_release(guard)
```

---

## Phase 1: N/A — Deleted

The `PluginGuard` implementation that was added to `polyplug_host` was removed along with the crate, as it was architecturally incorrect.

---

## Phase 2: ✅ Verified

- [x] **No action needed** - Rust loaders are already separate adapter crates
  - **Verification:** `crates/polyplug_dotnet/`, `crates/polyplug_python/`, `crates/polyplug_lua/`, `crates/polyplug_js/`, `crates/polyplug_js_deno/`, `crates/polyplug_native/`

- [x] **No action needed** - Rust codegen already generates optimal vtable dispatch
  - **Verification:** Generated code uses `*vtable.functions.add(fn_id)` with one indirect call

- [x] **No action needed** - FFI layer already has guard pattern
  - **Verification:** `OpaquePluginGuard` in `ffi.rs` with `resolve_plugin`, `plugin_vtable`, `plugin_release`

---

## Verification

- [x] `cargo check --workspace` passes
- [x] No code references `polyplug_host`
- [x] Rust hosts use `polyplug` directly (e.g., `examples/hosts/rust/`)

---

## PRD References

- PRD §8: "polyplug crate (crates.io) — PluginRuntime builder, type-safe ABI wrappers"
- PRD §443-460: Rust example shows direct `use polyplug::PluginRuntime` — no host lib