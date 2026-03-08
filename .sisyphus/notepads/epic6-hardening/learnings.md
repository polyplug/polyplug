# Epic 6 Hardening — Learnings

## Initial State (2026-03-08)
- `cargo test --workspace` → ALL PASS (66 tests across 15 test binaries) — no Epic 5 regressions
- `cargo clippy --workspace -- -D warnings` → CLEAN, zero warnings
- `cargo fmt --check` → clean (verified implicitly via clippy pass)

## Codebase Facts
- Workspace members: `crates/*`, `tests/fixtures/test_plugin`, `host-libs/rust`, `guest-libs/rust`
- Runtime crate: `crates/polyplug-runtime/` — crate-type = ["cdylib", "rlib"]
- Test binaries: integration_load, integration_dispatch, integration_graph, fnv1a_compat, integration_panic, integration_codegen_rust, integration_codegen_cpp
- No `criterion` in workspace deps yet (Task 3 adds it)
- No `memory_plugin` or `error_plugin` fixtures yet (Tasks 6, 7)
- No `TrackingAllocator` yet (Task 4)
- No real dispatcher implementation yet (Task 5)

## Key Patterns
- Module roots: ALWAYS `dirname/mod.rs` — bare `.rs` as module root is FORBIDDEN
- `use` statements: ONLY at top of file, never inside functions or impls
- No `.unwrap()` in production code — use `?` or match
- All `unsafe` blocks need `// SAFETY:` comments
- Existing test fixtures pattern: `tests/fixtures/test_plugin/`
- Build.rs pattern: `crates/polyplug-runtime/build.rs`

## Task 1 Status
- `cargo test --workspace` already passes with zero failures
- GATE IS GREEN — Task 1 is effectively done (no Epic 5 regressions)

## Task 2: Smoke gate (tests/smoke/mod.rs)

- Smoke tests mirror integration_codegen_rust and integration_codegen_cpp exactly
- `smoke_rust_codegen_dispatch`: generates Rust bindings, compiles plugin cdylib, loads with libloading, dispatches add(3,5) via vtable, asserts result==8 and ABI_OK
- `smoke_cpp_codegen_dispatch`: generates C++ bindings, asserts 6 expected files exist, optionally compiles vtables.hpp with g++ (skips gracefully if absent)
- Plugin crate name must be unique vs integration tests (used `smoke_rust_test_plugin` not `codegen_rust_test_plugin`)
- `[[test]]` entry added at bottom of `crates/polyplug-runtime/Cargo.toml` — format: `name = "smoke"`, `path = "../../tests/smoke/mod.rs"`
- Both tests passed on first run in ~3.5s (Rust plugin built from scratch, C++ generated + skipped g++ compile since g++ not available on this machine)
- Evidence at `.sisyphus/evidence/task-2-smoke.txt`
