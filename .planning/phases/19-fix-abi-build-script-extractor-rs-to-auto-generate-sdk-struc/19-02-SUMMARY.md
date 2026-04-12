---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
plan: 02
subsystem: codegen
tags: [codegen, fn-pointer, cfunctype, delegate, static-assert, layout-tests, size-assertions]

# Dependency graph
requires:
  - phase: 19-01
    provides: "Build script extractor with auto-discovery, generated SDK files for 5 languages"
provides:
  - "Typed function pointer signatures in all 5 codegen generators (CFUNCTYPE, delegates, typedefs)"
  - "Size assertion output per struct in all 5 generated SDK files"
  - "Layout test source files for all 5 SDK languages"
  - "Known size table (20 structs) with maintenance documentation"
affects: [19-03, 19-04, 19-05, sdk_validator]

# Tech tracking
tech-stack:
  added: []
  patterns: ["depth-matching paren parser for fn ptr parsing", "known-size table for struct size hints", "per-language layout test generation"]

key-files:
  created:
    - "sdks/python/abi/test_layout.py"
    - "sdks/csharp/abi/LayoutTests.cs"
    - "sdks/lua/abi/test_layout.lua"
    - "sdks/js/abi/test_layout.ts"
    - "sdks/cpp/abi/test_layout.cpp"
  modified:
    - "crates/polyplug_codegen/src/languages/python.rs"
    - "crates/polyplug_codegen/src/languages/csharp.rs"
    - "crates/polyplug_codegen/src/languages/lua.rs"
    - "crates/polyplug_codegen/src/languages/js.rs"
    - "crates/polyplug_codegen/src/languages/cpp.rs"
    - "crates/polyplug_abi/build/generate.rs"
    - "crates/polyplug_abi/build/main.rs"

key-decisions:
  - "Used depth-matching paren parser instead of string.find for fn ptr param/end boundaries to handle void-return functions correctly"
  - "Added convert_fn_param helper to Python/C# generators to split name:type pairs before type conversion"
  - "Size hints use a known-size table approach rather than runtime computation (simpler, table verified by layout tests)"

patterns-established:
  - "Fn ptr param parsing: split on ':' to separate param name from type in compact quote!() output"
  - "Size hint propagation: known-size table in generate.rs -> AbiStruct.size_hint -> StructInfo.size_hint -> generator output"
  - "Layout test generation: generate test source files only, scaffolding is manual per D-32"

requirements-completed: [D-09, D-20, D-21, D-22, D-23, D-24, D-25, D-31, D-33, D-35]

# Metrics
duration: 18min
completed: 2026-04-12
---

# Phase 19 Plan 02: Typed Fn Ptr Generation and Size Assertions Summary

**Typed function pointer signatures (CFUNCTYPE/delegates/typedefs) in all 5 codegen generators, struct size assertions, and layout test files for all 5 SDK languages**

## Performance

- **Duration:** 18 min
- **Started:** 2026-04-12T23:10:36Z
- **Completed:** 2026-04-12T23:28:37Z
- **Tasks:** 2
- **Files modified:** 17

## Accomplishments
- Fixed function pointer parameter type conversion in Python and C# generators (was emitting raw Rust syntax like `host:*constHostInterface` instead of ctypes/C# types)
- Fixed void-returning function pointer parsing in Lua and C++ generators (was producing extra `)` characters like `void(*)(const HostInterface*, GuestContractInstance, ))`)
- Added known size table with 20 struct sizes and maintenance documentation
- All 5 generators now emit size assertions/comments per struct with known sizes
- Generated layout test source files for all 5 SDK languages (pytest, xUnit, simple assert, Deno.test, static_assert)

## Task Commits

Each task was committed atomically:

1. **Task 1: Fix typed fn ptr generation in all 5 codegen generators** - `43aa808` (test), `1116bfd` (feat)
2. **Task 2: Add struct size assertions and layout test generation** - `eb73d8a` (feat)

