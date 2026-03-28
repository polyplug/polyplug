# Host Contracts Migration Plan - GENERATED CODE APPROACH

## TL;DR

> **Quick Summary**: **THE ULTIMATE PLAN** - Generate host contract vtable factories for ALL 6 languages with **CUSTOM ENUM TYPES**! `create_logger_vtable(Box::new(impl))` with `LogLevel` enum - **GENERATED**, not manual!
> 
> **Deliverables**: 
> - **ABI FIX**: Add `impl_ptr` to `NativeHostContractDispatch`
> - **POLYPLUGC ENHANCEMENT**: Generate vtable factories for ALL 6 languages
> - **CUSTOM TYPES**: `LogLevel` enum with Debug/Info/Warn/Error variants
> - **`log_with_level(level, message)`**: Level FIRST, custom enum type!
> - **NO MANUAL CODE**: Everything generated, everything perfect
> 
> **Estimated Effort**: Large (~18-20 hours)
> **Critical Path**: Task 0 (ABI fix) → Task 0.5 (example fixes) → Task 1-3 (codegen ALL languages) → examples migration

---

## Design Document: Host Vtable Factory ABI

### Problem Statement
Current manual vtable creation is error-prone and defeats the purpose of code generation. We need generated vtable factories like guests have.

---

## Type Layout System - COMPLETE REFERENCE

### How Polyplug Calculates Layouts

**Polyplug relies on Rust's `#[repr(C)]`** - NOT custom layout calculation. The compiler handles everything via:
- `size_of<T>()` - Total size including padding
- `align_of<T>()` - Alignment requirement
- `offset_of!(T, field)` - Exact byte offset of each field

**Key Insight**: Layout is determined at compile time by the Rust compiler, not by polyplug code.

### Complete Type Layout Table (x86_64)

#### Primitive Types
| Type | Size (bytes) | Alignment | Notes |
|------|--------------|-----------|-------|
| `u8`, `i8`, `bool` | 1 | 1 | No padding |
| `u16`, `i16` | 2 | 2 | 2-byte aligned |
| `u32`, `i32`, `f32` | 4 | 4 | 4-byte aligned |
| `u64`, `i64`, `f64` | 8 | 8 | 8-byte aligned |
| `usize`, `isize` | 8 | 8 | Pointer-sized |

#### ABI Built-in Types (from `polyplug_abi/src/lib.rs`)
```rust
#[repr(C)]
pub struct StringView {
    pub ptr: *const u8,    // offset: 0,  size: 8, align: 8
    pub len: usize,        // offset: 8,  size: 8, align: 8
}
// Total: 16 bytes, Alignment: 8

#[repr(C)]
pub struct Buffer {
    pub ptr: *mut u8,      // offset: 0,  size: 8, align: 8
    pub len: usize,        // offset: 8,  size: 8, align: 8
    pub cap: usize,        // offset: 16, size: 8, align: 8
}
// Total: 24 bytes, Alignment: 8
```

| Type | Size | Align | Field Offsets |
|------|------|-------|---------------|
| **StringView** | 16 | 8 | ptr:0, len:8 |
| **Buffer** | 24 | 8 | ptr:0, len:8, cap:16 |
| **AbiError** | 24 | 8 | code:0, message:8 |
| **PluginHandle** | 8 | 4 | index:0, generation:4 |
| **HostContext** | 16 | 8 | runtime:0, bundle_id:8 |

#### Enum Types
| Repr | Size | Alignment | Underlying Type |
|------|------|-----------|-----------------|
| `u8` | 1 | 1 | `uint8_t` / `u8` |
| `u16` | 2 | 2 | `uint16_t` / `u16` |
| `u32` | 4 | 4 | `uint32_t` / `u32` |
| `u64` | 8 | 8 | `uint64_t` / `u64` |

**Example**: `LogLevel` with `repr = "u32"` is **4 bytes, align 4**.

### Struct Layout Calculation Algorithm

**For `#[repr(C)]` structs**, fields are laid out in declaration order with this algorithm:

```
for each field in struct.fields:
    // Align current offset to field's alignment
    current_offset = round_up(current_offset, field.align)
    
    // Assign field offset
    field.offset = current_offset
    
    // Advance offset by field size
    current_offset += field.size

// Final size aligned to struct's alignment (max of all fields)
struct.size = round_up(current_offset, struct.align)
struct.align = max(field.align for all fields)
```

**Where `round_up(n, align) = ((n + align - 1) / align) * align`**

### Example with Padding Calculation

**`LogWithLevelArgs`** for `log_with_level(level, message)`:

```rust
#[repr(C)]
struct LogWithLevelArgs {
    level: LogLevel,      // size: 4, align: 4
    // PADDING: 4 bytes (to align message to 8)
    message: StringView,  // size: 16, align: 8, offset: 8
}
```

**Step-by-step:**
1. Start: `current_offset = 0`
2. Field `level` (align 4): `round_up(0, 4) = 0` → offset 0
3. Size 4: `current_offset = 0 + 4 = 4`
4. Field `message` (align 8): `round_up(4, 8) = 8` → offset 8
5. **Padding inserted: 4 bytes (offset 4-7)**
6. Size 16: `current_offset = 8 + 16 = 24`
7. Struct align = max(4, 8) = 8
8. Final size: `round_up(24, 8) = 24`

**Result: 24 bytes total** (NOT 20!)

### Cross-Language Layout Consistency

**All languages MUST match these layouts exactly:**

| Language | Enum Pattern | Struct Pattern |
|----------|--------------|----------------|
| **Rust** | `#[repr(u32)] enum LogLevel { ... }` | `#[repr(C)] struct Args { ... }` |
| **C++** | `enum class LogLevel : uint32_t { ... }` | `struct Args { ... };` |
| **C#** | `enum LogLevel : uint { ... }` | `[StructLayout(LayoutKind.Sequential)] struct Args { ... }` |
| **Python** | `ctypes.c_uint32` values | `ctypes.Structure` with `_fields_` |
| **Lua** | `ffi.cdef[[enum { ... }]]` | `ffi.cdef[[struct { ... }]]` |
| **JS** | `Number` constants | `Uint8Array` view with offsets |

### VM Language Specific Notes

**For Python/Lua/JS (VM languages):**

1. **Manual padding calculation REQUIRED** - VMs don't handle padding automatically
2. **Use explicit offset values** in struct field definitions
3. **Verify with assertions**:
   ```python
   assert ctypes.sizeof(LogWithLevelArgs) == 24
   assert ctypes.offsetof(LogWithLevelArgs, message) == 8
   ```

4. **Pack args for multi-param functions**:
   ```python
   class LogWithLevelArgs(ctypes.Structure):
       _fields_ = [
           ("level", ctypes.c_uint32),      # offset 0
           ("_pad", ctypes.c_uint32),       # offset 4 (explicit padding!)
           ("message", StringView),          # offset 8
       ]
   ```

### Critical Verification Points

**Every generator MUST verify:**
1. ✅ Struct size matches expected value
2. ✅ Each field at correct offset
3. ✅ No implicit padding (explicit padding in VM languages)
4. ✅ Alignment requirements met
5. ✅ Cross-language consistency

**Test with:**
```rust
assert_eq!(size_of::<LogWithLevelArgs>(), 24);
assert_eq!(align_of::<LogWithLevelArgs>(), 8);
assert_eq!(offset_of!(LogWithLevelArgs, level), 0);
assert_eq!(offset_of!(LogWithLevelArgs, message), 8);
```

---

### Critical Bug in Current Manual Implementation
The existing example at `examples/host_contracts/logger/host/rust/src/main.rs:150` has a **bug**:
```rust
// WRONG: args is the StringView message, not the logger pointer!
let logger: &ConsoleLogger = unsafe { &*(args as *const ConsoleLogger) };
```

**Root Cause**: The thunk signature `fn(*const (), *mut ()) -> AbiError` doesn't include `impl_ptr`.

### Solution: Modified Thunk Signature
**Option B (Chosen)**: Pass `impl_ptr` as first argument:
```rust
unsafe extern "C" fn thunk(
    impl_ptr: *const (),  // NEW: Implementation pointer
    args: *const (),      // Function arguments
    _out: *mut ()         // Output pointer (unused for void returns)
) -> AbiError
```

**ABI Impact**: This is a breaking change for the host contract ABI, but acceptable pre-1.0.

### Generated Code Structure

