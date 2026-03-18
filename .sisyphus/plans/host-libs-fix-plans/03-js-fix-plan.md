# JavaScript (Deno) Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The JavaScript host lib for Deno has performance issues with FFI calls and vtable access. Deno's FFI is fast (V8 fast calls ~50-200ns), but the current implementation creates overhead through repeated operations.

## Issues Found

### 1. Loaders Embedded in Main Package (CRITICAL)

**Current State:**
```
host-libs/js/loaders/native.ts
host-libs/js/loaders/python.ts
host-libs/js/loaders/lua.ts
host-libs/js/loaders/js.ts
host-libs/js/loaders/js_deno.ts
host-libs/js/loaders/dotnet.ts
```

All loaders in the main `polyplug` module.

**Required Change:**
Each loader should be a separate JSR package:
```
host-libs/js/loaders/@polyplug/loaders-native/
host-libs/js/loaders/@polyplug/loaders-python/
host-libs/js/loaders/@polyplug/loaders-lua/
host-libs/js/loaders/@polyplug/loaders-js/
host-libs/js/loaders/@polyplug/loaders-js-deno/
host-libs/js/loaders/@polyplug/loaders-dotnet/
```

Each with:
- `deno.json` or `jsr.json`
- `mod.ts`
- `deno.d.ts`

### 2. No VTable Caching in Guard (PERFORMANCE)

**Current State (polyplug.js lines 264-292):**
```javascript
export class Guard {
  #lib;
  #ptr;

  constructor(lib, ptr) {
    this.#lib = lib;
    this.#ptr = ptr;
  }

  vtable() {
    return this.#lib.symbols.polyplug_runtime_plugin_vtable(this.#ptr);
  }
}
```

Every call to `vtable()` performs a P/Invoke call.

**Required Change:**
Cache the vtable pointer at construction:
```typescript
export class Guard {
  #lib: Deno.DynamicLibrary<typeof SYMBOLS>;
  #ptr: Deno.PointerValue;
  #vtable: Deno.PointerValue;  // Cached

  constructor(lib: Deno.DynamicLibrary<typeof SYMBOLS>, ptr: Deno.PointerValue) {
    this.#lib = lib;
    this.#ptr = ptr;
    // Cache vtable at construction - one P/Invoke
    this.#vtable = lib.symbols.polyplug_runtime_plugin_vtable(ptr);
  }

  vtable(): Deno.PointerValue {
    return this.#vtable;  // No P/Invoke on hot path
  }
}
```

### 3. callPluginFn Creates New Objects Every Call (PERFORMANCE - CRITICAL)

**Current State (polyplug.js lines 108-151):**
```javascript
export function callPluginFn(lib, vtablePtr, funcIdx, input) {
  // Creates new UnsafePointerView every call
  const view = new Deno.UnsafePointerView(vtablePtr);
  const funcCount = view.getBigUint64(0);
  const funcsPtr = view.getBigUint64(8);
  
  // Creates new UnsafePointerView every call
  const funcsView = new Deno.UnsafePointerView(Deno.UnsafePointer.create(funcsPtr));
  const funcPtr = funcsView.getBigUint64(funcIdx * 8);
  
  // Creates new UnsafeFnPointer EVERY CALL
  const func = new Deno.UnsafeFnPointer(
    funcPtr,
    new Deno.UnsafeFunctionPrototype({ parameters: ["pointer", "pointer"], result: "u32" })
  );
  
  // Creates new buffers every call
  const inputBuf = new Uint8Array(inputData);
  const outputBuf = new Uint8Array(16);
  // ...
}
```

