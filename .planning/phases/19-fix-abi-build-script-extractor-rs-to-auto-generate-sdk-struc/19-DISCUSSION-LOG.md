# Phase 19: Fix ABI Build Script + Auto-Generate SDK Structs - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-04-12
**Phase:** 19-fix-abi-build-script-extractor-rs-to-auto-generate-sdk-struc
**Areas discussed:** Build script scope, generated vs hand-written boundary, ast-grep approach, ordering, SDK validation, type representation, layout tests, PluginRegistrar removal, loader configs

---

## Build Script Extractor Fix

| Option | Description | Selected |
|--------|-------------|----------|
| Walk module tree recursively | Parse `pub mod X;` from lib.rs, read sub-files recursively | ✓ |
| Rustdoc JSON output | Use `cargo doc --output-format json` for structured type info | |
| Macro expansion | Use `cargo expand` to inline everything | |
| Explicit file list | Manually list source file paths | |

**User's choice:** Walk module tree recursively
**Notes:** Most straightforward, keeps syn-based extraction, no new dependencies

---

## Type Discovery Strategy

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-discover all #[repr(C)] | No whitelist, extract by convention | ✓ |
| Whitelist with opt-in marker | e.g. `/// ABI_EXPORT` doc comment | |
| Manual whitelist (current) | Keep ABI_TYPES list, update it | |

**User's choice:** Auto-discover all #[repr(C)] public types

---

## Functions Extraction

| Option | Description | Selected |
|--------|-------------|----------|
| Extract Rust functions | Generate fnv1a_64, contract_id, etc. into SDKs | |
| Don't extract functions | SDKs don't need hash functions — runtime handles IDs, codegen pre-computes | ✓ |

**User's choice:** Don't extract functions at all. Runtime does the computation, codegen embeds pre-computed IDs as constants.

---

## Constants Extraction

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-discover POLYPLUG_* | Extract any pub const starting with POLYPLUG_ | ✓ |
| Manual whitelist | Keep ABI_CONSTANTS list | |

**User's choice:** Auto-discover POLYPLUG_* constants

---

## Generated vs Hand-Written Boundary

| Option | Description | Selected |
|--------|-------------|----------|
| All #[repr(C)] in separate abi.* files | Struct definitions auto-generated, host files import | ✓ |
| Core types generated, complex hand-written | HostInterface stays hand-written | |
| Generate structs, keep helper files separate | string_view_helper.lua stays as separate file | |

**User's choice:** All structs auto-generated in separate abi.* files. Helper files merged into abi.*.

---

## ast-grep Role

| Option | Description | Selected |
|--------|-------------|----------|
| Full ast-grep: preserve method bodies | Surgical signature updates, DELETED_ prefix for removed methods | ✓ |
| Simple: generate structs only | No method preservation, separate helper files | |
| Marker comments: section-based | AUTO-GENERATED ABOVE / HAND-WRITTEN BELOW markers | |

**User's choice:** Full ast-grep approach for preserving hand-written helper methods
**Notes:** ast-grep as CLI tool (not Rust lib). Already used in sdk_validator crate.

---

## Ordering & Migration

| Option | Description | Selected |
|--------|-------------|----------|
| Build script → ast-grep → delete hand-written | Safe, incremental, each step testable | ✓ |
| ast-grep → build script → run migration | Safety net first | |
| Together → single migration pass | Faster but harder to debug | |

**User's choice:** Build script first, then ast-grep, then delete hand-written

---

## SDK Validation Method

| Option | Description | Selected |
|--------|-------------|----------|
| Rust-driven with convention-based naming | Extract method names from Rust impl blocks, auto-derive language names | ✓ |
| Auto-generated yaml from Rust | Generate yaml manifest from Rust source | |
| No method validation | Validate struct fields only | |

**User's choice:** Rust-driven, convention-based naming (no config file)

---

## HostInterface Function Pointers

| Option | Description | Selected |
|--------|-------------|----------|
| Typed fn pointers (type-safe) | Each SDK gets typed fn ptr signatures per language | ✓ |
| Opaque void* (current pattern) | Store as void*, cast manually at call sites | |
| Both: opaque + typed aliases | Generate both, SDKs choose during migration | |

**User's choice:** Typed function pointers for type safety

---

## RuntimeConfig Layout

| Option | Description | Selected |
|--------|-------------|----------|
| Match Rust: 16-byte struct | compatibility, hot_reload_enabled, on_reload | ✓ |
| Keep 24-byte SDK version | With retry fields (dangerous, doesn't match ABI) | |

**User's choice:** Always match Rust exactly. 16-byte struct.

---

## Array<T> Handling

| Option | Description | Selected |
|--------|-------------|----------|
| Monomorphized concrete types | ArrayGuestContractHandle, ArrayBundleId, etc. | |
| Single void* generic Array | One definition with void* items + len + align | ✓ |
| Don't generate | Keep anonymous inline structs at call sites | |

**User's choice:** Single generic Array with void*

---

## Layout Verification

| Option | Description | Selected |
|--------|-------------|----------|
| Auto-generated per-SDK layout tests | Test files generated alongside abi.* | ✓ |
| Trust codegen + Rust offset tests | No separate SDK tests | |
| JSON manifest + Rust-side verification | Single Rust test reads all SDK files | |

**User's choice:** Auto-generated layout test files per SDK. Scaffolding is manual.

---

## Loader Config Structs

| Option | Description | Selected |
|--------|-------------|----------|
| Keep hand-written | Not in polyplug_abi, leave alone | |
| Build script scans multiple crates | Scan polyplug_abi + loader crates for #[repr(C)] | ✓ |
| Per-crate ABI manifest | TOML or similar per loader crate | |

**User's choice:** Build script scans multiple crates (polyplug_abi + loader crates). No manifest files.
**Notes:** User explicitly rejected manifest/TOML approach.

---

## Build Script Location

| Option | Description | Selected |
|--------|-------------|----------|
| Stay as build.rs | Runs automatically during cargo build | ✓ |
| Move to polyplugc CLI | Explicit command | |
| Build.rs + justfile task | Auto + manual | |

**User's choice:** Stay as build.rs

---

## PluginRegistrar

| Option | Description | Selected |
|--------|-------------|----------|
| Remove entirely, use HostInterface directly | It's just an alias for HostInterface | ✓ |
| Move to polyplug_abi | Make it official ABI type | |
| Keep in guest SDK, scan it | Auto-generate from guest SDK source | |

**User's choice:** Remove PluginRegistrar everywhere. Use HostInterface* directly. Verification required that no references remain.

---

## Deleted Symbols (FFI Functions)

| Option | Description | Selected |
|--------|-------------|----------|
| ast-grep handles FFI calls too | Mark deleted calls with DELETED_ prefix | |
| Out of scope, manual fix | Only 2 FFI functions exist, manual one-time fix | ✓ |

**User's choice:** Manual fix. ast-grep only handles struct definitions and methods.

---

## Claude's Discretion

- Exact ast-grep rule patterns and integration points
- Loader crate source file discovery implementation details
- Test file content generation specifics
- Error message wording for build failures
- Namespace conventions per SDK (follow existing idiomatic patterns)

## Deferred Ideas

- QuickJS split-pointer representation for guest SDK
- Full sdk_validator rewrite to be fully Rust-driven
- Additional helper methods beyond current set
- ResolveHandle forward declaration cleanup in C++

---
