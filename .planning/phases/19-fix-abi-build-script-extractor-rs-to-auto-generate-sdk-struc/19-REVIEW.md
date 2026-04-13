---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
reviewed: 2026-04-13T00:00:00Z
depth: standard
files_reviewed: 7
files_reviewed_list:
  - crates/polyplug_abi/build/generate.rs
  - crates/polyplug_codegen/src/languages/csharp.rs
  - sdks/cpp/abi/polyplug/abi.hpp
  - sdks/cpp/host/polyplug/handle.hpp
  - sdks/csharp/abi/Abi.cs
  - sdks/js/abi/abi.ts
  - sdks/lua/abi/abi.lua
findings:
  critical: 3
  warning: 6
  info: 3
  total: 12
status: issues_found
---

# Phase 19: Code Review Report

**Reviewed:** 2026-04-13T00:00:00Z
**Depth:** standard
**Files Reviewed:** 7
**Status:** issues_found

## Summary

Reviewed the ABI build script code generator (`generate.rs`), the C# codegen backend (`csharp.rs`), and the five generated SDK ABI files (C++, C#, Lua, JavaScript, plus C++ `handle.hpp`). Three critical issues found: all three are caused by the code generator emitting raw Rust type syntax (`crate::host::...`, `T*`, `c_char`) into non-Rust target language files, which will cause compilation or parse failures. Six warnings cover duplicate enum definitions, C# `Debug.Assert` outside method bodies, Lua comment style inconsistencies, a broken PascalCase-to-UPPER_SNAKE_CASE converter, and other code quality concerns. Three informational items cover dead code and noisy build output.

## Critical Issues

### CR-01: Raw Rust path syntax `crate::host::*` emitted into C++, Lua, and C# generated files

**File:** `sdks/cpp/abi/polyplug/abi.hpp:210-216`
**Also:** `sdks/lua/abi/abi.lua:181-187`, `sdks/csharp/abi/Abi.cs:252`

**Issue:** The generated code contains Rust module path syntax like `crate::host::HostContractInstance` and `crate::host::HostContractInterface` in C++ typedefs, Lua typedefs, and C# delegate definitions. These are Rust-specific paths that are syntactically invalid in all three target languages. This will cause:
- C++: compilation failure (`crate` is not a valid identifier in this context)
- C#: compilation failure (`crate::host` is not valid C# syntax)
- Lua/LuaJIT FFI: parse failure from `ffi.cdef`

This indicates the type mapping in the code generator is not translating these types correctly. The `HostContractInstance` and `HostContractInterface` types used as return/parameter types in `HostInterface` function pointers are resolving to their raw Rust FQN instead of the target-language equivalent.

**Fix:** The `rust_type_to_csharp`, C++ type mapper, and Lua type mapper need to recognize `crate::host::HostContractInstance` and `crate::host::HostContractInterface` and map them to the correct target types. For C++ this should be `HostContractInstance` / `HostContractInterface`. For C# it should be `HostContractInstance` / `HostContractInterface`. The root cause is likely in how the ABI extractor records these types or how the codegen resolves cross-module type references.

### CR-02: Generic `T*` emitted in Array struct for C++ and Lua generated files

**File:** `sdks/cpp/abi/polyplug/abi.hpp:849`
**Also:** `sdks/lua/abi/abi.lua:847`

**Issue:** The generated `Array` struct emits `T* items;` as the pointer field. `T*` is generic syntax that is not valid in C without a typedef, and it is not valid for LuaJIT `ffi.cdef` at all. The C++ output has `T*` which will not compile without a template declaration wrapping the struct. The Lua output has `T*` inside `ffi.cdef` which will cause a LuaJIT parse error at load time. The C# version correctly maps to `IntPtr`, but C++ and Lua do not.

**Fix:** The C++ and Lua code generators should emit `void* items;` (or equivalent opaque pointer) for the Array struct's items field, since the Array type is generic and the actual element type is not known at codegen time. The C# generator already handles this correctly by using `IntPtr`.

