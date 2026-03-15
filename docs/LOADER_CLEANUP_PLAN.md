# Loader Crates Cleanup Plan

## Scope

All loader crates under `crates/`:
- `polyplug_native`
- `polyplug_lua`
- `polyplug_python`
- `polyplug_dotnet`
- `polyplug_js`
- `polyplug_js_deno`

Plus one change to `polyplug` (runtime) to remove `pub mod testing`.

---

## Phase 0 — Remove `pub mod testing` from polyplug runtime

The `runtime::testing` and `loader::testing` modules expose internal helpers
(`read_init_bundle_id`, `set_init_bundle_id`, `test_host_find_by_contract`,
`RegistrarContext`, `make_registrar_context`) purely for integration tests.
They pollute the public API and violate the principle that test scaffolding
should not live in production code.

### Strategy

Move the two integration test files into **unit tests** inside the polyplug
crate (inline `#[cfg(test)] mod tests` blocks) where they can access
private internals directly. Then delete both `pub mod testing` blocks.

**Size note**: `integration_trust_boundary.rs` is ~483 lines (5 custom
`BundleLoader` impls + 5 tests) and `integration_registrar_security.rs` is
~149 lines. This adds ~632 lines of `#[cfg(test)]` code to the source files.
This is acceptable because `#[cfg(test)]` compiles to nothing in release
builds and the tests need access to `pub(crate)` internals that integration
tests (external crate) cannot reach.

**Dead code note**: `set_init_bundle_id` in `runtime::testing` is never
called by any test. It is dead code and can be deleted without replacement.

### Steps

1. Move `tests/integration_trust_boundary.rs` logic into `runtime.rs` unit tests.
   - The tests use `test_host_find_by_contract` and `read_init_bundle_id` — both
     are private functions in `runtime.rs`, accessible from inline `#[cfg(test)]`.
2. Move `tests/integration_registrar_security.rs` logic into `loader/mod.rs` unit tests.
   - The tests use `RegistrarContext` / `make_registrar_context` — both are
     private helpers in `loader/mod.rs`, accessible from inline `#[cfg(test)]`.
3. Delete `tests/integration_trust_boundary.rs`.
4. Delete `tests/integration_registrar_security.rs`.
5. Delete `pub mod testing` from `runtime.rs`.
6. Delete `pub mod testing` from `loader/mod.rs`.
7. Remove any now-unused imports from both files.
8. Run `cargo test -p polyplug` — all moved tests must pass.

---

## Phase 1 — Fix compilation (broken loaders)

Three loaders fail to compile because `PluginContext` was extended with
`host_abi_version: u32` during the runtime refactor, but the loaders
were not updated.

### Affected files

| Crate | File | Line |
|---|---|---|
| `polyplug_lua` | `src/loader.rs` | ~380 |
| `polyplug_python` | `src/lib.rs` | ~193 |
| `polyplug_dotnet` | `src/lib.rs` | ~142 |

### Fix

Add `host_abi_version: polyplug::abi::POLYPLUG_ABI_VERSION` to each
`PluginContext` construction site. This is the correct value — it tells
the plugin which ABI version the host speaks.

**Note**: These three loaders could not be clippy-checked during
investigation because they fail to compile. After this fix, run clippy
on each — there may be additional warnings not listed in Phase 2.

---

## Phase 2 — Clippy / AGENTS.md compliance

### 2a. Missing `# Safety` docs on FFI functions

Clippy `-D warnings` rejects `unsafe extern "C" fn` without `# Safety`.

| Crate | File | Functions |
|---|---|---|
| `polyplug_native` | `ffi.rs` | `polyplug_native_loader_create`, `polyplug_native_loader_free` |
| `polyplug_js` | `ffi.rs` | `polyplug_js_loader_create`, `polyplug_js_loader_free` |
| `polyplug_js_deno` | `ffi.rs` | `polyplug_js_deno_loader_create`, `polyplug_js_deno_loader_free` |

Note: `polyplug_python/ffi.rs` and `polyplug_dotnet/ffi.rs` already have
`# Safety` docs — they are compliant.

### 2b. `derivable_impls` — manual Default for empty structs

Replace manual `impl Default` with `#[derive(Default)]`.

| Crate | File | Struct |
|---|---|---|
| `polyplug_native` | `config.rs` | `NativeConfig` |
| `polyplug_js_deno` | `config.rs` | `JsDenoConfig` |

### 2c. Missing SAFETY comments on unsafe blocks

Audit every `unsafe` block in every loader. Add `// SAFETY:` where missing.