#### Native Dispatch (Rust/C++ hosts)
```rust
// Generated in hosts/rust/src/generated/host/vtable_factories.rs

/// Create vtable for host.logger contract (NATIVE dispatch)
pub fn create_logger_vtable<T: HostLogger>(
    implementation: Box<T>
) -> &'static HostContractVTable {
    let impl_ptr: *mut T = Box::into_raw(implementation);
    
    // Generated ABI wrapper with panic safety
    unsafe extern "C" fn log_thunk(
        impl_ptr: *const (),      // Implementation pointer
        args: *const (),          // Arguments
        _out: *mut ()             // Output (void function)
    ) -> AbiError {
        // SAFETY: impl_ptr is valid and properly aligned
        let impl_ref: &T = unsafe { &*(impl_ptr as *const T) };
        
        // SAFETY: args points to valid StringView
        let message_sv: StringView = unsafe { *(args as *const StringView) };
        let message: &str = unsafe {
            std::str::from_utf8_unchecked(
                std::slice::from_raw_parts(message_sv.ptr, message_sv.len)
            )
        };
        
        // Call trait method with panic safety
        match std::panic::catch_unwind(|| {
            impl_ref.log(message);
        }) {
            Ok(_) => AbiError { code: ABI_OK, message: StringView::null() },
            Err(_) => AbiError { 
                code: ABI_PANIC, 
                message: StringView::from_static(b"host contract panicked") 
            },
        }
    }
    
    // Static function pointer array
    static FUNCTIONS: [unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError; 1] = 
        [log_thunk];
    
    let vtable = HostContractVTable {
        header: HostContractVTableHeader {
            vtable_version: 1,
            contract_id: HOSTLOGGER_CONTRACT_ID,
            contract_major: 1,
            contract_minor: 0,
            function_count: 1,
            dispatch_type: DispatchType::Native as u32,
            _padding: [0; 4],
        },
        dispatch: HostContractDispatch {
            native: NativeHostContractDispatch {
                impl_ptr: impl_ptr as *const (),
                functions: FUNCTIONS.as_ptr() as *const (),
            },
        },
    };
    
    Box::leak(Box::new(vtable))
}
```

#### VM Dispatch (Python/Lua/JS hosts)
```rust
// Generated in hosts/rust/src/generated/host/vtable_factories.rs

/// Create vtable for host.logger contract (VM dispatch)
/// Used when host is implemented in a VM language (Python, Lua, JS)
pub fn create_logger_vtable_vm(
    bridge_data: *mut c_void,  // VM-specific state
    dispatch_fn: unsafe extern "C" fn(
        bridge_data: *mut c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,
) -> &'static HostContractVTable {
    let vtable = HostContractVTable {
        header: HostContractVTableHeader {
            vtable_version: 1,
            contract_id: HOSTLOGGER_CONTRACT_ID,
            contract_major: 1,
            contract_minor: 0,
            function_count: 1,
            dispatch_type: DispatchType::VirtualMachine as u32,
            _padding: [0; 4],
        },
        dispatch: HostContractDispatch {
            vm: VmHostContractDispatch {
                call: dispatch_fn,
                bridge_data,
            },
        },
    };
    
    Box::leak(Box::new(vtable))
}
```

**VM Host Usage** (e.g., Python):
```python
# Python host implements host.logger
class ConsoleLogger:
    def log(self, message: str) -> None:
        print(f"[PLUGIN LOG] {message}")

# VM bridge dispatch function
def vm_dispatch(bridge_data, fn_id, args, out):
    logger = bridge_data.logger
    if fn_id == 0:  # log function
        message = decode_string_view(args)
        logger.log(message)
        return ABI_OK

# Use GENERATED vtable factory!
from generated.host.vtable_factories import create_logger_vtable_vm

vtable = create_logger_vtable_vm(
    bridge_data=vm_state,
    dispatch_fn=vm_dispatch
)
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
```

### Memory Ownership
- **vtable**: Leaked to `'static` via `Box::leak`
- **implementation**: Leaked via `Box::into_raw`
- **Intentional**: Both are intended to live for program lifetime
- **No cleanup**: Runtime doesn't support unregistering host contracts

### Panic Safety
All generated thunks MUST include `catch_unwind` to prevent panics crossing FFI boundary.

### Generic vs Trait Object
**Chosen**: Generic function `create_logger_vtable<T: HostLogger>(impl: Box<T>)`
- Benefits: No double indirection, monomorphized for concrete type
- Tradeoff: Code bloat if many different implementations used

---

## Language Support Scope

### Dispatch Types

**NATIVE Dispatch** (Rust/C++ hosts):
- Host implements trait directly
- Generated: `create_logger_vtable<T: HostLogger>(Box<T>)`
- Includes ABI thunks with `impl_ptr` parameter

**VM Dispatch** (Python/Lua/JS hosts):
- Host implemented in VM language
- Generated: `create_logger_vtable_vm(bridge_data, dispatch_fn)`
- VM provides dispatch function, no ABI thunks needed

### Phase 1: REQUIRED (Blocking)

**Rust Generator MUST support BOTH dispatch types:**
- **File**: `crates/polyplug_codegen/src/generators/rust.rs`
- **NATIVE**: `create_logger_vtable<T: HostLogger>(Box<T>)` for Rust hosts
- **VM**: `create_logger_vtable_vm(bridge_data, dispatch_fn)` for Python/Lua/JS hosts
- **Status**: BOTH required for plan completion

### Phase 2: REQUIRED (Blocking)

**ALL language generators MUST support BOTH dispatch types:**

**C++ Generator** (`crates/polyplug_codegen/src/generators/cpp.rs`):
- **NATIVE**: `create_logger_vtable<T: HostLogger>(std::unique_ptr<T>)` for C++ hosts
- **VM**: `create_logger_vtable_vm(void* bridge_data, VmDispatchFn dispatch_fn)` for VM hosts
- **Status**: REQUIRED

**C# Generator** (`crates/polyplug_codegen/src/generators/csharp.rs`):
- **NATIVE**: `CreateLoggerVTable<T: IHostLogger>(T implementation)` for C# hosts
- **VM**: `CreateLoggerVTableVm(IntPtr bridgeData, VmDispatchDelegate dispatchFn)` for VM hosts
- **Status**: REQUIRED

**Python Generator** (`crates/polyplug_codegen/src/generators/python.rs`):
- **NATIVE**: Python C API factories for native Python extensions
- **VM**: Python VM factories for VM-based hosts
- **Status**: REQUIRED

**Lua Generator** (`crates/polyplug_codegen/src/generators/lua.rs`):
- **NATIVE**: Lua C API factories for native Lua modules
- **VM**: Lua VM factories for VM-based hosts
- **Status**: REQUIRED

**JavaScript Generator** (`crates/polyplug_codegen/src/generators/js_deno.rs` and `js_quickjs.rs`):
- **NATIVE**: JS native bindings for Deno/QuickJS
- **VM**: JS VM factories for VM-based hosts
- **Status**: REQUIRED

**Plan Impact**:
- Task 1 (Rust generator) MUST support both NATIVE and VM
- Tasks 3, 9-13 MUST implement generators for all languages
- NO deferrals - all generators must work
- NO fallbacks - each generator generates its own code

---

## Context

### Original Request
Migrate host_contracts examples into existing flow using **GENERATED** code, not manual bridging.

### Technical Foundation
Based on research, host vtable generation is **fully feasible**:
- Guest vtables already generated successfully
- Same pattern applies to hosts
- ABI change required (add impl_ptr parameter)

### Metis Review Findings
**Identified Issues**:
- Current manual example has **critical bug** (wrong pointer usage)
- Thunk signature needs modification
- Panic safety required
- Runtime isolation must be maintained (no statics)

---

## Work Objectives

### Core Objective
Implement polyplugc vtable factory generation, then migrate examples to use it.

### Concrete Deliverables

#### Phase 1: Polyplugc Enhancement (BLOCKS Phase 2)
1. Modify `crates/polyplug_codegen/src/generators/rust.rs`
   - Add `generate_host_vtable_factories()` function
   - Generate `create_*_vtable<T: Trait>(impl: Box<T>)` functions
   - Generate ABI thunks with panic safety
   - Generate static function pointer arrays
2. Add tests for generated code
3. Update other language generators (optional/deferred)

#### Phase 2: Examples Migration (DEPENDS on Phase 1)
1. Add `host.logger` to `examples/api.toml`
2. Regenerate all code using enhanced polyplugc
3. Update 6 hosts to use `create_logger_vtable(Box::new(impl))`
4. Update 6 reporters to use `HostLoggerCaller`
5. Fix JS reporter

### Definition of Done
- [ ] polyplugc generates working vtable factories
- [ ] All generated code has panic safety
- [ ] No manual vtable creation in examples
- [ ] All examples use generated code
- [ ] `verify_hosts.sh` passes

---

## Verification Strategy

### Test Decision
- **TDD Approach**: Write tests first, then generate code
- **Test Types**: Unit tests for generator, integration tests for generated code
- **CI Integration**: Tests run automatically

---

## Execution Strategy

### Parallel Execution Waves

