# Native Loader Epic

Make native (Rust/C/C++) plugin support optional via a loader crate, just like managed languages.

## Current State
- Native plugins are loaded directly by the core runtime
- App developers cannot opt-out of native support
- No consistency with how managed languages are handled

## Goal
- Create `polyplug_native` loader crate
- Native plugins loaded via `libpolyplug_native.so` like Python/Lua/JS
- App developers can choose which loaders to register
- Consistent API across all loader types

---

## WAVE 0: Core FFI Addition

- [ ] **Task 1: Add `polyplug_runtime_register_loader` to core FFI**
  - Already done in loader-ffi-epic
  - Verify it exists in `crates/polyplug/src/ffi.rs`

---

## WAVE 1: Native Loader Crate

- [ ] **Task 2: Create `crates/polyplug_native/` crate structure**
  - Create directory structure
  - Create `Cargo.toml` with cdylib support
  - Add to workspace

- [ ] **Task 3: Create `crates/polyplug_native/src/config.rs`**
  - Empty config (native plugins need no special config)

- [ ] **Task 4: Create `crates/polyplug_native/src/loader.rs`**
  - Implement `BundleLoader` for native plugins
  - Use `dlopen` to load `.so` files
  - Extract vtables using existing native logic

- [ ] **Task 5: Create `crates/polyplug_native/src/ffi.rs`**
  - `polyplug_native_loader_create()` - returns loader instance
  - `polyplug_native_loader_free()` - cleanup

- [ ] **Task 6: Create `crates/polyplug_native/src/lib.rs`**
  - Module exports
  - Re-export types

- [ ] **Task 7: Build and verify `libpolyplug_native.so`**
  - `cargo build --release -p polyplug_native`
  - Verify symbols with `nm -D`

---

## WAVE 2: Host Library Updates

- [ ] **Task 8: Update `host-libs/cpp/polyplug/loaders.hpp`**
  - Add `register_native_loader()` function
  - Add `PolyplugNativeConfig` struct

- [ ] **Task 9: Update `host-libs/python/polyplug/loaders.py`**
  - Add `register_native_loader()` function
  - Add `_NativeConfig` class

- [ ] **Task 10: Update `host-libs/lua/polyplug.lua`**
  - Add `M.register_native_loader()` function

- [ ] **Task 11: Update `host-libs/js/polyplug.ts`**
  - Add `registerNativeLoader()` function
  - Add `NATIVE_SYMBOLS`

- [ ] **Task 12: Update `host-libs/csharp/src/Runtime.cs`**
  - Add `RegisterNativeLoader()` method
  - Add `NativeLoaderConfig` struct

---

## WAVE 3: Guest Libraries

- [ ] **Task 13: Create `guest-libs/rust/` structure**
  - `Cargo.toml` for guest library
  - `src/lib.rs` with plugin macros/helpers
  - Example plugin showing how to write native plugin

- [ ] **Task 14: Create `guest-libs/cpp/` structure**
  - Header files for C++ plugin development
  - CMakeLists.txt or Makefile
  - Example plugin

---

## WAVE 4: Examples

- [ ] **Task 15: Create `examples/guests/native/` example plugin**
  - Simple native plugin (Rust or C++)
  - Implements a contract
  - Has manifest.toml

- [ ] **Task 16: Update all host examples to use loader pattern**
  - `examples/hosts/rust_host/` - register native loader
  - `examples/hosts/cpp/` - register native loader
  - `examples/hosts/csharp/` - register native loader
  - `examples/hosts/python/` - register native loader
  - `examples/hosts/lua/` - register native loader

- [ ] **Task 17: Create `examples/hosts/native_host/` (optional)**
  - Native host that loads native plugins via loader
  - Demonstrates the pattern

---

## WAVE 5: Documentation

- [ ] **Task 18: Update `examples/README.md`**
  - Document native loader alongside other loaders
  - Explain that native is now optional
  - Update loader table to include native

- [ ] **Task 19: Create `guest-libs/rust/README.md`**
  - How to write native plugins
  - API reference

- [ ] **Task 20: Create `guest-libs/cpp/README.md`**
  - How to write C++ plugins
  - API reference

- [ ] **Task 21: Document JS situation clearly**
  - Explain js_quickjs vs js_deno
  - Explain why js_deno guests are limited
  - Update all relevant READMEs

---

## WAVE 6: Build Scripts

- [ ] **Task 22: Update `examples/build_all.sh`**
  - Build native loader
  - Build native guest example

- [ ] **Task 23: Update `examples/verify_hosts.sh`**
  - Include native loader verification

---

## FINAL WAVE: Quality Checks

- [ ] **F1: Build native loader crate**
  - `cargo build --release -p polyplug_native`

- [ ] **F2: Verify native loader symbols**
  - `nm -D target/release/libpolyplug_native.so | grep loader_`

- [ ] **F3: Build all host-libs with native support**
  - C++, Python, Lua, JS, C# all build successfully

- [ ] **F4: Run host example with native loader**
  - Host registers native loader
  - Loads native plugin via loader
  - Calls function successfully

---

## Notes

### Why This Matters

**Before:**
```rust
// App developer MUST support native
let rt = Runtime::new(); // Automatically loads native plugins
```

**After:**
```rust
// App developer CHOOSES which loaders to support
let rt = Runtime::new();
rt.register_loader(NativeLoader::new()); // Optional!
rt.register_loader(PythonLoader::new()); // Optional!
// Only registered loaders work
```

### Consistency

All loaders now follow the same pattern:
1. Create loader instance
2. Register with runtime
3. Runtime uses loader to load plugins of that type

| Loader | Library | Status |
|--------|---------|--------|
| native | `libpolyplug_native.so` | 🆕 New |
| dotnet | `libpolyplug_dotnet.so` | ✅ Existing |
| python | `libpolyplug_python.so` | ✅ Existing |
| lua | `libpolyplug_lua.so` | ✅ Existing |
| js | `libpolyplug_js.so` | ✅ Existing |
| js_deno | ❌ Cannot build | ⚠️ Documented |
