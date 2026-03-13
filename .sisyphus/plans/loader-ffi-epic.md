# Work Plan: Epic 27 — Loader Registration FFI + Real Multi-Language Hosts + Uniform Examples

## TL;DR

> **Core Objective**: Enable non-Rust hosts (C++, Python, Lua, Deno/JS, C#) to load non-native guest plugins (.NET, Python, Lua, JS) by adding a loader registration C FFI surface to each adapter crate and wiring it into every host lib and example.
>
> **Problem**: `BundleLoader` trait is Rust-only. Non-Rust hosts call `libpolyplug.so` via FFI but only get `NativeBundleLoader`. They cannot load non-native guests, contradicting the north star: "any language can be a host that loads any guest."
>
> **Solution**: Add `polyplug_runtime_register_loader(rt, *mut c_void)` to the existing `libpolyplug.so` C facade. Each loader crate adds `src/ffi.rs` and builds its own `cdylib` (`libpolyplug_dotnet.so`, etc.) exporting `polyplug_dotnet_loader_create()` + `polyplug_dotnet_loader_free()`. Host libs wrap the two-step create→register pattern. Examples become real single-language programs.
>
> **Architecture**:
> - `polyplug` crate: keeps `["rlib", "cdylib"]` — `libpolyplug.so` unchanged, gains one new symbol
> - Each loader crate: adds `cdylib` to its own `crate-type` — produces its own `.so`
> - No new crates. No feature flags. No monolith. No breaking changes.
>
> **Deliverables**:
> - 1 new symbol in `libpolyplug.so`
> - 5 loader `cdylib`s, each with `create` + `free` exports
> - 5 host lib loader registration additions
> - 6 real host examples (no Rust cdylib inside any non-Rust host)
> - 14 guest examples (2 per language × 7 runtimes), uniform contract, identical output
> - Updated `build.sh`, `README.md`, `api.toml`
>
> **Total**: 28 tasks + 4 quality checks across 8 waves
> **Critical Path**: Core FFI → Loader FFI → Host libs → Guests → Hosts → Verify
> **Parallel Execution**: YES — Waves 2 and 3 fully parallel; Wave 4 fully parallel

---

## ARCHITECTURE

### Crate Structure (unchanged except additions)

```
crates/
├── polyplug/              # ["rlib", "cdylib"] — libpolyplug.so UNCHANGED except +1 symbol
│   └── src/ffi.rs         # ADD: polyplug_runtime_register_loader()
├── polyplug_dotnet/       # ADD: src/ffi.rs, ADD cdylib → libpolyplug_dotnet.so
├── polyplug_python/       # ADD: src/ffi.rs, ADD cdylib → libpolyplug_python.so
├── polyplug_lua/          # ADD: src/ffi.rs, ADD cdylib → libpolyplug_lua.so
├── polyplug_js/           # ADD: src/ffi.rs, ADD cdylib → libpolyplug_js.so
└── polyplug_js_deno/      # ADD: src/ffi.rs, ADD cdylib → libpolyplug_js_deno.so
```

### Dependency Graph (cycle-free, unchanged)

```
libpolyplug_dotnet.so  →  polyplug (rlib)  +  polyplug_dotnet (rlib)
libpolyplug_python.so  →  polyplug (rlib)  +  polyplug_python (rlib)
libpolyplug_lua.so     →  polyplug (rlib)  +  polyplug_lua (rlib)
libpolyplug_js.so      →  polyplug (rlib)  +  polyplug_js (rlib)
libpolyplug_js_deno.so →  polyplug (rlib)  +  polyplug_js_deno (rlib)

libpolyplug.so         →  polyplug (rlib)   [no loader deps — unchanged]
```

Each loader cdylib calls `polyplug_runtime_register_loader()` from `libpolyplug.so`
at runtime via dynamic linking. No circular deps. No cross-dylib Rust type passing.
The `*mut c_void` is a `Box<dyn BundleLoader>` erased on the loader side and
reconstituted on the polyplug side — both compiled into their respective binaries,
no vtable mismatch possible.

### Exported Symbols

**`libpolyplug.so`** — existing symbols unchanged, one addition:
```c
// EXISTING (frozen — must not change)
OpaqueRuntime* polyplug_runtime_new();
void           polyplug_runtime_free(OpaqueRuntime* rt);
uint32_t       polyplug_load_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t len);
uint32_t       polyplug_load_bundle_opts(OpaqueRuntime* rt, const uint8_t* path, size_t len, uint8_t mode);
uint32_t       polyplug_reload_bundle(OpaqueRuntime* rt, const uint8_t* path, size_t len);
uint64_t       polyplug_find_by_contract(OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version);
uint64_t       polyplug_find_by_bundle(OpaqueRuntime* rt, uint64_t bundle_id, uint64_t contract_id, uint32_t min_version);
size_t         polyplug_find_all_by_contract(OpaqueRuntime* rt, uint64_t contract_id, uint32_t min_version, uint64_t* out, size_t out_cap);
const OpaqueGuard* polyplug_resolve_plugin(OpaqueRuntime* rt, uint64_t packed_handle);
void           polyplug_guard_free(const OpaqueGuard* guard);
const void*    polyplug_get_vtable(const OpaqueGuard* guard);
size_t         polyplug_last_error(uint8_t* out, size_t out_cap);

// NEW — added in this epic
uint32_t       polyplug_runtime_register_loader(OpaqueRuntime* rt, void* loader_ptr);
```

**`libpolyplug_dotnet.so`** (NEW):
```c
void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig* config);
void  polyplug_dotnet_loader_free(void* ptr);
```

**`libpolyplug_python.so`** (NEW):
```c
void* polyplug_python_loader_create(const PolyplugPythonConfig* config);
void  polyplug_python_loader_free(void* ptr);
```

**`libpolyplug_lua.so`** (NEW):
```c
void* polyplug_lua_loader_create(const PolyplugLuaConfig* config);
void  polyplug_lua_loader_free(void* ptr);
```

**`libpolyplug_js.so`** (NEW):
```c
void* polyplug_js_loader_create(const PolyplugJsConfig* config);
void  polyplug_js_loader_free(void* ptr);
```

**`libpolyplug_js_deno.so`** (NEW):
```c
void* polyplug_js_deno_loader_create(const PolyplugJsDenoConfig* config);
void  polyplug_js_deno_loader_free(void* ptr);
```

### Config FFI Structs (defined in each loader crate's ffi.rs)

```c
// polyplug_dotnet
typedef struct { const uint8_t* min_framework_ptr; size_t min_framework_len; } PolyplugDotnetConfig;

// polyplug_python
typedef struct { const uint8_t* min_version_ptr; size_t min_version_len; } PolyplugPythonConfig;

// polyplug_lua — no required fields, pad for C ABI
typedef struct { uint8_t _reserved; } PolyplugLuaConfig;

// polyplug_js — no required fields
typedef struct { uint8_t _reserved; } PolyplugJsConfig;

// polyplug_js_deno — no required fields
typedef struct { uint8_t _reserved; } PolyplugJsDenoConfig;
```

### Examples Structure (after this epic)

```
examples/
├── api.toml               # TWO contracts: data.Transformer + data.Reporter
├── build.sh               # builds all guests + all loader .so files + compiled hosts
├── README.md
├── expected_output.txt    # golden file — 14 lines, committed
├── hosts/
│   ├── rust/              # Cargo.toml + src/main.rs — uses polyplug crate directly
│   ├── cpp/               # main.cpp + Makefile — uses host-libs/cpp (+ loaders.hpp)
│   ├── csharp/            # Program.cs + .csproj — uses host-libs/csharp
│   ├── python/            # host.py — uses host-libs/python (+ loaders.py)
│   ├── lua/               # host.lua ONLY — uses host-libs/lua (no Cargo.toml, no src/)
│   └── js_deno/           # host.ts ONLY — uses host-libs/js (no Cargo.toml, no src/)
└── guests/
    ├── rust/decoder/      # implements data.Transformer
    ├── rust/reporter/     # implements data.Reporter
    ├── cpp/decoder/       # implements data.Transformer
    ├── cpp/reporter/      # implements data.Reporter
    ├── csharp/encoder/    # implements data.Transformer
    ├── csharp/reporter/   # implements data.Reporter
    ├── python/decoder/    # implements data.Transformer
    ├── python/reporter/   # implements data.Reporter
    ├── lua/transformer/   # implements data.Transformer
    ├── lua/reporter/      # implements data.Reporter
    ├── js_quickjs/transformer/  # implements data.Transformer (bundle.js)
    ├── js_quickjs/reporter/     # implements data.Reporter (bundle.js)
    ├── js_deno/transformer/     # implements data.Transformer (index.ts)
    └── js_deno/reporter/        # implements data.Reporter (index.ts)
```

### Uniform Output (all 6 hosts, identical, 14 lines)

```
[rust/decoder]              transform("hello") = "rust:transform(hello)"
[rust/reporter]             report("hello")    = "rust:report(hello)"
[cpp/decoder]               transform("hello") = "cpp:transform(hello)"
[cpp/reporter]              report("hello")    = "cpp:report(hello)"
[csharp/encoder]            transform("hello") = "csharp:transform(hello)"
[csharp/reporter]           report("hello")    = "csharp:report(hello)"
[python/decoder]            transform("hello") = "python:transform(hello)"
[python/reporter]           report("hello")    = "python:report(hello)"
[lua/transformer]           transform("hello") = "lua:transform(hello)"
[lua/reporter]              report("hello")    = "lua:report(hello)"
[js_quickjs/transformer]    transform("hello") = "js_quickjs:transform(hello)"
[js_quickjs/reporter]       report("hello")    = "js_quickjs:report(hello)"
[js_deno/transformer]       transform("hello") = "js_deno:transform(hello)"
[js_deno/reporter]          report("hello")    = "js_deno:report(hello)"
```

---

## WAVE 0: Core FFI — `polyplug_runtime_register_loader`

> **Blockers**: None
> **Parallelism**: Sequential (Task 1 must pass before Wave 2 begins)
> **Completes When**: `libpolyplug.so` exports the new registration symbol

---

- [ ] **Task 1: Add `polyplug_runtime_register_loader` to `crates/polyplug/src/ffi.rs`**

  **What**: Add one new exported function to the existing C facade. Do NOT change any existing symbols.

  **Implementation**:
  ```rust
  /// Registers a loader created by a loader cdylib into this runtime.
  /// `loader_ptr` must be a non-null `*mut c_void` produced by
  /// `polyplug_*_loader_create()`. This call transfers ownership —
  /// do not call `polyplug_*_loader_free()` after a successful registration.
  /// Returns 0 on success, non-zero on error (check `polyplug_last_error`).
  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_runtime_register_loader(
      rt: *mut OpaqueRuntime,
      loader_ptr: *mut std::ffi::c_void,
  ) -> u32 {
      // SAFETY: rt is a valid *mut OpaqueRuntime produced by polyplug_runtime_new.
      // loader_ptr is a Box<dyn BundleLoader> erased to *mut c_void by a loader
      // cdylib compiled against the same polyplug rlib. Reconstituting the Box is
      // valid because both sides agree on the concrete type via the rlib.
      if rt.is_null() || loader_ptr.is_null() {
          return 1;
      }
      let runtime = &mut *(rt as *mut Runtime);
      let loader: Box<dyn BundleLoader> = Box::from_raw(loader_ptr as *mut _);
      match runtime.register_loader(loader) {
          Ok(()) => 0,
          Err(_) => 2,
      }
  }
  ```

  **Must NOT do**:
  - Change any existing symbol signatures
  - Change `crates/polyplug/Cargo.toml` crate-type (stays `["rlib", "cdylib"]`)
  - Add RwLock or interior mutability anywhere (existing builder pattern is correct)

  **QA**:
  ```bash
  cargo build --release -p polyplug
  nm -D target/release/libpolyplug.so | grep polyplug_runtime_register_loader
  echo $?
  # Expected: exits 0, symbol line printed
  ```

  **Commit**: `feat(ffi): add polyplug_runtime_register_loader to C facade`

---

## WAVE 1: Loader FFI — per-loader `create` + `free` exports

> **Blockers**: Task 1 (libpolyplug.so must export register symbol first)
> **Parallelism**: Tasks 2–6 fully parallel — one agent per loader crate
> **Completes When**: All 5 loader `.so` files built and symbols verified

---

- [ ] **Task 2: `polyplug_dotnet` — add `src/ffi.rs` + `cdylib`**

  **What**: Add `src/ffi.rs` with create/free exports. Add `cdylib` to `crate-type` in `Cargo.toml`.

  **`Cargo.toml` change**:
  ```toml
  [lib]
  crate-type = ["rlib", "cdylib"]
  ```

  **`src/ffi.rs`**:
  ```rust
  use std::ffi::c_void;
  use crate::{DotnetLoader, DotnetConfig};

  #[repr(C)]
  pub struct PolyplugDotnetConfig {
      pub min_framework_ptr: *const u8,
      pub min_framework_len: usize,
  }

  /// Creates a DotnetLoader from config. Returns an opaque pointer.
  /// Caller must pass to `polyplug_runtime_register_loader` OR call
  /// `polyplug_dotnet_loader_free` — never both.
  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_dotnet_loader_create(
      config: *const PolyplugDotnetConfig,
  ) -> *mut c_void {
      // SAFETY: config is a valid pointer to a PolyplugDotnetConfig with
      // min_framework_ptr pointing to min_framework_len valid UTF-8 bytes.
      if config.is_null() { return std::ptr::null_mut(); }
      let cfg = &*config;
      if cfg.min_framework_ptr.is_null() { return std::ptr::null_mut(); }
      let bytes = std::slice::from_raw_parts(cfg.min_framework_ptr, cfg.min_framework_len);
      let min_framework = match std::str::from_utf8(bytes) {
          Ok(s) => s.to_string(),
          Err(_) => return std::ptr::null_mut(),
      };
      let loader = DotnetLoader::new(DotnetConfig { min_framework });
      Box::into_raw(Box::new(loader)) as *mut c_void
  }

  /// Frees a loader pointer without registering it. No-op on null.
  #[unsafe(no_mangle)]
  pub unsafe extern "C" fn polyplug_dotnet_loader_free(ptr: *mut c_void) {
      // SAFETY: ptr is either null or a valid Box<DotnetLoader> from
      // polyplug_dotnet_loader_create that has not been registered.
      if ptr.is_null() { return; }
      drop(Box::<DotnetLoader>::from_raw(ptr as *mut DotnetLoader));
  }
  ```

  **Wire `src/lib.rs`**: add `pub mod ffi;` (AGENTS.md: module declaration in lib.rs, implementation in ffi/mod.rs or ffi.rs)

  **QA**:
  ```bash
  cargo build --release -p polyplug_dotnet
  nm -D target/release/libpolyplug_dotnet.so | grep -E "polyplug_dotnet_loader_(create|free)"
  echo $?
  # Expected: exits 0, both symbols printed
  ```

  **Commit**: `feat(polyplug_dotnet): add loader FFI create/free exports`

- [ ] **Task 3: `polyplug_python` — add `src/ffi.rs` + `cdylib`**

  **Same pattern as Task 2.**

  **Config struct**:
  ```rust
  #[repr(C)]
  pub struct PolyplugPythonConfig {
      pub min_version_ptr: *const u8,
      pub min_version_len: usize,
  }
  ```

  **Exports**: `polyplug_python_loader_create`, `polyplug_python_loader_free`

  **QA**:
  ```bash
  cargo build --release -p polyplug_python
  nm -D target/release/libpolyplug_python.so | grep -E "polyplug_python_loader_(create|free)"
  echo $?
  # Expected: exits 0, both symbols printed
  ```

  **Commit**: `feat(polyplug_python): add loader FFI create/free exports`

- [ ] **Task 4: `polyplug_lua` — add `src/ffi.rs` + `cdylib`**

  **Config struct** (no required fields):
  ```rust
  #[repr(C)]
  pub struct PolyplugLuaConfig {
      pub _reserved: u8,
  }
  ```

  **Exports**: `polyplug_lua_loader_create`, `polyplug_lua_loader_free`

  **QA**:
  ```bash
  cargo build --release -p polyplug_lua
  nm -D target/release/libpolyplug_lua.so | grep -E "polyplug_lua_loader_(create|free)"
  echo $?
  # Expected: exits 0, both symbols printed
  ```

  **Commit**: `feat(polyplug_lua): add loader FFI create/free exports`

- [ ] **Task 5: `polyplug_js` — add `src/ffi.rs` + `cdylib`**

  **Config struct**:
  ```rust
  #[repr(C)]
  pub struct PolyplugJsConfig {
      pub _reserved: u8,
  }
  ```

  **Exports**: `polyplug_js_loader_create`, `polyplug_js_loader_free`

  **QA**:
  ```bash
  cargo build --release -p polyplug_js
  nm -D target/release/libpolyplug_js.so | grep -E "polyplug_js_loader_(create|free)"
  echo $?
  # Expected: exits 0, both symbols printed
  ```

  **Commit**: `feat(polyplug_js): add loader FFI create/free exports`

- [ ] **Task 6: `polyplug_js_deno` — add `src/ffi.rs` + `cdylib`**

  **Config struct**:
  ```rust
  #[repr(C)]
  pub struct PolyplugJsDenoConfig {
      pub _reserved: u8,
  }
  ```

  **Exports**: `polyplug_js_deno_loader_create`, `polyplug_js_deno_loader_free`

  **QA**:
  ```bash
  cargo build --release -p polyplug_js_deno
  nm -D target/release/libpolyplug_js_deno.so | grep -E "polyplug_js_deno_loader_(create|free)"
  echo $?
  # Expected: exits 0, both symbols printed
  ```

  **Commit**: `feat(polyplug_js_deno): add loader FFI create/free exports`

---

## WAVE 2: Host Libraries — loader registration wrappers

> **Blockers**: Tasks 2–6 (all loader .so files must exist)
> **Parallelism**: Tasks 7–11 fully parallel — one agent per host lib
> **Completes When**: All 5 host libs can register all 5 loaders

---

- [ ] **Task 7: `host-libs/cpp` — add `polyplug/loaders.hpp`**

  **What**: Create `host-libs/cpp/polyplug/loaders.hpp`. Do NOT modify `runtime.hpp` or `abi.hpp`.

  **File**:
  ```cpp
  #pragma once
  #include "runtime.hpp"
  #include <string_view>
  #include <stdexcept>

  // Link: -lpolyplug_dotnet -lpolyplug_python -lpolyplug_lua
  //       -lpolyplug_js -lpolyplug_js_deno

  extern "C" {
      struct PolyplugDotnetConfig { const uint8_t* min_framework_ptr; size_t min_framework_len; };
      struct PolyplugPythonConfig { const uint8_t* min_version_ptr;   size_t min_version_len; };
      struct PolyplugLuaConfig    { uint8_t _reserved; };
      struct PolyplugJsConfig     { uint8_t _reserved; };
      struct PolyplugJsDenoConfig { uint8_t _reserved; };

      void* polyplug_dotnet_loader_create(const PolyplugDotnetConfig*);
      void  polyplug_dotnet_loader_free(void*);
      void* polyplug_python_loader_create(const PolyplugPythonConfig*);
      void  polyplug_python_loader_free(void*);
      void* polyplug_lua_loader_create(const PolyplugLuaConfig*);
      void  polyplug_lua_loader_free(void*);
      void* polyplug_js_loader_create(const PolyplugJsConfig*);
      void  polyplug_js_loader_free(void*);
      void* polyplug_js_deno_loader_create(const PolyplugJsDenoConfig*);
      void  polyplug_js_deno_loader_free(void*);
      uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
  }

  namespace polyplug {

  inline void register_dotnet_loader(Runtime& rt, std::string_view min_framework = "10.0") {
      PolyplugDotnetConfig cfg{
          reinterpret_cast<const uint8_t*>(min_framework.data()),
          min_framework.size()
      };
      void* loader = polyplug_dotnet_loader_create(&cfg);
      if (!loader) throw std::runtime_error("polyplug: dotnet loader create failed");
      if (polyplug_runtime_register_loader(rt.handle(), loader) != 0)
          throw std::runtime_error("polyplug: dotnet loader register failed");
  }

  inline void register_python_loader(Runtime& rt, std::string_view min_version = "3.11") { /* same pattern */ }
  inline void register_lua_loader(Runtime& rt)    { /* same pattern, PolyplugLuaConfig{0} */ }
  inline void register_js_loader(Runtime& rt)     { /* same pattern */ }
  inline void register_js_deno_loader(Runtime& rt){ /* same pattern */ }

  } // namespace polyplug
  ```

  **QA**:
  ```bash
  echo '#include <polyplug/loaders.hpp>' | \
    g++ -c -I host-libs/cpp -x c++ - -o /tmp/test_loaders.o
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(host-libs/cpp): add loaders.hpp with loader registration`

- [ ] **Task 8: `host-libs/python` — add `polyplug/loaders.py`**

  **What**: Create `host-libs/python/polyplug/loaders.py`.

  **File** (pattern — implement all 5):
  ```python
  """Loader registration for polyplug non-native guests."""
  import ctypes
  import os

  _loader_libs: dict = {}

  def _get_loader_lib(name: str) -> ctypes.CDLL:
      if name not in _loader_libs:
          _loader_libs[name] = ctypes.CDLL(f"lib{name}.so")
      return _loader_libs[name]

  def _register(runtime, lib_name: str, create_fn: str, config_ptr) -> None:
      from polyplug.runtime import _get_polyplug_lib, _get_rt_ptr
      lib = _get_loader_lib(lib_name)
      create = getattr(lib, create_fn)
      create.restype = ctypes.c_void_p
      create.argtypes = [ctypes.c_void_p]
      loader_ptr = create(config_ptr)
      if not loader_ptr:
          raise RuntimeError(f"polyplug: {lib_name} loader create failed")
      polyplug_lib = _get_polyplug_lib()
      register = polyplug_lib.polyplug_runtime_register_loader
      register.restype = ctypes.c_uint32
      register.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
      err = register(_get_rt_ptr(runtime), loader_ptr)
      if err != 0:
          raise RuntimeError(f"polyplug: {lib_name} loader register failed: {err}")

  class _DotnetConfig(ctypes.Structure):
      _fields_ = [("min_framework_ptr", ctypes.c_char_p),
                  ("min_framework_len", ctypes.c_size_t)]

  def register_dotnet_loader(runtime, min_framework: str = "10.0") -> None:
      b = min_framework.encode()
      cfg = _DotnetConfig(b, len(b))
      _register(runtime, "polyplug_dotnet", "polyplug_dotnet_loader_create", ctypes.byref(cfg))

  class _PythonConfig(ctypes.Structure):
      _fields_ = [("min_version_ptr", ctypes.c_char_p),
                  ("min_version_len", ctypes.c_size_t)]

  def register_python_loader(runtime, min_version: str = "3.11") -> None:
      b = min_version.encode()
      cfg = _PythonConfig(b, len(b))
      _register(runtime, "polyplug_python", "polyplug_python_loader_create", ctypes.byref(cfg))

  class _EmptyConfig(ctypes.Structure):
      _fields_ = [("_reserved", ctypes.c_uint8)]

  def register_lua_loader(runtime) -> None:
      cfg = _EmptyConfig(0)
      _register(runtime, "polyplug_lua", "polyplug_lua_loader_create", ctypes.byref(cfg))

  def register_js_loader(runtime) -> None:
      cfg = _EmptyConfig(0)
      _register(runtime, "polyplug_js", "polyplug_js_loader_create", ctypes.byref(cfg))

  def register_js_deno_loader(runtime) -> None:
      cfg = _EmptyConfig(0)
      _register(runtime, "polyplug_js_deno", "polyplug_js_deno_loader_create", ctypes.byref(cfg))
  ```

  **QA**:
  ```bash
  PYTHONPATH="host-libs/python" \
  LD_LIBRARY_PATH="target/release" \
  python3 -c "from polyplug.loaders import register_dotnet_loader; print('OK')"
  echo $?
  # Expected: 0, prints OK
  ```

  **Commit**: `feat(host-libs/python): add loaders.py with loader registration`

- [ ] **Task 9: `host-libs/lua` — add loader registration to `polyplug.lua`**

  **What**: Add 5 registration functions to the existing module table `M` at the bottom of `polyplug.lua`. Use `ffi.load()` with lazy caching per loader.

  **Addition** (pattern — implement all 5):
  ```lua
  -- Lazy-loaded loader library handles
  local _loader_libs = {}

  local function get_loader_lib(name)
      if not _loader_libs[name] then
          _loader_libs[name] = ffi.load(name)
      end
      return _loader_libs[name]
  end

  function M.register_dotnet_loader(rt, opts)
      opts = opts or {}
      local min_fw = opts.min_framework or "10.0"
      -- Declare types inline to avoid global namespace pollution
      ffi.cdef[[
          typedef struct { const uint8_t* ptr; size_t len; } PolyplugDotnetCfg;
          void* polyplug_dotnet_loader_create(const PolyplugDotnetCfg* cfg);
          uint32_t polyplug_runtime_register_loader(void* rt, void* loader);
      ]] -- wrap in pcall on second invocation — ffi.cdef errors on redeclaration
      local lib = get_loader_lib("polyplug_dotnet")
      local fw_bytes = ffi.new("uint8_t[?]", #min_fw, min_fw)
      local cfg = ffi.new("PolyplugDotnetCfg", fw_bytes, #min_fw)
      local loader = lib.polyplug_dotnet_loader_create(cfg)
      if loader == nil then error("polyplug: dotnet loader create failed") end
      local err = M._lib.polyplug_runtime_register_loader(rt, loader)
      if err ~= 0 then error("polyplug: dotnet loader register failed: " .. err) end
  end

  function M.register_python_loader(rt, opts) ... end
  function M.register_lua_loader(rt) ... end
  function M.register_js_loader(rt) ... end
  function M.register_js_deno_loader(rt) ... end
  ```

  **Note**: `ffi.cdef` redeclaration must be guarded with `pcall` since LuaJIT errors on duplicate declarations. A clean approach: declare all loader FFI types once in a `pcall` block at module load time.

  **QA**:
  ```bash
  LD_LIBRARY_PATH="target/release" \
  luajit -e '
    local polyplug = dofile("host-libs/lua/polyplug.lua")
    print(type(polyplug.register_dotnet_loader))
    print(type(polyplug.register_python_loader))
    print("OK")
  '
  echo $?
  # Expected: 0, prints "function" twice then "OK"
  ```

  **Commit**: `feat(host-libs/lua): add loader registration functions`

- [ ] **Task 10: `host-libs/js` — add loader registration to `polyplug.ts`**

  **What**: Add 5 exported functions to `polyplug.ts`. Open each loader `.so` lazily via `Deno.dlopen`. Cache at module level.

  **Addition** (pattern — implement all 5):
  ```typescript
  const _loaderLibs: Map<string, Deno.DynamicLibrary<any>> = new Map();

  function getLoaderLib(name: string, symbols: Deno.ForeignLibraryInterface) {
      if (!_loaderLibs.has(name)) {
          _loaderLibs.set(name, Deno.dlopen(`lib${name}.so`, symbols));
      }
      return _loaderLibs.get(name)!;
  }

  const DOTNET_SYMBOLS = {
      polyplug_dotnet_loader_create: {
          parameters: ["pointer"] as const,
          result: "pointer" as const,
      },
  } satisfies Deno.ForeignLibraryInterface;

  export function registerDotnetLoader(rt: Deno.PointerValue, minFramework = "10.0"): void {
      const lib = getLoaderLib("polyplug_dotnet", DOTNET_SYMBOLS);
      const bytes = new TextEncoder().encode(minFramework);
      const buf = new Uint8Array(bytes);
      // Build config struct: ptr (8 bytes) + len (8 bytes) on 64-bit
      const cfgBuf = new ArrayBuffer(16);
      const view = new DataView(cfgBuf);
      const ptr = Deno.UnsafePointer.of(buf)!;
      // write pointer as BigInt lo/hi per existing polyplug.ts convention
      const ptrVal = BigInt(Deno.UnsafePointer.value(ptr));
      view.setBigUint64(0, ptrVal, true);
      view.setBigUint64(8, BigInt(bytes.length), true);
      const cfgPtr = Deno.UnsafePointer.of(new Uint8Array(cfgBuf))!;
      const loaderPtr = lib.symbols.polyplug_dotnet_loader_create(cfgPtr);
      if (loaderPtr === null) throw new Error("polyplug: dotnet loader create failed");
      const err = _polyplugLib.symbols.polyplug_runtime_register_loader(rt, loaderPtr);
      if (err !== 0) throw new Error(`polyplug: dotnet loader register failed: ${err}`);
  }

  export function registerPythonLoader(rt: Deno.PointerValue, minVersion = "3.11"): void { ... }
  export function registerLuaLoader(rt: Deno.PointerValue): void { ... }
  export function registerJsLoader(rt: Deno.PointerValue): void { ... }
  export function registerJsDenoLoader(rt: Deno.PointerValue): void { ... }
  ```

  **Note**: `_polyplugLib` is the existing module-level `Deno.dlopen` handle for `libpolyplug.so`. Add `polyplug_runtime_register_loader` to its symbol table. Follow the existing BigInt pointer convention in `polyplug.ts` exactly.

  **QA**:
  ```bash
  LD_LIBRARY_PATH="target/release" \
  deno check --allow-ffi host-libs/js/polyplug.ts
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(host-libs/js): add loader registration functions`

- [ ] **Task 11: `host-libs/csharp` — add loader registration to `Runtime.cs`**

  **What**: Add P/Invoke declarations and 5 public `Register*Loader` methods to the `Runtime` class. No unsafe. Uses `IntPtr` throughout per existing pattern.

  **Addition**:
  ```csharp
  // P/Invoke declarations — add to Runtime class
  [DllImport("polyplug", EntryPoint = "polyplug_runtime_register_loader")]
  private static extern uint PolyplugRuntimeRegisterLoader(IntPtr rt, IntPtr loader);

  [DllImport("polyplug_dotnet", EntryPoint = "polyplug_dotnet_loader_create")]
  private static extern IntPtr PolyplugDotnetLoaderCreate(IntPtr cfgPtr);

  [DllImport("polyplug_python", EntryPoint = "polyplug_python_loader_create")]
  private static extern IntPtr PolyplugPythonLoaderCreate(IntPtr cfgPtr);

  [DllImport("polyplug_lua", EntryPoint = "polyplug_lua_loader_create")]
  private static extern IntPtr PolyplugLuaLoaderCreate(IntPtr cfgPtr);

  [DllImport("polyplug_js", EntryPoint = "polyplug_js_loader_create")]
  private static extern IntPtr PolyplugJsLoaderCreate(IntPtr cfgPtr);

  [DllImport("polyplug_js_deno", EntryPoint = "polyplug_js_deno_loader_create")]
  private static extern IntPtr PolyplugJsDenoLoaderCreate(IntPtr cfgPtr);

  // Public registration methods — no unsafe, uses Marshal to pin strings
  public void RegisterDotnetLoader(string minFramework = "10.0") {
      byte[] bytes = System.Text.Encoding.UTF8.GetBytes(minFramework);
      var handle = System.Runtime.InteropServices.GCHandle.Alloc(bytes,
          System.Runtime.InteropServices.GCHandleType.Pinned);
      try {
          // Build PolyplugDotnetConfig: ptr (IntPtr) + len (UIntPtr) — sequential
          // Marshal to unmanaged struct via fixed-size byte array
          IntPtr loader = PolyplugDotnetLoaderCreate(/* blittable config ptr */);
          if (loader == IntPtr.Zero)
              throw new InvalidOperationException("polyplug: dotnet loader create failed");
          uint err = PolyplugRuntimeRegisterLoader(_handle, loader);
          if (err != 0)
              throw new InvalidOperationException($"polyplug: register loader failed: {err}");
      } finally {
          handle.Free();
      }
  }

  public void RegisterPythonLoader(string minVersion = "3.11") { ... }
  public void RegisterLuaLoader() { ... }
  public void RegisterJsLoader() { ... }
  public void RegisterJsDenoLoader() { ... }
  ```

  **Note**: Config structs must be passed as `IntPtr` to a pinned `StructLayout(Sequential)` struct. Marshal.StructureToPtr or a pinned byte array are both valid. Zero unsafe anywhere in `host-libs/csharp/`.

  **QA**:
  ```bash
  dotnet build host-libs/csharp/Polyplug.csproj
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(host-libs/csharp): add loader registration methods`

---

## WAVE 3: Guest Examples — all 14 guests

> **Blockers**: None — can run in parallel with Waves 1 and 2
> **Parallelism**: Tasks 12–18 fully parallel
> **Completes When**: All 14 guests build and have correct manifest.toml

Each guest implements exactly one of the two shared contracts from `examples/api.toml`:
- `data.Transformer` → `transform(input: string) -> string`  returns `"<lang>:transform(<input>)"`
- `data.Reporter`    → `report(value: string) -> string`      returns `"<lang>:report(<value>)"`

---

- [ ] **Task 12: Rust guests — `rust/decoder` (Transformer) + `rust/reporter` (Reporter)**

  These likely already exist. If so, verify they implement the correct contracts and return the correct strings. Update if needed. Do not rename existing directories.

  **QA**:
  ```bash
  cargo build --release --manifest-path examples/guests/rust/decoder/Cargo.toml
  cargo build --release --manifest-path examples/guests/rust/reporter/Cargo.toml
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/rust): align to uniform contract`

- [ ] **Task 13: C++ guests — `cpp/decoder` (Transformer) + `cpp/reporter` (Reporter)**

  **QA**:
  ```bash
  make -C examples/guests/cpp/decoder
  make -C examples/guests/cpp/reporter
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/cpp): align to uniform contract`

- [ ] **Task 14: C# guests — `csharp/encoder` (Transformer) + `csharp/reporter` (Reporter)**

  **QA**:
  ```bash
  dotnet build examples/guests/csharp/encoder
  dotnet build examples/guests/csharp/reporter
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/csharp): align to uniform contract`

- [ ] **Task 15: Python guests — `python/decoder` (Transformer) + `python/reporter` (Reporter)**

  **QA**:
  ```bash
  python3 -m py_compile examples/guests/python/decoder/decoder.py
  python3 -m py_compile examples/guests/python/reporter/summary_reporter.py
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/python): align to uniform contract`

- [ ] **Task 16: Lua guests — `lua/transformer` (Transformer) + `lua/reporter` (Reporter)**

  **QA**:
  ```bash
  luajit -b examples/guests/lua/transformer/reverse_transformer.lua /tmp/test_lua_t.luac
  luajit -b examples/guests/lua/reporter/reporter.lua /tmp/test_lua_r.luac
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/lua): align to uniform contract`

- [ ] **Task 17: JS QuickJS guests — rename `guests/js/` to `guests/js_quickjs/`, add transformer+reporter**

  **What**: The existing `guests/js/` has `reporter/` and `validator/`. Rename the directory to `js_quickjs/`. Replace or rename contents to `transformer/` (Transformer) and `reporter/` (Reporter). Each is a pre-bundled `bundle.js` + `manifest.toml`.

  **QA**:
  ```bash
  test -f examples/guests/js_quickjs/transformer/bundle.js
  test -f examples/guests/js_quickjs/reporter/bundle.js
  test ! -d examples/guests/js
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/js_quickjs): rename and align to uniform contract`

- [ ] **Task 18: JS Deno guests — create `guests/js_deno/transformer/` + `guests/js_deno/reporter/`**

  **What**: Create two new Deno guest directories. Each has `index.ts` + `manifest.toml` with `runtime = "js-deno"`. Uses `guest-libs/js/polyplug-guest.ts`.

  **QA**:
  ```bash
  deno check examples/guests/js_deno/transformer/index.ts
  deno check examples/guests/js_deno/reporter/index.ts
  echo $?
  # Expected: 0
  ```

  **Commit**: `feat(examples/guests/js_deno): add Deno guest examples`

---

## WAVE 4: Host Examples — 6 real hosts

> **Blockers**: Tasks 2–11 (host libs must have loader registration), Tasks 12–18 (guests must exist)
> **Parallelism**: Tasks 19–24 fully parallel — one agent per host
> **Completes When**: All 6 hosts run and produce output

---

- [ ] **Task 19: Delete fake hosts, rename `js/` to `js_deno/`**

  **Files to delete**:
  ```
  examples/hosts/lua/Cargo.toml
  examples/hosts/lua/Cargo.lock
  examples/hosts/lua/src/lib.rs
  examples/hosts/lua/polyplug_full.lua
  examples/hosts/js/Cargo.toml
  examples/hosts/js/Cargo.lock
  examples/hosts/js/src/lib.rs
  examples/hosts/js/polyplug_full.map
  examples/hosts/js/build.rs
  ```

  **Rename**: `examples/hosts/js/` → `examples/hosts/js_deno/` (keep `host.ts`)

  **QA**:
  ```bash
  find examples/hosts -name "Cargo.toml" -not -path "*/rust/*" | wc -l
  echo $?
  # Expected: prints 0, exits 0
  test -d examples/hosts/js_deno
  echo $?
  # Expected: 0
  test ! -d examples/hosts/js
  echo $?
  # Expected: 0
  ```

  **Commit**: `chore(examples): remove fake Rust cdylib hosts, rename js to js_deno`

- [ ] **Task 20: Rust host — `examples/hosts/rust/`**

  **What**: Update `src/main.rs` to register all 5 loaders, load all 14 guests in fixed order, call transform/report on each, print uniform output.

  **Must NOT do**: Change `Cargo.toml` structure — it already exists.

  **Loader registration** (Rust host uses crate API directly, no FFI):
  ```rust
  let runtime = PluginRuntime::new()
      .loader(DotnetLoader::new())
      .loader(PythonLoader::new())
      .loader(LuaLoader::new())
      .loader(JsLoader::new())
      .loader(JsDenoLoader::new())
      .init()?;
  ```

  **QA**:
  ```bash
  mkdir -p examples/_out
  LD_LIBRARY_PATH="target/release" \
  cargo run --release --manifest-path examples/hosts/rust/Cargo.toml \
    > examples/_out/rust.txt
  echo $?
  wc -l examples/_out/rust.txt
  # Expected: exits 0, prints "14 examples/_out/rust.txt"
  ```

  **Commit**: `feat(examples/hosts/rust): load all 14 guests, uniform output`

- [ ] **Task 21: C++ host — `examples/hosts/cpp/`**

  **What**: Update `main.cpp` to include `<polyplug/loaders.hpp>`, register all 5 loaders, load all 14 guests, print uniform output. Update `Makefile` to link `-lpolyplug_dotnet -lpolyplug_python -lpolyplug_lua -lpolyplug_js -lpolyplug_js_deno`.

  **QA**:
  ```bash
  LD_LIBRARY_PATH="target/release" \
  make -C examples/hosts/cpp
  mkdir -p examples/_out
  LD_LIBRARY_PATH="target/release" \
  ./examples/hosts/cpp/polyplug_host_cpp > examples/_out/cpp.txt
  echo $?
  wc -l examples/_out/cpp.txt
  # Expected: exits 0, prints "14 examples/_out/cpp.txt"
  ```

  **Commit**: `feat(examples/hosts/cpp): load all 14 guests, uniform output`

- [ ] **Task 22: C# host — `examples/hosts/csharp/`**

  **What**: Update `Program.cs` to call all 5 `Register*Loader()` methods, load all 14 guests, print uniform output.

  **QA**:
  ```bash
  LD_LIBRARY_PATH="target/release" \
  dotnet run --project examples/hosts/csharp/PolyplugHost.csproj \
    > examples/_out/csharp.txt
  echo $?
  wc -l examples/_out/csharp.txt
  # Expected: exits 0, prints "14 examples/_out/csharp.txt"
  ```

  **Commit**: `feat(examples/hosts/csharp): load all 14 guests, uniform output`

- [ ] **Task 23: Python host — `examples/hosts/python/host.py`**

  **What**: Update `host.py` to import `from polyplug.loaders import *`, register all 5 loaders, load all 14 guests, print uniform output.

  **QA**:
  ```bash
  mkdir -p examples/_out
  PYTHONPATH="host-libs/python" \
  LD_LIBRARY_PATH="target/release" \
  python3 examples/hosts/python/host.py > examples/_out/python.txt
  echo $?
  wc -l examples/_out/python.txt
  # Expected: exits 0, prints "14 examples/_out/python.txt"
  ```

  **Commit**: `feat(examples/hosts/python): load all 14 guests, uniform output`

- [ ] **Task 24: Lua host — `examples/hosts/lua/host.lua`**

  **What**: Rewrite `host.lua` to be a real Lua host. No Rust cdylib — calls `host-libs/lua/polyplug.lua` directly. Registers all 5 loaders. Loads all 14 guests. Prints uniform output.

  **QA**:
  ```bash
  mkdir -p examples/_out
  LD_LIBRARY_PATH="target/release" \
  luajit examples/hosts/lua/host.lua > examples/_out/lua.txt
  echo $?
  wc -l examples/_out/lua.txt
  # Expected: exits 0, prints "14 examples/_out/lua.txt"
  ```

  **Commit**: `feat(examples/hosts/lua): rewrite as real Lua host, load all 14 guests`

- [ ] **Task 25: Deno host — `examples/hosts/js_deno/host.ts`**

  **What**: Rewrite `host.ts` to be a real Deno host. No Rust cdylib. Imports from `host-libs/js/polyplug.ts` directly. Registers all 5 loaders. Loads all 14 guests. Prints uniform output.

  **QA**:
  ```bash
  mkdir -p examples/_out
  LD_LIBRARY_PATH="target/release" \
  deno run --allow-ffi --allow-env --allow-read \
    examples/hosts/js_deno/host.ts > examples/_out/js_deno.txt
  echo $?
  wc -l examples/_out/js_deno.txt
  # Expected: exits 0, prints "14 examples/_out/js_deno.txt"
  ```

  **Commit**: `feat(examples/hosts/js_deno): rewrite as real Deno host, load all 14 guests`

---

## WAVE 5: Golden Output + Diff Verification

> **Blockers**: Tasks 19–25 (all hosts must run)
> **Parallelism**: Task 26 first, then Task 27

---

- [ ] **Task 26: Commit golden output file**

  **What**: Run Rust host, save output as `examples/expected_output.txt`. Commit this file. This is the canonical reference — all other hosts must match it exactly.

  **QA**:
  ```bash
  LD_LIBRARY_PATH="target/release" \
  cargo run --release --manifest-path examples/hosts/rust/Cargo.toml \
    > examples/expected_output.txt
  cat examples/expected_output.txt
  wc -l examples/expected_output.txt
  # Expected: 14 lines, content matches the uniform output spec above
  ```

  **Commit**: `test(examples): add golden output file`

- [ ] **Task 27: Diff all hosts against golden**

  **What**: Verify all 5 non-Rust hosts produce byte-for-byte identical output to the golden file.

  **QA**:
  ```bash
  for host in cpp csharp python lua js_deno; do
    diff -u examples/expected_output.txt examples/_out/${host}.txt
    if [ $? -ne 0 ]; then
      echo "FAIL: ${host} output differs from golden"
      exit 1
    fi
    echo "OK: ${host} matches golden"
  done
  # Expected: prints "OK: <host> matches golden" 5 times, exits 0
  ```

  **Commit**: `test(examples): verify all 6 hosts produce identical output`

---

## WAVE 6: Build Script + Docs

> **Blockers**: Tasks 12–27
> **Parallelism**: Tasks 28–29 parallel

---

- [ ] **Task 28: Update `examples/build.sh`**

  **What**: Rewrite `build.sh` to:
  1. Build all 5 loader `.so` files: `cargo build --release -p polyplug_dotnet` etc.
  2. Build all Rust guests
  3. Build all C++ guests (`make`)
  4. Build all C# guests (`dotnet build`)
  5. Build all JS QuickJS guests (rolldown bundle or committed `bundle.js` — no build needed if pre-bundled)
  6. Build compiled hosts: Rust (`cargo build --release`), C++ (`make`), C# (`dotnet build`)
  7. Interpreted hosts (Python, Lua, Deno) — no build step, noted in comments

  **Must NOT include**: Any `cargo build --manifest-path examples/hosts/lua/Cargo.toml` or `examples/hosts/js/Cargo.toml` — those Rust cdylibs no longer exist.

  **QA**:
  ```bash
  ./examples/build.sh
  echo $?
  # Expected: 0
  ```

  **Commit**: `build(examples): update build.sh for real multi-language hosts`

- [ ] **Task 29: Update `examples/README.md` and `examples/api.toml`**

  **`api.toml`**: Ensure it defines exactly the two contracts from Epic 27 spec. Do not change if already correct.

  **`README.md`**: Full rewrite per the spec in Epic 27 (hosts table, guests table, running instructions, QuickJS cannot-be-host note).

  **QA**:
  ```bash
  grep -q "data.Transformer" examples/api.toml
  grep -q "data.Reporter" examples/api.toml
  grep -q "js_deno" examples/README.md
  grep -q "QuickJS cannot be a host" examples/README.md
  echo $?
  # Expected: 0
  ```

  **Commit**: `docs(examples): update README and api.toml`

---

## FINAL VERIFICATION WAVE

> **Blockers**: All previous waves complete
> **Parallelism**: F1–F4 fully parallel

---

- [ ] **F1: Full test suite**

  ```bash
  cargo test --workspace
  echo $?
  # Expected: 0
  ```

- [ ] **F2: Clippy**

  ```bash
  cargo clippy --workspace -- -D warnings
  echo $?
  # Expected: 0
  ```

- [ ] **F3: No `.unwrap()` in production code**

  ```bash
  grep -rn "\.unwrap()" --include="*.rs" crates/*/src/ \
    | grep -v "#\[cfg(test)\]" \
    | grep -v "//.*unwrap" \
    | wc -l
  # Expected: 0
  ```

- [ ] **F4: All `unsafe` blocks have `// SAFETY:` comment**

  ```bash
  grep -rn "unsafe" --include="*.rs" crates/*/src/ \
    | grep -vE "(//.*unsafe|/\*.*unsafe|// SAFETY:|#\[cfg\(test\)\])" \
    | grep -vE "^\s*//" \
    | wc -l
  # Expected: 0
  ```

---

## SUCCESS CRITERIA

1. `nm -D target/release/libpolyplug.so | grep polyplug_runtime_register_loader` — symbol present
2. `nm -D target/release/libpolyplug_dotnet.so | grep polyplug_dotnet_loader_create` — symbol present (same for python, lua, js, js_deno)
3. `find examples/hosts -name "Cargo.toml" -not -path "*/rust/*" | wc -l` — prints 0
4. `test -d examples/hosts/js_deno && test ! -d examples/hosts/js` — exits 0
5. `test -d examples/guests/js_quickjs && test -d examples/guests/js_deno && test ! -d examples/guests/js` — exits 0
6. All 6 hosts produce 14 lines of output matching `examples/expected_output.txt` — `diff` exits 0 for all 5 comparisons
7. `cargo test --workspace` — exits 0
8. `cargo clippy --workspace -- -D warnings` — exits 0
9. No `.unwrap()` in production Rust — count is 0
10. No `unsafe` without `// SAFETY:` — count is 0