```
Wave 0 (Foundation - Must Complete First):
├── Task 0: FIX ABI - Add impl_ptr field [ultrabrain]
└── Task 0.5: FIX examples - Use impl_ptr correctly [quick]

Wave 1 (Core Generation - Can Parallelize):
├── Task 1: Implement vtable generation [ultrabrain]
├── Task 1.5: Update runtime for new thunk signature [unspecified-high]
└── Task 2: Add tests for vtable generation [quick]

Wave 2 (Layout Tests - Independent):
└── Task 2.5: Add 25+ layout calculation tests [quick]

Wave 3 (All Language Generators - MAX PARALLEL):
├── Task 2.75: Test generated code in examples context [quick]
├── Task 3: Implement C++ generator [unspecified-high]
├── Task 4: Implement C# generator [unspecified-high]
├── Task 5: Implement Python generator [unspecified-high]
├── Task 6: Implement Lua generator [unspecified-high]
└── Task 7: Implement JavaScript generators [unspecified-high]

Wave 4 (API Definition - Independent):
└── Task 8: Add host.logger to api.toml [quick]

Wave 5 (Examples Migration - Parallel by Language):
├── Task 9: Regenerate and update Rust example [quick]
├── Task 10: Regenerate and update C++ example [quick]
├── Task 11: Regenerate and update C# example [quick]
├── Task 12: Regenerate and update Python example [quick]
├── Task 13: Regenerate and update Lua example [quick]
└── Task 14: Regenerate and update JavaScript example [quick]

Wave FINAL (Verification - After ALL):
├── Task F1: Plan compliance audit [oracle]
├── Task F2: Code quality review [unspecified-high]
├── Task F3: Real manual QA [unspecified-high]
└── Task F4: Scope fidelity check [deep]
-> Present results -> Get explicit user okay
```

### Dependency Matrix

| Task | Blocked By | Blocks |
|------|-----------|--------|
| 0 | — | 0.5, 1, 1.5, 2, 2.5, 2.75, 3-7, 8 |
| 0.5 | 0 | 2.75, 3-7, 9-14 |
| 1 | 0 | 2, 2.75, 3-7 |
| 1.5 | 0 | 2.75, 3-7, 9-14 |
| 2 | 1 | — |
| 2.5 | — | — (independent) |
| 2.75 | 1, 1.5 | 9-14 |
| 3-7 | 1, 1.5, 0.5 | 9-14 |
| 8 | — | — (independent) |
| 9-14 | 2.75, 3-7, 8 | F1-F4 |
| F1-F4 | 9-14 | — |

### Critical Path
Task 0 → Task 0.5 → Task 1 → Task 1.5 → Task 2.75 → Tasks 9-14 → F1-F4

---

## TODOs

- [ ] 0. FIX ABI - Add `impl_ptr` field to `NativeHostContractDispatch`

  **CRITICAL FOUNDATIONAL TASK** - ALL other tasks depend on this!
  
  **The Problem**:
  The `NativeHostContractDispatch` struct is MISSING the `impl_ptr` field that the working code uses!
  
  Current broken ABI:
  ```rust
  pub struct NativeHostContractDispatch {
      pub functions: *const *const (),  // MISSING impl_ptr!
  }
  ```
  
  Working code uses (from `examples/host_contracts/logger/host/rust/src/main.rs:129`):
  ```rust
  let dispatch: HostContractDispatch = HostContractDispatch {
      native: NativeHostContractDispatch {
          impl_ptr: logger_ptr as *const (),  // <-- This field doesn't exist!
          functions: functions.as_ptr() as *const (),
      },
  };
  ```
  
  **What to do**:
  
  **Step 1: Fix the ABI struct** in `crates/polyplug_abi/src/lib.rs` (~line 418):
  ```rust
  pub struct NativeHostContractDispatch {
      /// Pointer to the implementation (e.g., Box<ConsoleLogger> as *const ())
      pub impl_ptr: *const (),
      /// Pointer to a static array of function pointers, indexed by function_id.
      pub functions: *const *const (),
  }
  ```
  
  **Step 2: Update layout test** in `crates/polyplug_abi/src/lib.rs` (~line 936):
  ```rust
  #[test]
  fn layout_native_host_contract_dispatch() {
      assert_eq!(size_of::<NativeHostContractDispatch>(), 16);  // Was 8, now 16
      assert_eq!(offset_of!(NativeHostContractDispatch, impl_ptr), 0);
      assert_eq!(offset_of!(NativeHostContractDispatch, functions), 8);
  }
  ```
  
  **Step 3: Find and update ALL code creating `NativeHostContractDispatch`**
  Files to search and fix:
  ```bash
  grep -r "NativeHostContractDispatch {" crates/ --include="*.rs"
  ```
  Each occurrence needs to add `impl_ptr:` field.
  
  **Must NOT do**:
  - Skip any usages
  - Break backward compatibility without marking it breaking
  - Forget to update tests

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
  - **Skills**: []
  - **Reason**: Critical ABI change affecting entire codebase

  **Parallelization**:
  - **Can Run In Parallel**: NO
  - **Blocks**: Tasks 0.5, 1, 2, 3, 9-14
  - **Blocked By**: None

  **Acceptance Criteria**:
  - [ ] `NativeHostContractDispatch` has `impl_ptr: *const ()` field
  - [ ] Layout test passes with new size (16 bytes)
  - [ ] All usages of `NativeHostContractDispatch` updated
  - [ ] `cargo test -p polyplug_abi` passes
  - [ ] `cargo test -p polyplug` passes

  **QA Scenarios**:
  ```
  Scenario: ABI struct has impl_ptr field
    Tool: bash
    Steps:
      1. grep -A 3 "pub struct NativeHostContractDispatch" crates/polyplug_abi/src/lib.rs
      2. cargo test -p polyplug_abi layout_native_host_contract_dispatch
    Expected: Struct has impl_ptr, test passes
    Evidence: .sisyphus/evidence/task-0-abi-fix.txt
  
  Scenario: No usages broken
    Tool: bash
    Steps:
      1. cargo build -p polyplug_abi
      2. cargo build -p polyplug
    Expected: No compilation errors
    Evidence: .sisyphus/evidence/task-0-build.txt
  ```

  **Commit**: YES (breaking change)
  - Message: `fix(abi)!: add impl_ptr field to NativeHostContractDispatch`
  - Note: Breaking change - marked with `!`

---

