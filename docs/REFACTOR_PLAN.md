# Polyplug Runtime Refactoring Plan

## Executive Summary

This plan outlines comprehensive changes to the `crates/polyplug` crate to fix the ABI inconsistencies, rename functions for clarity, and establish a clean foundation for the polyplug runtime.

**Scope:** ONLY `crates/polyplug` - other crates will be updated later.
**Validation:** `cargo build -p polyplug && cargo test -p polyplug`

---

## 1. Goals

1. **Fix ABI inconsistency:** All `polyplug_init` functions use 2 parameters (registrar, ctx)
2. **Rename host functions:** All host exports use `polyplug_runtime_` prefix
3. **Add negotiation support:** Include `host_abi_version` in `PluginContext`
4. **Remove dead code:** Delete legacy functions from lib.rs
5. **Improve naming:** Make guard/vtable operations clearer

---

## 2. ABI Architecture Overview

### Plugin Exports (Guest → Host)

Plugins are `.so`/`.dll` files that export:

```c
// ABI version sentinel
uint32_t polyplug_abi_version(void);

// Plugin initialization with negotiation
AbiError polyplug_init(
    PluginRegistrar* registrar,
    const PluginContext* ctx
);
```

### Host Exports (libpolyplug.so → Application)

```c
// Runtime lifecycle
OpaqueRuntime* polyplug_runtime_create(void);
void polyplug_runtime_destroy(OpaqueRuntime* rt);

// Plugin loading
uint32_t polyplug_runtime_load_bundle(
    OpaqueRuntime* rt,
    const uint8_t* path,
    size_t path_len
);

// Plugin discovery
uint64_t polyplug_runtime_find_by_contract(
    OpaqueRuntime* rt,
    uint64_t contract_id,
    uint32_t min_version
);

// Plugin resolution
OpaquePluginGuard* polyplug_runtime_resolve_plugin(
    OpaqueRuntime* rt,
    uint64_t packed_handle
);
const void* polyplug_runtime_plugin_vtable(OpaquePluginGuard* guard);
void polyplug_runtime_plugin_release(OpaquePluginGuard* guard);

// Error handling
size_t polyplug_runtime_last_error(uint8_t* buf, size_t buf_len);
size_t polyplug_runtime_error_message_len(void);

// Note: Extensions accessed through HostVTable (fast path)
// No C export - plugins use: (*registrar.host).get_extension(extension_id)
```

---

## 3. Detailed Changes

### 3.1 Type Renames

| Current | New | Location |
|---------|-----|----------|
| `OpaqueGuard` | `OpaquePluginGuard` | `ffi.rs` |
| `OpaqueRuntime` | `OpaqueRuntime` | (unchanged) |

### 3.2 Function Renames (ffi.rs)

| Current | New | Purpose |
|---------|-----|---------|
| `polyplug_runtime_new` | `polyplug_runtime_create` | Create runtime instance |
| `polyplug_runtime_free` | `polyplug_runtime_destroy` | Destroy runtime instance |
| `polyplug_load_bundle` | `polyplug_runtime_load_bundle` | Load plugin bundle |
| `polyplug_reload_bundle` | `polyplug_runtime_reload_bundle` | Hot-reload bundle |
| `polyplug_rt_find_by_contract` | `polyplug_runtime_find_by_contract` | Find by contract ID |
| `polyplug_rt_find_by_bundle` | `polyplug_runtime_find_by_bundle` | Find by bundle + contract |
| `polyplug_rt_find_all_by_contract` | `polyplug_runtime_find_all_by_contract` | Find all matching |
| `polyplug_rt_resolve_plugin` | `polyplug_runtime_resolve_plugin` | Resolve handle to guard |
| `polyplug_guard_free` | `polyplug_runtime_plugin_release` | Release plugin guard |
| `polyplug_get_vtable` | `polyplug_runtime_plugin_vtable` | Get vtable from guard |
| `polyplug_last_error` | `polyplug_runtime_last_error` | Get error message |
| `polyplug_error_message_len` | `polyplug_runtime_error_message_len` | Get error length |
| `polyplug_get_extension` | **REMOVED** | Use HostVTable fast path instead (your decision: Option A) |
| `polyplug_runtime_register_loader` | `polyplug_runtime_register_loader` | (unchanged, already conforming) |

