# GuestContractInterface Design Rationale

> **Historical Document** — This document describes the design rationale for the `GuestContractInterface` architecture (previously called `PluginInterface`). The implementation is complete and this document is preserved for historical reference. For current implementation details, see the code in `crates/polyplug_abi/src/guest/` and `sdks/*/abi/`.

## Terminology Note

This document uses terminology renamed in v1.1:
- **GuestContractInterface**: Previously called "PluginInterface" or "vtable"
- **RuntimeAbi**: Previously called "HostVTable"
- **Guest Contract**: A contract implemented by plugins
- **Host Contract**: A contract provided by the host to plugins

## Overview

This document explains the design decisions behind the `GuestContractInterface` architecture, which replaces the previous `PluginInterface` design. The key goals are:

1. **Zero overhead for native plugins** - Direct function call, no indirection
2. **Minimal overhead for VM plugins** - One dispatch call, no global state
3. **No global state** - Per-plugin isolation, multiple runtimes can coexist
4. **Loader flexibility** - Each loader controls its own dispatch mechanism

---

## The Problem with the Old PluginInterface

### Previous Architecture

```rust
// OLD: PluginInterface forced all loaders into the same pattern
#[repr(C)]
pub struct OldPluginInterface {
    pub contract_id: u64,
    pub contract_version: u32,
    pub function_count: u32,
    pub functions: *const *const (),  // Array of native function pointers
}
```

**For native plugins:** Perfect. `functions[i]` is a direct pointer to compiled machine code.

**For VM plugins (Lua, JS, Python):** Broken. VM functions are NOT native function pointers. They require:
- A VM state (Lua state, JS context, Python interpreter)
- VM-specific calling convention

### The Workaround (Broken)

VM loaders used **static trampolines** + **global registries**:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  OLD ARCHITECTURE (BROKEN)                                                   │
│                                                                              │
│  vtable.functions[0] = trampoline_0  (static Rust function)                 │
│                                                                              │
│  trampoline_0(args, out) {                                                  │
│      // PROBLEM: Which VM state to use?                                     │
│      // PROBLEM: Which VM function to call?                                 │
│      lua_fn = GLOBAL_REGISTRY[0]  // Global state!                          │
│      lua_fn.call(args, out)                                                 │
│  }                                                                           │
│                                                                              │
│  Issues:                                                                     │
│  - Global state violates AGENTS.md                                          │
│  - Multiple runtimes corrupt each other                                     │
│  - Registry lookup adds overhead                                            │
│  - 64 trampoline slots limit                                                │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## The Solution: GuestContractInterface

### New Architecture

```rust
#[repr(C)]
pub struct GuestContractInterface {
    pub rt_ctx: *const HostContext,      // Per-plugin runtime context
    pub contract_id: u64,
    pub contract_version: u32,
    pub function_count: u32,
    pub dispatch_type: DispatchType,     // Native or VM?
    pub dispatch: DispatchMechanisms,    // Union of dispatch mechanisms
}

#[repr(C)]
pub enum DispatchType {
    Native = 0,
    VirtualMachine = 1,
}

#[repr(C)]
pub union DispatchMechanisms {
    native: NativeDispatch,
    vm: VmDispatch,
}

#[repr(C)]
pub struct NativeDispatch {
    pub functions: *const *const (),  // Direct function pointers
}

#[repr(C)]
pub struct VmDispatch {
    pub call: unsafe extern "C" fn(
        loader_data: *mut c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
    pub loader_data: *mut c_void,  // Loader-specific data
}
```

### Host Caller (Generated Code)

```rust
pub fn decode(&self, input: StringView) -> Result<StringView, ContractError> {
    let interface = self.guard.interface();
    
    if interface.dispatch_type == DispatchType::Native {
        // Native: Direct call, zero overhead
        let fn_ptr = *interface.dispatch.native.functions.add(0);
        let f: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = 
            core::mem::transmute(fn_ptr);
        f(args_ptr, out_ptr)
    } else {
        // VM: One dispatch call, no global lookup
        (interface.dispatch.vm.call)(
            interface.dispatch.vm.loader_data,
            0,  // fn_id
            args_ptr,
            out_ptr
        )
    }
}
```

---

## Overhead Analysis

### Native Plugins: Zero Overhead

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  NATIVE DISPATCH FLOW                                                        │
│                                                                              │
│  1. Branch check: dispatch_type == Native                                   │
│     Cost: ~0 cycles (perfectly predicted - same result every time)          │
│                                                                              │
│  2. Load function pointer: fn_ptr = functions[fn_id]                        │
│     Cost: ~1-3 cycles (L1 cache hit)                                        │
│                                                                              │
│  3. Indirect call: fn_ptr(args, out)                                        │
│     Cost: ~5-15 cycles (indirect branch)                                    │
│                                                                              │
│  Total: ~6-18 cycles (~2-5 ns on modern CPU)                                │
│                                                                              │
│  This is THE SAME as the old native interface architecture.                  │
│  Zero additional overhead.                                                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Why the branch is free:**

Modern CPUs have sophisticated branch predictors. For a given `PluginInterface`:
- The `dispatch_type` is set once at load time
- Every call uses the same branch direction
- After 1-2 calls, the CPU predicts correctly 100% of the time
- A correctly predicted branch costs 0 cycles