- [ ] 0.5. FIX broken examples - Update dispatch functions to use impl_ptr from vtable

  **CRITICAL** - Must complete AFTER Task 0 (ABI fix)
  
  **The Problem**:
  The logger example has a BUG in its dispatch function!
  
  Current broken code at `examples/host_contracts/logger/host/rust/src/main.rs:150`:
  ```rust
  unsafe extern "C" fn log_dispatch(args: *const (), out: *mut ()) -> AbiError {
      // BUG: Trying to get logger from args (which is the StringView message)!
      let logger: &ConsoleLogger = unsafe { &*(args as *const ConsoleLogger) };  // WRONG!
      logger.log(message);
  }
  ```
  
  **What to do**:
  
  **Step 1: Fix the dispatch function** in `examples/host_contracts/logger/host/rust/src/main.rs:150`:
  ```rust
  unsafe extern "C" fn log_dispatch(
      impl_ptr: *const (),  // NEW: impl_ptr passed from vtable
      args: *const (),
      _out: *mut ()
  ) -> AbiError {
      // CORRECT: Get logger from impl_ptr
      let logger: &ConsoleLogger = unsafe { &*(impl_ptr as *const ConsoleLogger) };
      
      // Get message from args (StringView)
      let message_sv: StringView = unsafe { *(args as *const StringView) };
      let message: &str = unsafe {
          std::str::from_utf8(std::slice::from_raw_parts(message_sv.ptr, message_sv.len))
              .unwrap_or("")
      };
      
      logger.log(message);
      
      AbiError { code: ABI_OK, message: StringView::null() }
  }
  ```
  
  **Step 2: Update vtable creation** in same file (around line 117):
  ```rust
  // Update function pointer signature to include impl_ptr
  let log_fn: unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError = log_dispatch;
  ```
  
  **Step 3: Update thunk signature documentation** - Add comment explaining the pattern:
  ```rust
  // Host contract dispatch function signature:
  // fn(impl_ptr: *const (), args: *const (), out: *mut ()) -> AbiError
  // - impl_ptr: Pointer to implementation (from vtable.dispatch.native.impl_ptr)
  // - args: Function arguments (e.g., StringView)
  // - out: Output buffer for return value
  ```
  
  **Files to fix**:
  - `examples/host_contracts/logger/host/rust/src/main.rs` - Fix dispatch function
  - Any other examples using `NativeHostContractDispatch` manually
  
  **Search for broken patterns**:
  ```bash
  grep -r "&\*(args as \*const" examples/ --include="*.rs"
  grep -r "let.*logger.*args as \*const" examples/ --include="*.rs"
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  - **Reason**: Straightforward fixes once ABI is correct

  **Parallelization**:
  - **Can Run In Parallel**: NO (must wait for Task 0)
  - **Blocks**: Tasks 1-15
  - **Blocked By**: Task 0

  **Acceptance Criteria**:
  - [ ] Logger example dispatch uses `impl_ptr` parameter
  - [ ] No code tries to cast `args` to implementation pointer
  - [ ] All examples compile and run correctly
  - [ ] `./examples/host_contracts/logger/build.sh` succeeds

  **QA Scenarios**:
  ```
  Scenario: Logger example fixed
    Tool: bash
    Steps:
      1. grep -n "impl_ptr" examples/host_contracts/logger/host/rust/src/main.rs
      2. grep -n "args as \*const ConsoleLogger" examples/host_contracts/logger/host/rust/src/main.rs
      3. ./examples/host_contracts/logger/build.sh
    Expected: Uses impl_ptr, no args cast to ConsoleLogger, build succeeds
    Evidence: .sisyphus/evidence/task-0-5-example-fix.txt
  ```

  **Commit**: YES
  - Message: `fix(examples): use impl_ptr in host contract dispatch functions`

---

- [ ] 1. Implement host vtable generation in polyplugc

  **What to do**:
  
  **Step 1: Modify existing generator structure**
  - File: `crates/polyplug_codegen/src/generators/rust.rs`
  - Function: `generate_host()` (around line 350)
  - After calling `generate_host_contracts_file()` (line ~1619), add:
    ```rust
    // Generate vtable factories
    let vtable_factories = generate_host_vtable_factories(&ir)?;
    files.push(("host/vtable_factories.rs", vtable_factories));
    ```
  
  **Step 2: Implement `generate_host_vtable_factories()` function**
  - Add new function at end of rust.rs file:
    ```rust
    fn generate_host_vtable_factories(ir: &Ir) -> Result<String, CodegenError> {
        // For each [[host_contract]] in ir.host_contracts:
        //   - Generate create_<contract>_vtable<T: Trait>() function (NATIVE)
        //   - Generate create_<contract>_vtable_vm() function (VM)
        //   - Generate ABI thunk for each function (NATIVE only)
        //   - Generate static function pointer array (NATIVE only)
    }
    ```
  
  **Step 2a: Generate NATIVE dispatch factory**
  - For Rust/C++ hosts that implement traits directly
  - Generates `create_logger_vtable<T: HostLogger>(Box<T>)`
  - Includes ABI thunks with `impl_ptr` parameter
  
  **Step 2b: Generate VM dispatch factory**
  - For Python/Lua/JS hosts that use VM bridge
  - Generates `create_logger_vtable_vm(bridge_data, dispatch_fn)`
  - No ABI thunks needed (VM provides dispatch)
  - Takes bridge_data and dispatch function pointer
  
  **Step 3: Update mod.rs generation**
  - In `generate_host_mod()`, add to mod.rs content:
    ```rust
    pub mod vtable_factories;
    pub use vtable_factories::*;
    ```
  
  **Generated output structure**:
  - `generated/host/vtable_factories.rs` - Vtable factory functions
  - `generated/host/mod.rs` - Updated to include vtable_factories module
  - `generated/host/host_contracts.rs` - Existing trait definitions (unchanged)
  
  **Integration with existing code**:
  The existing `generate_host()` function currently generates:
  1. `host/mod.rs` - Module declarations
  2. `host/types.rs` - Type definitions
  3. `host/host_contracts.rs` - Trait definitions for host contracts
  4. `host/host_callers.rs` - Callers for plugin contracts
  
  You will ADD:
  5. `host/vtable_factories.rs` - Vtable factory functions
  
  **File relationships**:
  ```
  generated/host/
  ├── mod.rs                    (updated to include vtable_factories)
  ├── types.rs                  (unchanged)
  ├── host_contracts.rs         (unchanged - trait definitions)
  ├── host_callers.rs          (unchanged)
  └── vtable_factories.rs      (NEW - your generated code)
  ```

  **Generated code example** (with 2 functions to test multi-param):
  ```rust
  pub fn create_logger_vtable<T: HostLogger>(
      implementation: Box<T>
  ) -> &'static HostContractVTable {
      let impl_ptr = Box::into_raw(implementation);
      
      // Function 1: log(message) - Single arg, simple
      unsafe extern "C" fn log_thunk(
          impl_ptr: *const (),
          args: *const (),
          _out: *mut ()
      ) -> AbiError {
          let impl_ref = unsafe { &*(impl_ptr as *const T) };
          let message_sv = unsafe { *(args as *const StringView) };
          let message = unsafe {
              std::str::from_utf8_unchecked(
                  std::slice::from_raw_parts(message_sv.ptr, message_sv.len)
              )
          };
          
          match std::panic::catch_unwind(|| impl_ref.log(message)) {
              Ok(_) => AbiError { code: ABI_OK, message: StringView::null() },
              Err(_) => AbiError { 
                  code: ABI_PANIC,
                  message: StringView::from_static(b"panic in host.logger::log")
              },
          }
      }
      
      // Function 2: log_with_level(level, message) - TWO args with CUSTOM ENUM!
      unsafe extern "C" fn log_with_level_thunk(
          impl_ptr: *const (),
          args: *const (),
          _out: *mut ()
      ) -> AbiError {
          let impl_ref = unsafe { &*(impl_ptr as *const T) };
          
          // Args struct with ACCURATE LAYOUT (includes padding!)
          // Total size: 24 bytes (NOT 20!)
          // Layout: level[0-3] + padding[4-7] + message[8-23]
          #[repr(C)]
          struct LogWithLevelArgs {
              level: LogLevel,      // offset: 0,  size: 4,  align: 4
              // EXPLICIT PADDING: bytes 4-7 (to align message to 8)
              _pad: [u8; 4],        // offset: 4,  size: 4
              message: StringView,  // offset: 8,  size: 16, align: 8
          }
          // Verify: assert_eq!(size_of::<LogWithLevelArgs>(), 24);
          // Verify: assert_eq!(align_of::<LogWithLevelArgs>(), 8);
          // Verify: assert_eq!(offset_of!(LogWithLevelArgs, level), 0);
          // Verify: assert_eq!(offset_of!(LogWithLevelArgs, message), 8);
          
          let args_struct = unsafe { &*(args as *const LogWithLevelArgs) };
          
          // Get level from enum (u32 repr)
          let level: LogLevel = args_struct.level;
          
          // Get message from StringView
          let message = unsafe {
              std::str::from_utf8_unchecked(
                  std::slice::from_raw_parts(args_struct.message.ptr, args_struct.message.len)
              )
          };
          
          match std::panic::catch_unwind(|| impl_ref.log_with_level(level, message)) {
              Ok(_) => AbiError { code: ABI_OK, message: StringView::null() },
              Err(_) => AbiError { 
                  code: ABI_PANIC,
                  message: StringView::from_static(b"panic in host.logger::log_with_level")
              },
          }
      }
      
      // Static array with BOTH functions
      static FUNCTIONS: [unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError; 2] = 
          [log_thunk, log_with_level_thunk];
      
      let vtable = HostContractVTable {
          header: HostContractVTableHeader {
              vtable_version: 1,
              contract_id: HOSTLOGGER_CONTRACT_ID,
              contract_major: 1,
              contract_minor: 0,
              function_count: 2,  // <-- TWO functions
              dispatch_type: DispatchType::Native as u32,
              _padding: [0; 4],
          },
          dispatch: HostContractDispatch {
              native: NativeHostContractDispatch {
                  impl_ptr: impl_ptr as *const (),
                  functions: FUNCTIONS.as_ptr() as *const (),
              },
          },
      };
      
      Box::leak(Box::new(vtable))
  }
  ```
  
  **Key Points for Multi-Param**:
  - Single arg: Cast `args` directly to the type (e.g., `*const StringView`)
  - Multi-arg: Define `#[repr(C)]` struct, cast `args` to `*const Struct`
  - Function array includes ALL functions: `[log_thunk, log_with_level_thunk]`
  - `function_count: 2` reflects number of functions

  **Must NOT do**:
  - Use static/thread-local state (violates Runtime Isolation)
  - Skip panic safety
  - Forget SAFETY comments
  - Skip any contract functions

  **Recommended Agent Profile**:
  - **Category**: `ultrabrain`
  - **Skills**: []
  - **Reason**: Complex FFI code generation

  **Acceptance Criteria**:
  **NATIVE Dispatch:**
  - [ ] Generates `create_*_vtable<T: Trait>(Box<T>)` for each host contract
  - [ ] Thunk signature includes `impl_ptr` as first argument
  - [ ] All thunks have panic safety via `catch_unwind`
  - [ ] Uses `'static` for function pointer arrays
  - [ ] Includes `// SAFETY:` comments
  
  **VM Dispatch:**
  - [ ] Generates `create_*_vtable_vm(bridge_data, dispatch_fn)` for each host contract
  - [ ] VM factory uses `DispatchType::VirtualMachine`
  - [ ] VM factory takes `*mut c_void` bridge_data and dispatch function pointer
  - [ ] No thunks generated for VM (VM provides dispatch)

  **QA Scenarios**:
  ```
  Scenario: Polyplugc generates NATIVE vtable factory
    Tool: bash
    Steps:
      1. cargo build --release -p polyplugc
      2. ./target/release/polyplugc generate --api examples/api.toml --lang rust --out /tmp/gen
      3. cat /tmp/gen/host/vtable_factories.rs | grep -A 20 "create_logger_vtable<T"
    Expected: Function exists with impl_ptr parameter and panic safety
    Evidence: .sisyphus/evidence/task-1-native.txt
  
  Scenario: Polyplugc generates VM vtable factory
    Tool: bash
    Steps:
      1. cat /tmp/gen/host/vtable_factories.rs | grep -A 10 "create_logger_vtable_vm"
    Expected: Function exists with bridge_data and dispatch_fn parameters
    Evidence: .sisyphus/evidence/task-1-vm.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): generate host vtable factories with panic safety`

