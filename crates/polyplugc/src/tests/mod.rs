//! In-crate test modules.
//!
//! `polyplugc` is a bin-only CLI (no library target, CLAUDE.md Rule 21). Tests
//! that must reach private surfaces — the parser/IR internals and the
//! `write_output` / `force_regenerate` write semantics — live here as
//! `#[cfg(test)]` modules rather than as external integration tests in `tests/`,
//! which could only see a (non-existent) public library API. Tests that only
//! observe generated output stay in `tests/` and drive the compiled binary.

mod arena_parity;
mod generator_correctness;
mod incremental_write;
mod parser_errors;
mod toml_malformed;
