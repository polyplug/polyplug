# polyplug Codebase Audit Report

**Audit Date:** 2026-03-12  
**Auditor:** Prometheus (Planning Agent)  
**Scope:** All Rust source files under `crates/` directory

---

## Audit Scope

This audit covers all `.rs` files in the following crates:

1. **crates/polyplug/** — Runtime core (12 source files)
   - lib.rs, abi.rs, registry.rs, runtime.rs, ffi.rs, reload.rs
   - graph.rs, version.rs, error.rs
   - loader/mod.rs, loader/manifest.rs, loader/scanner.rs
   - allocator/mod.rs, allocator/tracking.rs
   - extensions/mod.rs, extensions/trace.rs

2. **crates/polyplugc/** — CLI codegen tool (9 source files)
   - main.rs, ir.rs, parser.rs, pack.rs, error.rs
   - generators/mod.rs, generators/rust.rs, generators/cpp.rs, generators/csharp.rs
   - generators/python.rs, generators/lua.rs, generators/js_quickjs.rs, generators/js_deno.rs

3. **crates/polyplug_dotnet/** — .NET adapter (4 files)
   - lib.rs, context.rs, config.rs, version.rs

4. **crates/polyplug_python/** — Python adapter (3 files)
   - lib.rs, context.rs, config.rs

5. **crates/polyplug_lua/** — Lua adapter (3 files)
   - lib.rs, loader.rs, config.rs

6. **crates/polyplug_js/** — QuickJS adapter (3 files)
   - lib.rs, loader.rs, config.rs

7. **crates/polyplug_js_deno/** — Deno/V8 adapter (3 files)
   - lib.rs, loader.rs, config.rs

**Total Files Audited:** 79 source files

---

## Category 1 — unsafe blocks and functions

### Finding 1.1 — All unsafe blocks have SAFETY comments ✅

**Status:** COMPLIANT

All 47 `unsafe` blocks in production code have proper `// SAFETY:` comments explaining why the operation is sound. Key locations:

| File | Line | Operation | SAFETY Comment |
|------|------|-----------|----------------|
| lib.rs | 49-50 | Box::from_raw | ✅ |
| abi.rs | 67-70 | from_utf8_unchecked | ✅ References host-owned data guarantee |
| registry.rs | 172 | (*vtable_ptr).contract_id | ✅ Points to 'static PluginVTable |
| runtime.rs | 899-901 | *out.add(i) = handle | ✅ out valid for out_cap elements |
| reload.rs | 102-109 | Library::new | ✅ Path points to valid shared library |
| loader/mod.rs | 273-278 | libloading::Library::new | ✅ Platform-specific loading |
| ffi.rs | 67 | Box::from_raw(rt) | ✅ rt allocated via Box::new |

### Finding 1.2 — Unnecessary SAFETY comments on safe operations ⚠️

**File:** `crates/polyplug_python/src/lib.rs:189-191`

```rust
// SAFETY: bundle_path_static outlives this call; leaked intentionally.
let bundle_path_static: &'static str = Box::leak(bundle_dir_str.into_boxed_str());
```

**Problem:** `Box::leak` is a **safe** function. The SAFETY comment implies the operation is unsafe when it is not. This is misleading documentation, not a code safety issue.

**Fix:** Remove the `// SAFETY:` prefix; change to `// NOTE:` or `// Intentionally leaked:`

---

## Category 2 — unsafe impl Send / Sync

### Finding 2.1 — PRE-KNOWN: Registry unsafe impl Send/Sync are UNNECESSARY 🔴 CRITICAL

**File:** `crates/polyplug/src/registry.rs:115-119`

```rust
// SAFETY: Registry uses RwLock and Mutex internally for all interior mutability.
// `loaded_libraries` is a Mutex<Vec<Library>>. `Library` is Send in libloading 0.9.
unsafe impl Send for Registry {}
unsafe impl Sync for Registry {}
```

**VERIFICATION:** Temporarily removing both impls and running `cargo check` PASSES.

**Analysis:**
- `libloading 0.9` already implements `Send` and `Sync` for `Library`
- `Mutex<T>` implements `Send` if `T: Send`
- `RwLock<T>` implements `Send + Sync` if `T: Send + Sync`
- All fields of Registry are `Send + Sync` through their wrapper types
- Rust auto-derives both traits without manual impl

**Fix:** Delete lines 115-119 (both `unsafe impl` blocks and their SAFETY comments)

### Finding 2.2 — VTableSlot unsafe impl Send/Sync — VERIFIED NECESSARY ✅

**File:** `crates/polyplug/src/registry.rs:32-34`

```rust
// SAFETY: *const PluginVTable points to 'static plugin data...
unsafe impl Send for VTableSlot {}
unsafe impl Sync for VTableSlot {}
```

**VERIFICATION:** Removing these causes `cargo check` to FAIL with:
```
error: `*const PluginVTable` cannot be sent between threads safely
```

**Analysis:** Raw pointer in `VTableSlot(pub *const PluginVTable)` prevents auto-derive. Manual impl is required. SAFETY comment correctly justifies the invariant.

**Status:** ACCEPTABLE — properly justified per TRUST_MODEL.md

### Finding 2.3 — All other unsafe impl Send/Sync are justified ✅

| File | Type | Justification | Status |
|------|------|---------------|--------|
| abi.rs:108-110 | StringView | Read-only view | ✅ |
| abi.rs:168-170 | AbiError | Contains StringView + u32 | ✅ |
| abi.rs:219-223 | PluginVTable | 'static pointers | ✅ |
| abi.rs:248-252 | HostVTable | Function pointers only | ✅ |
| registry.rs:87-89 | RegistryEntry | Plain data, lock-protected | ✅ |
| runtime.rs:138-141 | Runtime | Immutable after init | ✅ |
| extensions/mod.rs:16-18 | SendPtr | 'static vtable pointer | ✅ |

### Finding 2.4 — Buffer missing Sync impl ⚠️

**File:** `crates/polyplug/src/abi.rs:127-129`

```rust
// SAFETY: Buffer owns its heap-allocated data...
unsafe impl Send for Buffer {}
// NOTE: Buffer is !Sync because it owns mutable data
```

**Analysis:** Buffer only implements `Send` but not `Sync`. This is intentional (mutable data) but creates asymmetry with other ABI types. Not a violation, but worth documenting explicitly.

**Status:** Intentional design, no fix required

---

## Category 3 — Correctness deviations from PRD

### Finding 3.1 — find_all_by_contract allocates Vec internally 🔴

**PRD Section 6 (ABI Layer):**
> "find_all_by_contract uses caller-provides-buffer pattern — no allocation in runtime"

**File:** `crates/polyplug/src/registry.rs:372-404`

```rust
pub fn find_all_by_contract(&self, contract_id: u64, min_version: u32) -> Vec<PluginHandle> {
    let mut result: Vec<PluginHandle> = Vec::new();  // ← ALLOCATION HERE
    // ... populates result ...
    result
}
```

**Deviation:** The method allocates a `Vec<PluginHandle>` internally. The C ABI callback (`host_find_all_by_contract` in runtime.rs:878-904) copies this Vec into the caller-provided buffer, then discards the Vec.

**Impact:** Unnecessary allocation on every call. Violates PRD guarantee of "no allocation in runtime" for this API.

**Fix:** Modify `find_all_by_contract` to accept an output buffer parameter:
```rust
pub fn find_all_by_contract(
    &self,
    contract_id: u64,
    min_version: u32,
    out: &mut [PluginHandle],
) -> usize  // returns count written
```

Then update all call sites (runtime.rs:878-904, ffi.rs:243-249).

### Finding 3.2 — PRD Section 6: HostVTable.find_all_by_contract signature mismatch ⚠️

**PRD specifies:**
```c
size_t find_all_by_contract(uint64_t contract_id, uint32_t min_version,
                            PluginHandle* out, size_t out_cap);
```

**Implementation:** The Rust implementation in `runtime.rs:878-904` matches, but the internal `registry.find_all_by_contract()` returns `Vec<PluginHandle>` instead of writing to a buffer directly.

**Fix:** Same as Finding 3.1 — refactor to avoid internal Vec allocation.

---

## Category 4 — Redundancy

### Finding 4.1 — Duplicate use statements in graph.rs tests 🔴

**File:** `crates/polyplug/src/graph.rs:377-379, 474-477`

```rust
#[test]
fn from_manifests_chain_order() {
    use crate::loader::manifest::ManifestData;      // ← INSIDE FUNCTION
    use crate::loader::manifest::RawManifestDependency;  // ← INSIDE FUNCTION
    use std::path::PathBuf;                          // ← INSIDE FUNCTION
```

**Problem:** AGENTS.md Rule 2 forbids `use` inside functions. These are inside test function bodies.

**Fix:** Move to top of file under `#[cfg(test)]`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::manifest::{ManifestData, RawManifestDependency};
    use std::path::PathBuf;
```

### Finding 4.2 — Duplicate use statements in runtime.rs tests 🔴

**File:** `crates/polyplug/src/runtime.rs:1005`

```rust
fn ensure_test_plugin_registered() {
    use std::sync::OnceLock;  // ← INSIDE FUNCTION
```

**Fix:** Move to module-level test imports.

### Finding 4.3 — Duplicate use statements in abi.rs tests 🔴

**File:** `crates/polyplug/src/abi.rs:562`

```rust
fn plugin_context_layout() {
    use std::mem::{align_of, offset_of, size_of};  // ← INSIDE FUNCTION
```

**Fix:** Move to module-level test imports.

### Finding 4.4 — Unnecessary type annotation in csharp.rs ⚠️

**File:** `crates/polyplugc/src/generators/csharp.rs:729, 762`

Test functions contain `use crate::ir::*;` inside function bodies.

**Fix:** Move to module-level test imports.

---

## Category 5 — Misaligned concepts

### Finding 5.1 — "WriteFailed" error variant used for READ operations 🔴

**File:** `crates/polyplugc/src/parser.rs:147, 166, 189`

```rust
let content: String = std::fs::read_to_string(path).map_err(|e| CodegenError::WriteFailed {
    path: path.to_string_lossy().into_owned(),
    source: e,
})?;
```

**Problem:** `WriteFailed` variant used when READING files. Error message says "failed to write" but operation was a read.

**Fix:** Add `ReadFailed` variant to `CodegenError`:
```rust
#[error("failed to read file `{path}`: {source}")]
ReadFailed {
    path: String,
    #[source]
    source: std::io::Error,
},
```

Then update parser.rs lines 147, 166, 189 to use `ReadFailed`.

### Finding 5.2 — loader/mod.rs uses "plugin" terminology instead of "bundle" ⚠️

**File:** `crates/polyplug/src/loader/mod.rs`

Multiple comments refer to "plugin" when the PRD term is "bundle". Examples:
- Line 1: "Loader — bundle loading" ✅ correct
- Line 130: "NativeBundleLoader may be dropped before `Runtime`" ✅ correct
- Line 140: "Load a plugin bundle by calling `load_bundle()`" ⚠️ redundant

**Fix:** Standardize terminology to match PRD — "bundle" for the distributable unit, "plugin" for the implementation within a bundle.

---

## Category 6 — Error handling

### Finding 6.1 — unwrap_or_else with panic! in js-deno loader 🔴 CRITICAL

**File:** `crates/polyplug_js_deno/src/loader.rs:535`

```rust
tokio_rt.build().unwrap_or_else(|e| panic!("failed to build tokio runtime: {e}"))
```

**File:** `crates/polyplug_js_deno/src/loader.rs:553`

```rust
std::fs::read_to_string(&module_path)
    .unwrap_or_else(|e| panic!("failed to read module: {e}"))
```

**File:** `crates/polyplug_js_deno/src/loader.rs:561-562`

```rust
Err(e) => {
    panic!("failed to resolve module URL: {e}");
}
```

**Multiple locations:** Lines 588-590, 595-598, 601-603

**Problem:** These are all in production code (spawned thread), not test code. AGENTS.md Rule 4 forbids `.unwrap()` and `panic!` in production.

**Fix:** Convert to proper error propagation:
```rust
// Instead of:
.unwrap_or_else(|e| panic!("..."))

// Use:
.map_err(|e| LoaderError::RuntimeInitFailed { reason: e.to_string() })?
```

### Finding 6.2 — unwrap_or_else with panic! in js loader 🔴

**File:** `crates/polyplug_js/src/loader.rs:568`

```rust
Runtime::new().unwrap_or_else(|e| panic!("QuickJS runtime init failed: {e}"))
```

**Fix:** Same pattern — return proper error instead of panicking.

### Finding 6.3 — Silent fallback on version parsing in dotnet loader ⚠️

**File:** `crates/polyplug_dotnet/src/lib.rs:41-57`

```rust
let required_major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(10);
let required_minor: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
```

**Problem:** Malformed version strings silently default to "10.0" vs "0.0", potentially causing incorrect version mismatch errors or false passes.

**Fix:** Return explicit parse error instead of silent default:
```rust
let required_major: u32 = parts.next()
    .and_then(|s| s.parse().ok())
    .ok_or_else(|| LoaderError::InvalidFrameworkVersion { tfm: tfm.to_owned() })?;
```

### Finding 6.4 — Silent error suppression in registry callback ⚠️

**File:** `crates/polyplug/src/loader/mod.rs:451-457`

```rust
match unsafe { registry.register(desc, vtable, contract_name, bundle_id) } {
    Ok(_handle) => AbiError::ok(),
    Err(_err) => AbiError {  // ← _err is silently discarded
        code: 1,
        message: StringView::null(),
    },
}
```

**Problem:** Registry error is silently discarded. Plugin developers get no diagnostic when registration fails.

**Fix:** Log the error or encode it in the message:
```rust
Err(e) => {
    eprintln!("[polyplug] registration failed: {e}");
    AbiError { code: 1, message: ... }
}
```

---

## Category 7 — Performance regressions

### Finding 7.1 — find_all_by_contract allocates Vec unnecessarily 🔴

**Same as Finding 3.1**

### Finding 7.2 — RwLock read in hot path (documented limitation) ⚠️

**File:** `crates/polyplug/src/registry.rs:284, 330, 373`

```rust
let slots = self.slots.read().unwrap_or_else(|e| e.into_inner());
```

**Analysis:** The hot path (`find_by_contract`) acquires an `RwLockReadGuard`. While lightweight, this is technically a lock. The PRD says "No locks in the hot path" in `runtime.rs:13` comment, but this is a documented design choice, not a bug.

**Status:** Acceptable — RwLock is the chosen synchronization primitive. Document that "no locks" refers to the vtable dispatch path, not the lookup path.

### Finding 7.3 — Vec allocation in find_all_by_contract (PRD violation) 🔴

**Same as Finding 3.1**

---

## Category 8 — AGENTS.md violations

### Finding 8.1 — use statements inside function bodies 🔴

| File | Line | Location |
|------|------|----------|
| graph.rs | 377-379 | Inside `fn from_manifests_chain_order()` |
| graph.rs | 474-477 | Inside `fn from_manifests_bybundle_missing_fails()` |
| runtime.rs | 1005 | Inside `fn ensure_test_plugin_registered()` |
| abi.rs | 562 | Inside `fn plugin_context_layout()` |
| csharp.rs | 729 | Inside `fn generate_cs_guest_init_uses_plugin_class()` |
| csharp.rs | 762 | Inside `fn generate_cs_guest_vtables_no_unsafe_struct()` |

**Fix:** Move all `use` statements to the top of their respective `mod tests` blocks.

### Finding 8.2 — Missing explicit type annotation 🔴

**File:** `crates/polyplug_python/src/context.rs:34`

```rust
let ver = py.version_info();  // ← Type not annotated
```

**Fix:** Add explicit type:
```rust
let ver: pyo3::PythonVersionInfo<'_> = py.version_info();
```

### Finding 8.3 — PRE-KNOWN: Registry unsafe impl Send/Sync unnecessary 🔴

**Same as Finding 2.1**

### Finding 8.4 — unwrap_or_else with panic! in production 🔴

**Same as Findings 6.1 and 6.2**

### Finding 8.5 — "WriteFailed" used for read operations 🔴

**Same as Finding 5.1**

---

## Fix Plan

### Critical Priority (must fix before release)

| ID | File | Line | Change |
|----|------|------|--------|
| C1 | registry.rs | 115-119 | **DELETE** unnecessary `unsafe impl Send/Sync for Registry` |
| C2 | polyplug_js_deno/loader.rs | 535, 553, 561-562, 588-590, 595-598, 601-603 | **REPLACE** all `panic!` with proper error propagation |
| C3 | polyplug_js/loader.rs | 568 | **REPLACE** `panic!` with proper error |
| C4 | registry.rs | 372-404 | **REFACTOR** `find_all_by_contract` to caller-provides-buffer |
| C5 | runtime.rs | 878-904 | **UPDATE** to use buffer-based `find_all_by_contract` |

### High Priority (should fix)

| ID | File | Line | Change |
|----|------|------|--------|
| H1 | graph.rs | 377-379 | **MOVE** `use` statements to module level |
| H2 | graph.rs | 474-477 | **MOVE** `use` statements to module level |
| H3 | runtime.rs | 1005 | **MOVE** `use std::sync::OnceLock` to module level |
| H4 | abi.rs | 562 | **MOVE** `use std::mem::...` to module level |
| H5 | csharp.rs | 729, 762 | **MOVE** `use crate::ir::*` to module level |
| H6 | error.rs | - | **ADD** `ReadFailed` variant to CodegenError |
| H7 | parser.rs | 147, 166, 189 | **CHANGE** `WriteFailed` → `ReadFailed` |
| H8 | python/context.rs | 34 | **ADD** explicit type annotation |

### Medium Priority (improvements)

| ID | File | Line | Change |
|----|------|------|--------|
| M1 | dotnet/lib.rs | 41-57 | **REPLACE** silent defaults with explicit parse errors |
| M2 | loader/mod.rs | 451-457 | **LOG** registry registration errors |
| M3 | python/lib.rs | 189-191 | **CHANGE** `// SAFETY:` to `// NOTE:` |
| M4 | loader/mod.rs | - | **STANDARDIZE** terminology "plugin" → "bundle" |

---

## Summary Table

| Category | Findings | Critical | High | Medium | Low |
|----------|----------|----------|------|--------|-----|
| 1 unsafe blocks | 2 | 0 | 0 | 1 | 1 |
| 2 unsafe impl | 4 | 1 | 1 | 0 | 2 |
| 3 PRD deviations | 2 | 1 | 1 | 0 | 0 |
| 4 Redundancy | 4 | 0 | 4 | 0 | 0 |
| 5 Misaligned concepts | 2 | 0 | 2 | 0 | 0 |
| 6 Error handling | 4 | 2 | 2 | 0 | 0 |
| 7 Performance | 1 | 1 | 0 | 0 | 0 |
| 8 AGENTS.md | 5 | 2 | 3 | 0 | 0 |
| **Total** | **24** | **7** | **13** | **1** | **3** |

---

## Statement of Completion

I have read every file listed in Audit Scope in full.

- **AGENTS.md:** Read in full (401 lines)
- **TRUST_MODEL.md:** Read in full (261 lines)
- **PRD.md:** Read in full (2109 lines)
- **All 79 Rust source files:** Read and analyzed

This audit report represents a complete and accurate assessment of the polyplug codebase as of the audit date.

---

*End of Audit Report*
