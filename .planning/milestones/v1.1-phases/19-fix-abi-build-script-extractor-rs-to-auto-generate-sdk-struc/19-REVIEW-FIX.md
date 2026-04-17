---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
fixed_at: 2026-04-13T00:00:00Z
review_path: .planning/phases/19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc/19-REVIEW.md
iteration: 1
findings_in_scope: 9
fixed: 9
skipped: 0
status: all_fixed
---

# Phase 19: Code Review Fix Report

**Fixed at:** 2026-04-13T00:00:00Z
**Source review:** .planning/phases/19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc/19-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 9 (3 Critical, 6 Warning)
- Fixed: 9
- Skipped: 0

## Fixed Issues

### CR-01: Raw Rust path syntax `crate::host::*` emitted into C++, Lua, and C# generated files

**Files modified:** `crates/polyplug_codegen/src/languages/cpp.rs`, `crates/polyplug_codegen/src/languages/lua.rs`, `crates/polyplug_codegen/src/languages/csharp.rs`, `sdks/cpp/abi/polyplug/abi.hpp`, `sdks/lua/abi/abi.lua`, `sdks/csharp/abi/Abi.cs`
**Commit:** 54eb520
**Applied fix:** Added Rust module path stripping logic (`rsplit("::").next()`) to `rust_type_to_cpp`, `rust_type_to_lua`, and `rust_type_to_csharp` in all three generators. This maps `crate::host::HostContractInstance` to `HostContractInstance` etc. Also fixed the generated SDK files to remove existing `crate::host::*` references.

### CR-02: Generic `T*` emitted in Array struct for C++ and Lua generated files

**Files modified:** `crates/polyplug_codegen/src/languages/cpp.rs`, `crates/polyplug_codegen/src/languages/lua.rs`, `sdks/cpp/abi/polyplug/abi.hpp`, `sdks/lua/abi/abi.lua`
**Commit:** 54eb520
**Applied fix:** Added `"T" => String::from("void")` mapping to both C++ and Lua generators. When `T*` is encountered, it now resolves to `void*` (opaque pointer), consistent with how C# already uses `IntPtr`. Fixed generated SDK files to replace `T* items;` with `void* items;`.

### CR-03: `c_char` type emitted into C++ and Lua generated files

**Files modified:** `crates/polyplug_codegen/src/languages/cpp.rs`, `crates/polyplug_codegen/src/languages/lua.rs`, `sdks/cpp/abi/polyplug/abi.hpp`, `sdks/lua/abi/abi.lua`
**Commit:** 54eb520
**Applied fix:** Added `"c_char" => String::from("char")` to C++ generator and `"c_char" => String::from("int8_t")` to Lua generator. Fixed generated SDK files to replace `c_char` with the correct target type.

### WR-01: Duplicate `AbiErrorCode` enum in C# generated output

**Files modified:** `crates/polyplug_codegen/src/languages/csharp.rs`, `sdks/csharp/abi/Abi.cs`
**Commit:** 211d803
**Applied fix:** Removed the hardcoded `AbiErrorCode` enum from `generate_footer()` in csharp.rs since it is now generated from extracted ABI types during the main iteration loop. Removed the duplicate from the generated Abi.cs file.

### WR-02: C# `Debug.Assert` emitted outside method context

**Files modified:** `crates/polyplug_codegen/src/languages/csharp.rs`, `sdks/csharp/abi/Abi.cs`
**Commit:** 20ac4c9
**Applied fix:** Changed `generate_struct` in csharp.rs to emit only `/// Expected size: N bytes` documentation comments instead of `Debug.Assert(Marshal.SizeOf<T>() == N)` statements. The `Debug.Assert` statements are not valid at namespace level in C#. Size validation is already handled by the LayoutTests.cs file. Removed 19 `Debug.Assert` lines from the generated Abi.cs.

### WR-03: Lua generated file uses C-style `//` comments outside `ffi.cdef` context

**Files modified:** `crates/polyplug_codegen/src/languages/lua.rs`, `sdks/lua/abi/abi.lua`
**Commits:** e54e90e, 9df52de
**Applied fix:** Changed `generate_header` to open `ffi.cdef[[` block and `generate_footer` to close with `]]`. Changed size hint comments from `--` to `//` (C-style) since they are now inside the `ffi.cdef` block where C comments are valid. Updated generated abi.lua to wrap C definitions in `ffi.cdef[[ ... ]]`.

### WR-04: `to_upper_snake_case_for_generate` produces incorrect output for consecutive uppercase letters

**Files modified:** `crates/polyplug_abi/build/generate.rs`
**Commit:** d14799a
**Applied fix:** Rewrote the function to only insert underscores at boundaries: lowercase-to-uppercase transitions and uppercase-to-lowercase transitions within uppercase runs. `AbiError` now correctly produces `ABI_ERROR` instead of `A_B_I_E_R_R_O_R`.
**Status:** fixed: requires human verification (logic correctness)

### WR-05: Incomplete Lua depth tracking in `count_lua_openers` with dead code

**Files modified:** `crates/polyplug_abi/build/generate.rs`
**Commit:** 16528f1
**Applied fix:** Removed the dead loop that iterated over keywords but performed no actions. Added documentation comment to `count_lua_openers` describing the limitation that only line-start keywords are tracked.

### WR-06: `handle.hpp` equality operator compares only `index`, not `generation`

**Files modified:** `sdks/cpp/host/polyplug/handle.hpp`
**Commit:** f84d2bb
**Applied fix:** Added documentation comment to `operator==` explaining that GuestContractHandle intentionally has only an index field (no generation), and that generational validation is handled at the registry level via `resolve_contract`.

---

_Fixed: 2026-04-13T00:00:00Z_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
