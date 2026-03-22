# PluginInterface Refactor Plan

## Overview

Refactor `PluginVTable` to `PluginInterface` with hybrid dispatch:
- **Native plugins:** Zero overhead (direct function call)
- **VM plugins:** One call through dispatch function, no global state

## Architecture

```rust
#[repr(C)]
pub struct PluginInterface {
    pub rt_ctx: *const HostContext,
    pub contract_id: u64,
    pub contract_version: u32,
    pub function_count: u32,
    pub dispatch_type: DispatchType,
    pub dispatch: PluginDispatch,
}

#[repr(C)]
pub enum DispatchType {
    Native = 0,
    VirtualMachine = 1,
}

#[repr(C)]
pub union PluginDispatch {
    native: NativeDispatch,
    vm: VmDispatch,
}

#[repr(C)]
pub struct NativeDispatch {
    pub functions: *const *const (),
}

#[repr(C)]
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,
}
```

### Loader Data Structures

Each VM loader stores its own data needed for direct function dispatch:

```rust
// Lua loader
struct LuaLoaderData {
    functions: Vec<mlua::Function>,  // Direct function references
}

// JS/QuickJS loader
struct JsLoaderData {
    context: *mut Context,            // Keep context alive
    functions: Vec<StoredJsFunction>, // Direct function references
}

// JS/Deno loader (no channels!)
struct DenoLoaderData {
    isolate: *mut v8::Isolate,
    context: v8::Global<v8::Context>,
    functions: Vec<v8::Global<v8::Function>>,
}

// Python loader
struct PythonLoaderData {
    functions: Vec<Py<PyAny>>,  // Python callable objects
}

// .NET loader
struct DotnetLoaderData {
    functions: Vec<ManagedFunctionPtr>,  // .NET function pointers
}
```

### Dispatch Function Implementations

**The dispatch function IS the implementation.** No wrapper functions. All VM calling logic is in the dispatch function itself:

```rust
// Lua dispatch - contains ALL Lua calling logic
unsafe extern "C" fn lua_dispatch(
    loader_data: *mut c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let data = &*(loader_data as *const LuaLoaderData);
    let lua_fn = &data.functions[fn_id as usize];
    // Call mlua::Function directly - all logic here
    lua_fn.call::<()>((args as i64, out as i64))
        .map(|_| AbiError::ok())
        .unwrap_or(AbiError::new(ABI_ERROR_GENERIC))
}

// JS/QuickJS dispatch - contains ALL QuickJS calling logic
unsafe extern "C" fn js_dispatch(
    loader_data: *mut c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let data = &*(loader_data as *const JsLoaderData);
    let ctx = &*data.context;
    ctx.with(|ctx_ref| {
        let func = &data.functions[fn_id as usize];
        // All QuickJS calling logic here
        func.call(args, out)
    });
    AbiError::ok()
}

// Deno dispatch - contains ALL V8 calling logic (no channels!)
unsafe extern "C" fn deno_dispatch(
    loader_data: *mut c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let data = &*(loader_data as *const DenoLoaderData);
    let scope = &mut v8::HandleScope::new(data.isolate);
    let ctx = data.context.get(scope);
    let func = data.functions[fn_id as usize].get(scope);
    // All V8 calling logic here - direct, no channel
    let result = func.call(scope, ctx, &[]);
    // Write result to out...
    AbiError::ok()
}
```

**Key insight:** One function per loader. Direct. No wrappers. No global state. No channels.

---

## Phase 1: ABI Core Changes

### [x] 1.1: Update polyplug_abi types
- File: `crates/polyplug_abi/src/lib.rs`
- Add `DispatchType` enum
- Add `NativeDispatch` struct
- Add `VmDispatch` struct
- Add `PluginDispatch` union
- Rename `PluginVTable` to `PluginInterface`
- Update all doc comments
- Verification: `cargo build -p polyplug_abi`

### [x] 1.2: Update polyplug_abi tracking module
- File: `crates/polyplug_abi/src/tracking.rs`
- Update any references to `PluginVTable`
- Verification: `cargo test -p polyplug_abi`

### [x] 1.3: Update polyplug_abi ffi module
- File: `crates/polyplug_abi/src/ffi.rs`
- Update any FFI declarations referencing `PluginVTable`
- Verification: `cargo build -p polyplug_abi`

