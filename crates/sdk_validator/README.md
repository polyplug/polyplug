# SDK Validator

Cross-language SDK consistency validator for polyplug. Ensures every language SDK implements the golden helper method set defined in `checks/sdk_validator.yaml`, and that every hand-written or generated enum mirror matches the golden enum set exactly.

## Overview

The validator checks that the golden method set (e.g. the `StringView` helpers `to_str`, `starts_with`, `ends_with`, `strip_prefix`, `split`) has a **real definition** in every target SDK file for every language (Rust, Python, C#, C++, JavaScript/TypeScript, Lua).

Detection is AST-based — call sites, comments, and renamed lookalikes (`to_str2`) never count:

- Rust, Python, C#, C++, JS/TS: [ast-grep](https://ast-grep.github.io/) inline rules matching full definition shapes (including `pub const`/`unsafe`/lifetime-generic Rust fns, `inline`/`noexcept`/templated-return C++ functions, expression-bodied and extension C# methods, annotated and un-annotated JS functions plus arrow functions, annotated and un-annotated Python defs).
- Lua: tree-sitter walk detecting `function name()`, `function M.name()`, `function M:name()`, `local function name()`, and assignment forms (`M.name = function() end`).

**Per-file semantics:** every file listed for a language must independently implement every golden method. A method counts as ✓ for a language only if it is present in **all** of that language's target files; the report names the file(s) a missing method is absent from.

## Enum-Mirror Drift Validation

The `enums:` section defines golden enums (`AbiErrorCode`, `LogLevel`, `DispatchType`, `ReloadPhaseType`) with exact variant values; `enum_targets:` lists the per-language mirror files. The Rust ABI sources are themselves listed as targets, so yaml-vs-code disagreement fails validation and keeps the golden set honest.

Per (language, enum, file): every golden variant must be present with **exactly** the golden value, and any extra variant inside the same enum construct is drift (a stale variant). Variant names are PascalCase in every language — no naming transform — except the generated Lua cdef form, which uses the C `EnumName_Variant` prefix.

Detection per shape:

- Rust `enum_variant` under the named `enum_item`; C# `enum_member_declaration`; C++ `enumerator` (covers `enum class`); TS `export const enum` members (valueless members are still caught); JS object-literal pairs (`export const AbiErrorCode = { Ok: 0, ... }`); Lua tables (`M.AbiErrorCode = { Ok = 0, ... }`) via tree-sitter.
- Lua `ffi.cdef[[...]]` C enum text: tree-sitter sees one string literal, so this single detector is a careful text-level line parse of `EnumName_Variant = value,` (acceptable: the cdef file is generated; this target is defense-in-depth).

## Running

From the repo root:

```bash
cargo run -p sdk-validator -- --config checks/sdk_validator.yaml --fail-on-missing
# or
just validate-sdk
```

## CLI

```
Options:
  -c, --config <FILE>     Path to YAML configuration file [required]
  -j, --json              Output as JSON instead of human-readable table
  -s, --struct <NAME>     Filter validation to a specific struct
  -f, --fail-on-missing   Exit with code 1 if any methods are missing
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Validation ran; nothing missing, no enum drift (or no `--fail-on-missing`) |
| 1 | Validation ran; methods missing or enum drift (only with `--fail-on-missing`) |
| 2 | Configuration or tool error (see below) |

Exit code 2 (fatal, never silently reported as "missing"):

- ast-grep CLI not installed (or the `sg` on PATH is shadow-utils' unrelated `sg(1)`)
- a configured target file does not exist (names the language and path)
- an unknown language key in `targets:`, `naming:`, or `enum_targets:` (known: rust, python, csharp, cpp, js, lua)
- a language in `targets:` with no (or an invalid) `naming:` entry
- ast-grep execution/parse failures, Lua parser init failure
- duplicate or non-identifier method names in `methods:`
- an `enum_targets:` entry referencing an enum absent from `enums:`
- duplicate or non-identifier enum/variant names in `enums:`

## Configuration

`checks/sdk_validator.yaml` is the single source of truth. Target paths resolve **relative to the config base directory** — the config file's parent directory, or the optional `root:` field joined onto it (the config sets `root: ..`, so paths resolve from the repo root) — not the process CWD.

```yaml
version: 1

methods:            # golden method set, canonical snake_case
  StringView:
    - to_str
    - starts_with
    - ends_with
    - strip_prefix
    - split

naming:             # consumed: transforms canonical names per language
  rust: snake_case
  python: snake_case
  csharp: PascalCase   # to_str -> ToStr
  js: camelCase        # to_str -> toStr
  cpp: snake_case
  lua: snake_case

targets:            # every listed file must implement ALL methods
  rust:
    - sdks/rust/guest/src/lib.rs
  python:
    - sdks/python/polyplug_abi/polyplug_abi/string_view_helper.py
  csharp:
    - sdks/csharp/abi/Abi.cs
  js:
    - sdks/js/abi/abi.ts
  cpp:
    - sdks/cpp/abi/polyplug/abi.hpp
  lua:
    - sdks/lua/abi/abi.lua

enums:              # golden enums, exact variant values (PascalCase names)
  DispatchType:
    Native: 0
    VirtualMachine: 1
  # AbiErrorCode, LogLevel, ReloadPhaseType ...

enum_targets:       # per-language mirror files; each must match exactly
  rust:
    DispatchType:
      - crates/polyplug_abi/src/dispatch/dispatch_type.rs
  lua:
    DispatchType:
      - sdks/lua/abi/abi.lua                 # generated cdef mirror
      - sdks/lua/guest/polyplug_guest.lua    # hand-written table mirror
```

## Example Output

```
StringView Methods:
  Method       | rust | python | csharp | cpp | js | lua |
  -------------|------|--------|--------|-----|----|-----|
  ends_with    |  ✓   |   ✓    |   ✓    |  ✓  | ✗  |  ✓  |
  ...
  Missing from:
    ends_with [js]: sdks/js/abi/abi.ts

Summary: 29/30 method implementations found (96.7%)

AbiErrorCode Variants:
  Variant       | Value | rust | python | csharp | cpp | js | lua |
  --------------|-------|------|--------|--------|-----|----|-----|
  ReentrantCall | 9     |  ✓   |   ✓    |   ✓    |  ✓  | ✗  |  ✗  |
  ...
  Drift:
    ReentrantCall [lua] sdks/lua/guest/polyplug_guest.lua: expected 9, missing

Enums: 188/190 variant checks passed
```

JSON output (`--json`) mirrors this: each method's `missing_in` is an array of `{ "language": ..., "files": [...] }` objects naming the file(s) the method is missing from, and `per_enum` carries per-variant `found_in`/`mismatches` plus `extra_variants`, alongside `enum_checks_total` / `enum_checks_passed` / `enums_complete`.

## Prerequisites

ast-grep must be installed (`ast-grep` preferred; `sg` accepted if it really is ast-grep):

```bash
cargo install ast-grep --locked
ast-grep --version
```

The Lua validator uses tree-sitter; its dependencies are built into the crate.

## Adding a New Method

1. Add it to `methods:` in `checks/sdk_validator.yaml`.
2. Implement it in **all** validated target files (all 6 languages) in the same change.
3. Run `just validate-sdk` — it must stay green.

## Adding or Changing an Enum Variant

1. Change the Rust ABI source (owner approval required pre-1.0; frozen at 1.0).
2. Update `enums:` in `checks/sdk_validator.yaml` to match.
3. Update every hand-written mirror listed under `enum_targets:` in the same change (generated mirrors regenerate from the ABI build script).
4. Run `just validate-sdk` — it must stay green.

See rule 19 in the repo `CLAUDE.md`: `checks/sdk_validator.yaml` is the single source of truth for built-in-type helper methods.

## License

MIT License — see the main [polyplug LICENSE](../../LICENSE) for details.
