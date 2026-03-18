# Lua Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The Lua host lib uses LuaJIT FFI which is extremely fast (~2x native), but the current implementation has overhead due to repeated casts and lookups. **Contains a critical bug: `find_by_bundle` returns a dummy handle.**

---

## [CRITICAL BUG FIX] Phase 0: Fix find_by_bundle

**Blockers:** None  
**Parallel:** No - **MUST BE DONE FIRST**

- [ ] Fix `M.Runtime:find_by_bundle` in `host-libs/lua/polyplug.lua` to call actual runtime function
  - **Verification:** Function calls `lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)` instead of returning `ffi.cast("uint64_t", 1)`

---

## Phase 1: Add Guard Class with VTable Caching

**Blockers:** Phase 0 complete  
**Parallel:** No

- [ ] Create `Guard` class in `host-libs/lua/polyplug.lua` with vtable caching at construction
  - **Verification:** `Guard.new(lib, guard_ptr)` calls `lib.polyplug_runtime_guard_vtable(guard_ptr)` once and stores in `self._vtable`

- [ ] Add `Guard:vtable()` method that returns cached pointer
  - **Verification:** `guard:vtable()` returns `self._vtable`; no FFI call on method invocation

- [ ] Add `Guard:destroy()` method for explicit cleanup
  - **Verification:** `guard:destroy()` calls `polyplug_runtime_guard_destroy`; sets `self._guard = nil` to prevent double-free

---

## Phase 2: Module-Level FFI Type Caching

**Blockers:** None  
**Parallel:** Yes - can be done independently of Phase 1

- [ ] Add module-level `VTableType` using `ffi.typeof("const PluginVTable*")`
  - **Verification:** `VTableType` defined once at module scope; reused in all vtable casts

- [ ] Add module-level `DispatchFnType` using `ffi.typeof("uint32_t (*)(const void*, void*)")`
  - **Verification:** `DispatchFnType` defined once at module scope; reused for all function pointer casts

- [ ] Add `func_cache` table for caching function pointer wrappers
  - **Verification:** `func_cache = {}` at module scope; populated on first call, reused on subsequent calls

- [ ] Rewrite `M.call_plugin_fn` to use cached types and function pointers
  - **Verification:** Function uses `ffi.cast(VTableType, ...)`, checks `func_cache[func_ptr]`; no `ffi.typeof` or repeated `ffi.cast` inside function body

---

## Phase 3: Add Structured Error Handling

**Blockers:** None  
**Parallel:** Yes

- [ ] Add `PolyplugError` table with error code constants
  - **Verification:** `PolyplugError = { NOT_FOUND = 4, STALE_HANDLE = 5, FUNCTION_NOT_AVAIL = 6 }` exists at module scope

- [ ] Add `M.last_error(lib)` function for retrieving error messages
  - **Verification:** Function calls `polyplug_runtime_error_message_len()` and `polyplug_runtime_last_error()`; returns string

---

## [PARALLEL GROUP: LOADER RESTRUCTURING]

**Blockers:** None  
**Parallel:** Yes - all 6 loaders can be restructured in parallel

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-native/` with `.rockspec` and module structure
  - **Verification:** `luarocks pack host-libs/lua/loaders/polyplug-loaders-native/polyplug-loaders-native-1.0-1.rockspec` succeeds

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-python/` with `.rockspec`
  - **Verification:** `luarocks pack` succeeds for python loader

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-lua/` with `.rockspec`
  - **Verification:** `luarocks pack` succeeds for lua loader

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-js/` with `.rockspec`
  - **Verification:** `luarocks pack` succeeds for js loader

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-js-deno/` with `.rockspec`
  - **Verification:** `luarocks pack` succeeds for js-deno loader

- [ ] Create `host-libs/lua/loaders/polyplug-loaders-dotnet/` with `.rockspec`
  - **Verification:** `luarocks pack` succeeds for dotnet loader

- [ ] Remove old loader files from `host-libs/lua/loaders/`
  - **Verification:** Old `host-libs/lua/loaders/*.lua` files deleted; no `require` references old paths

---

## Phase 5: Update Codegen for Lua

**Blockers:** Phase 2 complete  
**Parallel:** No

- [ ] Update `generate_host_caller_function` in `crates/polyplug_codegen/src/generators/lua.rs` to use cached FFI types
  - **Verification:** Generated code uses module-level `VTableType` and `DispatchFnType`; no `ffi.cast` per call

- [ ] Run `cargo test --lib lua` to verify codegen tests pass
  - **Verification:** All Lua codegen tests pass with exit code 0

---

## New Directory Structure

```
host-libs/lua/
├── polyplug.lua                 # Core runtime (no loaders)
├── polyplug.d.lua               # Type definitions
├── scanner.lua
├── loaders/
│   ├── polyplug-loaders-native/
│   │   ├── polyplug-loaders-native-1.0-1.rockspec
│   │   └── polyplug/loaders/native.lua
│   ├── polyplug-loaders-python/
│   │   ├── polyplug-loaders-python-1.0-1.rockspec
│   │   └── polyplug/loaders/python.lua
│   ├── polyplug-loaders-lua/
│   │   ├── polyplug-loaders-lua-1.0-1.rockspec
│   │   └── polyplug/loaders/lua.lua
│   ├── polyplug-loaders-js/
│   │   ├── polyplug-loaders-js-1.0-1.rockspec
│   │   └── polyplug/loaders/js.lua
│   ├── polyplug-loaders-js-deno/
│   │   ├── polyplug-loaders-js-deno-1.0-1.rockspec
│   │   └── polyplug/loaders/js_deno.lua
│   └── polyplug-loaders-dotnet/
│       ├── polyplug-loaders-dotnet-1.0-1.rockspec
│       └── polyplug/loaders/dotnet.lua
└── README.md
```

---

## Performance Expectations

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~100ns (FFI calls) | ~2ns (cached) |
| Function cast | ~50ns (ffi.cast) | ~0ns (cached) |
| Guard operations | ~200ns (create/destroy) | ~10ns (reused) |
| **Hot path** | ~350ns | ~50-100ns |

---

## Critical Bug Summary

**`find_by_bundle` is completely broken:**

```lua
-- BEFORE (WRONG):
function M.Runtime:find_by_bundle(bundle_name, contract, min_version)
    return ffi.cast("uint64_t", 1)  -- Returns dummy handle!
end

-- AFTER (CORRECT):
function M.Runtime:find_by_bundle(bundle_id, contract_id, min_version)
    return self._lib.polyplug_runtime_find_by_bundle(self._ptr, bundle_id, contract_id, min_version)
end
```

This bug causes all bundle-specific plugin lookups to fail silently.

---

## PRD References

- PRD §8: "LuaJIT FFI host lib, Runtime metatable, Guard metatable"
- PRD §8: "register_*_loader() functions (one per loader)" - separate packages
- PRD §10 (Lua): "Performance: LuaJIT FFI call overhead is within 2x of native vtable dispatch"

---

## Estimated Effort

- Phase 0: 15 minutes (CRITICAL BUG FIX)
- Phase 1: 1 hour
- Phase 2: 1 hour
- Phase 3: 30 minutes
- Phase 4: 2 hours (parallel execution)
- Phase 5: 1 hour
- Testing: 1 hour

**Total: ~5 hours**