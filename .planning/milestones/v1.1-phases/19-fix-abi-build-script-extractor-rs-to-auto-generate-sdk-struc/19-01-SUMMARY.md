---
phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
plan: 01
subsystem: build-script, codegen
tags: [syn, quote, build-rs, codegen, repr-c, auto-discovery, module-tree]

# Dependency graph
requires:
  - phase: prior-phases
    provides: "modular ABI source tree with 25+ #[repr(C)] types across 7 sub-modules"
provides:
  - "Recursive module tree walker (walk_module_tree) in extractor.rs"
  - "Auto-discovery of all #[repr(C)] types without whitelists"
  - "cargo:rerun-if-changed tracking for all 42 source files"
  - "Loader config struct scanning from 5 sibling crates"
  - "Build-fail validation for unrepresentable types (D-09)"
  - "Generated SDK files for all 5 languages with 28 discovered types"
affects: [19-02, 19-03, 19-04, 19-05, sdk_validator]

# Tech tracking
tech-stack:
  added: []
  patterns: ["recursive module tree walking via syn", "attribute-based auto-discovery (no whitelists)", "loader crate scanning via workspace_root resolution"]

key-files:
  created: []
  modified:
    - "crates/polyplug_abi/build/extractor.rs"
    - "crates/polyplug_abi/build/types.rs"
    - "crates/polyplug_abi/build/mapper.rs"
    - "crates/polyplug_abi/build/generate.rs"
    - "crates/polyplug_abi/build/main.rs"
    - "sdks/python/abi/abi.py"
    - "sdks/cpp/abi/polyplug/abi.hpp"
    - "sdks/csharp/abi/Abi.cs"
    - "sdks/lua/abi/abi.lua"
    - "sdks/js/abi/abi.ts"

key-decisions:
  - "Descend into all mod declarations (pub and private) because inner sub-modules use private mod but contain pub #[repr(C)] types re-exported by parent"
  - "Handle Item::Function variant in generate.rs with empty output since polyplug_codegen::data::Item still has it for polyplugc CLI use"
  - "Loader config structs extracted inline in main.rs rather than reusing extractor module to avoid complex refactoring"

patterns-established:
  - "Attribute-based type discovery: extract any pub struct/union with #[repr(C)] and any pub enum with #[repr(u*)]"
  - "Convention-based constant discovery: extract any pub const with name starting with POLYPLUG_"
  - "Module tree walking: resolve mod X to X.rs or X/mod.rs, recurse into both pub and private mods"

requirements-completed: [D-01, D-02, D-03, D-04, D-05, D-06, D-07, D-08, D-09, D-17]

# Metrics
duration: 14min
completed: 2026-04-12
---

# Phase 19 Plan 01: Rewrite Build Script Extractor Summary

**Recursive module tree walker with attribute-based auto-discovery discovers 28 ABI types across 42 source files, zero whitelists, generates all 5 SDK language files**

## Performance

- **Duration:** 14 min
- **Started:** 2026-04-12T01:54:30Z
- **Completed:** 2026-04-12T02:08:58Z
- **Tasks:** 2
- **Files modified:** 10

## Accomplishments
- Replaced hardcoded ABI_TYPES/ABI_CONSTANTS/ABI_FUNCTIONS whitelists with attribute-based auto-discovery
- Built recursive module tree walker that descends into all mod declarations (37 source files in polyplug_abi)
- Scans 5 loader crates (polyplug_native, polyplug_python, polyplug_lua, polyplug_js, polyplug_dotnet) for config structs
- Emits cargo:rerun-if-changed for all 42 tracked source files
- Validates that all field types are representable in target languages (fails build with clear error for dyn/impl/where types)
- Generated SDK files contain 21 structs, 7 enums, 1 union, and 1 constant across all 5 languages

## Task Commits

Each task was committed atomically:

1. **Task 1: Rewrite extractor.rs with recursive module tree walking and auto-discovery** - `21182fd` (refactor)
2. **Task 2: Wire build script entry point with rerun-if-changed and loader crate scanning** - `3d6d298` (feat)

## Files Created/Modified
- `crates/polyplug_abi/build/extractor.rs` - Rewrote with walk_module_tree, removed all whitelists, auto-discovery by attribute
- `crates/polyplug_abi/build/types.rs` - Removed AbiFunction variant, AbiFunction struct, add_function(), functions field; added merge()
- `crates/polyplug_abi/build/mapper.rs` - Removed create_hash_functions(), map_function(), AbiFunction imports, all function-related tests
- `crates/polyplug_abi/build/generate.rs` - Removed hash function generation, added validate_representable_types(), accepts tracked_files for rerun-if-changed
- `crates/polyplug_abi/build/main.rs` - Replaced extract_types() with extract_from_dir(), added loader crate scanning, removed single-file rerun-if-changed
- `sdks/python/abi/abi.py` - Auto-generated with 28 discovered types (was broken placeholder)
- `sdks/cpp/abi/polyplug/abi.hpp` - Auto-generated with 28 discovered types
- `sdks/csharp/abi/Abi.cs` - Auto-generated with 28 discovered types
- `sdks/lua/abi/abi.lua` - Auto-generated with 28 discovered types
- `sdks/js/abi/abi.ts` - Auto-generated with 28 discovered types