---

## Phase 2: Core Runtime Changes

### [x] 2.1: Update polyplug registry
- File: `crates/polyplug/src/registry.rs`
- Update `PluginEntry` to use `PluginInterface`
- Update `resolve()` return type
- Verification: `cargo build -p polyplug`

### [x] 2.2: Update polyplug runtime
- File: `crates/polyplug/src/runtime.rs`
- Update `host_register_plugin` to handle new `PluginInterface`
- Update any vtable references
- Verification: `cargo build -p polyplug`

### [x] 2.3: Update polyplug loader module
- File: `crates/polyplug/src/loader/mod.rs`
- Update `load_bundle` to use `PluginInterface`
- Verification: `cargo build -p polyplug`

### [x] 2.4: Update polyplug error types
- File: `crates/polyplug/src/error.rs`
- Add any new error variants needed
- Verification: `cargo build -p polyplug`

---

## Phase 3: Native Loader (Reference Implementation)

### [x] 3.1: Update native loader
- File: `crates/polyplug_native/src/loader.rs`
- Build `PluginInterface` with `DispatchType::Native`
- Store function pointers in `NativeDispatch.functions`
- Verification: `cargo build -p polyplug_native`

### [x] 3.2: Update native loader tests
- File: `crates/polyplug_native/src/lib.rs` (if has tests)
- Verification: `cargo test -p polyplug_native`

---

## Phase 4: VM Loaders (Parallel Group)

### [PARALLEL GROUP: VM_LOADERS]

### [x] 4.1: Update Lua loader
- Files: `crates/polyplug_lua/src/loader.rs`, `crates/polyplug_lua/src/lib.rs`
- Create `LuaLoaderData` struct
- Implement `lua_dispatch` function
- Remove `FUNCTION_REGISTRY` global
- Remove trampolines
- Build `PluginInterface` with `DispatchType::VirtualMachine`
- Verification: `cargo build -p polyplug_lua`

### [x] 4.2: Update Lua loader tests
- File: `crates/polyplug_lua/tests/lua_loader.rs`
- Update any manual vtable creation
- Verification: `cargo test -p polyplug_lua -- --test-threads=1`

### [x] 4.3: Update JS/QuickJS loader
- Files: `crates/polyplug_js/src/loader.rs`, `crates/polyplug_js/src/lib.rs`
- Create `JsLoaderData` struct
- Implement `js_dispatch` function
- Remove `FUNCTION_REGISTRY`, `JS_CONTEXTS`, `SLOT_TO_BUNDLE` globals
- Remove trampolines
- Remove thread_locals: `CURRENT_CONTEXT`, `CURRENT_BUNDLE_ID`
- Build `PluginInterface` with `DispatchType::VirtualMachine`
- Verification: `cargo build -p polyplug_js`

### [x] 4.4: Update JS/QuickJS loader tests
- File: `crates/polyplug_js/tests/quickjs_loader.rs`
- Update any manual vtable creation
- Verification: `cargo test -p polyplug_js -- --test-threads=1`

### [x] 4.5: Update JS/Deno loader
- Files: `crates/polyplug_js_deno/src/loader.rs`, `crates/polyplug_js_deno/src/lib.rs`
- Create `DenoLoaderData` struct with direct V8 references:
  ```rust
  struct DenoLoaderData {
      isolate: *mut v8::Isolate,
      context: v8::Global<v8::Context>,
      functions: Vec<v8::Global<v8::Function>>,
  }
  ```
- Implement `deno_dispatch` function (direct V8 call, no channels)
- Remove `DENO_FUNCTION_REGISTRY` global
- Remove trampolines
- Remove channel-based dispatch (no longer needed)
- Remove dedicated bundle thread spawning
- Build `PluginInterface` with `DispatchType::VirtualMachine`
- Note: Deno is single-threaded by design; direct dispatch aligns with its model
- Verification: `cargo build -p polyplug_js_deno`

### [x] 4.6: Update JS/Deno loader tests
- File: `crates/polyplug_js_deno/tests/deno_loader.rs`
- Update any manual vtable creation
- Verification: `cargo test -p polyplug_js_deno -- --test-threads=1`

