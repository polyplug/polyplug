# polyplug Fix Plan

**Generated from:** polyplug-codebase-audit.md  
**Date:** 2026-03-12  
**Total Fixes:** 24 findings across 8 categories

---

## Executive Summary

This fix plan addresses **24 findings** from the comprehensive codebase audit:
- **7 Critical** — Must fix before release (panics in production, PRD violations, unnecessary unsafe)
- **13 High** — Should fix (AGENTS.md violations, error handling issues)
- **1 Medium** — Improvements recommended
- **3 Low** — Documentation/style issues

---

## Wave 1: Critical Fixes (7 tasks)

### Fix C1 — Remove unnecessary unsafe impl Send/Sync for Registry

**Priority:** Critical  
**Effort:** Trivial (< 5 min)  
**Category:** Category 2 (unsafe impl Send/Sync)

**Files affected:**
- `crates/polyplug/src/registry.rs:115-119`

**Exact change:**
```rust
// DELETE these lines (115-119):
// SAFETY: Registry uses RwLock and Mutex internally for all interior mutability.
// `loaded_libraries` is a Mutex<Vec<Library>>. `Library` is Send in libloading 0.9.
// All mutable state is lock-protected; sharing across threads is safe.
unsafe impl Send for Registry {}
// SAFETY: Same reasoning as Send above...
unsafe impl Sync for Registry {}
```

**Verification:**
```bash
cd crates/polyplug && cargo check
# Should compile without the manual impls
```

---

### Fix C2 — Replace panics with proper errors in js-deno loader

**Priority:** Critical  
**Effort:** Medium (1-2 hours)  
**Category:** Category 6 (Error handling)

**Files affected:**
- `crates/polyplug_js_deno/src/loader.rs` (6 locations)

**Changes required:**

1. **Line 535:** `tokio_rt.build()` panic
```rust
// BEFORE:
let runtime = tokio_rt.build().unwrap_or_else(|e| panic!("failed to build tokio runtime: {e}"));

// AFTER:
let runtime = tokio_rt.build().map_err(|e| PolyplugError::Loader(LoaderError::JsRuntimeInitFailed {
    reason: format!("failed to build tokio runtime: {e}"),
}))?;
```

2. **Line 553:** `read_to_string` panic
```rust
// BEFORE:
let code = std::fs::read_to_string(&module_path).unwrap_or_else(|e| panic!("..."));

// AFTER:
let code = std::fs::read_to_string(&module_path).map_err(|e| PolyplugError::Loader(LoaderError::BundleReadFailed {
    path: module_path.display().to_string(),
    source: e,
}))?;
```

3. **Lines 561-562:** Module resolution panic
4. **Lines 588-590:** execute_script panic
5. **Lines 595-598:** load_main_es_module panic
6. **Lines 601-603:** run_event_loop panic

**Add new error variants to LoaderError:**
```rust
#[error("JS runtime initialization failed: {reason}")]
JsRuntimeInitFailed { reason: String },

#[error("failed to read bundle at `{path}`: {source}")]
BundleReadFailed { path: String, #[source] source: std::io::Error },

#[error("module resolution failed: {reason}")]
ModuleResolutionFailed { reason: String },

#[error("failed to execute JS script: {reason}")]
JsExecutionFailed { reason: String },
```

**Verification:**
```bash
cargo test --package polyplug_js_deno
```

---

### Fix C3 — Replace panic with proper error in js loader

**Priority:** Critical  
**Effort:** Small (< 30 min)  
**Category:** Category 6 (Error handling)

**Files affected:**
- `crates/polyplug_js/src/loader.rs:568`

**Exact change:**
```rust
// BEFORE:
let runtime = rquickjs::Runtime::new().unwrap_or_else(|e| panic!("QuickJS runtime init failed: {e}"));

// AFTER:
let runtime = rquickjs::Runtime::new().map_err(|e| PolyplugError::Loader(LoaderError::JsRuntimeInitFailed {
    reason: format!("QuickJS runtime init failed: {e}"),
}))?;
```

