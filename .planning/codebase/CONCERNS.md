# Codebase Concerns

**Analysis Date:** 2026-04-02

## Work in Progress

**Active Refactoring (Native Decoupling):**
- Issue: Major architectural refactoring to remove native coupling from polyplug core
- Files: `REFACTORING_PLAN_NATIVE_DECOUPLING.md` (1198 lines planning document)
- Status: Plan marked "NOT STARTED" but git shows 28 modified files with 3434 deletions
- Impact: Breaking changes acceptable (crate not published yet)
- Fix approach: 6-phase execution outlined in plan
  - Phase 1: Update BundleLoader trait (add reload method)
  - Phase 2: Create generic reload framework
  - Phase 3: Create NativeLoader in polyplug_native
  - Phase 4: Remove native coupling from core
  - Phase 5: Require explicit runtime in manifest
  - Phase 6: Use newtype IDs (BundleId, PluginContractId)

**Uncommitted Changes (693 additions, 3434 deletions):**
- Modified crates: polyplug, polyplug_native, polyplug_dotnet, polyplug_js, polyplug_lua, polyplug_python
- SDK ABI files deleted: cpp, csharp, js, lua, python (moved/ regenerated)
- New file: `crates/polyplug_native/src/error.rs` (untracked)

## Tech Debt

**Build Script Needs Update:**
- Issue: polyplug_abi build script uses old src/lib.rs parsing approach
- Files: `crates/polyplug_abi/build/main.rs:41`
- Comment: `// TODO: Update build script to work with the new modular rust files in abi crate`
- Impact: Build may fail or generate incorrect SDK bindings
- Fix approach: Update extractor to handle modular ABI crate structure

**Manifest Parsing Location:**
- Issue: TOML parsing in core polyplug should move to host Rust SDK
- Files: `crates/polyplug/src/loader/manifest.rs:7`
- Comment: `// TODO: Move toml parse to host rust SDK`
- Impact: Core crate has unnecessary parsing responsibility
- Fix approach: Move parsing logic to `sdks/rust/host/src/manifest.rs`

**Deprecated Contract Syntax:**
- Issue: Parser accepts deprecated `[[contract]]` syntax with warning
- Files: `crates/polyplugc/src/parser.rs:178`
- Warning: `eprintln!("warning: [[contract]] is deprecated, use [[plugin_contract]] instead")`
- Impact: Users may use outdated syntax; warning only (not error)
- Fix approach: Eventually remove deprecated syntax support after migration period

**Stub Registrar Callback:**
- Issue: registrar_callback in loader returns `AbiError::ok()` without registering vtables
- Files: `crates/polyplug/tests/library_lifetime.rs:115-168` (reference)
- Impact: Tests cannot verify actual registration via registry.find()
- Fix approach: Implement proper state passing (separate TODO per comments)

**Template Placeholder TODOs:**
- Issue: Generated plugin templates contain TODO comments for implementation
- Files: `crates/polyplugc/src/pack.rs:73,91,107,170,207,225,265`
- Pattern: `// TODO: implement the contract functions for this plugin`
- Impact: Generated code requires manual implementation (expected behavior)
- Fix approach: Documentation/guides for users on implementing contracts

**AST-grep Method Preservation:**
- Issue: Build script has TODO for preserving helper method bodies during regeneration
- Files: `crates/polyplug_abi/build/main.rs:28-39`
- Plan: Use ast-grep to add/update/delete method signatures only, preserve bodies
- Mitigation: `sdk_validator` to fail if `DELETED_` prefix found (deleted method detection)
- Fix approach: Implement ast-grep integration as described

## Security Considerations

**Unsafe FFI Boundaries:**
- Risk: Heavy unsafe code at FFI boundaries required for plugin loading
- Files: `crates/polyplug_native/src/loader.rs:61-78`, `crates/polyplug/src/ffi.rs:136-194`
- Current mitigation:
  - `catch_unwind` prevents panics crossing ABI boundary
  - ABI version check before calling init
  - Bundle tampering detection (`BundleTampered` error)
  - SAFETY comments documenting contracts