### [x] 4.7: Update Python loader
- Files: `crates/polyplug_python/src/lib.rs`, `crates/polyplug_python/src/context.rs`
- Create `PythonLoaderData` struct
- Implement `python_dispatch` function
- Build `PluginInterface` with `DispatchType::VirtualMachine`
- Verification: `cargo build -p polyplug_python`

### [x] 4.8: Update Python loader tests
- File: `crates/polyplug_python/tests/python_loader.rs`
- Update any manual vtable creation
- Verification: `cargo test -p polyplug_python -- --test-threads=1`

### [x] 4.9: Update .NET loader
- Files: `crates/polyplug_dotnet/src/lib.rs`, `crates/polyplug_dotnet/src/context.rs`
- Create `DotnetLoaderData` struct
- Implement `dotnet_dispatch` function
- Remove `CLR_CONTEXT` global (make per-runtime)
- Build `PluginInterface` with `DispatchType::VirtualMachine`
- Verification: `cargo build -p polyplug_dotnet`

### [x] 4.10: Update .NET loader tests
- File: `crates/polyplug_dotnet/tests/dotnet_loader.rs`
- Update any manual vtable creation
- Verification: `cargo test -p polyplug_dotnet -- --test-threads=1`

---

## Phase 5: Code Generators (Parallel Group)

### [PARALLEL GROUP: CODEGEN]

### [x] 5.1: Update Rust generator
- File: `crates/polyplug_codegen/src/generators/rust.rs`
- Update host caller to use dispatch pattern:
  ```rust
  if vtable.dispatch_type == DispatchType::Native {
      let fn_ptr = *vtable.dispatch.native.functions.add(fn_id);
      fn_ptr(args, out)
  } else {
      (vtable.dispatch.vm.call)(vtable.dispatch.vm.loader_data, fn_id, args, out)
  }
  ```
- Update guest vtable generation
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.2: Update C++ generator
- File: `crates/polyplug_codegen/src/generators/cpp.rs`
- Same dispatch pattern as Rust
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.3: Update C# generator
- File: `crates/polyplug_codegen/src/generators/csharp.rs`
- Same dispatch pattern
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.4: Update Python generator
- File: `crates/polyplug_codegen/src/generators/python.rs`
- Same dispatch pattern
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.5: Update Lua generator
- File: `crates/polyplug_codegen/src/generators/lua.rs`
- Same dispatch pattern
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.6: Update JS/Deno generator
- File: `crates/polyplug_codegen/src/generators/js_deno.rs`
- Same dispatch pattern
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.7: Update JS/QuickJS generator
- File: `crates/polyplug_codegen/src/generators/js_quickjs.rs`
- Same dispatch pattern
- Verification: `cargo build -p polyplug_codegen`

### [x] 5.8: Run codegen tests
- Files: `crates/polyplug_codegen/tests/*.rs`
- Verification: `cargo test -p polyplug_codegen`

---

## Phase 6: Host Libraries (Parallel Group)

### [PARALLEL GROUP: HOST_LIBS]

### [x] 6.1: Update Rust host library
- Files: `host-libs/rust/src/**/*.rs`
- Update `PluginInterface` struct definition
- Verification: `cargo build` in `host-libs/rust`

### [x] 6.2: Update C++ host library
- Files: `host-libs/cpp/include/**/*.hpp`, `host-libs/cpp/src/**/*.cpp`
- Update `PluginInterface` struct
- Verification: Build succeeds

### [x] 6.3: Update C# host library
- Files: `host-libs/csharp/src/**/*.cs`
- Update `PluginInterface` struct
- Verification: `dotnet build` in `host-libs/csharp`

### [x] 6.4: Update Python host library
- Files: `host-libs/python/polyplug/**/*.py`
- Update `PluginInterface` ctypes definition
- Verification: Python import succeeds

### [x] 6.5: Update Lua host library
- Files: `host-libs/lua/polyplug/**/*.lua`
- Update `PluginInterface` FFI definition
- Verification: Lua require succeeds

### [x] 6.6: Update JS host library
- Files: `host-libs/js-deno/**/*.ts`
- Update `PluginInterface` type definition
- Verification: TypeScript compile succeeds

---

## Phase 7: Guest Libraries (Parallel Group)

### [PARALLEL GROUP: GUEST_LIBS]