**Verification:**
```bash
cargo check --package polyplug_js
```

---

### Fix C4 & C5 — Refactor find_all_by_contract to caller-provides-buffer

**Priority:** Critical  
**Effort:** Medium (2-4 hours)  
**Category:** Category 3 (PRD deviation) + Category 7 (Performance)

**Files affected:**
- `crates/polyplug/src/registry.rs:372-404`
- `crates/polyplug/src/runtime.rs:878-904`
- `crates/polyplug/src/ffi.rs:243-249`

**Step 1: Change Registry API**
```rust
// BEFORE (registry.rs:372):
pub fn find_all_by_contract(&self, contract_id: u64, min_version: u32) -> Vec<PluginHandle> {

// AFTER:
pub fn find_all_by_contract(
    &self,
    contract_id: u64,
    min_version: u32,
    out: &mut [PluginHandle],
) -> usize {
    // Write into out buffer, return count written
}
```

**Step 2: Update Runtime callback**
```rust
// BEFORE (runtime.rs:878-904):
pub(crate) unsafe extern "C" fn host_find_all_by_contract(
    contract_id: u64, min_version: u32, out: *mut PluginHandle, out_cap: usize,
) -> usize {
    let handles: Vec<PluginHandle> = registry.find_all_by_contract(contract_id, min_version);
    // ... copy from Vec to out ...
}

// AFTER:
pub(crate) unsafe extern "C" fn host_find_all_by_contract(
    contract_id: u64, min_version: u32, out: *mut PluginHandle, out_cap: usize,
) -> usize {
    let registry = /* get registry */;
    // SAFETY: out is valid for out_cap elements per ABI contract.
    let out_slice: &mut [PluginHandle] = std::slice::from_raw_parts_mut(out, out_cap);
    registry.find_all_by_contract(contract_id, min_version, out_slice)
}
```

**Step 3: Update FFI layer**
```rust
// BEFORE (ffi.rs:243-249):
let handles: Vec<PluginHandle> = runtime.0.find_all_by_contract(contract_id, min_version);

// AFTER:
// Directly use the callback which now handles the buffer properly
```

**Verification:**
```bash
cargo test --package polyplug --test integration_dispatch
```

---

## Wave 2: High Priority Fixes (13 tasks)

### Fix H1-H5 — Move use statements to module level

**Priority:** High  
**Effort:** Small (< 1 hour total)  
**Category:** Category 8 (AGENTS.md violations)

**Files to fix:**

1. **graph.rs:377-379**
```rust
// BEFORE (inside fn from_manifests_chain_order):
fn from_manifests_chain_order() {
    use crate::loader::manifest::ManifestData;
    use crate::loader::manifest::RawManifestDependency;
    use std::path::PathBuf;

// AFTER: Move to module level
#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::manifest::{ManifestData, RawManifestDependency};
    use std::path::PathBuf;
    // ... existing test code ...
}
```

2. **graph.rs:474-477** — Same pattern
3. **runtime.rs:1005** — Same pattern
4. **abi.rs:562** — Same pattern
5. **csharp.rs:729, 762** — Same pattern

---

### Fix H6-H7 — Add ReadFailed variant and update usages

**Priority:** High  
**Effort:** Small (< 30 min)  
**Category:** Category 5 (Misaligned concepts)

**Step 1: Add variant (error.rs)**
```rust
#[error("failed to read file `{path}`: {source}")]
ReadFailed {
    path: String,
    #[source]
    source: std::io::Error,
},
```

**Step 2: Update parser.rs**
```rust
// Line 147, 166, 189: Change WriteFailed to ReadFailed
let content: String = std::fs::read_to_string(path).map_err(|e| CodegenError::ReadFailed {
    path: path.to_string_lossy().into_owned(),
    source: e,
})?;
```

---