---

- [ ] 2. Add tests for vtable generation

  **What to do**:
  - Add unit tests in `crates/polyplug_codegen/tests/`
  - Test that generated code compiles
  - Test that vtable factory creates valid vtable
  - Test panic safety

  **Test cases**:
  ```rust
  #[test]
  fn test_vtable_factory_creates_valid_vtable() {
      let vtable = create_logger_vtable(Box::new(TestLogger));
      assert!(!vtable.is_null());
      assert_eq!(vtable.header.contract_id, HOSTLOGGER_CONTRACT_ID);
  }
  
  #[test]
  fn test_thunk_handles_panic() {
      struct PanickingLogger;
      impl HostLogger for PanickingLogger {
          fn log(&self, _msg: &str) { panic!("test"); }
      }
      let vtable = create_logger_vtable(Box::new(PanickingLogger));
      // Call thunk and verify it returns error, not panic
  }
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 1.5, 2.5)
  - **Parallel Group**: Wave 1
  - **Blocks**: Task 2.75
  - **Blocked By**: Task 1

  **Acceptance Criteria**:
  - [ ] Tests compile and pass
  - [ ] Tests cover happy path and panic case

  **QA Scenarios**:
  ```
  Scenario: Tests pass
    Tool: bash
    Steps:
      1. cargo test -p polyplug_codegen
    Expected: All tests pass
    Evidence: .sisyphus/evidence/task-2-tests.txt
  ```

  **Commit**: YES

---

- [ ] 1.5. Update runtime to support new thunk signature (ABI BREAKING CHANGE)

  **What to do**:
  The new thunk signature adds `impl_ptr` as the first parameter:
  ```rust
  // OLD signature (current):
  unsafe extern "C" fn thunk(args: *const (), out: *mut ()) -> AbiError
  
  // NEW signature (required):
  unsafe extern "C" fn thunk(
      impl_ptr: *const (),  // NEW: Implementation pointer from vtable
      args: *const (),      // Function arguments
      out: *mut ()          // Output pointer
  ) -> AbiError
  ```
  
  **Files to update**:
  
  1. **`crates/polyplug_abi/src/lib.rs`** - Update HostContractDispatch
     - Current `NativeHostContractDispatch.functions` points to `*const ()`
     - New: Functions must accept `impl_ptr` as first parameter
     - Update function pointer type definition
  
  2. **`crates/polyplug/src/runtime.rs`** - Update dispatch logic
     - Find where host contract functions are called
     - Update to pass `vtable.dispatch.native.impl_ptr` as first argument
     - Location: Look for `HostContractDispatch` usage
  
  3. **`crates/polyplug/src/ffi.rs`** - Update FFI bindings if needed
     - Check if any FFI declarations need updating
  
  **Implementation details**:
  ```rust
  // In runtime.rs, when dispatching to host contract:
  let vtable: &HostContractVTable = /* get vtable */;
  let impl_ptr: *const () = vtable.dispatch.native.impl_ptr;
  let fn_ptr: *const () = /* get function pointer from array */;
  
  // Call with impl_ptr as first argument
  let dispatch_fn: unsafe extern "C" fn(*const (), *const (), *mut ()) -> AbiError =
      std::mem::transmute(fn_ptr);
  let err: AbiError = dispatch_fn(impl_ptr, args_ptr, out_ptr);
  ```
  
  **Breaking change impact**:
  - This changes the HostContractVTable ABI
  - Existing manually-created vtables will break
  - Acceptable for pre-1.0, but must be documented
  
  **Must NOT do**:
  - Try to maintain backward compatibility (too complex)
  - Skip updating runtime dispatch logic

  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  - **Reason**: Core ABI changes to runtime

  **Parallelization**:
  - **Can Run In Parallel**: YES (with Task 1)
  - **Blocks**: Task 3
  - **Blocked By**: None

  **Acceptance Criteria**:
  - [ ] `crates/polyplug_abi/src/lib.rs` updated with new function signature
  - [ ] `crates/polyplug/src/runtime.rs` passes impl_ptr when dispatching
  - [ ] All existing tests pass with new ABI
  - [ ] Documentation updated about breaking change

  **QA Scenarios**:
  ```
  Scenario: Runtime dispatches with impl_ptr
    Tool: bash
    Steps:
      1. grep -n "impl_ptr" crates/polyplug/src/runtime.rs
      2. cargo test -p polyplug
    Expected: Tests pass, impl_ptr passed to dispatch
    Evidence: .sisyphus/evidence/task-1-5-abi.txt
  ```

  **Commit**: YES
  - Message: `feat(abi)!: add impl_ptr parameter to host contract thunks`
  - Note: Breaking change marked with `!`

---

- [ ] 2.5. Add 25+ layout calculation tests in polyplug_codegen

  **CRITICAL**: Comprehensive test suite to verify ALL type layout calculations are accurate!
  
  **Location**: `crates/polyplug_codegen/tests/layout_calculations.rs`
  
  **Purpose**: Ensure polyplugc correctly calculates sizes, alignments, and offsets for ALL types
  
  **Category 1: Primitive Types (6 tests)**
  - `layout_u8_size_align` - u8: size=1, align=1
  - `layout_u16_size_align` - u16: size=2, align=2
  - `layout_u32_size_align` - u32: size=4, align=4
  - `layout_u64_size_align` - u64: size=8, align=8
  - `layout_usize_size_align` - usize: size=8, align=8 (x86_64)
  - `layout_bool_size_align` - bool: size=1, align=1
  
  **Category 2: ABI Built-in Types (5 tests)**
  - `layout_stringview_fields_and_size` - StringView: size=16, align=8, ptr@0, len@8
  - `layout_buffer_fields_and_size` - Buffer: size=24, align=8, ptr@0, len@8, cap@16
  - `layout_abierror_fields_and_size` - AbiError: size=24, align=8, code@0, message@8
  - `layout_plug_handle_fields_and_size` - PluginHandle: size=8, align=4, index@0, generation@4
  - `layout_hostcontext_fields_and_size` - HostContext: size=16, align=8, runtime@0, bundle_id@8
  
  **Category 3: Enum Types (5 tests)**
  - `layout_enum_u8_size_align` - #[repr(u8)] enum: size=1, align=1
  - `layout_enum_u16_size_align` - #[repr(u16)] enum: size=2, align=2
  - `layout_enum_u32_size_align` - #[repr(u32)] enum: size=4, align=4
  - `layout_loglevel_size_align` - LogLevel (repr=u32): size=4, align=4
  - `layout_enum_single_variant` - Single variant enum: correct size
  
  **Category 4: Struct Layouts with Padding (6 tests)**
  - `layout_simple_struct_no_padding` - All fields naturally aligned, no padding
  - `layout_struct_with_internal_padding` - Small field before large field needs padding
  - `layout_logwithlevelargs_layout` - LogWithLevelArgs: size=24, level@0, message@8, pad@4
  - `layout_struct_trailing_padding` - Final size rounded to alignment
  - `layout_nested_struct` - Struct containing other structs
  - `layout_struct_with_enum_field` - Struct containing enum field (LogLevel)
  
  **Category 5: Complex Multi-Param Cases (5+ tests)**
  - `layout_two_primitives_no_padding` - (u32, u32): size=8, align=4
  - `layout_two_primitives_with_padding` - (u32, u64): size=16, align=8, second@8
  - `layout_three_params_mixed` - (u8, StringView, u32): size=32, align=8
  - `layout_enum_then_stringview` - (LogLevel, StringView): size=24, align=8
  - `layout_stringview_then_enum` - (StringView, LogLevel): size=24, align=8, different offsets
  - `layout_multiple_enums` - (LogLevel, LogLevel, u64): size=24, align=8
  
  **Total: 27+ layout tests**
  
  **Test structure**:
  ```rust
  #[test]
  fn layout_logwithlevelargs_layout() {
      let struct_layout = calculate_struct_layout(&[
          ("level", Type::Enum(EnumDef::new("LogLevel", Repr::U32))),
          ("message", Type::Builtin(Builtin::StringView)),
      ]);
      
      assert_eq!(struct_layout.size, 24);
      assert_eq!(struct_layout.align, 8);
      assert_eq!(struct_layout.fields[0].offset, 0);   // level
      assert_eq!(struct_layout.fields[0].size, 4);
      assert_eq!(struct_layout.fields[1].offset, 8);   // message (after padding)
      assert_eq!(struct_layout.fields[1].size, 16);
      
      // Verify padding is calculated
      let padding = struct_layout.fields[1].offset - 
                    (struct_layout.fields[0].offset + struct_layout.fields[0].size);
      assert_eq!(padding, 4, "Expected 4 bytes padding between level and message");
  }
  ```
  
  **Cross-Language Consistency:**
  - [ ] Rust layout matches C++ layout
  - [ ] C++ layout matches C# layout
  - [ ] VM languages (Python/Lua/JS) have explicit padding
  - [ ] All languages: `sizeof(LogWithLevelArgs) == 24`

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 1, 1.5, 2)
  - **Parallel Group**: Wave 2
  - **Blocks**: None (independent)
  - **Blocked By**: None

  **Acceptance Criteria**:
  - [ ] 27+ layout tests implemented
  - [ ] All tests pass
  - [ ] Tests cover primitives, ABI types, enums, structs, padding
  - [ ] Cross-language layout consistency verified

  **QA Scenarios**:
  ```
  Scenario: Layout tests pass
    Tool: bash
    Steps:
      1. cargo test -p polyplug_codegen --test layout_calculations
    Expected: 27+ tests pass
    Evidence: .sisyphus/evidence/task-2-5-layout-tests.txt
  ```

  **Commit**: YES
  - Message: `test(polyplugc): add 27+ layout calculation tests`

---

- [ ] 2.75. Test generated code in examples context

  **What to do**:
  - Create minimal test api.toml
  - Generate code using ALL generators
  - Verify it integrates with runtime

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: NO (depends on Tasks 1, 1.5, 2)
  - **Parallel Group**: Wave 3
  - **Blocks**: Tasks 3-7, 9-14
  - **Blocked By**: Tasks 1, 1.5, 2

  **QA Scenarios**:
  ```
  Scenario: Generated code works end-to-end
    Tool: bash
    Steps:
      1. Generate code for test contract
      2. Implement trait
      3. Register with runtime
      4. Call from guest
    Expected: Bidirectional communication works
    Evidence: .sisyphus/evidence/task-2-75-e2e.txt
  ```

  **Commit**: NO

---

- [ ] 3. Implement C++ host vtable generator

  **What to do**:
  Add host vtable factory generation to C++ generator.
  
  **File**: `crates/polyplug_codegen/src/generators/cpp.rs`
  
  **Generate**:
  - NATIVE: `create_logger_vtable<T>(std::unique_ptr<T>)`
  - VM: `create_logger_vtable_vm(void*, VmDispatchFn)`
  
  **Pattern**: Similar to Rust, using C++ idioms:
  - Use `std::unique_ptr` for ownership
  - Use `extern "C"` for ABI functions
  - Use templates for generic implementation
  
  **Reference**: Task 1 (Rust) provides the pattern
  
  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 4-7)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 10
  - **Blocked By**: Tasks 1, 1.5, 2.75

  **Acceptance Criteria**:
  - [ ] C++ generator produces vtable factory functions
  - [ ] NATIVE and VM dispatch both supported
  - [ ] Generated code compiles

  **QA Scenarios**:
  ```
  Scenario: C++ generator works
    Tool: bash
    Steps:
      1. grep -q "create_logger_vtable" crates/polyplug_codegen/src/generators/cpp.rs
      2. cargo build -p polyplug_codegen
    Expected: Function exists, build succeeds
    Evidence: .sisyphus/evidence/task-3-cpp.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): add C++ host vtable factory generation`

---

- [ ] 4. Implement C# host vtable generator

  **What to do**:
  Add host vtable factory generation to C# generator.
  
  **File**: `crates/polyplug_codegen/src/generators/csharp.rs`
  
  **Generate**:
  - NATIVE: `CreateLoggerVTable<T>(T implementation)`
  - VM: `CreateLoggerVTableVm(IntPtr bridgeData, VmDispatchDelegate dispatchFn)`
  
  **Pattern**: C# interop and delegate types:
  - Use `[UnmanagedFunctionPointer]` for callbacks
  - Use `IntPtr` for opaque pointers
  - Use delegates for dispatch functions
  
  **Reference**: Task 1 (Rust) provides the pattern
  
  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3, 5-7)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 11
  - **Blocked By**: Tasks 1, 1.5, 2.75

  **Acceptance Criteria**:
  - [ ] C# generator produces vtable factory functions
  - [ ] NATIVE and VM dispatch both supported
  - [ ] Generated code compiles

  **QA Scenarios**:
  ```
  Scenario: C# generator works
    Tool: bash
    Steps:
      1. grep -q "CreateLoggerVTable" crates/polyplug_codegen/src/generators/csharp.rs
      2. cargo build -p polyplug_codegen
    Expected: Function exists, build succeeds
    Evidence: .sisyphus/evidence/task-4-csharp.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): add C# host vtable factory generation`