### [x] 7.1: Update Rust guest library
- Files: `guest-libs/rust/src/**/*.rs`
- Update any vtable-related code
- Verification: `cargo build` in `guest-libs/rust`

### [x] 7.2: Update C++ guest library
- Files: `guest-libs/cpp/include/**/*.hpp`
- Update vtable struct
- Verification: Build succeeds

### [x] 7.3: Update C# guest library
- Files: `guest-libs/csharp/src/**/*.cs`
- Update vtable struct
- Verification: `dotnet build` in `guest-libs/csharp`

### [x] 7.4: Update Python guest library
- Files: `guest-libs/python/polyplug_guest/**/*.py`
- Update `PluginInterface` ctypes definition in `abi.py`
- Verification: Python import succeeds

### [x] 7.5: Update Lua guest library
- Files: `guest-libs/lua/polyplug_guest/**/*.lua`
- Update FFI definitions
- Verification: Lua require succeeds

### [x] 7.6: Update JS guest library
- Files: `guest-libs/js/**/*.ts`, `guest-libs/js/**/*.js`
- Update type definitions
- Verification: TypeScript compile succeeds

---

## Phase 8: Regenerate Examples

### [x] 8.1: Regenerate all example guests
- Run `polyplugc generate` for all example bundles
- Directories: `examples/guests/*/generated/`
- Verification: All generated files compile

### [x] 8.2: Regenerate all example hosts
- Run `polyplugc generate` for all example hosts
- Directories: `examples/hosts/*/generated/`
- Verification: All hosts build

### [x] 8.3: Build all examples
- Run `./examples/build_all.sh`
- Verification: Script exits 0

---

## Phase 9: Update Tests

### [x] 9.1: Update polyplug core tests
- Files: `crates/polyplug/tests/**/*.rs`
- Update any manual `PluginVTable` creation to `PluginInterface`
- Verification: `cargo test -p polyplug`

### [x] 9.2: Update polyplugc tests
- Files: `crates/polyplugc/tests/**/*.rs`
- Update vtable references
- Verification: `cargo test -p polyplugc`

### [x] 9.3: Update integration tests
- Files: `tests/integration/tests/**/*.rs`
- Update all vtable creation
- Verification: `cargo test --test integration`

### [x] 9.4: Update benchmark tests
- File: `crates/polyplug/benches/vtable_dispatch.rs`
- Update to use new dispatch pattern
- Verification: `cargo bench` runs

---

## Phase 10: Final Verification

### [x] 10.1: Full workspace build
- Command: `cargo build --workspace`
- Verification: Zero errors

### [x] 10.2: Full workspace test
- Command: `cargo test --workspace`
- Verification: All tests pass

### [x] 10.3: Clippy check
- Command: `cargo clippy --workspace -- -D warnings`
- Verification: Zero warnings

### [x] 10.4: Format check
- Command: `cargo fmt --check`
- Verification: Zero issues

### [x] 10.5: Run integration tests with all loaders
- Command: `cargo test --test integration`
- Verification: All cross-language tests pass

### [x] 10.6: Run examples end-to-end
- Run each example host with its plugins
- Verification: All examples produce correct output

---

## Summary

| Phase | Description | Tasks |
|-------|-------------|-------|
| 1 | ABI Core Changes | 3 |
| 2 | Core Runtime Changes | 4 |
| 3 | Native Loader | 2 |
| 4 | VM Loaders (Parallel) | 10 |
| 5 | Code Generators (Parallel) | 8 |
| 6 | Host Libraries (Parallel) | 6 |
| 7 | Guest Libraries (Parallel) | 6 |
| 8 | Regenerate Examples | 3 |
| 9 | Update Tests | 4 |
| 10 | Final Verification | 6 |

**Total: 52 tasks**

**Parallel Groups:**
- Phase 4 (VM_LOADERS): 10 tasks in parallel
- Phase 5 (CODEGEN): 8 tasks in parallel
- Phase 6 (HOST_LIBS): 6 tasks in parallel
- Phase 7 (GUEST_LIBS): 6 tasks in parallel

**Blockers:**
- Phase 1 must complete before all others
- Phase 2 must complete before Phase 3-7
- Phases 3-7 can run in parallel after Phase 2
- Phase 8 requires Phases 5-7 complete
- Phase 9 requires Phase 8 complete
- Phase 10 requires all previous phases complete