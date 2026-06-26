# Contributing to polyplug

How to build, run the gate, and what to read before you start.

## Coding rules

Read [`CLAUDE.md`](CLAUDE.md) first. It defines the non-negotiable rules for this
codebase — module structure, error handling, the ABI contract, no type aliases,
no cross-crate re-exports, and more. A reviewer will reject a PR for any
violation, so check your change against it before opening one.

### Import hygiene (no inline fully-qualified paths)

**Rule:** `use` statements belong at the top of every file. Inline fully-qualified
paths at use-sites — `std::collections::HashMap::new()`, `crate::error::MyError`,
`polyplug_abi::AbiError` written inside a function body or expression — are
forbidden (CLAUDE.md, rule 20 — the use-site companion to rule 2's "`use` at file top").

**Exceptions** (the rule does not flag these):

- `core::str::*` / `std::str::*` — primitive-method collision; short names conflict
  with built-ins.
- Module-qualified forms whose leading segment is an *imported module*, not a crate
  root — e.g. `ptr::write`, `mem::size_of`, `fs::read` — do not cross a crate
  boundary and are idiomatic Rust.
- `crate::` paths inside `macro_rules!` bodies — macros may need to name their home
  crate explicitly for hygienic expansion.
- FFI function-pointer `type` aliases (the Rule 16 exception) — a `type` alias
  whose right-hand side is `unsafe extern "C"` / `extern "system"` is a single
  definition point; it lives in a `use`-topped file and is imported by callers, so
  it never creates inline FQ drift.

**Guard:** `just verify-no-fq-paths` (requires `ast-grep` ≥ 0.40)

```
ast-grep scan --rule checks/no_inline_fq_paths.yaml crates sdks
```

The rule file lives at `checks/no_inline_fq_paths.yaml`. It is enforced in CI
inside the **SDK Consistency** job (`.github/workflows/ci.yml`), which already
installs `ast-grep`, immediately after the `sdk-validator` step.

## Build

```sh
cargo build --workspace
```

## The gate

`just gate` is the single source of truth for "everything passes": format,
import hygiene, the SDK helper validator, ABI-mirror drift, clippy, a build of
**every** SDK (host and guest, all six languages — the build exits non-zero if
any one fails), a strict mdbook docs build, and the full test matrix (Rust
workspace, every language's host-runtime tests, the C++/C# SDK suites, and
integration tests).

```sh
just gate
```

### Enforced by lefthook

This repo uses [lefthook](https://github.com/evilmartians/lefthook) so a red
build can never be pushed. Install the hooks once per clone:

```sh
lefthook install
```

Each check is its own named command, split by cost:

- **pre-commit** (fast): `fmt`, `import-hygiene`, `sdk-validator` (ABI-mirror
  drift + helper parity), `lint`, `js-typecheck`, `docs` — each globbed to the
  files it checks.
- **pre-push** (heavy): `tests` — `just test-all` (full matrix + cross-language
  conformance). clippy already compiled at commit time, so there is no build.
- **commit-msg**: Conventional Commits format.

Bypass only in a genuine emergency: `LEFTHOOK=0 git push`.

Zero warnings are tolerated. Test failures must be fixed, never skipped or
`#[ignore]`d (see CLAUDE.md §18).

## Running the examples

The example hosts load each language's reference plugins and assert byte-identical
output across all six languages:

```sh
just verify-examples
# or directly:
examples/verify_hosts.sh
```

## License and contributions

polyplug is licensed under the **MIT License**, with Islam Nofl as the sole
copyright holder.

By submitting a contribution (a pull request, patch, or any code), you agree that
your contribution is provided under the MIT License and that you have the right to
submit it. No separate CLA is required — your submission is your agreement.