---

- [ ] 5. Implement Python host vtable generator

  **What to do**:
  Add host vtable factory generation to Python generator.
  
  **File**: `crates/polyplug_codegen/src/generators/python.rs`
  
  **Generate**:
  - NATIVE: C API factories using ctypes
  - VM: VM factories for Python runtime
  
  **Pattern**: Python C API and ctypes:
  - Use `ctypes.CDLL` for loading
  - Use `ctypes.Structure` for structs
  - Handle GIL properly
  
  **Reference**: Task 1 (Rust) provides the pattern
  
  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3-4, 6-7)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 12
  - **Blocked By**: Tasks 1, 1.5, 2.75

  **Acceptance Criteria**:
  - [ ] Python generator produces vtable factory functions
  - [ ] NATIVE and VM dispatch both supported
  - [ ] Generated code works with Python C API

  **QA Scenarios**:
  ```
  Scenario: Python generator works
    Tool: bash
    Steps:
      1. grep -q "create_logger_vtable" crates/polyplug_codegen/src/generators/python.rs
      2. cargo build -p polyplug_codegen
    Expected: Function exists, build succeeds
    Evidence: .sisyphus/evidence/task-5-python.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): add Python host vtable factory generation`

---

- [ ] 6. Implement Lua host vtable generator

  **What to do**:
  Add host vtable factory generation to Lua generator.
  
  **File**: `crates/polyplug_codegen/src/generators/lua.rs`
  
  **Generate**:
  - NATIVE: C API factories
  - VM: VM factories for Lua runtime
  
  **Pattern**: Lua C API:
  - Use `lua_CFunction` for callbacks
  - Use `luaL_Reg` for registration
  - Handle Lua state properly
  
  **Reference**: Task 1 (Rust) provides the pattern
  
  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3-5, 7)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 13
  - **Blocked By**: Tasks 1, 1.5, 2.75

  **Acceptance Criteria**:
  - [ ] Lua generator produces vtable factory functions
  - [ ] NATIVE and VM dispatch both supported
  - [ ] Generated code works with Lua C API

  **QA Scenarios**:
  ```
  Scenario: Lua generator works
    Tool: bash
    Steps:
      1. grep -q "create_logger_vtable" crates/polyplug_codegen/src/generators/lua.rs
      2. cargo build -p polyplug_codegen
    Expected: Function exists, build succeeds
    Evidence: .sisyphus/evidence/task-6-lua.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): add Lua host vtable factory generation`

---

- [ ] 7. Implement JavaScript host vtable generators

  **What to do**:
  Add host vtable factory generation to BOTH JavaScript generators.
  
  **Files**: 
  - `crates/polyplug_codegen/src/generators/js_deno.rs`
  - `crates/polyplug_codegen/src/generators/js_quickjs.rs`
  
  **Generate**:
  - NATIVE: Native bindings for each engine
  - VM: VM factories for Deno/QuickJS runtime
  
  **Pattern**: Deno/QuickJS specific:
  - Use engine-specific FFI mechanisms
  - Handle JS value conversions
  - Manage engine lifecycle
  
  **Reference**: Task 1 (Rust) provides the pattern
  
  **Recommended Agent Profile**:
  - **Category**: `unspecified-high`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 3-6)
  - **Parallel Group**: Wave 3
  - **Blocks**: Task 14
  - **Blocked By**: Tasks 1, 1.5, 2.75

  **Acceptance Criteria**:
  - [ ] Deno generator produces vtable factory functions
  - [ ] QuickJS generator produces vtable factory functions
  - [ ] NATIVE and VM dispatch both supported
  - [ ] Generated code works with respective engines

  **QA Scenarios**:
  ```
  Scenario: JavaScript generators work
    Tool: bash
    Steps:
      1. grep -q "create_logger_vtable" crates/polyplug_codegen/src/generators/js_deno.rs
      2. grep -q "create_logger_vtable" crates/polyplug_codegen/src/generators/js_quickjs.rs
      3. cargo build -p polyplug_codegen
    Expected: Functions exist, build succeeds
    Evidence: .sisyphus/evidence/task-7-js.txt
  ```

  **Commit**: YES
  - Message: `feat(polyplugc): add JavaScript host vtable factory generation`

---

