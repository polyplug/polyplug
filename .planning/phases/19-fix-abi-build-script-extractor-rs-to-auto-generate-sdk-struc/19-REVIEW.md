---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
reviewed: 2026-04-12T00:00:00Z
depth: quick
files_reviewed: 18
files_reviewed_list:
  - crates/polyplug_abi/build/extractor.rs
  - crates/polyplug_abi/build/generate.rs
  - crates/polyplug_abi/build/main.rs
  - crates/polyplug_abi/build/mapper.rs
  - crates/polyplug_abi/build/types.rs
  - crates/polyplug_codegen/src/languages/cpp.rs
  - crates/polyplug_codegen/src/languages/csharp.rs
  - crates/polyplug_codegen/src/languages/js.rs
  - crates/polyplug_codegen/src/languages/lua.rs
  - crates/polyplug_codegen/src/languages/python.rs
  - crates/polyplug_codegen/src/data.rs
  - sdks/python/host/polyplug/runtime.py
  - sdks/csharp/host/NativeMethods.cs
  - sdks/lua/host/polyplug/runtime.lua
  - sdks/js/host/polyplug/mod.js
  - sdks/cpp/host/polyplug/runtime.hpp
  - sdks/cpp/guest/polyplug/guest.hpp
  - sdks/js/guest/polyplug_guest.js
findings:
  critical: 2
  warning: 4
  info: 4
  total: 10
status: issues_found
---

# Phase 19: Code Review Report

**Reviewed:** 2026-04-12T00:00:00Z
**Depth:** quick
**Files Reviewed:** 18
**Status:** issues_found

## Summary

Quick-depth pattern scan across 18 files spanning the ABI build script extractor, codegen language generators, and SDK host/guest bindings. Found 2 critical issues, 4 warnings, and 4 informational items. The critical issues are a function that silently discards its result (returns empty string unconditionally) and duplicated typedef generation that causes redundant work. The SDK files are otherwise clean of hardcoded secrets, injection vulnerabilities, and dangerous function calls.

## Critical Issues

### CR-01: StringViewHelper.toString always returns empty string

**File:** `sdks/js/guest/polyplug_guest.js:182-185`
**Issue:** The `StringViewHelper.toString()` method unconditionally returns `''` after checking for null/empty input. The actual string decoding logic is never executed. This is dead code that will silently produce wrong results for any caller using this method instead of the separate `toStr()` function.
**Fix:**
```javascript
static toString(sv) {
    if (!sv || sv.len === 0) return '';
    // Delegate to the working toStr implementation
    return toStr(sv);
}
```

### CR-02: CppGenerator and LuaGenerator emit duplicate typedefs for function pointer fields

**File:** `crates/polyplug_codegen/src/languages/cpp.rs:252-258` and `crates/polyplug_codegen/src/languages/cpp.rs:284-289`
**Issue:** In `generate_struct`, function pointer typedefs are generated twice: once in the pre-scan loop (lines 252-258) that collects into `typedefs`, and again inside the field iteration loop (lines 284-289) via `generate_fn_ptr_typedef`. The pre-scan loop emits them to the `typedefs` string which is then prepended to the output. The second call inside the field loop discards the first return value (the typedef text) and only uses the type name. While the second call does not produce visible duplication in the output (the typedef text is discarded via `_`), it recomputes the typedef string wastefully. More importantly, the `generate_fn_ptr_typedef` call on line 287 is a redundant re-computation -- the first call already produced the same typedef name. The same pattern exists in `lua.rs:249-255` and `lua.rs:277-282`.
**Fix:** Store the `(typedef_text, type_name)` pairs from the pre-scan in a `HashMap<String, String>` keyed by field name, and look up the type name during field iteration instead of calling `generate_fn_ptr_typedef` a second time.

## Warnings

### WR-01: C++ Runtime::build() ignores config and callback in both branches

**File:** `sdks/cpp/host/polyplug/runtime.hpp:67-83`
**Issue:** The `Builder::build()` method has a conditional that checks `config_.has_value() || on_reload_cb_.has_value()`, but both the true and false branches call `polyplug_runtime_create()` without passing any options. The "with options" FFI function `polyplug_runtime_create_with_options` is declared at line 34 but never called. This means configuration and reload callbacks are silently ignored.
**Fix:**
```cpp
Runtime build() {
    if (config_.has_value() || on_reload_cb_.has_value()) {
        // Build RuntimeConfig and call polyplug_runtime_create_with_options
        RuntimeConfig cfg = config_.value_or(RuntimeConfig{});
        if (on_reload_cb_.has_value()) {
            // Wire callback into cfg.on_reload
        }
        const HostInterface* h = polyplug_runtime_create_with_options(&cfg);
        if (h == nullptr) {
            throw std::runtime_error("polyplug_runtime_create_with_options returned null");
        }
        return Runtime(h);
    } else {
        const HostInterface* h = polyplug_runtime_create();
        if (h == nullptr) {
            throw std::runtime_error("polyplug_runtime_create returned null");
        }
        return Runtime(h);
    }
}
```

