# C++ Host Lib Fix Plan

## Status: NEEDS FIXES

## Summary

The C++ host lib has several performance issues and architectural problems similar to what the C# host lib had before being fixed.

---

## Phase 1: Add PluginGuard with VTable Caching

**Blockers:** None  
**Parallel:** No

- [ ] Add `PluginGuard` class to `host-libs/cpp/polyplug/runtime.hpp` with vtable caching at construction
  - **Verification:** `PluginGuard` constructor calls `polyplug_runtime_plugin_vtable()` once and caches result; `vtable()` method returns cached pointer with no FFI call

- [ ] Add `resolve_plugin()` method to `Runtime` class that returns `PluginGuard` instead of raw handle
  - **Verification:** `Runtime::resolve_plugin(uint64_t packed_handle)` returns `PluginGuard`; compilation succeeds

---

## Phase 2: Update Codegen for Cached VTable Dispatch

**Blockers:** Phase 1 complete  
**Parallel:** No

- [ ] Update `generate_cpp_host_contract` in `crates/polyplug_codegen/src/generators/cpp.rs` to accept vtable pointer in constructor
  - **Verification:** Generated `ImageDecodeContract` constructor signature is `ImageDecodeContract(const PluginVTable* vtable)` instead of `(PluginHandle handle, const HostVTable* host)`

- [ ] Update `generate_cpp_host_function` to use cached vtable instead of calling `resolve_plugin` every call
  - **Verification:** Generated function body uses `vtable_->functions[fn_id]` directly; no call to `host_->resolve_plugin()`

- [ ] Run `cargo test --lib csharp` to verify codegen changes don't break existing tests
  - **Verification:** All codegen tests pass with exit code 0

---

## [PARALLEL GROUP: LOADER RESTRUCTURING]

**Blockers:** Phase 1 complete (loaders must not depend on PluginGuard changes)  
**Parallel:** Yes - all 6 loaders can be restructured in parallel

- [ ] Create `host-libs/cpp/loaders/native/` with `CMakeLists.txt` and `polyplug_loaders_native.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_native` target

- [ ] Create `host-libs/cpp/loaders/python/` with `CMakeLists.txt` and `polyplug_loaders_python.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_python` target

- [ ] Create `host-libs/cpp/loaders/lua/` with `CMakeLists.txt` and `polyplug_loaders_lua.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_lua` target

- [ ] Create `host-libs/cpp/loaders/js/` with `CMakeLists.txt` and `polyplug_loaders_js.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_js` target

- [ ] Create `host-libs/cpp/loaders/js_deno/` with `CMakeLists.txt` and `polyplug_loaders_js_deno.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_js_deno` target

- [ ] Create `host-libs/cpp/loaders/dotnet/` with `CMakeLists.txt` and `polyplug_loaders_dotnet.hpp`
  - **Verification:** New directory structure exists; `CMakeLists.txt` defines `polyplug_loaders_dotnet` target

- [ ] Remove old loader files from `host-libs/cpp/polyplug/loaders/`
  - **Verification:** `host-libs/cpp/polyplug/loaders/` directory is deleted; no references to old headers remain

- [ ] Create workspace `host-libs/cpp/CMakeLists.txt` that includes all loader subdirectories
  - **Verification:** `cmake -B build -S host-libs/cpp` succeeds; all loader targets are configured

---

## Phase 4: Update Examples and Documentation

**Blockers:** Phase 2 and Phase 3 complete  
**Parallel:** No

- [ ] Update C++ example code to use new `PluginGuard` API
  - **Verification:** Example compiles and runs; uses `runtime.resolve_plugin(handle)` and `guard.vtable()`

- [ ] Update README.md with new loader package structure and usage
  - **Verification:** README documents separate loader packages; includes installation instructions for each

---

## New Directory Structure

```
host-libs/cpp/
├── polyplug/
│   ├── abi.hpp
│   ├── error.hpp
│   ├── handle.hpp
│   ├── runtime.hpp              # Updated with PluginGuard
│   └── polyplug.hpp
├── loaders/
│   ├── native/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_native.hpp
│   ├── python/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_python.hpp
│   ├── lua/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_lua.hpp
│   ├── js/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_js.hpp
│   ├── js_deno/
│   │   ├── CMakeLists.txt
│   │   └── polyplug_loaders_js_deno.hpp
│   └── dotnet/
│       ├── CMakeLists.txt
│       └── polyplug_loaders_dotnet.hpp
└── CMakeLists.txt
```

---

## PRD References

- PRD §7: "Hot path call: One guard load. One pointer dereference. One indirect call."
- PRD §8: "loaders/python.hpp, loaders/lua.hpp, etc. (one per loader)" - separate packages
- PRD §15: "Caller-owns memory: The caller allocates memory for return values."

---

## Estimated Effort

- Phase 1: 1 hour
- Phase 2: 2 hours
- Phase 3: 3 hours (parallel execution)
- Phase 4: 2 hours

**Total: ~8 hours**