- Recommendations:
  - Ensure all unsafe blocks have SAFETY documentation
  - Validate all pointers before use
  - Consider sanitizers/fuzzing for FFI paths

**Hot-Reload Safety Contract:**
- Risk: Raw pointer caching can cause SIGSEGV/SIGBUS after reload
- Files: `crates/polyplug/src/reload.rs:74-76` (comments)
- Contract: Host MUST release cached raw pointers on `ReloadPhase::Reloaded`
- Current mitigation:
  - `wait_for_quiescence()` uses Arc strong_count
  - Callback notification before library drop
  - Documentation warns against pointer caching
- Recommendations:
  - Add runtime assertion/debug builds to detect stale pointer usage
  - Consider canary values in freed memory regions

**Bundle Tampering Detection:**
- Risk: Malicious plugin could impersonate another bundle
- Files: `crates/polyplug/src/error.rs:172-179` (`BundleTampered` error)
- Current mitigation: Verify bundle_id matches manifest during init
- Recommendations: Document this security feature prominently

**Dynamic Library Loading:**
- Risk: dlopen/LoadLibrary loads arbitrary code into process
- Files: `crates/polyplug_native/src/loader.rs:61`
- Current mitigation:
  - Path comes from manifest (user-controlled)
  - ABI version mismatch detection
- Recommendations:
  - Consider bundle signing/verification
  - Document trust assumptions for plugin directories

## Performance Bottlenecks

**Large Generator Files:**
- Problem: Code generators are very large (2000+ lines each)
- Files:
  - `crates/polyplugc/src/generators/rust.rs` (3185 lines)
  - `crates/polyplugc/src/generators/cpp.rs` (2884 lines)
  - `crates/polyplugc/src/generators/python.rs` (2843 lines)
  - `crates/polyplugc/src/generators/csharp.rs` (2714 lines)
  - `crates/polyplugc/src/generators/js_quickjs.rs` (2366 lines)
  - `crates/polyplugc/src/generators/lua.rs` (2268 lines)
- Cause: Template-based code generation with many cases
- Improvement path:
  - Extract shared utilities
  - Consider template files external to code
  - Add incremental generation support

**Quiescence Wait Loop:**
- Problem: Hot-reload waits up to 5 seconds for in-flight calls
- Files: `crates/polyplug/src/reload.rs:77-100`
- Cause: Spin loop checking Arc strong_count every 1ms
- Improvement path:
  - Consider event-based notification
  - Shorter timeout with retry mechanism

**Registry Mutex Contention:**
- Problem: Multiple Mutex locks in plugin registry operations
- Files: `crates/polyplug/src/registry/plugin_registry.rs`
- Cause: Thread-safe plugin registration and lookup
- Improvement path: Consider RwLock for read-heavy workload

## Fragile Areas

**Hot-Reload Implementation:**
- Files: `crates/polyplug/src/reload.rs`, `crates/polyplug_native/src/loader.rs`
- Why fragile: Library handle lifetime management is critical; premature drop causes SIGBUS
- Safe modification:
  - Ensure library stored before init call
  - Wait for quiescence before dropping old library
  - Fire callbacks in correct order
- Test coverage: `hot_reload_safety.rs`, `stress_hot_reload.rs`, `stress_quiescence_race.rs`

**VTable Registration:**
- Files: `crates/polyplug/src/registry/plugin_registry.rs`
- Why fragile: Generation-based handles require precise coordination
- Safe modification:
  - Always increment generation on vtable swap
  - Validate handle before vtable access
  - Never reuse slot indices
- Test coverage: `registry_edge_cases.rs`, `stress_concurrent_registry.rs`

