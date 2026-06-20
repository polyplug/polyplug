# Contributing to polyplug

Thanks for your interest in polyplug. This guide covers the essentials: how to
build, how to run the gate, and what to read before you start.

## Coding rules

Read [`CLAUDE.md`](CLAUDE.md) first. It defines the non-negotiable rules for this
codebase — module structure, error handling, the ABI contract, no type aliases,
no cross-crate re-exports, and more. A reviewer will reject a PR for any
violation, so check your change against it before opening one.

## Build

```sh
cargo build --workspace
```

## The gate

Every PR must pass all three checks. Run them locally before pushing:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

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