- [ ] 8. Add host.logger to api.toml (ULTIMATE - uses custom enum type!)

  **What to do**:
  - Add to `examples/api.toml`:
    ```toml
    # CUSTOM ENUM TYPE - proves custom type support!
    [[enum]]
    name = "LogLevel"
    repr = "u32"
    
    [[enum.variants]]
    name = "Debug"
    value = "0"
    
    [[enum.variants]]
    name = "Info"
    value = "1"
    
    [[enum.variants]]
    name = "Warn"
    value = "2"
    
    [[enum.variants]]
    name = "Error"
    value = "3"
    
    [[host_contract]]
    name = "host.logger"
    version = "1.0.0"
    
    # Function 1: Single parameter (simple case)
    [[host_contract.functions]]
    name = "log"
    params = [{ name = "message", type = "StringView" }]
    returns = "void"
    
    # Function 2: TWO parameters with CUSTOM ENUM!
    # LEVEL IS FIRST (as requested)!
    [[host_contract.functions]]
    name = "log_with_level"
    params = [
        { name = "level", type = "LogLevel" },      # <-- FIRST! Custom enum!
        { name = "message", type = "StringView" }   # <-- SECOND! StringView!
    ]
    returns = "void"
    ```
  
  **Why this is ULTIMATE:**
  - ✅ Custom **ENUM** type (`LogLevel`)
  - ✅ **Level FIRST** parameter (as requested)
  - ✅ Multi-arg with **custom types**
  - ✅ Proves the system handles **everything**:
    - Primitive types (StringView)
    - Custom types (enums)
    - Single arg functions
    - Multi-arg functions with mixed types
    - Argument packing/unpacking
  
  **Generated code will be:**
  ```rust
  // Enum generated by polyplugc
  #[repr(u32)]
  pub enum LogLevel {
      Debug = 0,
      Info = 1,
      Warn = 2,
      Error = 3,
  }
  
  // Args struct for multi-param function
  #[repr(C)]
  struct LogWithLevelArgs {
      level: LogLevel,      // Custom enum!
      message: StringView,  // Primitive!
  }
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`

  **QA Scenarios**:
  ```
  Scenario: api.toml valid
    Tool: bash
    Steps:
      1. grep "\[\[host_contract\]\]" examples/api.toml
    Expected: Match found
    Evidence: .sisyphus/evidence/task-4-api-toml.txt
  ```

  **Commit**: YES

---

- [ ] 6. Update Rust host using GENERATED code (implement BOTH functions)

  **What to do**:
  - Import: `use generated::host::vtable_factories::create_logger_vtable;`
  - Import the generated enum: `use generated::types::LogLevel;`
  - Implement trait with BOTH functions:
    ```rust
    struct ConsoleLogger;
    
    impl HostLogger for ConsoleLogger {
        // Function 1: Single arg
        fn log(&self, message: &str) {
            println!("[PLUGIN LOG] {}", message);
        }
        
        // Function 2: TWO args with CUSTOM ENUM!
        // LEVEL IS FIRST (as requested)!
        fn log_with_level(&self, level: LogLevel, message: &str) {
            let level_str = match level {
                LogLevel::Debug => "DEBUG",
                LogLevel::Info => "INFO",
                LogLevel::Warn => "WARN",
                LogLevel::Error => "ERROR",
            };
            println!("[{}] {}", level_str, message);
        }
    }
    ```
  - Register: `let vtable = create_logger_vtable(Box::new(ConsoleLogger));`
  
  **Why this is ULTIMATE:**
  - Tests primitive types (StringView in log)
  - Tests **custom enum types** (LogLevel in log_with_level)
  - Tests **level as FIRST parameter** (as requested)
  - Tests multi-arg with mixed types (enum + StringView)
  - Proves the system handles **everything**!

  **Recommended Agent Profile**:
  - **Category**: `quick`

  **QA Scenarios**:
  ```
  Scenario: Host uses generated code
    Tool: bash
    Steps:
      1. grep "create_logger_vtable" examples/hosts/rust/src/main.rs
    Expected: Match found, no manual vtable code
    Evidence: .sisyphus/evidence/task-6-host.txt
  ```

  **Commit**: YES

---

- [ ] 7. Update Rust reporter

  **What to do**:
  - Import `HostLoggerCaller` from generated code
  - Use in `report()`:
    ```rust
    if let Some(logger) = HostLoggerCaller::from_host(get_host_vtable(), 1) {
        let _ = logger.log("Processing report...".to_string());
    }
    ```

  **Recommended Agent Profile**:
  - **Category**: `quick`

  **QA Scenarios**:
  ```
  Scenario: Reporter uses host logger
    Tool: bash
    Steps:
      1. grep "HostLoggerCaller" examples/guests/rust/reporter/src/lib.rs
    Expected: Match found
    Evidence: .sisyphus/evidence/task-7-reporter.txt
  ```

  **Commit**: YES

---

- [ ] 8. Verify Rust pipeline

  **What to do**:
  - Build: `./examples/build_all.sh`
  - Run: `./hosts/rust/target/release/pipeline_host`
  - Verify `[PLUGIN LOG]` in output

  **QA Scenarios**:
  ```
  Scenario: Pipeline logs work
    Tool: bash
    Steps:
      1. cd examples && ./build_all.sh
      2. POLYPLUG_PLUGIN_PATH=./plugins ./hosts/rust/target/release/pipeline_host 2>&1 | grep "\[PLUGIN LOG\]"
    Expected: Matches found
    Evidence: .sisyphus/evidence/task-8-verify.txt
  ```

  **Commit**: NO

---

- [ ] 9. Regenerate and update Rust example

  **What to do**:
  Regenerate code and update Rust host/reporter to use generated vtable factories.
  
  **Files to modify**:
  - `examples/hosts/rust/src/main.rs` - Use generated vtable factory
  - `examples/guests/rust/reporter/src/main.rs` - Use generated caller (if needed)
  
  **Update host to use generated code**:
  ```rust
  // OLD: Manual vtable creation
  let dispatch = HostContractDispatch { ... };
  
  // NEW: Use generated vtable factory
  use generated::host::vtable_factories::create_logger_vtable;
  let vtable = create_logger_vtable(Box::new(ConsoleLogger));
  runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable);
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 10-14)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 1, 1.5, 2.75, 8

  **Acceptance Criteria**:
  - [ ] Rust host uses `create_logger_vtable(Box::new(impl))`
  - [ ] No manual vtable creation code
  - [ ] Rust example compiles and runs

  **QA Scenarios**:
  ```
  Scenario: Rust host uses generated code
    Tool: bash
    Steps:
      1. grep "create_logger_vtable" examples/hosts/rust/src/main.rs
      2. cd examples/hosts/rust && cargo build
    Expected: Uses generated function, build succeeds
    Evidence: .sisyphus/evidence/task-9-rust-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/rust): use generated vtable factory`

---

- [ ] 10. Regenerate and update C++ example

  **What to do**:
  Regenerate code and update C++ host/reporter to use generated vtable factories.
  
  **Files to modify**:
  - `examples/hosts/cpp/main.cpp` - Use generated vtable factory
  - `examples/guests/cpp/reporter/reporter.cpp` - Use generated caller
  
  **Update host to use generated code**:
  ```cpp
  // Use generated vtable factory
  #include "generated/host/vtable_factories.hpp"
  auto vtable = create_logger_vtable(std::make_unique<ConsoleLogger>());
  runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable);
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9, 11-14)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 3, 2.75, 8

  **Acceptance Criteria**:
  - [ ] C++ host uses generated vtable factory
  - [ ] No manual vtable creation code
  - [ ] C++ example compiles with `g++ -std=c++20`

  **QA Scenarios**:
  ```
  Scenario: C++ host uses generated code
    Tool: bash
    Steps:
      1. grep "create_logger_vtable" examples/hosts/cpp/main.cpp
      2. cd examples/hosts/cpp && g++ -std=c++20 main.cpp -o host
    Expected: Uses generated function, compiles
    Evidence: .sisyphus/evidence/task-10-cpp-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/cpp): use generated vtable factory`

---

- [ ] 11. Regenerate and update C# example

  **What to do**:
  Regenerate code and update C# host/reporter to use generated vtable factories.
  
  **Files to modify**:
  - `examples/hosts/csharp/Program.cs` - Use generated vtable factory
  - `examples/guests/csharp/reporter/Reporter.cs` - Use generated caller
  
  **Update host to use generated code**:
  ```csharp
  // Use generated vtable factory
  using Generated.Host.VtableFactories;
  var vtable = VtableFactories.CreateLoggerVTable(new ConsoleLogger());
  runtime.RegisterHostContract(HOSTLOGGER_CONTRACT_ID, vtable);
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9-10, 12-14)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 4, 2.75, 8

  **Acceptance Criteria**:
  - [ ] C# host uses generated vtable factory
  - [ ] No manual vtable creation code
  - [ ] C# example builds with `dotnet build`

  **QA Scenarios**:
  ```
  Scenario: C# host uses generated code
    Tool: bash
    Steps:
      1. grep "CreateLoggerVTable" examples/hosts/csharp/Program.cs
      2. cd examples/hosts/csharp && dotnet build
    Expected: Uses generated function, build succeeds
    Evidence: .sisyphus/evidence/task-11-csharp-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/csharp): use generated vtable factory`

---

- [ ] 12. Regenerate and update Python example

  **What to do**:
  Regenerate code and update Python host/reporter to use generated vtable factories.
  
  **Files to modify**:
  - `examples/hosts/python/host.py` - Use generated vtable factory
  - `examples/guests/python/reporter/reporter.py` - Use generated caller
  
  **Update host to use generated code**:
  ```python
  # Use generated vtable factory
  from generated.host.vtable_factories import create_logger_vtable_vm
  vtable = create_logger_vtable_vm(bridge_data, dispatch_fn)
  runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9-11, 13-14)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 5, 2.75, 8

  **Acceptance Criteria**:
  - [ ] Python host uses generated vtable factory
  - [ ] No manual vtable creation code
  - [ ] Python example runs without errors

  **QA Scenarios**:
  ```
  Scenario: Python host uses generated code
    Tool: bash
    Steps:
      1. grep "create_logger_vtable" examples/hosts/python/host.py
      2. python3 examples/hosts/python/host.py
    Expected: Uses generated function, runs successfully
    Evidence: .sisyphus/evidence/task-12-python-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/python): use generated vtable factory`