### WR-02: C++ find_guest_contract marked noexcept but calls ensure_host() which throws

**File:** `sdks/cpp/host/polyplug/runtime.hpp:149-153`
**Issue:** `find_guest_contract` is declared `noexcept`, but `ensure_host()` at line 150 calls `throw std::runtime_error("Runtime is destroyed")`. Throwing from a `noexcept` function calls `std::terminate`. The same issue affects `resolve_guest_contract` (line 189) and `find_by_bundle` (line 157).
**Fix:** Either remove `noexcept` from these methods, or change `ensure_host()` to return an error code / use a different failure mode for `noexcept` methods.

### WR-03: Lua runtime module-level mutable state is shared across all instances

**File:** `sdks/lua/host/polyplug/runtime.lua:119-120`
**Issue:** `M._pending_reload_callback` and `M._pending_config` are module-level state. If `on_reload` or `set_config` is called, the state persists for all subsequent `Runtime.new()` calls. There is no mechanism to clear this state after consumption, meaning a second `Runtime.new()` call will re-use the previous callback/config unintentionally.
**Fix:** Clear `M._pending_reload_callback` and `M._pending_config` to `nil` after they are consumed in `M.Runtime.new()`.

### WR-04: JS guest toStr passes Number(ptr) to UnsafePointerView which expects pointer

**File:** `sdks/js/guest/polyplug_guest.js:297-299`
**Issue:** When `typeof sv.ptr === 'bigint'` and Deno is available, the code does `const ptrNum = Number(ptr)` then passes `ptrNum` to `new Deno.UnsafePointerView(ptrNum)`. On a 64-bit system, `Number()` truncates BigInt values larger than 2^53, causing pointer corruption for addresses in the upper half of the address space.
**Fix:**
```javascript
if (typeof Deno !== 'undefined' && Deno.UnsafePointerView) {
    const view = new Deno.UnsafePointerView(ptr); // Pass BigInt directly
    return view.getUtf8String(sv.len);
}
```

## Info

### IN-01: TODO comment in Lua runtime

**File:** `sdks/lua/host/polyplug/runtime.lua:254`
**Issue:** `-- TODO: Implement via list_bundles + find_guest_contract if needed` -- a deprecated method stub with a TODO marker. This is intentional technical debt for a removed FFI function, so low priority.
**Fix:** Consider removing the deprecated `find_by_bundle` method entirely if it is not planned for reimplementation.

### IN-02: Console.log reference in commented-out JS SDK code

**File:** `sdks/js/host/polyplug/native-loader.ts:147`
**Issue:** A commented-out `console.log` example appears in a JSDoc comment. This is documentation-only, not a debug artifact.
**Fix:** No action needed -- this is intentional documentation.

### IN-03: Build script main.rs uses unwrap/expect freely

**File:** `crates/polyplug_abi/build/main.rs:37,40,42,48,107`
**Issue:** Multiple `unwrap()` and `expect()` calls in the build script entry point. This is acceptable per the crate-level `#![allow(clippy::expect_used)]` and `#![allow(clippy::unwrap_used)]` directives at line 12-13. Build scripts are expected to panic on configuration errors rather than propagate errors gracefully.
**Fix:** No action needed.

### IN-04: Duplicated is_option/strip_option/is_array/is_function_pointer methods across generators

**File:** `crates/polyplug_codegen/src/languages/cpp.rs:23-35`, `crates/polyplug_codegen/src/languages/csharp.rs:23-39`, `crates/polyplug_codegen/src/languages/js.rs:18-35`, `crates/polyplug_codegen/src/languages/lua.rs:18-31`, `crates/polyplug_codegen/src/languages/python.rs:18-53`
**Issue:** Each language generator re-implements the same `is_option`, `strip_option`, `is_array`, and (in some cases) `is_function_pointer` helper methods with identical logic. These could be extracted into a shared utility module or a trait method.
**Fix:** Extract common type-inspection methods into a shared `TypeUtils` trait or module to reduce duplication.

---

_Reviewed: 2026-04-12T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: quick_
