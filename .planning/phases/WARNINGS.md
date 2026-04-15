---
title: "polyplug Project Warnings & Technical Debt"
created: 2026-04-13
status: deferred
---

# polyplug Warnings & Technical Debt

These are non-blocking issues found during the project-wide audit. They don't cause failures today but should be addressed over time to improve code quality, maintainability, and developer experience.

---

## W1: Dead `sg_scan_methods` function

**File:** `crates/polyplug_abi/build/generate.rs:1092`
**What:** The `sg_scan_methods` function is never called anywhere in the codebase. It was prepared for ast-grep-based method body extraction, but helper methods are now inlined as const strings (Plan 19-06).
**Action:** Delete the function entirely. If ast-grep is needed in the future, it can be re-implemented then.
**Risk if unfixed:** Compiler warning on every build, dead code confusion for contributors.

---

## W2: `cargo:warning` noise on every build

**File:** `crates/polyplug_abi/build/generate.rs:1199-1201`
**What:** The build script unconditionally emits `cargo:warning=ast-grep (sg) available for future method preservation` and a "not found in PATH" variant on every build. Since ast-grep is no longer required (helpers are inlined), this is noise that appears in IDEs and CI.
**Action:** Remove or gate behind a `cfg` flag. If ast-grep is not being used, don't emit the warning.
**Risk if unfixed:** Annoying build output that trains developers to ignore warnings.

---

## W3: Unused workspace dependencies

**File:** `Cargo.toml` (workspace root)
**What:** Three dependencies declared at workspace level but never used by any crate:
- `arc-swap` (line 76) — was for hot-reload atomic pointer swapping, never wired up
- `notify` (line 77) — was for filesystem watching for hot-reload, never wired up
- `serial_test` (line 81) — declared but never used in any test file

**Action:** Remove all three from the workspace `Cargo.toml` `[workspace.dependencies]` section. If they're needed later, add them then.
**Risk if unfixed:** Bloated dependency tree, confusion about what the project actually depends on, slower `cargo metadata` resolution.

---

## W4: `vector.rs` empty placeholder

**File:** `crates/polyplug_abi/src/types/vector.rs`
**What:** File contains only a TODO comment: `// TODO: FFI-safe Vec<T> type for returning owned collections across the FFI boundary.` It's declared as `mod vector` in `types/mod.rs` but exports nothing.
**Action:** Either implement the FFI-safe vector type or delete the file and remove the `mod vector` declaration. The `Array<T>` type in `array.rs` may already serve this purpose.
**Risk if unfixed:** Dead file that contributors think is implemented.

---

## W5: Heavy `#[allow(dead_code)]` in polyplugc

**Files:** Multiple files in `crates/polyplugc/src/`
**What:** ~30+ `#[allow(dead_code)]` annotations suppress warnings for:
- `parser.rs`: `parse_bundle`, `parse_bundle_str` (never called, only `parse_bundle_with_api` is used), `RawHostContract`, most fields of `RawDependency`
- `ir.rs`: `minor_patch_encoded`, most fields of `ResolvedBundle`, `ResolvedDependency` enum, `ResolvedHostContract` fields
- `generators/mod.rs`: `force_regenerate` field, `language_name` trait method
- `generators/rust.rs`: `GUEST_ALLOCATOR_TEMPLATE` const
- `generators/lua.rs`: `generate_host_caller_function` and two other functions

**Context:** The `polyplugc` CLI code generator was built speculatively with many features that aren't yet wired up. The parser supports bundle parsing but only API parsing is used. The IR has many resolved fields that generators don't consume yet.
**Action:** Either implement the features that use this code, or remove the dead code and add it back when needed. The `parse_bundle`/`parse_bundle_str` functions are the most likely candidates for removal.
**Risk if unfixed:** Large dead code surface makes it hard to understand what's actually functional. Refactoring becomes risky because it's unclear what's reachable.

---

## W6: `sdk_validator` crate-wide dead code suppression

**Files:** `crates/sdk_validator/src/{main.rs,lib.rs,config.rs}`
**What:** `#![allow(dead_code)]` at the crate level suppresses all dead code warnings. This hides real dead code.
**Action:** Remove the crate-level suppression and fix individual dead code warnings. Either use the functions or remove them.
**Risk if unfixed:** The crate could rot silently without any compiler feedback.

---

## W7: Debug `eprintln!` in JS loader production code

**File:** `crates/polyplug_js/src/loader.rs` (multiple lines)
**What:** Debug logging statements like `eprintln!("[polyplug_js] js_dispatch: calling JS function fn_id={}")` are left in the production dispatch path.
**Action:** Replace with a proper logging framework (`tracing` with a debug level) or remove entirely. Never use `eprintln!` in library code that ships to users.
**Risk if unfixed:** Noisy stderr output for anyone using the JS loader. Performance impact from string formatting on every dispatch call.

---

## W8: Over-public types in core crate

**Files:** `crates/polyplug/src/runtime.rs`
**What:** These types are declared `pub` but only used within the `polyplug` crate:
- `LoadOptions` (line 77) — not used by any external crate
- `WarningCb` (line 68) — callback type alias, only used internally
- `ReloadCb` (line 71) — callback type alias, only used internally
- `LoadedBundle` (in `loader/loaded_bundle.rs`) — only used in `Runtime._bundles` which is `pub(crate)`

**Action:** Change visibility to `pub(crate)` for all of these. They are implementation details that don't need to be in the public API.
**Risk if unfixed:** Exposes implementation details that become part of the semver contract. External code could depend on them, making future changes breaking.

---

## W9: `polyplug_native` has zero tests

