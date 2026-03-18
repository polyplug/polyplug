# JavaScript (Deno) Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The JavaScript host lib for Deno has performance issues with FFI calls and vtable access. Deno's FFI is fast (V8 fast calls ~50-200ns), but the current implementation creates overhead through repeated operations.

---

## Phase 1: Add VTable Caching to Guard

**Blockers:** None  
**Parallel:** No

- [ ] Modify `Guard` class in `host-libs/js/polyplug.js` to cache vtable pointer at construction
  - **Verification:** `Guard` constructor calls `lib.symbols.polyplug_runtime_plugin_vtable(ptr)` once and stores in `#vtable` private field

- [ ] Update `vtable()` method to return cached pointer
  - **Verification:** `guard.vtable()` returns `#vtable` field; no FFI call on method invocation

---

## Phase 2: Module-Level Function Pointer Caching

**Blockers:** None  
**Parallel:** Yes

- [ ] Add module-level `_funcCache` Map for caching `UnsafeFnPointer` instances
  - **Verification:** `const _funcCache = new Map<bigint, UnsafeFnPointer>()` exists at module scope

- [ ] Add module-level `_DISPATCH_FN_TYPE` prototype definition
  - **Verification:** `const _DISPATCH_FN_TYPE = new UnsafeFunctionPrototype(...)` defined once at module scope

- [ ] Rewrite `callPluginFn` to use cached function pointers
  - **Verification:** Function checks `_funcCache.get(funcPtr)` before creating new `UnsafeFnPointer`; cache populated on first call

---

## Phase 3: Optimize Memory Access Patterns

**Blockers:** None  
**Parallel:** Yes

- [ ] Replace repeated `UnsafePointerView` creation with direct `BigUint64Array` access
  - **Verification:** VTable structure read via `new BigUint64Array(UnsafePointerView.getArrayBuffer(vtablePtr, 16))`; single view creation

- [ ] Add lo/hi u32 split optimization for u64 values in hot path
  - **Verification:** Hot path uses `{ lo: number, hi: number }` pattern instead of `bigint` for contract IDs and pointers

---

## [PARALLEL GROUP: LOADER RESTRUCTURING]

**Blockers:** None  
**Parallel:** Yes - all 6 loaders can be restructured in parallel

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-native/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-python/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-lua/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-js/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-js-deno/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Create `host-libs/js/loaders/@polyplug/loaders-dotnet/` with `deno.json` and `mod.ts`
  - **Verification:** `deno publish --dry-run` succeeds in loader directory

- [ ] Remove old loader files from `host-libs/js/loaders/`
  - **Verification:** Old `host-libs/js/loaders/*.ts` files deleted; no imports reference old paths

- [ ] Update `host-libs/js/deno.json` to reference new loader packages
  - **Verification:** `deno task` in `host-libs/js/` succeeds with new loader imports

---

## Phase 5: Update Codegen for QuickJS

**Blockers:** Phase 2 complete  
**Parallel:** No

- [ ] Update `generate_host_caller_class_quickjs` in `crates/polyplug_codegen/src/generators/js_quickjs.rs` to use cached function pointers
  - **Verification:** Generated code uses module-level `_funcCache` pattern; no `new UnsafeFnPointer` per call

- [ ] Run `cargo test --lib js_quickjs` to verify codegen tests pass
  - **Verification:** All JS codegen tests pass with exit code 0

---

## New Directory Structure

```
host-libs/js/
├── polyplug.ts                  # Core runtime (no loaders)
├── polyplug.d.ts
├── polyplug.js                  # Updated with Guard caching
├── deno.json
├── loaders/
│   ├── @polyplug/loaders-native/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   ├── @polyplug/loaders-python/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   ├── @polyplug/loaders-lua/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   ├── @polyplug/loaders-js/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   ├── @polyplug/loaders-js-deno/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   └── @polyplug/loaders-dotnet/
│       ├── deno.json
│       ├── mod.ts
│       └── deno.d.ts
└── README.md
```

---

## Performance Expectations

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~150ns (FFI call) | ~10ns (cached) |
| Function pointer creation | ~200ns (new) | ~0ns (cached) |
| Type cast | ~50ns | ~0ns |
| **Hot path** | ~400-500ns | ~50-100ns |

---

## PRD References

- PRD §8: "Deno.dlopen host lib, Runtime class, TypeScript types"
- PRD §8: "register*Loader() functions (one per loader)" - separate packages
- PRD §10 (JS): "Performance: <10ns (V8 fast call), ~150ns (BigInt/slow path)"

---

## Estimated Effort

- Phase 1: 30 minutes
- Phase 2: 1 hour
- Phase 3: 1 hour
- Phase 4: 2 hours (parallel execution)
- Phase 5: 1 hour
- Testing: 1 hour

**Total: ~5.5 hours**