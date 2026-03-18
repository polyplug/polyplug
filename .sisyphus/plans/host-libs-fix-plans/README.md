# Host Libs Fix Plans Overview

## Summary

Analysis of all host libraries for zero-overhead and MAX performance issues, following the C# fix as the reference implementation.

---

## Status Matrix

| Host Lib | Status | Critical Bug | Est. Effort |
|----------|--------|--------------|-------------|
| C# | ✅ DONE | None | Completed |
| C++ | 🔴 NEEDS FIXES | None | ~9h |
| Python | 🔴 NEEDS FIXES | None | ~6.5h |
| JavaScript | 🔴 NEEDS FIXES | None | ~6h |
| Lua | 🔴 NEEDS FIXES + BUG | `find_by_bundle` returns `1` | ~5.75h |
| Rust | 🟡 OPTIONAL | None | ~1.75h (optional) |

**Total effort: ~28.75 hours (26.75h required + 1.75h optional)**

---

## Plans

| Plan | Critical Bug | Key Issues |
|------|--------------|------------|
| [01-cpp-fix-plan.md](./01-cpp-fix-plan.md) | None | Loaders embedded, no vtable caching, codegen resolves every call |
| [02-python-fix-plan.md](./02-python-fix-plan.md) | None | ctypes types created every call, no caching, no vtable caching |
| [03-js-fix-plan.md](./03-js-fix-plan.md) | None | UnsafeFnPointer created every call, no vtable caching |
| [04-lua-fix-plan.md](./04-lua-fix-plan.md) | **YES** | `find_by_bundle` broken, no Guard class, ffi.cast every call |
| [05-rust-fix-plan.md](./05-rust-fix-plan.md) | None | Optional PluginGuard for RAII consistency |

---

## Common Issues Across Hosts

### 1. Loaders Embedded in Main Package
All host libs (except Rust) have loaders embedded. Per PRD, each loader should be a separate package.

**Affected:** C++, Python, JavaScript, Lua

### 2. No VTable Caching
All host libs resolve vtable on every call instead of caching at Guard construction.

**PRD §7:** "Hot path call: One guard load. One pointer dereference. One indirect call."

**Affected:** C++, Python, JavaScript, Lua

### 3. Function Pointer Wrappers Created Every Call
All host libs create new function pointer wrappers on every call.

**Affected:** Python (CFUNCTYPE), JavaScript (UnsafeFnPointer), Lua (ffi.cast)

---

## Critical Bug: Lua `find_by_bundle`

```lua
-- BROKEN: Returns dummy handle
function M.Runtime:find_by_bundle(bundle_name, contract, min_version)
    return ffi.cast("uint64_t", 1)  -- WRONG!
end
```

**Impact:** All bundle-specific plugin lookups fail silently.

**Fix Priority:** IMMEDIATE (Phase 0 in Lua plan)

---

## Implementation Order Recommendation

1. **Lua Phase 0** - Fix `find_by_bundle` bug (15 min)
2. **C++ Full Fix** - Most similar to C#, native interop priority
3. **Python Full Fix** - High-usage language
4. **JavaScript Full Fix** - High-usage language
5. **Lua Phases 1-5** - Complete remaining fixes
6. **Rust Optional** - If consistency desired

---

## PRD Compliance Matrix

| Requirement | C# | C++ | Python | JS | Lua | Rust |
|-------------|----|----|--------|----|----|------|
| Separate loader packages | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| VTable caching | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| Function pointer caching | ✅ | ✅ | ❌ | ❌ | ❌ | ✅ |
| Zero-overhead hot path | ✅ | ⚠️ | ❌ | ⚠️ | ❌ | ✅ |

---

## Key Learnings from C# Fix

### What We Fixed

| Issue | Before | After |
|-------|--------|-------|
| Loader packages | Embedded in main | Separate NuGet packages |
| P/Invoke style | DllImport | LibraryImport (source-generated) |
| GC transition | 50-200ns per call | ~5-15ns with SuppressGCTransition |
| Dispatch style | Marshal.GetDelegateForFunctionPointer | delegate* unmanaged (4-6x faster) |
| VTable access | P/Invoke every call | Cached in PluginGuard |

### Performance Numbers

| Operation | Before | After |
|-----------|--------|-------|
| VTable access | 50-200ns | ~0ns (cached) |
| Function dispatch | 250-800ns | 5-15ns |
| **Hot path** | 300-1000ns | 20-30ns |

---

## Next Steps

1. Pick first host lib to fix (recommend C++)
2. Create dedicated session for implementation
3. After host libs fixed, analyze guest libs

---

## Self-Review

| Aspect | Status | Notes |
|--------|--------|-------|
| All plans have checkbox format | ✅ | All tasks use `- [ ]` |
| Tasks are atomic | ✅ | Each task = one action + one verification |
| Verifications are concrete | ✅ | All verifications are testable |
| Parallel groups marked | ✅ | Each plan shows [PARALLEL GROUP: X] |
| Blockers identified | ✅ | Sequential dependencies clearly stated |
| Critical bug highlighted | ✅ | Lua `find_by_bundle` in Phase 0 |
| Effort estimated | ✅ | Each plan has time breakdown |