---

- [ ] 13. Regenerate and update Lua example

  **What to do**:
  Regenerate code and update Lua host/reporter to use generated vtable factories.
  
  **Files to modify**:
  - `examples/hosts/lua/host.lua` - Use generated vtable factory
  - `examples/guests/lua/reporter/reporter.lua` - Use generated caller
  
  **Update host to use generated code**:
  ```lua
  -- Use generated vtable factory
  local vtable_factories = require("generated.host.vtable_factories")
  local vtable = vtable_factories.create_logger_vtable_vm(bridge_data, dispatch_fn)
  runtime:register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9-12, 14)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 6, 2.75, 8

  **Acceptance Criteria**:
  - [ ] Lua host uses generated vtable factory
  - [ ] No manual vtable creation code
  - [ ] Lua example runs without errors

  **QA Scenarios**:
  ```
  Scenario: Lua host uses generated code
    Tool: bash
    Steps:
      1. grep "create_logger_vtable" examples/hosts/lua/host.lua
      2. lua examples/hosts/lua/host.lua
    Expected: Uses generated function, runs successfully
    Evidence: .sisyphus/evidence/task-13-lua-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/lua): use generated vtable factory`

---

- [ ] 14. Regenerate and update JavaScript example

  **What to do**:
  Regenerate code and update JavaScript host/reporter to use generated vtable factories.
  
  **Part 1: Update JS host to use generated code**
  - `examples/hosts/js/host.js` - Use generated vtable factory
  
  **Part 2: FIX the broken reporter**
  - `examples/guests/js/reporter/reporter.js` - Fix reporter logic
  - Current reporter just returns input
  - Must implement actual report logic:
    ```javascript
    // Parse "TRANSFORMED:name|value|count" format
    // Generate "Report: name has value 'value' with count count"
    ```
  
  **Update host to use generated code**:
  ```javascript
  // Use generated vtable factory
  import { createLoggerVtableVm } from "./generated/host/vtable_factories.js";
  const vtable = createLoggerVtableVm(bridgeData, dispatchFn);
  runtime.registerHostContract(HOSTLOGGER_CONTRACT_ID, vtable);
  ```

  **Recommended Agent Profile**:
  - **Category**: `quick`
  - **Skills**: []
  
  **Parallelization**:
  - **Can Run In Parallel**: YES (with Tasks 9-13)
  - **Parallel Group**: Wave 5
  - **Blocks**: Final Verification
  - **Blocked By**: Tasks 7, 2.75, 8

  **Acceptance Criteria**:
  - [ ] JS host uses generated vtable factory
  - [ ] JS reporter properly transforms data (REQUIRED)
  - [ ] No manual vtable creation code
  - [ ] JS example runs without errors

  **QA Scenarios**:
  ```
  Scenario: JS host uses generated code and reporter fixed
    Tool: bash
    Steps:
      1. grep "createLoggerVtable" examples/hosts/js/host.js
      2. grep -E "Report:|split" examples/guests/js/reporter/reporter.js
      3. node examples/hosts/js/host.js
    Expected: Uses generated function, reporter has actual logic, runs successfully
    Evidence: .sisyphus/evidence/task-14-js-example.txt
  ```

  **Commit**: YES
  - Message: `feat(examples/js): use generated vtable factory and fix reporter`

---

## Final Verification Wave

- [ ] F1. **Plan Compliance Audit** — `oracle`
  - Check: polyplugc generates vtable factories
  - Check: No manual vtable creation in examples
  - Check: All thunks have panic safety
  - Output: VERDICT

- [ ] F2. **Code Quality Review** — `unspecified-high`
  - Run: `cargo clippy -- -D warnings`
  - Check: All unsafe blocks have SAFETY comments
  - Check: No AGENTS.md violations
  - Output: VERDICT

- [ ] F3. **Real Manual QA** — `unspecified-high`
  - Build: `./build_all.sh`
  - Run: `./verify_hosts.sh`
  - Verify: `[PLUGIN LOG]` messages present
  - Output: VERDICT

- [ ] F4. **Scope Fidelity Check** — `deep`
  - Check: Only polyplugc + examples modified
  - Check: No changes to other plugins
  - Check: JS reporter fixed
  - Output: VERDICT

---

## Commit Strategy

```
Phase 1:
Task 1: feat(polyplugc): generate host vtable factories
Task 2: test(polyplugc): add vtable factory tests

Phase 2:
Task 4: feat(examples): add host.logger contract
Task 6: feat(examples/rust-host): use generated vtable factory
Task 7: feat(examples/rust-reporter): add host.logger calls
Tasks 9-13: feat(examples/<lang>): use generated code
```

---

## Success Criteria

### Phase 0: ABI Fix (MUST complete first)
- [ ] `NativeHostContractDispatch` has `impl_ptr: *const ()` field
- [ ] ABI layout tests pass (size = 16 bytes)
- [ ] All existing code compiles with new ABI
- [ ] Logger example dispatch uses `impl_ptr` from vtable
- [ ] No code tries to cast `args` to implementation pointer
- [ ] `./examples/host_contracts/logger/build.sh` succeeds

### Phase 1: Code Generation (MUST work for all languages)
**NATIVE Dispatch:**
- [ ] All 6 generators produce NATIVE factories
- [ ] Native vtable factory works: `create_logger_vtable(Box::new(impl))`
- [ ] All generated code has panic safety
- [ ] `cargo test -p polyplug_codegen` passes

**VM Dispatch:**
- [ ] All 6 generators produce VM factories
- [ ] VM vtable factory works: `create_logger_vtable_vm(bridge_data, dispatch_fn)`
- [ ] VM hosts use generated VM factories (no manual code)

### Phase 2: Examples (MUST all pass)
- [ ] `./examples/build_all.sh` succeeds
- [ ] `./examples/verify_hosts.sh` passes
- [ ] Output contains `[PLUGIN LOG]` messages
- [ ] Output contains `[INFO]`, `[WARN]`, or `[ERROR]` messages (from log_with_level)
- [ ] Both `log()` and `log_with_level()` functions work correctly
- [ ] No manual vtable creation code exists in any example
- [ ] JS reporter properly transforms data (fixed)

### Multi-Param Verification (Critical)
- [ ] Single-arg function `log(message)` works
- [ ] Multi-arg function `log_with_level(level, message)` works
- [ ] Generated code correctly packs/unpacks multi-arg structs
- [ ] Function dispatch uses correct function_id (0 for log, 1 for log_with_level)

### Custom Type Verification (ULTIMATE)
- [ ] Custom enum `LogLevel` generated for all 6 languages
- [ ] Enum has correct variants (Debug, Info, Warn, Error)
- [ ] Enum has correct repr (u32)
- [ ] `log_with_level` uses enum as FIRST parameter
- [ ] Output shows `[DEBUG]`, `[INFO]`, `[WARN]`, `[ERROR]` prefixes
- [ ] Custom types work in host contracts (proves full type system support)

### Type Layout Verification (CRITICAL - ACCURATE LAYOUTS)
**Primitive Types:**
- [ ] `u8`/`i8`/`bool`: size=1, align=1
- [ ] `u16`/`i16`: size=2, align=2
- [ ] `u32`/`i32`: size=4, align=4
- [ ] `u64`/`i64`: size=8, align=8

**ABI Built-ins:**
- [ ] `StringView`: size=16, align=8, ptr:0, len:8
- [ ] `Buffer`: size=24, align=8, ptr:0, len:8, cap:16

**Custom Types:**
- [ ] `LogLevel` (enum u32): size=4, align=4
- [ ] `LogWithLevelArgs`: size=24, align=8, level:0, message:8
- [ ] Padding calculated correctly (4 bytes between level and message)

**Layout Assertions in Generated Code:**
```rust
assert_eq!(size_of::<StringView>(), 16);
assert_eq!(size_of::<LogWithLevelArgs>(), 24);
assert_eq!(offset_of!(LogWithLevelArgs, level), 0);
assert_eq!(offset_of!(LogWithLevelArgs, message), 8);
```

**Cross-Language Consistency:**
- [ ] Rust layout matches C++ layout
- [ ] C++ layout matches C# layout
- [ ] VM languages (Python/Lua/JS) have explicit padding
- [ ] All languages: `sizeof(LogWithLevelArgs) == 24`

### Quality Gates
- [ ] `cargo clippy -- -D warnings` clean
- [ ] All `unsafe` blocks have SAFETY comments
- [ ] No AGENTS.md rule violations
- [ ] Both NATIVE and VM factories properly documented

