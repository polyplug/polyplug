# Hot-Reload Notification Implementation Plan

## Overview

Implement the hot-reload notification system that:
- Hides `PluginGuard` and `PluginVTable` from app developers
- Uses Arc reference counting for automatic instance tracking
- Provides three-phase notification (Preparing/Reloaded/Failed) with retry support
- Aborts reload after max retries (3) to prevent crashes
- Achieves zero overhead on the hot path

**Design Document:** `docs/HOT_RELOAD_DESIGN.md`

**Affected Components:**
- Runtime core (Rust)
- Codegen (Rust, C++, C#, Python, Lua, JavaScript)
- Host libs (C++, Python, C#, Lua, JavaScript)
- Guest libs (verification only)
- Tests
- Documentation

---

## Phase 1: Runtime Core (Rust)

**Blockers: None - can start immediately**

- [ ] Add `ReloadPhase` enum to `crates/polyplug/src/reload.rs`:
  - `Preparing { bundle_id: u64, bundle_name: String, retry_count: u32 }`
  - `Reloaded { bundle_id: u64, bundle_name: String }`
  - `Failed { bundle_id: u64, bundle_name: String, reason: String }`
  - Export from `crates/polyplug/src/lib.rs`

- [ ] Add `RuntimeConfig` struct to `crates/polyplug/src/runtime.rs`:
  - `hot_reload_max_retries: u32` (default: 3)
  - `hot_reload_retry_interval: Duration` (default: 1 second)
  - `hot_reload_abort_on_max_retries: bool` (default: true)
  - Implement `Default` trait
  - Extensible for future options (log_level, allocator, etc.)

- [ ] Update `ReloadCallback` type in `crates/polyplug/src/runtime.rs`:
  - Change from `Fn(ReloadEvent)` to `Fn(ReloadPhase)`
  - Update `on_reload_cb` field type

- [ ] Add `config` field to `Runtime` and `RuntimeBuilder`:
  - Store `RuntimeConfig` in `Runtime`
  - Add `RuntimeBuilder::config(config: RuntimeConfig)` method

- [ ] Modify `reload_bundle_impl` in `crates/polyplug/src/reload.rs`:
  - Use `RuntimeConfig` values instead of hardcoded constants
  - Fire `Preparing` notification BEFORE vtable swap
  - Add retry loop using `config.hot_reload_retry_interval`
  - Increment `retry_count` on each retry
  - After `config.hot_reload_max_retries`:
    - If `config.hot_reload_abort_on_max_retries`: abort and fire `Failed`
    - Else: keep retrying forever
  - On abort: call `emit_warning()`, fire `Failed`, return error without swap
  - Fire `Reloaded` notification AFTER vtable swap, BEFORE quiescence wait

- [ ] Add `on_reload` method to `RuntimeBuilder` in `crates/polyplug/src/runtime.rs`:
  - Accept callback: `impl Fn(ReloadPhase) + Send + Sync + 'static`
  - Store as `Option<ReloadCallback>`

- [ ] Update FFI layer in `crates/polyplug/src/ffi.rs`:
  - Add `polyplug_runtime_on_reload` function for C ABI
  - Add `polyplug_runtime_set_config` function for C ABI
  - Expose callback registration and config to host libs

- [ ] Add unit tests for `ReloadPhase` enum and notification flow:
  - Test successful reload with instance cleanup
  - Test retry mechanism (retry_count increments)
  - Test abort after max retries (Failed notification)
  - Test that old vtable is kept on abort
  - Test custom `RuntimeConfig` values

---

## Phase 2: Codegen - Rust Generator

**Blockers: Phase 1 complete**

- [ ] Update `crates/polyplug_codegen/src/generators/rust.rs`:
  - Modify `generate_host_caller_struct` to hide `PluginVTable` and `PluginGuard`
  - Add factory method `new(handle: PluginHandle, runtime: &'static Runtime) -> Option<Self>`
  - Store `PluginVTableGuard` internally (not exposed)
  - Make struct methods use hidden guard internally
  - Add `is_valid()` and `reset()` methods

- [ ] Update generated `host_callers.rs` output:
  - Remove `vtable()` method from public API
  - Add `impl Drop` if needed for explicit cleanup logging
  - Ensure move-only semantics (delete copy constructor/assignment)

- [ ] Add integration test for generated Rust host callers with hot-reload

---

## Phase 3: Codegen - C++ Generator

**Blockers: Phase 1 complete**
**[PARALLEL GROUP: CODEGEN-LANGUAGES]**

- [ ] Update `crates/polyplug_codegen/src/generators/cpp.rs`:
  - Modify `generate_cpp_host_contract` to hide `PluginVTable*`
  - Add factory method `static std::optional<ClassName> create(Runtime& rt, uint32_t min_version = 0)`
  - Store `PluginGuard` internally as private member
  - Remove `vtable()` from public API
  - Add `is_valid()`, `reset()`, and `explicit operator bool()`

- [ ] Update `generate_cpp_host_function`:
  - Use internal `guard_.vtable()` instead of passed vtable pointer
  - Handle error cases with exceptions or error codes

- [ ] Update generated `host_callers.hpp` output:
  - Include `<optional>` for factory method return type
  - Make class move-only (delete copy constructor/assignment)
  - Add proper RAII semantics

- [ ] Add integration test for generated C++ host callers with hot-reload

---

## Phase 4: Codegen - Python Generator

**Blockers: Phase 1 complete**
**[PARALLEL GROUP: CODEGEN-LANGUAGES]**

- [ ] Update `crates/polyplug_codegen/src/generators/python.rs`:
  - Modify `generate_host_caller_class` to hide vtable
  - Add factory method `@classmethod def create(cls, rt, min_version=0) -> Optional[Self]`
  - Store `PluginGuard` internally as `_guard`
  - Remove `get_vtable()` from public API
  - Add `is_valid()` and `reset()` methods

- [ ] Update generated `host_callers.py` output:
  - Use internal `_guard` for vtable access
  - Implement `__bool__` for truthiness checks
  - Add proper `__del__` for cleanup (optional, Python GC handles it)

- [ ] Add integration test for generated Python host callers with hot-reload

---

## Phase 5: Codegen - C# Generator

**Blockers: Phase 1 complete**
**[PARALLEL GROUP: CODEGEN-LANGUAGES]**

- [ ] Update `crates/polyplug_codegen/src/generators/csharp.rs`:
  - Modify `generate_cs_host_callers` to hide vtable
  - Add factory method `public static ClassName? Create(Runtime rt, uint minVersion = 0)`
  - Store `PluginGuard` internally as private field
  - Remove `GetVTable()` from public API
  - Implement `IDisposable` for explicit cleanup

- [ ] Update generated `HostCallers.cs` output:
  - Add `IDisposable` interface implementation
  - Make class properly disposable
  - Add `IsValid` property and `Reset()` method

- [ ] Add integration test for generated C# host callers with hot-reload

---

## Phase 6: Codegen - Lua Generator

**Blockers: Phase 1 complete**
**[PARALLEL GROUP: CODEGEN-LANGUAGES]**

- [ ] Update `crates/polyplug_codegen/src/generators/lua.rs`:
  - Modify `generate_host_caller_function` to hide vtable
  - Add factory function that returns instance table
  - Store guard internally in instance table
  - Remove vtable access from public API

- [ ] Update generated `host_callers.lua` output:
  - Use internal guard for vtable access
  - Add `is_valid()` and `reset()` methods
  - Proper metatable for OOP-style usage

- [ ] Add integration test for generated Lua host callers with hot-reload

---

## Phase 7: Codegen - JavaScript Generators

**Blockers: Phase 1 complete**
**[PARALLEL GROUP: CODEGEN-LANGUAGES]**

- [ ] Update `crates/polyplug_codegen/src/generators/js_deno.rs`:
  - Modify `generate_host_caller_class_deno` to hide vtable
  - Add factory method `static create(rt, minVersion = 0)`
  - Store guard internally as private field
  - Remove vtable access from public API

- [ ] Update `crates/polyplug_codegen/src/generators/js_quickjs.rs`:
  - Same changes as Deno generator
  - Adapt for QuickJS-specific patterns

- [ ] Update generated `host_callers.js` output:
  - Use internal guard for vtable access
  - Add `isValid()` and `reset()` methods
  - Proper class structure with private fields

- [ ] Add integration test for generated JavaScript host callers with hot-reload

---

## Phase 8: Host Libs - C++

**Blockers: Phase 3 complete**

- [ ] Update `host-libs/cpp/polyplug/runtime.hpp`:
  - Add `on_reload` method to `Runtime` class
  - Accept callback: `std::function<void(const ReloadPhase&)>`
  - Store callback internally
  - Add `set_config` method

- [ ] Add `host-libs/cpp/polyplug/runtime_config.hpp` (create new file):
  - Add `RuntimeConfig` struct with hot-reload options
  - Extensible for future options

- [ ] Update `host-libs/cpp/polyplug/abi.hpp`:
  - Add `ReloadPhase` struct with `type` field and `Type` enum
  - Simple struct: `type`, `bundle_id`, `bundle_name`, `retry_count`, `reason`

- [ ] Update `host-libs/cpp/polyplug/error.hpp`:
  - Add `ABI_ERROR_RELOADING` constant if not present

- [ ] Update `host-libs/cpp/polyplug/handle.hpp`:
  - Ensure `PluginGuard` is move-only
  - Add `reset()` method

- [ ] Update CMakeLists.txt if needed for new files

- [ ] Add unit tests for C++ host lib with reload notification:
  - Test `Preparing` handling with instance cleanup
  - Test `Reloaded` handling
  - Test `Failed` handling (reload aborted)
  - Test custom `RuntimeConfig`

---

## Phase 9: Host Libs - Python

**Blockers: Phase 4 complete**

- [ ] Update `host-libs/python/polyplug/runtime.py`:
  - Add `on_reload` method to `Runtime` class
  - Accept callback: `Callable[[ReloadPhase], None]`
  - Store callback internally
  - Add `set_config` method

- [ ] Add `host-libs/python/polyplug/runtime_config.py` (create new file):
  - Add `RuntimeConfig` dataclass with hot-reload options
  - Extensible for future options

- [ ] Update `host-libs/python/polyplug/abi.py` (or create if needed):
  - Add `ReloadPhase` class with `type` attribute and `TYPE_` constants
  - Simple class: `type`, `bundle_id`, `bundle_name`, `retry_count`, `reason`

- [ ] Update `host-libs/python/polyplug/__init__.py`:
  - Export `ReloadPhase`, `RuntimeConfig`

- [ ] Add unit tests for Python host lib with reload notification:
  - Test `Preparing` handling with instance cleanup
  - Test `Reloaded` handling
  - Test `Failed` handling (reload aborted)
  - Test custom `RuntimeConfig`

---

## Phase 10: Host Libs - C#

**Blockers: Phase 5 complete**

- [ ] Update `host-libs/csharp/Polyplug/src/Runtime.cs`:
  - Add `OnReload` method
  - Accept callback: `Action<ReloadPhase>`
  - Store callback internally
  - Add `SetConfig` method

- [ ] Add `host-libs/csharp/Polyplug/src/RuntimeConfig.cs` (create new file):
  - Add `RuntimeConfig` class with hot-reload options
  - Extensible for future options

- [ ] Add `host-libs/csharp/Polyplug/src/ReloadPhase.cs` (create new file):
  - Add `ReloadPhase` class with `Type` property and `ReloadPhaseType` enum
  - Simple class: `Type`, `BundleId`, `BundleName`, `RetryCount`, `Reason`

- [ ] Update `host-libs/csharp/Polyplug/src/PluginGuard.cs`:
  - Ensure proper `IDisposable` implementation
  - Add `Reset()` method

- [ ] Add unit tests for C# host lib with reload notification:
  - Test `Preparing` handling with instance cleanup
  - Test `Reloaded` handling
  - Test `Failed` handling (reload aborted)
  - Test custom `RuntimeConfig`

---

## Phase 11: Host Libs - Lua

**Blockers: Phase 6 complete**

- [ ] Update `host-libs/lua/polyplug/runtime.lua`:
  - Add `on_reload` method to Runtime table
  - Accept callback function
  - Store callback internally
  - Add `set_config` method

- [ ] Add `host-libs/lua/polyplug/runtime_config.lua` (create new file):
  - Add `RuntimeConfig` table with hot-reload options
  - Extensible for future options

- [ ] Add `host-libs/lua/polyplug/reload_phase.lua` (create new file):
  - Add `ReloadPhase` table with `type` field and `TYPE_` constants
  - Simple table: `type`, `bundle_id`, `bundle_name`, `retry_count`, `reason`

- [ ] Add unit tests for Lua host lib with reload notification:
  - Test `Preparing` handling with instance cleanup
  - Test `Reloaded` handling
  - Test `Failed` handling (reload aborted)
  - Test custom `RuntimeConfig`

---

## Phase 12: Host Libs - JavaScript

**Blockers: Phase 7 complete**

- [ ] Update `host-libs/js/polyplug/runtime.js`:
  - Add `onReload` method to Runtime class
  - Accept callback function
  - Store callback internally
  - Add `setConfig` method

- [ ] Add `host-libs/js/polyplug/runtime_config.js` (create new file):
  - Add `RuntimeConfig` class with hot-reload options
  - Extensible for future options

- [ ] Add `host-libs/js/polyplug/reload_phase.js` (create new file):
  - Add `ReloadPhase` class with `type` property and `TYPE_` constants
  - Simple class: `type`, `bundleId`, `bundleName`, `retryCount`, `reason`

- [ ] Add unit tests for JavaScript host lib with reload notification:
  - Test `Preparing` handling with instance cleanup
  - Test `Reloaded` handling
  - Test `Failed` handling (reload aborted)
  - Test custom `RuntimeConfig`

---

## Phase 13: Guest Libs Verification

**Blockers: None - can run in parallel with Phase 2-7**
**[PARALLEL GROUP: GUEST-VERIFICATION]**

- [ ] Verify `guest-libs/rust/` - no changes needed, vtable is static in plugin

- [ ] Verify `guest-libs/cpp/` - no changes needed, vtable is static in plugin

- [ ] Verify `guest-libs/csharp/` - no changes needed, vtable is static in plugin

- [ ] Verify `guest-libs/python/` - no changes needed, vtable is static in plugin

- [ ] Verify `guest-libs/lua/` - no changes needed, vtable is static in plugin

- [ ] Verify `guest-libs/js/` - no changes needed, vtable is static in plugin

---

## Phase 14: Integration Tests

**Blockers: Phase 1-12 complete**

- [ ] Create `tests/integration/hot_reload_test.rs`:
  - Test notification flow with mock host
  - Test retry mechanism (retry_count increments)
  - Test abort after max retries (Failed notification)
  - Test instance cleanup via Arc count
  - Test that old vtable is kept on abort

- [ ] Create `tests/integration/cpp/hot_reload_test.cpp`:
  - Test C++ host with reload notification
  - Test instance creation/destruction
  - Test retry handling
  - Test Failed notification handling

- [ ] Create `tests/integration/python/test_hot_reload.py`:
  - Test Python host with reload notification
  - Test instance creation/destruction
  - Test Failed notification handling

- [ ] Create `tests/integration/csharp/HotReloadTest.cs`:
  - Test C# host with reload notification
  - Test instance creation/destruction
  - Test Failed notification handling

- [ ] Update `examples/hosts/` to use new API:
  - Update `examples/hosts/rust/src/main.rs`
  - Update `examples/hosts/cpp/main.cpp`
  - Update `examples/hosts/python/host.py`
  - Update `examples/hosts/csharp/Program.cs`

---

## Phase 15: Documentation

**Blockers: Phase 1-14 complete**

- [ ] Update `docs/HOT_RELOAD_DESIGN.md` with final implementation details

- [ ] Update `host-libs/cpp/README.md` with new API usage

- [ ] Update `host-libs/python/README.md` with new API usage

- [ ] Update `host-libs/csharp/README.md` with new API usage

- [ ] Update `crates/polyplug_codegen/README.md` with codegen changes

- [ ] Update `AGENTS.md` if any conventions changed

---

## Phase 16: Final Verification

**Blockers: Phase 1-15 complete**

- [ ] Run `cargo test --workspace` - all tests pass

- [ ] Run `cargo clippy -- -D warnings` - zero warnings

- [ ] Run `cargo fmt --check` - formatting clean

- [ ] Build all example hosts and guests

- [ ] Run integration tests for all languages

- [ ] Manual verification: hot-reload with notification in example app

---

## Summary

| Phase | Component | Parallel Group | Blockers |
|-------|-----------|----------------|----------|
| 1 | Runtime Core | - | None |
| 2 | Codegen Rust | - | Phase 1 |
| 3 | Codegen C++ | CODEGEN-LANGUAGES | Phase 1 |
| 4 | Codegen Python | CODEGEN-LANGUAGES | Phase 1 |
| 5 | Codegen C# | CODEGEN-LANGUAGES | Phase 1 |
| 6 | Codegen Lua | CODEGEN-LANGUAGES | Phase 1 |
| 7 | Codegen JS | CODEGEN-LANGUAGES | Phase 1 |
| 8 | Host Lib C++ | - | Phase 3 |
| 9 | Host Lib Python | - | Phase 4 |
| 10 | Host Lib C# | - | Phase 5 |
| 11 | Host Lib Lua | - | Phase 6 |
| 12 | Host Lib JS | - | Phase 7 |
| 13 | Guest Libs | GUEST-VERIFICATION | None |
| 14 | Integration Tests | - | Phase 1-12 |
| 15 | Documentation | - | Phase 1-14 |
| 16 | Final Verification | - | Phase 1-15 |

**Total Tasks: 76**

**Key Design Decisions:**
- `RuntimeConfig` struct (general, extensible for future options):
  - `hot_reload_max_retries`: default 3 (host can change)
  - `hot_reload_retry_interval`: default 1 second (host can change)
  - `hot_reload_abort_on_max_retries`: default true (host can disable)
- `ReloadPhase` is a simple struct with `type` field - no complex patterns
- On abort: Fire `Failed` notification, keep old vtable, return error
- Logging: Use existing `on_warning` callback (not stdout/stderr)

**Estimated Effort:**
- Phase 1 (Runtime): 3-4 hours (includes config system)
- Phase 2-7 (Codegen): 4-6 hours (parallelizable)
- Phase 8-12 (Host Libs): 3-4 hours
- Phase 13 (Guest Libs): 1 hour
- Phase 14 (Tests): 2-3 hours
- Phase 15 (Docs): 1-2 hours
- Phase 16 (Verification): 1 hour

**Total: 15-21 hours**