**Required Change:**
Cache function pointer wrappers and reuse buffers:
```typescript
// Module-level cache for function wrappers
const _funcCache = new Map<bigint, Deno.UnsafeFnPointer>();

// Pre-defined function type (created once)
const _DISPATCH_FN_TYPE = new Deno.UnsafeFunctionPrototype(
  { parameters: ["pointer", "pointer"], result: "u32" }
);

export function callPluginFn(
  vtablePtr: Deno.PointerValue,
  funcIdx: number,
  input: string
): string {
  // Direct memory access without creating new views
  const vtableData = new BigUint64Array(
    Deno.UnsafePointerView.getArrayBuffer(vtablePtr, 16)
  );
  const funcCount = vtableData[0];
  const funcsPtr = vtableData[1];
  
  if (funcIdx >= Number(funcCount)) {
    throw new Error(`function index ${funcIdx} out of bounds`);
  }
  
  // Get function pointer
  const funcPtr = new BigUint64Array(
    Deno.UnsafePointerView.getArrayBuffer(funcsPtr, 8 * Number(funcCount))
  )[funcIdx];
  
  // Check cache for function wrapper
  let func = _funcCache.get(funcPtr);
  if (!func) {
    func = new Deno.UnsafeFnPointer(funcPtr, _DISPATCH_FN_TYPE);
    _funcCache.set(funcPtr, func);
  }
  
  // ... rest of call
}
```

### 4. Generated Code Creates Objects Every Call (CODEGEN)

**Current State (codegen js_quickjs.rs lines 586-616):**
```typescript
// Generated code creates new objects every call
out.push_str("        const fnPtr = this.vtable.functions[");  // Array access
out.push_str("        const fn = fnPtr as unknown as (args: any, out: any) => { lo: number; hi: number };\n");
out.push_str("        const outVal = { lo: 0, hi: 0 };\n");
```

**Required Change:**
Generated code should use cached function pointers and pre-allocated output.

### 5. Missing BigInt Handling Optimization

**Current State:**
JavaScript/TypeScript uses `bigint` for u64 values, which is slower than number.

**Required Change:**
For hot-path operations, use the lo/hi split pattern (as done in QuickJS generator):
```typescript
// Instead of: bigint (slow)
const contractId: bigint = 0xCC4232FAB0410D2Bn;

// Use: lo/hi pair (fast)
const contractId = { lo: 0xB0410D2B, hi: 0xCC4232FA };
```

## Files to Modify

1. **host-libs/js/polyplug.js**
   - Add vtable caching to `Guard` class
   - Add function pointer caching
   - Optimize `callPluginFn`

2. **host-libs/js/polyplug.d.ts**
   - Update type definitions

3. **host-libs/js/loaders/*.ts**
   - Move to separate packages

4. **crates/polyplug_codegen/src/generators/js_quickjs.rs**
   - Apply similar optimizations to generated callers

## New Directory Structure

```
host-libs/js/
├── polyplug.ts                  # Core runtime (no loaders)
├── polyplug.d.ts
├── deno.json
├── loaders/
│   ├── @polyplug/loaders-native/
│   │   ├── deno.json
│   │   ├── mod.ts
│   │   └── deno.d.ts
│   ├── @polyplug/loaders-python/
│   │   └── ...
│   ├── @polyplug/loaders-lua/
│   │   └── ...
│   ├── @polyplug/loaders-js/
│   │   └── ...
│   ├── @polyplug/loaders-js-deno/
│   │   └── ...
│   └── @polyplug/loaders-dotnet/
│       └── ...
└── README.md
```

## Performance Expectations

| Operation | Current | Optimized |
|-----------|---------|-----------|
| VTable access | ~150ns (FFI call) | ~10ns (cached) |
| Function pointer creation | ~200ns (new) | ~0ns (cached) |
| Type cast | ~50ns | ~0ns |
| **Hot path** | ~400-500ns | ~50-100ns |

Deno FFI can achieve near-native performance with proper caching.

## Implementation Order

1. Add vtable caching to `Guard`
2. Add function pointer caching module-level
3. Rewrite `callPluginFn` with caching
4. Move loaders to separate packages
5. Update codegen

## Estimated Effort

- Guard vtable caching: 30 minutes
- Function pointer caching: 1 hour
- callPluginFn optimization: 1 hour
- Loader restructuring: 2 hours
- Testing: 1 hour

**Total: ~5.5 hours**

## PRD References

- PRD §8: "Deno.dlopen host lib, Runtime class, TypeScript types"
- PRD §8: "register*Loader() functions (one per loader)" - should be separate packages
- PRD §10 (JS): "Performance: <10ns (V8 fast call), ~150ns (BigInt/slow path)"