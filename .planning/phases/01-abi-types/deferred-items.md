# Deferred Items - Plan 01-06

Items discovered during execution that are out of scope per deviation rules.

## Out-of-Scope PluginContractId Usage

### 1. compatibility/mod.rs (test code only)

**Location:** `crates/polyplug/src/compatibility/mod.rs` lines 20, 128, 129, 218

**Issue:** Test code imports and uses `PluginContractId` directly. While this still works via backward compatibility alias (`pub type PluginContractId = GuestContractId`), it triggers deprecation warnings.

**Why deferred:** Not in plan's `files_modified` frontmatter. Plan explicitly listed only 4 files. Test-only usage, lower priority than production code.

**Recommendation:** Include in future gap closure plan or address during SDK cleanup phase.

---

## Pre-Existing Build Errors (Unrelated to Task Changes)

### 2. polyplug_abi deprecated PluginContractId

**Location:** `crates/polyplug_abi/src/plugin/plugin_interface.rs`

**Issue:** Import and usage of deprecated `PluginContractId` in `GuestContractInterface` struct.

**Why deferred:** Different crate (polyplug_abi), not in polyplug crate scope. Pre-existing issue unrelated to this plan's changes.

### 3. BundleId vs u64 mismatches in ffi.rs

**Location:** `crates/polyplug/src/ffi.rs` lines 81, 91, 102+ (multiple)

**Issue:** Type mismatches between `BundleId` and `u64` in FFI boundary code. Causes compilation errors.

**Why deferred:** Pre-existing issue, unrelated to GuestContractId/PluginContractId migration. Requires separate fix in FFI layer.

### 4. Overall polyplug crate build failure

**Issue:** `cargo build -p polyplug` fails with 23 errors due to pre-existing type mismatches (BundleId/u64) and other unrelated issues.

**Why deferred:** Plan's compilation criterion ("polyplug crate compiles without type errors") cannot be met due to pre-existing blockers unrelated to this task's changes. The 4 target files pass all acceptance criteria.

---

## Plan 07 Deferred Items (Added 2026-04-03)

After fixing bundle_id.id() conversion (lines 81, 91, 102), additional pre-existing errors remain:

### 5. RuntimeConfigC vs RuntimeConfig mismatch (ffi.rs lines 208-209)

**Issue:** `config_c.into_runtime_config()` returns RuntimeConfig, but builder expects RuntimeConfigC. Type conversion error between FFI C struct and Rust type.

**Why deferred:** Pre-existing type mismatch not caused by bundle_id.id() change. Not covered by gap closure plans 05-09.

### 6. HostContractInterface.header field access (ffi.rs line 594)

**Issue:** Code accesses `vtable_ref.header.contract_id` but HostContractInterface has no `header` field. Available fields: `contract_id`, `contract_version`, `singleton`, `dispatch_type`, `create_instance`, etc.

**Why deferred:** Pre-existing struct field access error. Requires code review to determine correct field path.

### 7. GuestContractId/BundleId missing serde traits (manifest.rs)

**Issue:** GuestContractId and BundleId lack `serde::Deserialize` and `Default` trait implementations. Required for manifest parsing with `#[serde(default)]` attributes.

**Why deferred:** Trait implementations needed in polyplug_utils crate. Not covered by gap closure plans.

### 8. plugin_registry.rs contract_id type mismatch (line 149)

**Issue:** `(*interface_ptr).contract_id` is GuestContractId, not u64. Expected type mismatch on extraction.

**Why deferred:** Pre-existing type mismatch. May need `.id()` call or type change.

---

## Notes

The backward compatibility alias `PluginContractId = GuestContractId` in polyplug_utils allows old code to continue functioning with deprecation warnings. This is intentional for smooth migration.