**Cross-Language Tests:**
- Files: `tests/integration/tests/cross_language.rs` (1737 lines)
- Why fragile: Tests FFI contracts across multiple language implementations
- Safe modification: Maintain exact ABI signatures
- Test coverage: Covers native, Python, Lua, JS, .NET interop

**BundleLoader Trait:**
- Files: `crates/polyplug/src/loader/bundle_loader.rs`
- Why fragile: Trait is core abstraction for all loaders
- Safe modification:
  - Keep trait methods minimal
  - Add new methods with default implementations
  - Document safety contracts for each method
- Test coverage: Multiple loader tests per language

## Scaling Limits

**Plugin Slot Capacity:**
- Current capacity: 2^32 slots (u32 index)
- Limit: Registry uses fixed-size slot table
- Scaling path: Use dynamic HashMap if needed, but current limit is theoretical

**Contract ID Hash Collision:**
- Current capacity: 64-bit FNV-1a hash
- Limit: Hash collision detected but prevents registration
- Scaling path: Use full contract name string if collision becomes practical concern

**Bundle Count:**
- Current capacity: Unlimited (HashMap storage)
- Limit: Memory and filesystem constraints
- Scaling path: Lazy loading for large plugin directories

**Quiescence Timeout:**
- Current capacity: 5 seconds maximum wait
- Limit: Long-running plugin calls may exceed timeout
- Scaling path: Make timeout configurable per bundle

## Dependencies at Risk

**libloading (Native Loader):**
- Risk: Core dependency for native plugin loading
- Version: Used in `polyplug_native` crate
- Impact: Plugin loading fails if library unavailable
- Migration plan: Part of refactoring - moving to polyplug_native crate

**rquickjs (JS Loader):**
- Risk: QuickJS bindings for JavaScript plugins
- Impact: JS plugin support unavailable if crate has issues
- Alternative: Could use different JS engine (deno_core, v8)

**petgraph (Dependency Graph):**
- Risk: Graph algorithms for dependency resolution
- Impact: Cycle detection and ordering unavailable
- Alternative: Custom graph implementation or other graph crate

**thiserror (Error Types):**
- Risk: Error type derivation
- Impact: All error types would need manual implementation
- Alternative: Manual Error implementations (more boilerplate)

## Missing Critical Features

**Helper Method Preservation:**
- Problem: SDK regeneration may overwrite user-added helper methods
- What's missing: ast-grep integration for method body preservation
- Blocks: Users cannot safely add helper methods to SDK files

**Explicit Runtime Registration:**
- Problem: Currently auto-registers native loader
- What's missing: Explicit loader registration API
- Blocks: Clean separation of loader implementations (being addressed in refactoring)

**Bundle Signing/Verification:**
- Problem: No verification of plugin authenticity
- What's missing: Cryptographic signature verification
- Blocks: Security-sensitive deployment scenarios

## Test Coverage Gaps

**Stub Registrar Callback:**
- What's not tested: Actual vtable registration via registrar_callback
- Files: `crates/polyplug/tests/library_lifetime.rs:168`
- Risk: Registration logic may have bugs not detected by stub-based tests
- Priority: Medium (separate TODO acknowledged)

**Hot-Reload Edge Cases:**
- What's not tested: Concurrent reload of multiple bundles, partial failures
- Files: `stress_hot_reload.rs`, `stress_quiescence_race.rs` exist
- Risk: Race conditions in multi-bundle scenarios
- Priority: High (existing stress tests may cover)

**Miri Exclusions:**
- What's not tested: FFI/dlopen paths under Miri memory checker
- Files: Many tests have `#[cfg(not(miri))]`
- Risk: Memory safety bugs in unsafe code not detected
- Priority: Low (Miri limitation, not codebase issue)

**Error Path Coverage:**
- What's not tested: All error variants may not have integration test coverage
- Files: `stress_error.rs` exists, error.rs has unit tests for Display
- Risk: Some error paths may not be exercised in real scenarios
- Priority: Medium

---

*Concerns audit: 2026-04-02*