No known missing SAFETY comments in the currently-compilable loaders —
`polyplug_native/loader.rs:36-37` already has one. However, the three
broken loaders (lua, python, dotnet) could not be fully audited until
Phase 1 unblocks compilation. Full audit required during implementation.

### 2d. Undocumented `#[allow(...)]` attributes

- `polyplug_js_deno/loader.rs` — `JsCallRequest` has `#[allow(dead_code)]`
  without a comment explaining why. Add documentation.

### 2e. `#[allow(clippy::expect_used)]` in test modules

- `polyplug_js/loader.rs` and `polyplug_js_deno/loader.rs` both have
  `#[allow(clippy::expect_used)]` on test modules.
  Per workspace lint config, `expect_used = "warn"` already. The module-level
  allow is redundant — remove it unless tests actually call `.expect()`.

### 2f. Silent `unwrap_or` patterns in production code

- `polyplug_dotnet/lib.rs:56` — `s.parse::<u32>().unwrap_or(0)` silently
  treats non-numeric minor version components as 0. This is a deliberate
  behavior choice (lenient parsing), not a bug. Add a comment documenting
  the intent so future reviewers don't flag it.

---

## Phase 3 — Architectural fixes

### 3a. Synthetic contract names in JS loaders

Both JS loaders fabricate contract names from the hash:
- `polyplug_js/loader.rs`: `format!("js_contract_{:#x}", contract_id_val)`
- `polyplug_js_deno/loader.rs`: `format!("js_deno_contract_{:#x}", contract_id_val)`

This is the same M3 bug we fixed in the runtime's `registrar_callback`.

**Scope concern**: A full fix requires extending the `registerVtable()` JS
API to accept a contract name string, which also touches guest-libs JS code
(out of scope for this plan). Two options:

1. **Deferred** — leave as-is, note the issue, fix when guest-libs are updated.
2. **Partial fix** — apply the same fallback pattern used in the runtime's
   `registrar_callback`: read `desc.contract_name` from the PluginDescriptor,
   fall back to `"contract_{hash}"` if null/empty. This keeps the fix
   loader-side only and does not require JS API changes.

Recommend option 2 (partial fix) for this plan.

### 3b. Module-level documentation

Every loader `lib.rs` has a module doc, but individual source files are
inconsistent. Add `//!` module docs to files that are missing them:

- `polyplug_native/loader.rs` — missing
- `polyplug_native/ffi.rs` — missing
- `polyplug_lua/ffi.rs` — missing
- `polyplug_js/ffi.rs` — missing

### 3c. Intentional memory leak documentation

All non-native loaders use `Box::leak()` for vtables, fn pointer arrays,
and string data. This is by design (plugins live for process lifetime).
Add a module-level doc note in each loader explaining the design decision
so future contributors do not file "memory leak" bugs.

### 3d. NativeLoader architectural concern (deferred)

`NativeLoader::load()` calls `global_registry()` and `load_bundle()`
directly — it bypasses the standard loader flow where the runtime calls
`load_bundle()` and passes a registrar. This means:

- NativeLoader is the **only** loader that accesses the global registry directly.
- It re-parses the manifest (the runtime already parsed it before calling load).
- It follows a different code path than all other loaders.

This is a design-level issue that should be addressed in a separate plan.
Not blocking for this cleanup — note it and move on.

---

## Phase 4 — Validation

Per-crate validation. Every crate must pass all four checks.

```bash
# For each loader crate (replace $CRATE):
cargo fmt -p $CRATE --check
cargo clippy -p $CRATE --lib -- -D warnings
cargo clippy -p $CRATE --tests -- -D warnings -A clippy::expect_used
cargo clippy -p $CRATE --benches -- -D warnings -A clippy::expect_used
cargo test -p $CRATE
cargo test -p $CRATE -- --ignored

# polyplug runtime (after Phase 0 changes):
cargo fmt -p polyplug --check
cargo clippy -p polyplug --lib -- -D warnings
cargo clippy -p polyplug --tests -- -D warnings -A clippy::expect_used
cargo clippy -p polyplug --benches -- -D warnings -A clippy::expect_used
cargo test -p polyplug
cargo test -p polyplug -- --ignored
```

### Acceptance criteria

- [ ] Zero clippy warnings with `-D warnings` on all 7 crates
- [ ] `cargo fmt --check` clean on all 7 crates
- [ ] All existing tests pass (no regressions)
- [ ] No `pub mod testing` in polyplug runtime
- [ ] All `unsafe` blocks have `// SAFETY:` comments
- [ ] All `#[allow(...)]` attributes are documented