## Decisions Made
- **Descend into all mod declarations** (not just pub mod): The inner module tree uses `mod X;` (private) for sub-files like `types/string_view.rs`, but the structs inside are `pub #[repr(C)]` and re-exported by parent modules. Only walking pub mod would miss all leaf types.
- **Handle Item::Function gracefully**: The `polyplug_codegen::data::Item` enum still has a `Function` variant used by the polyplugc CLI. The build script match handles it with an empty string output rather than removing it from the codegen crate.
- **Inline loader config extraction**: Extracting loader config structs is done directly in main.rs rather than reusing the extractor module functions, keeping the extractor focused on module-tree walking.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Module tree walker must descend into private mod declarations**
- **Found during:** Task 1 (extractor rewrite)
- **Issue:** Plan specified "iterate items looking for Item::Mod with pub visibility" but inner modules (types/string_view.rs, plugin/guest_contract_handle.rs, etc.) are declared with `mod` not `pub mod` in their parent mod.rs files. Walking only pub mod would discover 0 leaf types.
- **Fix:** Changed walk_module_tree to descend into ALL file-based mod declarations (both pub and private). Type-level visibility filtering (pub structs only) still ensures only public ABI types are extracted.
- **Files modified:** crates/polyplug_abi/build/extractor.rs
- **Verification:** cargo build -p polyplug_abi generates all 28 types (21 structs, 7 enums, 1 union) vs. 0 structs with pub-only walking
- **Committed in:** 21182fd (Task 1 commit)

**2. [Rule 1 - Bug] Unused Path type in main.rs loader scanning**
- **Found during:** Task 2 (build compilation)
- **Issue:** Variable `loader_dir: &Path` was declared but never used, and `Path` type wasn't imported.
- **Fix:** Removed the unused variable.
- **Files modified:** crates/polyplug_abi/build/main.rs
- **Verification:** cargo build -p polyplug_abi compiles cleanly
- **Committed in:** 3d6d298 (Task 2 commit)

**3. [Rule 1 - Bug] Non-exhaustive match on Item::Function**
- **Found during:** Task 2 (build compilation)
- **Issue:** generate_language_sdk() match on Item did not handle the Function variant still present in polyplug_codegen::data::Item.
- **Fix:** Added `Item::Function(_) => String::new()` arm with comment explaining the variant is kept for polyplugc CLI use.
- **Files modified:** crates/polyplug_abi/build/generate.rs
- **Verification:** cargo build -p polyplug_abi compiles cleanly
- **Committed in:** 3d6d298 (Task 2 commit)

---

**Total deviations:** 3 auto-fixed (1 blocking, 2 bugs)
**Impact on plan:** All auto-fixes necessary for correctness. The private-mod-descending deviation is the most significant -- it differs from the plan's literal "pub mod only" wording but matches the plan's intent of discovering all types.

## Issues Encountered
- Generated SDK files contain Rust-style type syntax for complex types (e.g., `Option<unsafeextern"C"fn(ReloadPhase)>` in Python). This is expected "placeholder-quality" output as noted in the plan objective. Future plans (19-02+) will enhance the codegen generators to produce idiomatic per-language output.

## Known Stubs

| File | Stub | Reason |
|------|------|--------|
| sdks/*/abi/abi.* | Function pointer fields rendered as compact Rust syntax | Codegen generators need enhancement for typed fn ptr per-language output (Plan 19-02+) |
| sdks/*/abi/abi.* | Loader config structs (NativeConfig etc.) not included | Loader configs don't have #[repr(C)], not extracted |

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Build script correctly discovers all ABI types -- ready for Plan 19-02 (ast-grep integration and codegen enhancement)
- All 5 SDK abi.* files are generated with struct/enum/union/const definitions
- The generated files are placeholder-quality -- typed function pointer generation and layout assertions come in later plans

---
*Phase: 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc*
*Completed: 2026-04-12*

## Self-Check: PASSED

- All 10 files verified present (extractor.rs, types.rs, mapper.rs, generate.rs, main.rs, 5 SDK files)
- Commit 21182fd (refactor) verified in git log
- Commit 3d6d298 (feat) verified in git log
- 0 whitelist references (ABI_TYPES, ABI_CONSTANTS, ABI_FUNCTIONS) in extractor.rs
- 0 create_hash_functions references in mapper.rs
- 0 AbiFunction references in types.rs
- walk_module_tree found 3 times in extractor.rs
- cargo test -p polyplug_abi --lib: 58 passed
- cargo build -p polyplug_abi: success