### 3.3 PluginContext Update (abi.rs)

**Add field to `PluginContext`:**

```rust
#[repr(C)]
pub struct PluginContext {
    /// Absolute canonical path to the directory containing the loaded bundle.
    pub bundle_path: StringView,
    
    /// Host's supported ABI version for negotiation (Option C).
    /// Plugin can use this to determine available features.
    /// Set to POLYPLUG_ABI_VERSION by host.
    pub host_abi_version: u32,
}
```

**Purpose:** Enables forward/backward compatibility negotiation (Option C).

**Breaking Change:** Old PluginContext was 16 bytes, new is 24 bytes. Old plugins will read garbage for host_abi_version field. This is a **breaking ABI change**.

**Required Test Updates:**
- Update `layout_plugin_context` test (line ~564) for new size (24) and offset (16 for host_abi_version)

**Usage in plugin:**
```rust
pub unsafe extern "C" fn polyplug_init(
    registrar: *mut PluginRegistrar,
    ctx: *const PluginContext
) -> AbiError {
    let host_version = (*ctx).host_abi_version;
    
    // Negotiate: plugin adapts to host capabilities
    if host_version >= 2 {
        // Host supports new features
    } else {
        // Use legacy mode for backward compatibility
    }
    
    // Register vtables...
    AbiError::ok()
}
```

**Note:** This is one-way negotiation (host→plugin). Plugin doesn't report its version back - it just adapts to what the host supports.

### 3.4 Dead Code Removal (lib.rs)

**Remove these functions from `crates/polyplug/src/lib.rs`:**

