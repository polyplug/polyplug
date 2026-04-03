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

## Notes

The backward compatibility alias `PluginContractId = GuestContractId` in polyplug_utils allows old code to continue functioning with deprecation warnings. This is intentional for smooth migration.