### CR-03: `c_char` type emitted into C++ and Lua generated files

**File:** `sdks/cpp/abi/polyplug/abi.hpp:471`
**Also:** `sdks/lua/abi/abi.lua:442`

**Issue:** The `RuntimeInterface_load_bundle_fn` typedef uses `const c_char*` as the path parameter type. `c_char` is a Rust FFI type alias (`std::ffi::c_char`) that does not exist in C++ standard headers (it should be `char`) or in LuaJIT FFI. This will cause compilation or parse failures in both target languages.

**Fix:** The type mapper should translate `c_char` to `char` for C++ and the full `const c_char*` to `const char*` for both C++ and Lua. Add `c_char` to the type mapping tables in the C++ and Lua code generators.

## Warnings

### WR-01: Duplicate `AbiErrorCode` enum in C# generated output

**File:** `sdks/csharp/abi/Abi.cs:1136-1162` and `sdks/csharp/abi/Abi.cs:1187-1201`

**Issue:** The `AbiErrorCode` enum is emitted twice in the generated C# file. The first occurrence (line 1136) is from the main code generation loop iterating over extracted ABI types. The second occurrence (line 1187) is from `generate_footer()` in the C# generator (`csharp.rs:412-426`), which hardcodes `AbiErrorCode` and `AbiConstants`. This will cause a C# compilation error due to duplicate type definition within the same namespace.

**Fix:** Either remove `AbiErrorCode` from `generate_footer()` in `csharp.rs` (since it is now generated from extracted types), or skip it during the main iteration loop by filtering it out. The footer should only contain types that are NOT already extracted from the ABI definition.

### WR-02: C# `Debug.Assert` emitted outside method context

**File:** `sdks/csharp/abi/Abi.cs:23,50,67,116,200,506,715,825,842,862,887,910,934,951,1008,1051,1067,1082`

**Issue:** The generated C# file emits `Debug.Assert(Marshal.SizeOf<T>() == N)` statements as standalone statements at namespace level (outside any method body). In C#, executable statements must be inside method bodies. These will cause compilation errors. The code generator emits these right after each struct definition as size documentation, but they are not syntactically valid at the namespace level. This is produced by `csharp.rs:324-333` in the `generate_struct` method.

**Fix:** Either wrap these asserts in a static test method (e.g., a `ValidateLayout()` method in a static class), emit them as comments (`// Expected size: N bytes`), or remove them since the layout test file (`LayoutTests.cs`) already validates sizes using xUnit.

### WR-03: Lua generated file uses C-style `//` comments outside `ffi.cdef` context

**File:** `sdks/lua/abi/abi.lua:7-10,20-21,40-51` (and many more)

**Issue:** The generated Lua file uses C-style `//` line comments for doc content. While LuaJIT's `ffi.cdef` does accept C-style comments (since it parses C syntax), the file structure appears to place all content within `ffi.cdef` blocks where `//` is valid. However, if the file structure changes or if any content ends up outside `ffi.cdef`, the `//` comments would be syntax errors in Lua. The generated file is inconsistent: it uses `--` for some structural elements but `//` for all doc content.

**Fix:** The Lua code generator should ensure `//` comments only appear within `ffi.cdef` string blocks and use `--` for any content outside those blocks. This requires the generator to track whether it is inside an `ffi.cdef` context.

### WR-04: `to_upper_snake_case_for_generate` produces incorrect output for consecutive uppercase letters

**File:** `crates/polyplug_abi/build/generate.rs:1383-1396`

**Issue:** The function inserts an underscore before every uppercase character (except at position 0), then uppercases all other characters. For an input like `AbiError`, this produces `A_B_I_E_R_R_O_R` instead of the intended `ABI_ERROR`. The function only looks at individual character casing without considering consecutive uppercase runs. This affects the generated JS layout test file imports, which use constants like `ABI_ERROR_SIZE`.