**File:** `crates/polyplug_native/` — no `tests/` directory, no `#[cfg(test)]` blocks
**What:** The native loader (`libloading`-based .so/.dll/.dylib loader) is the most fundamental plugin loading mechanism but has no unit or integration tests of its own. The integration tests in `tests/integration/` exercise it end-to-end, but the loader's internal error paths (missing symbols, wrong ABI version, failed dlopen) are untested.
**Action:** Add at minimum:
- Test that loading a valid native bundle succeeds
- Test that loading with wrong ABI version fails with correct error
- Test that loading with missing `polyplug_init` symbol fails
- Test reload path (load → reload → verify new interface)
**Risk if unfixed:** Silent regressions in the most-used loader when making changes to the loader infrastructure.

---

## W10: Stale `registrar` naming in SDK READMEs

**Files:** All SDK READMEs except Python's
**What:** Examples use `registrar` as a variable/object name, implying a `PluginRegistrar` pattern that was removed in Phase 19. The current API uses direct `HostInterface` function pointers.
**Files affected:**
- `sdks/csharp/README.md:64` — `registrar.Register<IPipelineDecoder>`
- `sdks/lua/README.md:50` — `registrar.register()`
- `sdks/js/README.md:56` — `registrar.register()`
- `sdks/cpp/README.md:60` — `registrar.Register<>()`
- `sdks/rust/guest/README.md:241,248,494` — `registrar` variable

**Action:** Rewrite all quick-start examples to use `host.register_contract(host, &descriptor, &interface)` directly. The `registrar` pattern doesn't exist in the codebase — it's aspirational documentation for a future high-level API.
**Note:** This is also tracked in FIX-PLAN.md Wave 6 Task 6.2 since the fix is the same work.

---

## W11: Rust guest README documents wrong struct layouts

**File:** `sdks/rust/guest/README.md`
**What:** The README documents struct layouts that significantly diverge from the actual auto-generated ABI:
- `GuestContractInterface` shown as `{ function_count, functions }` but actual is `{ contract_id, contract_version, dispatch_type, create_instance, destroy_instance, dispatch }`
- `PluginDescriptor` shown with flat `version_major/minor/patch` but actual uses `Version { major, minor, patch }` nested struct
- `HostInterface` shown with 7 fields and wrong function signatures vs. actual 16+ fields

**Action:** Update struct documentation to match auto-generated types. Best approach: copy from the actual `polyplug_abi` docs or reference them with a link.
**Note:** Also tracked in FIX-PLAN.md Wave 6 Task 6.3.

---

## W12: `host_call_method` is a placeholder

**File:** `crates/polyplug/src/runtime.rs:836-885`
**What:** The `host_call_method` function (cross-dispatch for plugin-plugin communication) exists and handles null-safety checks but returns `AbiErrorCode::Generic` with "not yet implemented". The function is exposed through `HostInterface.call_guest_method`.
**Context:** This was designed for cross-dispatch where a native plugin calls a method on a VM plugin (or vice versa). The infrastructure (dispatch types, function pointer slots) exists but the actual routing logic is not implemented.
**Action:** Either implement cross-dispatch or clearly document that `call_guest_method` is not functional. If deferring, consider returning a specific error code like `AbiErrorCode::NotImplemented` instead of `Generic`.
**Risk if unfixed:** Users may try to use cross-dispatch and get a generic error with no indication that it's not implemented.

---

## W13: Intentional memory leak via `Box::leak()` for HostInterface contexts

**File:** `crates/polyplug/src/runtime.rs:249-280`
**What:** `as_context_ptr()` calls `Box::leak()` on a `HostInterface` instance, leaking 144 bytes per call. This provides the self-passing pattern where `HostInterface` functions receive the host as their first argument.
**Context:** This is intentional — the host pointer must live for the process lifetime since plugins hold references to it. The comment acknowledges the leak. With typical usage (1-5 runtimes per process), this is negligible (144-720 bytes).
**Action:** Consider using `Box::into_raw` + storing the raw pointer for cleanup, or using a `OnceCell`/`Arc` pattern. But for the current "not published yet" stage with typical 1 runtime, this is acceptable.
**Risk if unfixed:** Accumulates 144 bytes per context pointer creation. Only matters if many contexts are created/destroyed in long-running processes.

---

## W14: `polyplug_dotnet` and `polyplug_python` missing `polyplug_utils` dependency

**Files:**
- `crates/polyplug_dotnet/Cargo.toml` — `polyplug_utils` only in dev-deps
- `crates/polyplug_python/Cargo.toml` — no `polyplug_utils` at all

**What:** Other loaders (`polyplug_native`, `polyplug_lua`, `polyplug_js`) all depend on `polyplug_utils` for shared hash utilities. The .NET and Python loaders don't, meaning if they need shared utilities in the future, the pattern will be inconsistent.
**Action:** Add `polyplug_utils` as a regular dependency for consistency. Low priority since they currently don't use it.
**Risk if unfixed:** Inconsistency if shared patterns (hashing, type conversion) are needed across all loaders.

---

## W15: `tempfile` version inconsistency in `sdks/rust/host`

**File:** `sdks/rust/host/Cargo.toml`
**What:** Declares `tempfile = "3"` in `[dev-dependencies]` while the workspace defines `tempfile = { version = "3.27" }`. Should use `tempfile = { workspace = true }`.
**Action:** Change to workspace reference for version consistency.
**Risk if unfixed:** Minor — could pull a different tempfile version than the rest of the workspace.

---

## Priority Guidance

| Priority | Warnings | Reason |
|----------|----------|--------|
| Do soon | W3, W7, W8 | Easy wins, reduces noise and exposure |
| Do eventually | W1, W2, W4, W5, W6, W9 | Code hygiene and test coverage |
| Low priority | W10, W11, W12, W13, W14, W15 | Documentation and aspirational features |