### Fix H8 — Add explicit type annotation

**Priority:** High  
**Effort:** Trivial (< 5 min)  
**Category:** Category 8 (AGENTS.md violations)

**File:** `crates/polyplug_python/src/context.rs:34`

```rust
// BEFORE:
let ver = py.version_info();

// AFTER:
let ver: pyo3::types::PyTuple = py.version_info();
```

---

## Wave 3: Medium Priority (1 task)

### Fix M1 — Replace silent version defaults with explicit errors

**Priority:** Medium  
**Effort:** Small (< 30 min)  
**Category:** Category 6 (Error handling)

**File:** `crates/polyplug_dotnet/src/lib.rs:41-57`

```rust
// BEFORE:
let required_major: u32 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(10);

// AFTER:
let major_str: &str = parts.next().ok_or_else(|| LoaderError::InvalidFrameworkVersion {
    tfm: tfm.to_owned(),
    reason: "missing major version".to_owned(),
})?;
let required_major: u32 = major_str.parse().map_err(|_| LoaderError::InvalidFrameworkVersion {
    tfm: tfm.to_owned(),
    reason: format!("invalid major version: {major_str}"),
})?;
```

**Add new error variant:**
```rust
#[error("invalid .NET framework version in TFM `{tfm}`: {reason}")]
InvalidFrameworkVersion { tfm: String, reason: String },
```

---

## Wave 4: Low Priority (3 tasks)

### Fix L1 — Log registry registration errors

**Priority:** Low  
**Effort:** Trivial (< 10 min)  
**Category:** Category 6 (Error handling)

**File:** `crates/polyplug/src/loader/mod.rs:451-457`

```rust
// BEFORE:
Err(_err) => AbiError { code: 1, message: StringView::null() }

// AFTER:
Err(e) => {
    eprintln!("[polyplug] registration failed for bundle {bundle_id}: {e}");
    AbiError { code: 1, message: StringView::null() }
}
```

---

### Fix L2 — Fix misleading SAFETY comment

**Priority:** Low  
**Effort:** Trivial (< 5 min)  
**Category:** Category 1 (unsafe blocks)

**File:** `crates/polyplug_python/src/lib.rs:189-191`

```rust
// BEFORE:
// SAFETY: bundle_path_static outlives this call; leaked intentionally.

// AFTER:
// NOTE: Intentionally leaked; bundle_path_static outlives this call.
```

---

### Fix L3 — Standardize terminology

**Priority:** Low  
**Effort:** Small (< 30 min)  
**Category:** Category 5 (Misaligned concepts)

**File:** `crates/polyplug/src/loader/mod.rs` (various comments)

Search for "plugin bundle" and replace with "bundle" where redundant.

---

## Implementation Order

### Phase 1: Safety & Correctness (C1, C4-C5)
1. Remove unnecessary Registry Send/Sync impls
2. Refactor find_all_by_contract to eliminate allocation

### Phase 2: Stability (C2-C3)
3. Replace all panics in js-deno loader
4. Replace panic in js loader

### Phase 3: Code Quality (H1-H8)
5. Move use statements to module level
6. Add ReadFailed variant
7. Add explicit type annotation

### Phase 4: Polish (M1, L1-L3)
8. Replace silent version defaults
9. Log registration errors
10. Fix documentation

---

## Verification Commands

After all fixes:
```bash
# Full check
cargo check --workspace

# Run tests
cargo test --workspace

# Clippy with all features
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --check
```

---

## Summary

| Wave | Fixes | Effort Estimate | Critical Path |
|------|-------|-----------------|---------------|
| 1 (Critical) | 5 | 4-7 hours | Yes |
| 2 (High) | 8 | 2-3 hours | No |
| 3 (Medium) | 1 | < 1 hour | No |
| 4 (Low) | 3 | < 1 hour | No |
| **Total** | **17** | **~8-12 hours** | — |

---

*End of Fix Plan*