### VM Plugins: Minimal Overhead

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  VM DISPATCH FLOW                                                            │
│                                                                              │
│  1. Branch check: dispatch_type == VM                                       │
│     Cost: ~0 cycles (perfectly predicted)                                   │
│                                                                              │
│  2. Load dispatch function: call = vm.call                                  │
│     Cost: ~1-3 cycles                                                       │
│                                                                              │
│  3. Call dispatch: call(loader_data, fn_id, args, out)                      │
│     Cost: ~5-15 cycles (indirect call)                                      │
│                                                                              │
│  4. Inside dispatch:                                                        │
│     - Cast loader_data to LuaLoaderData/JsLoaderData/etc.                   │
│     - Access functions[fn_id] directly (no global lookup!)                  │
│     - Call VM function                                                       │
│                                                                              │
│  Total dispatch overhead: ~10-20 cycles (~3-6 ns)                           │
│  VM call overhead: varies by VM (Lua ~50ns, JS ~100ns)                      │
│                                                                              │
│  Key improvement: NO global registry lookup!                                │
└─────────────────────────────────────────────────────────────────────────────┘
```

**Comparison:**

| Approach | Native | VM (Lua) | VM (JS) |
|----------|--------|----------|---------|
| Old (trampoline + global) | ~5 ns | ~100 ns | ~200 ns |
| New (dispatch + loader_data) | ~5 ns | ~60 ns | ~120 ns |
| Improvement | 0% | 40% | 40% |

---

## Design Decisions

### Why a Union Instead of Two Structs?

**Option A: Two separate structs**
```rust
struct NativeGuestContractInterface { ... }
struct VmGuestContractInterface { ... }
```
Problem: Host code needs to know which type at compile time. But host code is generated once and must work with any plugin type.

**Option B: Union (chosen)**
```rust
union DispatchMechanisms {
    native: NativeDispatch,
    vm: VmDispatch,
}
```
Solution: Single type, runtime dispatch based on `dispatch_type`. Generated code handles both cases.

### Why loader_data Instead of Storing Functions in GuestContractInterface?

**Option A: Store functions in GuestContractInterface**
```rust
struct GuestContractInterface {
    functions: *const *const (),  // Works for native, useless for VM
}
```
Problem: VM functions aren't native pointers. Can't store `mlua::Function` or `v8::Global<v8::Function>` in a `*const ()`.

**Option B: loader_data (chosen)**
```rust
struct VmDispatch {
    call: dispatch_fn,
    loader_data: *mut c_void,  // Points to LuaLoaderData, JsLoaderData, etc.
}
```
Solution: Each loader defines its own data structure. Type-safe internally, opaque to host.

### Why Not Eliminate Trampolines Entirely?

**For native:** No trampolines. Direct function pointers.

**For VM:** Trampolines are necessary because:
1. VM functions are NOT native function pointers
2. VM calling convention requires VM-specific setup (scope, context, etc.)
3. The dispatch function IS the "trampoline" - it bridges native and VM worlds

The key improvement: **One dispatch function per loader**, not 64 static trampolines per plugin.

### Why Not Use libffi or JIT?

**libffi closures:**
- Adds external dependency
- ~50-100 ns overhead per call
- More complex memory management

**JIT-generated thunks:**
- Platform-specific code generation
- Security concerns (executable memory)
- Significant complexity

**Our approach:**
- No external dependencies
- Predictable overhead
- Simple, auditable code

---

## Loader Data Structures

Each VM loader defines its own data structure:

```rust
// Lua: Store mlua::Function directly
struct LuaLoaderData {
    functions: Vec<mlua::Function>,
}

// JS/QuickJS: Store context + functions
struct JsLoaderData {
    context: *mut Context,
    functions: Vec<StoredJsFunction>,
}

// JS/Deno: Store V8 globals (no channels!)
struct DenoLoaderData {
    isolate: *mut v8::Isolate,
    context: v8::Global<v8::Context>,
    functions: Vec<v8::Global<v8::Function>>,
}

// Python: Store Py<PyAny> callables
struct PythonLoaderData {
    functions: Vec<Py<PyAny>>,
}

// .NET: Store managed function pointers
struct DotnetLoaderData {
    functions: Vec<ManagedFunctionPtr>,
}
```

**Key insight:** Each loader has complete control over its data. No global state. No shared registries.

---

## Dispatch Function Implementations

The dispatch function IS the implementation. No wrappers:

```rust
// Lua dispatch - ALL calling logic here
unsafe extern "C" fn lua_dispatch(
    loader_data: *mut c_void,
    fn_id: u32,
    args: *const (),
    out: *mut (),
) -> AbiError {
    let data = &*(loader_data as *const LuaLoaderData);
    let lua_fn = &data.functions[fn_id as usize];
    lua_fn.call::<()>((args as i64, out as i64))
        .map(|_| AbiError::ok())
        .unwrap_or(AbiError::new(ABI_ERROR_GENERIC))
}

// Deno dispatch - ALL V8 logic here (no channels!)
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
    func.call(scope, ctx, &[]);
    AbiError::ok()
}
```

---

## Summary

| Aspect | Old (PluginInterface) | New (GuestContractInterface) |
|--------|-------------------|----------------------|
| Native overhead | ~5 ns | ~5 ns (zero change) |
| VM overhead | ~100-200 ns | ~60-120 ns (40% faster) |
| Global state | Yes (violates AGENTS.md) | No |
| Multiple runtimes | Broken | Works |
| Trampolines | 64 static per plugin | 1 dispatch per loader |
| Loader flexibility | Forced into functions array | Complete control |

**The GuestContractInterface design achieves:**
1. Zero overhead for native (critical path)
2. Minimal overhead for VM (one call, no lookup)
3. No global state (per-plugin isolation)
4. Loader flexibility (each loader controls dispatch)
5. Simple, auditable code (no JIT, no libffi)