## Files Created/Modified
- `crates/polyplug_codegen/src/languages/python.rs` - Added convert_fn_param helper, size assertions in generate_struct, 5 new tests
- `crates/polyplug_codegen/src/languages/csharp.rs` - Added convert_fn_param helper, StructLayout Size attribute, Debug.Assert, 3 new tests
- `crates/polyplug_codegen/src/languages/lua.rs` - Rewrote convert_function_pointer with depth-matching parser, size comments, 3 new tests
- `crates/polyplug_codegen/src/languages/js.rs` - Use size_hint for SIZE constants, 3 new tests
- `crates/polyplug_codegen/src/languages/cpp.rs` - Rewrote convert_function_pointer with depth-matching parser, static_assert, 3 new tests
- `crates/polyplug_abi/build/generate.rs` - Added KNOWN_SIZES table, populate_size_hints, generate_layout_tests, per-language layout test generators
- `crates/polyplug_abi/build/main.rs` - Changed abi_types to &mut for populate_size_hints
- `sdks/python/abi/test_layout.py` - Generated pytest layout test file (19 struct size tests)
- `sdks/csharp/abi/LayoutTests.cs` - Generated xUnit layout test file (19 struct size tests)
- `sdks/lua/abi/test_layout.lua` - Generated Lua assert layout test file
- `sdks/js/abi/test_layout.ts` - Generated Deno.test layout test file
- `sdks/cpp/abi/test_layout.cpp` - Generated static_assert layout test file
- `sdks/*/abi/abi.*` - Regenerated with typed fn ptr signatures and size assertions

## Decisions Made
- **Depth-matching paren parser** for fn ptr parsing: The previous `find(")->")` approach failed for void-returning functions (no `->` separator). The new approach counts paren depth to find the matching closing paren for the parameter list, then extracts the return type from whatever follows.
- **Known-size table approach** for struct sizes: Rather than computing sizes from field layouts (complex, error-prone for nested types), a simple lookup table with 20 known sizes is used. The table is verified by generated layout tests, so stale entries cause test failures rather than silent corruption.
- **Test source files only** per D-32: Layout test generation writes only the test source code. Project files (pytest.ini, .csproj, etc.) are created manually.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Python/C# fn ptr params emitted as raw Rust syntax**
- **Found during:** Task 1 (RED test phase)
- **Issue:** `parse_function_pointer` in Python and C# generators passed compact `name:type` strings like `host:*constHostInterface` directly to `rust_type_to_python`/`rust_type_to_csharp` without splitting on `:`. The result was CFUNCTYPE params like `host:*constHostInterface` instead of `ctypes.c_void_p`.
- **Fix:** Added `convert_fn_param()` helper to both generators that splits on `:` and only converts the type part.
- **Files modified:** python.rs, csharp.rs
- **Verification:** cargo test -p polyplug_codegen: 82 passed
- **Committed in:** 1116bfd

**2. [Rule 1 - Bug] Lua/C++ void-returning fn ptrs produced extra `)` characters**
- **Found during:** Task 1 (analysis of generated output)
- **Issue:** `convert_function_pointer` used `find(")->")` to locate the end of the parameter list. For functions with no explicit return type (Rust `fn()` without `->`), this pattern was not found, causing the closing `)` of the parameter list to be included as a parameter, producing output like `void(*)(const HostInterface*, GuestContractInstance, ))`.
- **Fix:** Replaced `find(")->")` with a depth-matching paren parser that correctly identifies the matching closing paren for the `fn(` parameter list.
- **Files modified:** lua.rs, cpp.rs
- **Verification:** Generated output no longer contains double-parens; cargo test passes
- **Committed in:** 1116bfd

---

**Total deviations:** 2 auto-fixed (2 bugs)
**Impact on plan:** Both auto-fixes correct pre-existing bugs in the generator code that produced invalid output. No scope creep.

## Issues Encountered
- Build script `#[cfg(test)]` tests are compiled but not discovered by `cargo test`. The tests in `generate.rs` serve as documentation and compile-time verification rather than runtime tests. The actual test coverage comes from the `polyplug_codegen` crate tests.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All 5 generators produce correct typed fn ptr signatures and size assertions
- Layout test files exist for all 5 languages
- Ready for Plan 19-03 (ast-grep integration and method body preservation)

---
*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Completed: 2026-04-12*

## Self-Check: PASSED

- All 17 created/modified files verified present
- Commit 43aa808 (test) verified in git log
- Commit 1116bfd (feat) verified in git log
- Commit eb73d8a (feat) verified in git log
- cargo test -p polyplug_codegen: 82 passed
- cargo test -p polyplug_abi: 58 passed
- cargo build -p polyplug_abi: success
- grep CFUNCTYPE python: 33 occurrences
- grep delegate csharp: 34 occurrences
- grep static_assert cpp: 19 occurrences
- All 5 layout test files exist (test_layout.py, LayoutTests.cs, test_layout.lua, test_layout.ts, test_layout.cpp)
