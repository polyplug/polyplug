---
title: "Project-Wide SDK Fix Plan"
priority: critical
created: 2026-04-13
status: pending
scope: all 5 SDKs + codegen + test fixtures
---

# polyplug Project-Wide SDK Fix Plan

All 5 parallel audits are complete. This plan fixes every CRITICAL, HIGH, and MEDIUM issue found. WARNINGS are documented separately in `WARNINGS.md`.

## Root Causes

Three root causes produce most of the issues:

1. **Codegen bugs** — Python CFUNCTYPE return type not stripped of Rust path; JS `type_size()` doesn't know named type sizes
2. **Host files reference stale type names** — `NativeMethods.HostInterface` (C#), `ReloadPhaseFfi` (C#), `StringViewC` (C#), `HostRuntimeConfig` (Lua) — these are old names that should use auto-generated types
3. **C++ host has multiple functional bugs** — Builder ignores config, wrong handle size, wrong Array layout, noexcept+throw UB

---

## Wave 1: Codegen Fixes

These must land first because Wave 2+ depends on regenerated SDK files.

### Task 1.1: Fix Python CFUNCTYPE return type path stripping

**File:** `crates/polyplug_codegen/src/languages/python.rs`
**Bug:** Line that generates CFUNCTYPE for `HostInterface.get_host_contract` emits `crate::host::HostContractInstance` as the return type instead of `HostContractInstance`. The `rsplit("::")` fix from CR-01 only strips paths in struct field types, not in function pointer return types.

**Fix:** In the Python codegen, ensure ALL type names (including CFUNCTYPE return types and parameter types) go through the same path-stripping logic that strips `crate::module::` prefixes. The fix should:
- Find where CFUNCTYPE return types are emitted
- Apply the same `rsplit("::").last()` stripping that struct fields use
- Test: after fix, rebuild and verify `sdks/python/abi/abi.py` has no `crate::` references

### Task 1.2: Fix JS `type_size()` to know named type sizes

**File:** `crates/polyplug_codegen/src/languages/js.rs`
**Bug:** The `type_size()` function defaults to `8` for unknown types. This produces wrong offsets for structs containing `Version` (12 bytes), `StringView` (16 bytes), or enums like `AbiErrorCode`/`DispatchType`/`ReloadPhaseType` (4 bytes as `#[repr(u32)]`).

**Wrong offsets (found by audit):**
- `GuestContractInterface.DISPATCH_TYPE` = 16, should be 20
- `HostContractInterface.SINGLETON` = 16, should be 20
- `PluginDescriptor.CONTRACT_NAME` = 8, should be 16
- `PluginDescriptor.VERSION` = 16, should be 32
- `ReloadPhase.REASON` = 24, should be 32

**Fix:** Add a size lookup table for named types, matching the `KNOWN_SIZES` already in `generate.rs`:
```
"StringView" -> (16, 8)    // (size, align)
"Version"    -> (12, 4)
"Buffer"     -> (24, 8)
"AbiError"   -> (24, 8)
"Array"      -> (24, 8)
// All #[repr(u32)] enums -> (4, 4):
"AbiErrorCode", "DispatchType", "Compatibility", "ReloadPhaseType",
"ContractType", "RuntimeLanguage", "ParseVersionError" -> (4, 4)
```
Also fix `type_align()` correspondingly.

**Test:** After fix, rebuild and compare JS offsets against Rust `offset_of!` values for all structs.

### Task 1.3: Rebuild all SDK files

After Tasks 1.1 and 1.2, run `cargo build -p polyplug_abi` to regenerate all SDK files. Verify:
- `sdks/python/abi/abi.py` — zero `crate::` references
- `sdks/js/abi/abi.ts` — correct offset constants (check PluginDescriptor, ReloadPhase, HostContractInterface, GuestContractInterface)
- All 5 SDK files pass their size assertions

---

## Wave 2: C# Host Rewrite

### Task 2.1: Fix C# Runtime.cs type references

**File:** `sdks/csharp/host/Runtime.cs`
**Current broken references:**
- Line 16: `NativeMethods.HostInterface` → should be `Polyplug.Abi.HostInterface` (auto-generated)
- Line 126: `NativeMethods.ReloadPhaseFfi` → should be `Polyplug.Abi.ReloadPhase` (auto-generated)
- Line 138-144: `ConvertReloadPhase(NativeMethods.ReloadPhaseFfi)` → direct use of `Polyplug.Abi.ReloadPhase`
- Line 147: `NativeMethods.StringViewC` → should be `Polyplug.Abi.StringView` (auto-generated)
- Line 161: `ReloadCallbackNative(NativeMethods.ReloadPhaseFfi)` → use `Polyplug.Abi.ReloadPhase`
- Line 240: `NativeMethods.HostInterface` → `Polyplug.Abi.HostInterface`

**Additional fixes needed in Runtime.cs:**
- `OnReload` uses separate FFI export `polyplug_runtime_on_reload` — should use `RuntimeConfig.on_reload` field via `polyplug_runtime_create_with_options` like all other SDKs
- `ThrowLastError` creates+destroys a temporary runtime just to read global error — refactor to use a static error-reading approach or accept an existing host pointer
- `ReloadPhase` conversion has `RetryCount` field — auto-generated `ReloadPhase` doesn't have retry_count. Remove it.
- `StringViewToString` helper should work with `Polyplug.Abi.StringView` not `NativeMethods.StringViewC`

**NativeMethods.cs** is clean — it only has P/Invoke declarations. No changes needed there.

### Task 2.2: Fix C# ReloadPhase handling

The auto-generated `ReloadPhase` struct (in `Abi.cs`) has:
```csharp
public ReloadPhaseType PhaseType;
public ulong BundleId;
public StringView BundleName;
public StringView Reason;
```

There is no `RetryCount` field. The `OnReloadNative` callback should receive the auto-generated `ReloadPhase` directly (it's a C-compatible struct). Rewrite the callback path:
- Remove `ConvertReloadPhase` method entirely
- Remove `ReloadPhaseFfi` type usage
- The native callback receives `Polyplug.Abi.ReloadPhase` by value
- Convert `StringView` fields to strings using a helper that works with `Polyplug.Abi.StringView`

---

## Wave 3: C++ Host Fixes

### Task 3.1: Fix C++ Builder::build() to actually use options

**File:** `sdks/cpp/host/polyplug/runtime.hpp`

**Current bug (lines 67-83):** Both if/else branches call `polyplug_runtime_create()` identically. Config and callback are collected but discarded.

**Fix:** When `config_.has_value() || on_reload_cb_.has_value()`:
1. Build a `RuntimeConfig` struct from the stored config
2. Build a `RuntimeCreateOptions` struct wrapping config + callback
3. Call `polyplug_runtime_create_with_options(&options)`
4. Store the callback in a static/thread-local for the C ABI callback trampoline

This requires:
- Defining `RuntimeCreateOptions` (or using the ABI struct directly from abi.hpp)
- A static callback trampoline that dispatches to the stored `std::function`

### Task 3.2: Fix C++ noexcept/throw UB

**Current bug:** `find_guest_contract`, `resolve_guest_contract`, `find_all_guest_contracts` are `const noexcept` but call `ensure_host()` which throws `std::runtime_error`.

**Fix:** Remove `noexcept` from all methods that call `ensure_host()`. These should be:
- `load_bundle` — remove noexcept (already not noexcept, verify)
- `reload_bundle` — remove noexcept
- `find_guest_contract` — remove `noexcept`
- `find_all_guest_contracts` — remove `noexcept` (already not, verify)
- `get_last_error` — remove noexcept

### Task 3.3: Fix C++ GuestContractHandle size mismatch

**Current bug:** `find_guest_contract` returns `uint64_t` and casts fn pointer as `uint64_t(*)(const HostInterface*, uint64_t, uint32_t)`. But `HostInterface.find_guest_contract` returns `GuestContractHandle` which is 4 bytes (a single `u32` index). The C ABI passes a 4-byte return value, but the C++ code expects 8 bytes.

**Fix:**
- `find_guest_contract` should return `GuestContractHandle` (from abi.hpp)
- Cast fn pointer with correct return type: `GuestContractHandle(*)(const HostInterface*, uint64_t, uint32_t)`
- All callers that unpack handles need to work with `GuestContractHandle` not `uint64_t`

### Task 3.4: Fix C++ Array struct to use ABI Array (3 fields)

**Current bug (lines 168-171):** Hand-written `ArrayResult { uint64_t* ptr; size_t len; }` — only 2 fields (16 bytes). The actual ABI `Array` struct has 3 fields: `{ void* items; size_t len; size_t align }` (24 bytes).

**Fix:** Use the auto-generated `Array` struct from `abi.hpp`:
```cpp
auto func = reinterpret_cast<const Array*(*)(const HostInterface*, uint64_t, uint32_t)>(host_->find_all_guest_contracts);
const Array* arr = func(host_, contract_id, min_version);
```
Then iterate `arr->items` for `arr->len` entries. Free using `arr->len * sizeof(uint64_t)` and `arr->align` (or `alignof(uint64_t)`).

### Task 3.5: Remove C++ no-op and deprecated methods

- **`set_config`** (line 258-261): Empty body with misleading comment. Delete entirely or mark `[[deprecated]]` with clear message.
- **`release_plugin`** (line 252-256): Silent no-op. Delete or mark `[[deprecated]]`.
- **`find_by_bundle`** (line 157-161): Returns UINT64_MAX. Delete or mark `[[deprecated]]`.
- **`resolve_plugin`** (line 246-248): Alias for `resolve_guest_contract`. Mark `[[deprecated]]`.

### Task 3.6: Fix C++ POLYPLUG_GUEST_MAIN macro — add BundleInitContext

**File:** `sdks/cpp/guest/polyplug/guest.hpp`
**Current:** `extern "C" AbiError polyplug_init(const HostInterface* host)` — missing `ctx` parameter.
**Fix:** `extern "C" AbiError polyplug_init(const HostInterface* host, const BundleInitContext* ctx)` — matches Rust ABI.

Also fix stale comments:
- Line 16: `registrar->register_plugin` → `host->register_contract`
- Line 157: same fix

---

## Wave 4: Lua Host — Remove Hand-Written RuntimeConfig

### Task 4.1: Replace HostRuntimeConfig with auto-generated RuntimeConfig

**File:** `sdks/lua/host/polyplug/runtime.lua`

**Current (lines 26-34):** Hand-written 24-byte `HostRuntimeConfig` with fields `hot_reload_max_retries`, `hot_reload_retry_interval_ms`, `hot_reload_abort_on_max_retries` that don't exist in the current ABI.

**Fix:**
1. Remove the entire `HostRuntimeConfig` typedef from `ffi.cdef` (lines 26-34)
2. Use the auto-generated `RuntimeConfig` from `abi.lua` (which is 16 bytes: `compatibility`, `hot_reload_enabled`, `on_reload`)
3. Build `RuntimeCreateOptions` using `RuntimeConfig` from abi
4. The `RuntimeCreateOptions` wrapper struct (lines 46-49) should reference the ABI `RuntimeConfig` instead
5. Remove the `ReloadPhaseCallback` typedef (lines 36-44) — use the ABI `ReloadPhase` callback from the auto-generated code, or use the `RuntimeConfig.on_reload` function pointer directly

---

## Wave 5: Test Fixture Fixes

### Task 5.1: Fix C# test fixture

**File:** `tests/fixtures/csharp_plugin/Plugin.cs`
**Broken references:**
- `hostVTable->RegisterPlugin` → `host->RegisterContract`
- `s_interface.RtCtx` → remove (GuestContractInterface has no RtCtx field)
- `PluginDispatch` → `DispatchMechanisms` (from auto-generated ABI)
- `GuestContractInterface` fields should match auto-generated layout: `ContractId`, `ContractVersion`, `DispatchType`, `CreateInstance`, `DestroyInstance`, `Dispatch`

### Task 5.2: Fix Python test fixture

**File:** `tests/fixtures/test_plugin_python/test_plugin.py`
**Issues:**
- Lines 129-141: Hand-written `HostInterface` struct with `register_plugin` field — remove and import from `polyplug_abi`
- Uses `register_plugin` — should use `register_contract`
- `polyplug_init` takes 3 args (`rt_ctx, host_vtable, ctx_ptr`) — should match root fixture signature (2 args: `host_addr, ctx_ptr`)

### Task 5.3: Fix Python RuntimeConfig test

**File:** `sdks/python/host/tests/test_runtime_config_c.py`
**Issue:** Asserts `RuntimeConfig` is 24 bytes with fields `hot_reload_max_retries`, `hot_reload_retry_interval_ms`, `hot_reload_abort_on_max_retries`.
**Fix:** Update to assert 16 bytes with correct fields: `compatibility`, `hot_reload_enabled`, `on_reload`.

### Task 5.4: Fix test_plugin.py contract_version

**File:** `tests/fixtures/test_plugin.py`
**Issue:** Line 123 sets `contract_version = (0 << 16) | 0 = 0`. Should be a real version like `(1 << 16) | 0` for 1.0.0.

---

## Wave 6: Stale Naming and Documentation

### Task 6.1: Fix JS guest register_plugin → register_contract

**File:** `sdks/js/guest/polyplug_guest.js`
**Issue:** Line 116 JSDoc has `register_plugin` — should be `register_contract`.

### Task 6.2: Fix all SDK READMEs

The following READMEs use `registrar` variable/object in examples, implying the old `PluginRegistrar` pattern:
- `sdks/csharp/README.md` — line 64: `registrar.Register<IPipelineDecoder>`
- `sdks/lua/README.md` — line 50: `registrar.register()`
- `sdks/js/README.md` — line 56: `registrar.register()`
- `sdks/cpp/README.md` — line 60: `registrar.Register<>()`
- `sdks/rust/guest/README.md` — lines 241, 248, 494: `registrar` variable; lines 231-262: wrong struct layouts

**Fix:** Update all examples to show direct `HostInterface` usage:
```
host.register_contract(host, &descriptor, &interface)
```
Remove any `registrar` object pattern.

### Task 6.3: Fix Rust guest README struct layouts

**File:** `sdks/rust/guest/README.md`
**Issues:**
- Lines 231-262: Documents `HostInterface` with 7 fields and wrong signatures. The actual `HostInterface` has 16+ fields.
- `GuestContractInterface` documented as `{ function_count, functions }` but actual is `{ contract_id, contract_version, dispatch_type, create_instance, destroy_instance, dispatch }`.
- `PluginDescriptor` documented with flat `version_major/minor/patch` but actual uses `Version { major, minor, patch }`.

**Fix:** Update struct documentation to match auto-generated ABI. Either copy from the actual generated output or reference the polyplug_abi docs.

---

## Execution Order

```
Wave 1 (codegen)  →  Wave 2 (C# host)  →  Wave 3 (C++ host)
                                           ↓
                        Wave 4 (Lua host)  →  Wave 5 (test fixtures)  →  Wave 6 (docs)
```

Waves 2, 3, and 4 can run in parallel after Wave 1 completes (they touch different files).
Waves 5 and 6 can also run in parallel after Waves 2-4.

## Verification

After all waves:
1. `cargo build -p polyplug_abi` — clean build with zero warnings
2. `cargo test -p polyplug_abi` — all 58 tests pass
3. Grep for `crate::` in `sdks/*/abi/*` — zero results
4. Grep for `PluginRegistrar\|register_plugin\|RegisterPlugin\|ReloadPhaseFfi\|StringViewC\|HostRuntimeConfig\|PluginDispatch` in `sdks/` — zero results (excluding .planning/)
5. Verify JS offsets match Rust `offset_of!` for all structs
6. Verify C# compiles without errors
7. Verify all test fixtures use current ABI types