**Fix:**
```rust
fn to_upper_snake_case_for_generate(s: &str) -> String {
    let mut result = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_uppercase() {
            if prev_lower || (!result.is_empty() && result.ends_with('_')) {
                // Only add underscore at lowercase->uppercase boundary
            } else if !result.is_empty() {
                result.push('_');
            }
            result.push(c);
            prev_lower = false;
        } else {
            result.push(c.to_ascii_uppercase());
            prev_lower = c.is_ascii_lowercase();
        }
    }
    result
}
```
A correct implementation would group consecutive uppercase letters and only insert underscores at boundaries.

### WR-05: Incomplete Lua depth tracking in `count_lua_openers` with dead code

**File:** `crates/polyplug_abi/build/generate.rs:799-843`

**Issue:** The `count_lua_openers` function has a dead loop at lines 799-803 that iterates over keywords (`["if", "for", "while", "function"]`) but does nothing inside the loop body (the comment says "already counted in openers"). This code is unreachable and misleading. Additionally, the function only counts openers at the start of a line (`trimmed.starts_with(...)`) and misses nested constructs where keywords appear mid-line. The `then` branch at lines 840-841 has a comment about `elseif` but no implementation. While the inline helpers in `HELPER_LUA` are simple enough that this works today, any more complex Lua patterns could cause the depth tracker to desync.

**Fix:** Remove the dead loop at lines 799-803. Document the limitation that only line-start keywords are tracked. If more complex Lua extraction is needed in the future, consider a proper parser.

### WR-06: `handle.hpp` equality operator compares only `index`, not `generation`

**File:** `sdks/cpp/host/polyplug/handle.hpp:19-21`

**Issue:** The `operator==` for `GuestContractHandle` compares only `a.index == b.index`. The architecture documentation states the system uses a "generational index pattern for safe handle management" with `{ index: u32, generation: u32 }`. The current generated `GuestContractHandle` in `abi.hpp` only has an `index` field (no `generation`), which means either: (a) the generation field was intentionally removed from the ABI, or (b) there is an extraction gap. If generation is ever re-introduced, this comparison would silently treat stale handles as equal to valid ones.

**Fix:** If generation is intentionally removed, add a comment to `GuestContractHandle` documenting this design decision. If generation should be tracked, update the struct and comparison operators.

## Info

### IN-01: `sg_scan_methods` function is unused

**File:** `crates/polyplug_abi/build/generate.rs:1093-1127`

**Issue:** The `sg_scan_methods` function is defined but never called anywhere in the codebase. It appears to be infrastructure prepared for future ast-grep-based method extraction (D-14), but since helper methods are now inlined as const strings (D-12), this function is dead code.

**Fix:** Either remove the function or add a comment indicating it is reserved for future use.

### IN-02: `is_sg_available` emits cargo warnings on every build

**File:** `crates/polyplug_abi/build/generate.rs:1199-1203`

**Issue:** The build script unconditionally prints `cargo:warning=ast-grep (sg) available...` or `cargo:warning=ast-grep (sg) not found...` on every build. Cargo warnings are visible in IDEs and CI logs. Since ast-grep is not required for the build (helpers are inlined), these messages add noise.

**Fix:** Consider using `println!("cargo:rustc-env=...")` or only emitting the warning when the result changes.

### IN-03: `UNREPRESENTABLE_PATTERNS` check could produce false positives

**File:** `crates/polyplug_abi/build/generate.rs:84`

**Issue:** The pattern `"dyn "` would match types containing "dyn " anywhere in the string, such as hypothetical field names or documentation. Similarly `"impl "` could match inside doc strings. This is unlikely to cause problems in practice for this codebase, but a more precise check would be more robust.

**Fix:** Consider using word-boundary-aware matching or checking specifically at type boundaries.

---

_Reviewed: 2026-04-13T00:00:00Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