| Function | Reason |
|----------|--------|
| `polyplug_runtime_init` | Dead, replaced by `polyplug_runtime_create` in ffi.rs |
| `polyplug_runtime_destroy` | **CRITICAL:** Must be deleted BEFORE ffi.rs rename to avoid symbol collision (both are #[no_mangle]) |
| `polyplug_find_by_contract` | Global state version, use ffi.rs `_rt_` version |
| `polyplug_find_by_bundle` | Global state version, use ffi.rs `_rt_` version |
| `polyplug_find_all_by_contract` | Global state version, use ffi.rs `_rt_` version |
| `polyplug_resolve_plugin` | Global state version, use ffi.rs `_rt_` version |
| `polyplug_get_extension` | **Per your decision:** Use HostVTable only (Option A), no C export |

**Keep in lib.rs:**
- `polyplug_abi_version()` - Used by tests (convenience)

**Note:** `polyplug_get_extension()` is **REMOVED** per your decision - extensions accessed ONLY through HostVTable fast path, no C export.

### 3.5 Loader Updates (loader/mod.rs)

**Update `PluginContext` creation:**

```rust
let ctx: crate::abi::PluginContext = crate::abi::PluginContext {
    bundle_path: bundle_path_sv,
    host_abi_version: POLYPLUG_ABI_VERSION, // NEW FIELD
};
```

**Update documentation comments:**
- Clarify the 2-parameter `polyplug_init` signature
- Document `host_abi_version` negotiation

---

## 4. Files to Modify

### Primary Changes:

1. **`crates/polyplug/src/ffi.rs`**
   - Rename all functions with `polyplug_runtime_` prefix
   - Rename `OpaqueGuard` to `OpaquePluginGuard`
   - Update all doc comments

2. **`crates/polyplug/src/abi.rs`**
   - Add `host_abi_version: u32` to `PluginContext`
   - Update documentation

3. **`crates/polyplug/src/loader/mod.rs`**
   - Update `PluginContext` construction to include `host_abi_version`
   - Update comments for 2-param `polyplug_init`

4. **`crates/polyplug/src/lib.rs`**
   - Remove dead legacy functions
   - Keep only `polyplug_abi_version`
   - Remove `polyplug_get_extension` (per your decision: Option A, HostVTable only)
   - **CRITICAL:** Delete `polyplug_runtime_destroy` from lib.rs BEFORE ffi.rs rename to avoid symbol collision

5. **`crates/polyplug/src/runtime.rs`**
   - Update documentation for new naming

6. **`crates/polyplug/tests/integration_extension.rs`**
   - Update tests that call removed `polyplug_get_extension` function

### Documentation Updates:

6. **`docs/ABI_ARCHITECTURE.md`** (already written)
   - Update with new function names
   - Fix guard/vtable names: `polyplug_runtime_guard_free` → `polyplug_runtime_plugin_release`
   - Fix guard/vtable names: `polyplug_runtime_get_vtable` → `polyplug_runtime_plugin_vtable`
   - Fix type name: `OpaqueGuard` → `OpaquePluginGuard`
   - Add negotiation documentation

7. **`docs/REFACTOR_PLAN.md`** (this file)
   - Final plan document

---

## 5. Validation Criteria

### Build:
```bash
cargo build -p polyplug
cargo check -p polyplug
```

### Tests:
```bash
cargo test -p polyplug --lib
cargo test -p polyplug --test integration_load
cargo test -p polyplug --test integration_context
```

### No Warnings:
```bash
cargo check -p polyplug 2>&1 | grep -c "warning" || echo "0 warnings"
```

---

## 6. Breaking Changes

This is a **major breaking ABI change** affecting:

1. **Host API:** All function names changed to `polyplug_runtime_*` prefix
2. **Plugin ABI:** `polyplug_init` now takes 2 parameters
3. **PluginContext:** Size changed from 16 → 24 bytes (added `host_abi_version`)
4. **Extensions:** No C export, only HostVTable access (your decision: Option A)

**Breaking: PluginContext Layout**
- Old plugins compiled against 16-byte PluginContext will read garbage for `host_abi_version`
- This is an **ABI break** requiring plugin recompilation

**Migration for host apps:**
```c
// OLD
OpaqueRuntime* rt = polyplug_runtime_new();
polyplug_load_bundle(rt, path, len);

// NEW
OpaqueRuntime* rt = polyplug_runtime_create();
polyplug_runtime_load_bundle(rt, path, len);
```

**Migration for plugins:**
```c
// OLD
AbiError polyplug_init(PluginRegistrar* registrar)

// NEW
AbiError polyplug_init(PluginRegistrar* registrar, const PluginContext* ctx)
```

**Migration for extensions:**
```c
// OLD (C export)
const void* ext = polyplug_get_extension(extension_id);

// NEW (HostVTable fast path - your decision: Option A)
const void* ext = (*(*registrar).host).get_extension(extension_id);
```

---

## 7. Future Work (Post-Refactor)

1. Update `crates/polyplug_codegen` generators to use new names
2. Update `examples/hosts/` to use new API
3. Update `examples/guests/` to use 2-param init
4. Update test fixtures in `tests/fixtures/`
5. Update guest libraries (`guest-libs/`, `host-libs/`)
6. Update documentation (`guest-libs/*/README.md`, `docs/`)

---

## 8. Implementation Order (CRITICAL)

To avoid symbol collisions and build failures:

1. **First:** Delete `polyplug_runtime_destroy` from `lib.rs`
2. **Then:** Rename `polyplug_runtime_free` → `polyplug_runtime_destroy` in `ffi.rs`
3. **Then:** Remove other dead functions from `lib.rs`
4. **Finally:** Update all other files

## 9. Summary

This refactor implements your decisions:
1. **Consistent naming:** All host functions use `polyplug_runtime_` prefix
2. **Clear types:** `OpaquePluginGuard` instead of `OpaqueGuard`
3. **ABI negotiation (Option C):** `host_abi_version` in PluginContext
4. **Extensions (Option A):** HostVTable only, NO C export
5. **Clean API:** Removal of dead legacy functions per your requirements

**Fixed per your review:**
- ✅ Removed `polyplug_get_extension` from keep list
- ✅ Added symbol collision warning for `polyplug_runtime_destroy`
- ✅ Noted ABI_ARCHITECTURE.md needs guard/vtable name fixes
- ✅ Added `polyplug_runtime_register_loader` to table
- ✅ Added test update for PluginContext layout
- ✅ Clarified breaking changes including layout change
- ✅ Added implementation ordering to avoid collisions

**Ready to proceed with implementation?**
