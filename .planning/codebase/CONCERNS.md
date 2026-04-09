# Codebase Concerns

**Analysis Date:** 2026-04-05 (updated from 2026-04-02)

## Resolved Work

**Architecture Refactoring (v1.1 Milestone):**
- ✅ Instance-based plugin model implemented
- ✅ VTableSlot wrapper removed
- ✅ PluginGuard removed
- ✅ Generation counter removed from GuestContractHandle
- ✅ Guest/Host contract separation completed
- ✅ Callback-based hot-reload implemented
- ✅ All SDKs updated for new ABI

## Tech Debt

**Manifest Parsing Location:**
- Issue: TOML parsing in core polyplug should move to host Rust SDK
- Files: `crates/polyplug/src/loader/manifest.rs`
- Impact: Core crate has unnecessary parsing responsibility
- Fix approach: Move parsing logic to `sdks/rust/host/src/manifest.rs`
- Priority: Low (not blocking)

**Deprecated Contract Syntax:**
- Issue: Parser accepts deprecated `[[contract]]` syntax with warning
- Files: `crates/polyplugc/src/parser.rs`
- Impact: Users may use outdated syntax; warning only (not error)
- Fix approach: Eventually remove deprecated syntax support after migration period
- Priority: Low

## Security Considerations

**Unsafe FFI Boundaries:**
- Risk: Heavy unsafe code at FFI boundaries required for plugin loading
- Files: `crates/polyplug_native/src/loader.rs`, `crates/polyplug/src/ffi.rs`
- Current mitigation:
  - `catch_unwind` prevents panics crossing ABI boundary
  - ABI version check before calling init
  - Bundle tampering detection (`BundleTampered` error)
  - SAFETY comments documenting contracts
- Recommendations:
  - Ensure all unsafe blocks have SAFETY documentation
  - Validate all pointers before use

**Hot-Reload Safety Contract:**
- Risk: Raw pointer caching can cause SIGSEGV/SIGBUS after reload
- Contract: Host MUST destroy all instances in `ReloadPhase::Preparing` callback
- Current mitigation:
  - Warning callback if instances may remain
  - Callback notification before library drop
  - Documentation warns against pointer caching
- Recommendations:
  - Document this safety feature prominently

**Bundle Tampering Detection:**
- Risk: Malicious plugin could impersonate another bundle
- Files: `crates/polyplug/src/error.rs` (`BundleTampered` error)
- Current mitigation: Verify bundle_id matches manifest during init
- Recommendations: Document this security feature prominently

## Performance Considerations

**Large Generator Files:**
- Problem: Code generators are very large (2000+ lines each)
- Files: `crates/polyplugc/src/generators/*.rs`
- Cause: Template-based code generation with many cases
- Improvement path: Extract shared utilities, consider template files external to code
- Priority: Low

**Registry RwLock:**
- Implementation: Uses `RwLock` for registration (rare) and read guards for dispatch (common)
- Status: Appropriate for read-heavy workload

## Fragile Areas

**Hot-Reload Implementation:**
- Files: `crates/polyplug/src/reload.rs`, `crates/polyplug_native/src/loader.rs`
- Why fragile: Library handle lifetime management is critical; premature drop causes SIGBUS
- Safe modification:
  - Ensure library stored before init call
  - Fire callbacks in correct order
- Test coverage: `hot_reload_safety.rs`, `stress_hot_reload.rs`

**BundleLoader Trait:**
- Files: `crates/polyplug/src/loader/bundle_loader.rs`
- Why fragile: Trait is core abstraction for all loaders
- Safe modification:
  - Keep trait methods minimal
  - Add new methods with default implementations
  - Document safety contracts for each method

## Scaling Limits

**Plugin Slot Capacity:**
- Current capacity: 2^32 slots (u32 index)
- Limit: Registry uses fixed-size slot table
- Scaling path: Use dynamic HashMap if needed

**Contract ID Hash Collision:**
- Current capacity: 64-bit FNV-1a hash
- Limit: Hash collision detected but prevents registration
- Scaling path: Use full contract name string if collision becomes practical concern

**Bundle Count:**
- Current capacity: Unlimited (HashMap storage)
- Limit: Memory and filesystem constraints

## Test Coverage Gaps

**Miri Exclusions:**
- What's not tested: FFI/dlopen paths under Miri memory checker
- Files: Many tests have `#[cfg(not(miri))]`
- Risk: Memory safety bugs in unsafe code not detected
- Priority: Low (Miri limitation, not codebase issue)

---
*Concerns audit: 2026-04-05 (updated)*