# Host Libs Fix Plans Overview

## Summary

Analysis of all host libraries for zero-overhead and MAX performance issues, following the C# fix as the reference implementation.

## Status

| Host Lib | Status | Critical Issues | Est. Effort |
|----------|--------|-----------------|-------------|
| C# | ✅ DONE | Fixed | 8h (completed) |
| C++ | 🔴 NEEDS FIXES | Loaders embedded, no vtable caching | ~8h |
| Python | 🔴 NEEDS FIXES | ctypes overhead, no caching | ~5.5h |
| JavaScript | 🔴 NEEDS FIXES | FFI object creation, no caching | ~5.5h |
| Lua | 🔴 NEEDS FIXES | **Broken find_by_bundle**, no caching | ~5h |
| Rust | 🟡 MINOR | Optional PluginGuard for consistency | ~1.5h (optional) |

**Total estimated effort: ~25.5 hours**

## Plans

1. [C++ Fix Plan](./01-cpp-fix-plan.md)
2. [Python Fix Plan](./02-python-fix-plan.md)
3. [JavaScript Fix Plan](./03-js-fix-plan.md)
4. [Lua Fix Plan](./04-lua-fix-plan.md)
5. [Rust Fix Plan](./05-rust-fix-plan.md)

## Common Issues Across All Hosts

### 1. Loaders Embedded in Main Package

All host libs (except Rust which uses adapter crates) have loaders embedded in the main package. Per PRD §8 and §24, each loader should be a separate package.

**Affected:** C++, Python, JavaScript, Lua

### 2. No VTable Caching

All host libs resolve the vtable on every call instead of caching it at `PluginGuard` construction.

**PRD §7 Quote:** "Hot path call: One guard load. One pointer dereference. One indirect call."

**Affected:** C++, Python, JavaScript, Lua

### 3. Function Pointer Casts on Every Call

All host libs create new function pointer wrappers on every call instead of caching them.

**PRD §10 Quote:** "delegate* unmanaged used for vtable calls: calli IL = ~4–6x faster than Marshal.GetDelegateForFunctionPointer"

**Affected:** Python (ctypes.CFUNCTYPE), JavaScript (UnsafeFnPointer), Lua (ffi.cast)

## Critical Bug

**Lua `find_by_bundle` is completely broken:**

```lua
-- Current (WRONG):
function M.Runtime:find_by_bundle(bundle_name, contract, min_version)
    return ffi.cast("uint64_t", 1)  -- Returns dummy handle!
end
```

This must be fixed immediately as it breaks all bundle-specific plugin lookups.

## PRD Compliance Matrix

| Requirement | C# | C++ | Python | JS | Lua | Rust |
|-------------|----|----|--------|----|----|------|
| Separate loader packages | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| VTable caching | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| Function pointer caching | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ |
| Zero-overhead hot path | ✅ | ⚠️ | ❌ | ⚠️ | ❌ | ✅ |
| caller-provides-buffer | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |

## Recommended Implementation Order

1. **Lua: Fix `find_by_bundle`** - Critical bug, 15 minutes
2. **C++: Full fix** - Most similar to C#, highest priority for native interop
3. **Python: Full fix** - High-usage language
4. **JavaScript: Full fix** - High-usage language
5. **Lua: Full fix** - Complete the remaining issues
6. **Rust: Optional PluginGuard** - For consistency

## Next Steps

1. Commit these plans
2. Pick the first host lib to fix (recommend C++ as most similar to C#)
3. Create a dedicated session for that host lib's fixes
4. After host libs are fixed, do the same analysis for guest libs

## Files Created

```
.sisyphus/plans/host-libs-fix-plans/
├── README.md (this file)
├── 01-cpp-fix-plan.md
├── 02-python-fix-plan.md
├── 03-js-fix-plan.md
├── 04-lua-fix-plan.md
└── 05-rust-fix-plan.md
```

---

## Key Learnings from C# Fix

### What We Fixed in C#

1. **Loaders to separate packages**
   - Before: All in `Polyplug/` project
   - After: `Loaders/Native/`, `Loaders/Python/`, etc.

2. **`LibraryImport` instead of `DllImport`**
   - Source-generated P/Invoke (AOT-safe)
   - Faster than runtime marshalling

3. **`[SuppressGCTransition]` on hot path**
   - Eliminates GC transition overhead (~50-200ns → ~5-15ns)
   - NOT on `host_alloc/host_free` (may trigger GC)

4. **`delegate* unmanaged` for vtable dispatch**
   - Direct function pointer call
   - ~4-6x faster than `Marshal.GetDelegateForFunctionPointer`

5. **VTable caching in `PluginGuard`**
   - Cache vtable pointer at construction
   - No P/Invoke on hot path

6. **No `CallFunction` extension method**
   - Violated zero-overhead
   - Generated code should dispatch directly

### Performance Numbers (C#)

| Operation | Before | After |
|-----------|--------|-------|
| Guard creation | ~100ns | ~100ns (unchanged) |
| VTable access | ~50-200ns (P/Invoke) | ~0ns (cached) |
| Function dispatch | ~250-800ns | ~5-15ns |
| **Hot path** | ~300-1000ns | ~20-30ns |

Apply similar optimizations to each host lib.