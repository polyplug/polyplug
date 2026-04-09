# Host Contracts API Reference

## Terminology Note

This document uses terminology renamed in v1.1:
- **RuntimeAbi**: Previously called "HostInterface" - the runtime's ABI provided to guests
- **HostContractInterface**: Previously called "HostContractVTable" - a contract the host implements

## Overview

This document provides the complete API reference for host contracts, including C ABI structures, generated code patterns, and usage in all supported languages.

## C ABI Structures

### DispatchType Enum

```rust
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchType {
    Native = 0,
    VirtualMachine = 1,
}
```

**Layout (x86_64)**:
- Size: 4 bytes
- Alignment: 4 bytes

**Variants**:
- `Native (0)` - Direct function pointer dispatch (Rust, C++, C#)
- `VirtualMachine (1)` - VM dispatch through bridge (Python, Lua, JavaScript)

---

### HostContractInterfaceHeader

```rust
#[repr(C)]
pub struct HostContractInterfaceHeader {
    pub interface_version: u32,      // Offset: 0
    pub contract_id: u64,         // Offset: 8
    pub contract_major: u32,      // Offset: 16
    pub contract_minor: u32,      // Offset: 20
    pub function_count: u32,      // Offset: 24
    pub dispatch_type: DispatchType,  // Offset: 28
    pub _padding: [u32; 4],       // Offset: 32 (alignment)
}
```

**Layout (x86_64)**:
- Size: 48 bytes
- Alignment: 8 bytes

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `interface_version` | `u32` | Structure version. Current: 1 |
| `contract_id` | `u64` | FNV-1a hash of `"host_contract:{name}@{major}"` |
| `contract_major` | `u32` | Major version (breaking changes) |
| `contract_minor` | `u32` | Minor version (backwards-compatible) |
| `function_count` | `u32` | Number of functions in the dispatch array |
| `dispatch_type` | `DispatchType` | `Native` or `VirtualMachine` |
| `_padding` | `[u32; 4]` | Alignment padding |

---

### NativeHostContractDispatch

```rust
#[repr(C)]
pub struct NativeHostContractDispatch {
    pub impl_ptr: *const (),        // Offset: 0 - Implementation pointer
    pub functions: *const *const (), // Offset: 8 - Function pointer array
}
```

**Layout (x86_64)**:
- Size: 16 bytes
- Alignment: 8 bytes

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `impl_ptr` | `*const ()` | Pointer to the host implementation trait object |
| `functions` | `*const *const ()` | Array of function pointers, indexed by `fn_id` |

**Memory Layout**:
```
functions: [*const (); function_count]
           │
           ├─> fn_0: extern "C" fn(args: *const (), out: *mut ()) -> AbiError
           ├─> fn_1: extern "C" fn(args: *const (), out: *mut ()) -> AbiError
           └─> ...
```

---

### VmHostContractDispatch

```rust
#[repr(C)]
pub struct VmHostContractDispatch {
    pub call: unsafe extern "C" fn(
        bridge_data: *mut c_void,
        fn_id: u32,
        args: *const (),
        out: *mut (),
    ) -> AbiError,                    // Offset: 0
    pub bridge_data: *mut c_void,     // Offset: 8
}
```

**Layout (x86_64)**:
- Size: 16 bytes
- Alignment: 8 bytes

**Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `call` | function pointer | Dispatch function for VM calls |
| `bridge_data` | `*mut c_void` | Opaque VM-specific state |

**Call Signature**:
```rust
unsafe extern "C" fn(
    bridge_data: *mut c_void,  // VM-specific data
    fn_id: u32,                // Function index (0-based)
    args: *const (),           // Packed arguments
    out: *mut (),              // Output buffer
) -> AbiError
```

---

### HostContractDispatch Union

```rust
#[repr(C)]
pub union HostContractDispatch {
    pub native: NativeHostContractDispatch,
    pub vm: VmHostContractDispatch,
}
```

**Layout (x86_64)**:
- Size: 16 bytes (size of largest member)
- Alignment: 8 bytes

**Usage**:
```rust
match header.dispatch_type {
    DispatchType::Native => {
        let funcs: &[*const ()] = unsafe {
            core::slice::from_raw_parts(
                dispatch.native.functions,
                header.function_count as usize,
            )
        };
    }
    DispatchType::VirtualMachine => {
        let call_fn = unsafe { dispatch.vm.call };
        let bridge_data = unsafe { dispatch.vm.bridge_data };
    }
}
```

---

### HostContractInterface

```rust
#[repr(C)]
pub struct HostContractInterface {
    pub header: HostContractInterfaceHeader,  // Offset: 0
    pub dispatch: HostContractDispatch,    // Offset: 48
}
```

**Layout (x86_64)**:
- Size: 64 bytes
- Alignment: 8 bytes

---

## RuntimeAbi Changes

The `RuntimeAbi` structure includes a callback for host contract discovery:

```rust
#[repr(C)]
pub struct RuntimeAbi {
    // ... existing fields ...
    
    /// Get a host contract interface by contract ID.
    /// 
    /// # Arguments
    /// * `rt_ctx` - Runtime context pointer
    /// * `contract_id` - FNV-1a hash of "host_contract:{name}@{major}"
    /// * `min_minor_version` - Minimum minor version required
    /// 
    /// # Returns
    /// Pointer to HostContractInterface, or NULL if not found/incompatible
    pub get_host_contract: unsafe extern "C" fn(
        rt_ctx: *mut c_void,
        contract_id: u64,
        min_minor_version: u32,
    ) -> *const HostContractInterface,
}
```

**Usage from Guest**:
```rust
let interface_ptr = unsafe {
    (runtime_abi.get_host_contract)(
        runtime_abi.rt_ctx,
        HOSTLOGGER_CONTRACT_ID,
        0,  // min_minor_version
    )
};

if interface_ptr.is_null() {
    // Host doesn't implement this contract
    return None;
}

let interface = unsafe { &*interface_ptr };
```

---

## Generated Code Structure

### Host-Side Generation

For each host contract, the generator produces:

```
host/
├── host_contracts.rs    # Trait definitions
├── registration.rs      # Registration helpers
├── interfaces.rs        # Interface construction
└── mod.rs               # Module exports
```

#### Trait Definition (Rust)

```rust
/// Host contract: host.logger@1
pub trait HostLogger: Send + Sync {
    fn log(&self, message: &str);
}
```

#### Registration Function (Rust)

```rust
pub fn create_logger_interface(
    impl_: Box<dyn HostLogger>
) -> &'static HostContractInterface {
    // Leak implementation for 'static lifetime
    let impl_ptr = Box::into_raw(impl_);
    
    // Create function pointer array
    static FUNCTIONS: [unsafe extern "C" fn(*const (), *mut ()) -> AbiError; 1] = [
        log_dispatch,
    ];
    
    // Build interface
    let interface = HostContractInterface {
        header: HostContractInterfaceHeader {
            interface_version: 1,
            contract_id: HOSTLOGGER_CONTRACT_ID,
            contract_major: 1,
            contract_minor: 0,
            function_count: 1,
            dispatch_type: DispatchType::Native,
            _padding: [0; 4],
        },
        dispatch: HostContractDispatch {
            native: NativeHostContractDispatch {
                impl_ptr: impl_ptr as *const (),
                functions: FUNCTIONS.as_ptr() as *const (),
            },
        },
    };
    
    Box::leak(Box::new(interface))
}
```

#### Dispatch Function (Rust)

```rust
unsafe extern "C" fn log_dispatch(
    args: *const (),
    out: *mut (),
) -> AbiError {
    // SAFETY: args is guaranteed to point to StringView by codegen
    let message: &str = unsafe {
        let sv = &*(args as *const StringView);
        core::str::from_utf8_unchecked(
            core::slice::from_raw_parts(sv.ptr, sv.len)
        )
    };
    
    // Get implementation pointer
    // Call trait method
    // Return AbiError::ok()
    
    AbiError {
        code: ABI_OK,
        message: StringView::null(),
    }
}
```

---

### Guest-Side Generation

For each host contract, the generator produces:

```
guest/
├── host_contract_callers.rs  # Contract caller structs
├── host_types.rs             # Shared types
└── mod.rs                    # Module exports
```

#### Contract Caller Struct (Rust)

```rust
/// Contract caller for host.logger@1
pub struct HostLoggerCaller {
    interface: &'static HostContractInterface,
}

impl HostLoggerCaller {
    pub const CONTRACT_ID: u64 = HOSTLOGGER_CONTRACT_ID;
    pub const REQUIRED_MAJOR: u32 = 1;
    pub const MIN_MINOR: u32 = 0;
    
    /// Factory method - creates instance from RuntimeAbi
    pub unsafe fn from_host(
        runtime_abi: &RuntimeAbi,
        min_minor: u32,
    ) -> Option<Self> {
        let interface_ptr = (runtime_abi.get_host_contract)(
            runtime_abi.rt_ctx,
            Self::CONTRACT_ID,
            min_minor,
        );
        
        if interface_ptr.is_null() {
            return None;
        }
        
        let interface = &*interface_ptr;
        
        // Verify major version
        if interface.header.contract_major != Self::REQUIRED_MAJOR {
            return None;
        }
        
        Some(Self { interface })
    }
    
    /// Check if the contract is valid
    pub fn is_valid(&self) -> bool {
        !self.interface as *const _ as usize == 0
    }
    
    /// Call the log function
    pub fn log(&self, message: String) -> Result<(), ContractError> {
        let sv = StringView::from_string(&message);
        let args = LogArgs { message: sv };
        
        let func = unsafe { self.get_function(0) };
        let err = unsafe {
            func(&args as *const LogArgs as *const (), core::ptr::null_mut())
        };
        
        if err.code != ABI_OK {
            return Err(ContractError::from_abi(err));
        }
        
        Ok(())
    }
    
    unsafe fn get_function(&self, fn_id: u32) -> HostContractFn {
        let funcs = self.interface.dispatch.native.functions;
        let func_ptr = *funcs.add(fn_id as usize);
        core::mem::transmute(func_ptr)
    }
}
```

---

## Error Codes

### Host Contract Error Codes

| Code | Name | Meaning |
|------|------|---------|
| 100 | `ABI_HOST_CONTRACT_NOT_FOUND` | Contract ID not registered |
| 101 | `ABI_HOST_CONTRACT_VERSION_MISMATCH` | Minor version incompatible |
| 102 | `ABI_HOST_CONTRACT_CALL_FAILED` | Function execution failed |

### Error Handling Pattern

```rust
// Check for contract availability
match HostLoggerCaller::from_host(runtime_abi, 0) {
    Some(logger) if logger.is_valid() => {
        // Contract available
        match logger.log("message") {
            Ok(()) => { /* Success */ }
            Err(e) => { /* Call failed */ }
        }
    }
    _ => {
        // Contract not available
        // Use fallback behavior
    }
}
```

---

## Language-Specific APIs

### Rust

#### Host Registration

```rust
use polyplug::runtime::Runtime;
use generated::host::host_contracts::{HostLogger, create_logger_interface};

// Implement trait
struct MyLogger;
impl HostLogger for MyLogger {
    fn log(&self, message: &str) {
        println!("{}", message);
    }
}

// Register
let runtime = Runtime::builder().build()?;
let logger = Box::new(MyLogger);
let interface = create_logger_interface(logger);
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, interface)?;
```

#### Guest Usage

```rust
use generated::host_contract_callers::HostLoggerCaller;
use polyplug_guest::ffi::get_runtime_abi;

unsafe {
    let logger = HostLoggerCaller::from_host(get_runtime_abi(), 0);
    if let Some(logger) = logger {
        if logger.is_valid() {
            logger.log("Hello")?;
        }
    }
}
```

---

### Python

#### Host Registration

```python
from generated.contracts import HostLogger
from generated.registration import HostContractRegistration

class MyLogger(HostLogger):
    def log(self, message: str) -> None:
        print(f"[LOG] {message}")

logger = MyLogger()
HostContractRegistration.register_host_logger(runtime, logger)
```

#### Guest Usage

```python
from generated.host_callers import HostLoggerCaller

logger = HostLoggerCaller.from_host(runtime_abi)
if logger:
    logger.log("Hello")
```

---

### Lua

#### Host Registration

```lua
local contracts = require("generated.contracts")
local registration = require("generated.registration")

local logger = {
  log = function(self, message)
    print(string.format("[LOG] %s", message))
  end,
}

setmetatable(logger, contracts.HostLogger)
registration.register_host_logger(runtime, logger)
```

#### Guest Usage

```lua
local callers = require("generated.host_callers")

local logger = callers.HostLoggerCaller.from_host(runtime_abi)
if logger then
  logger:log("Hello")
end
```

---

### JavaScript (Deno)

#### Host Registration

```typescript
import { HostLogger } from "./generated/contracts.ts";
import { HostContractRegistration } from "./generated/registration.ts";

class MyLogger implements HostLogger {
  log(message: string): void {
    console.log(`[LOG] ${message}`);
  }
}

const logger = new MyLogger();
HostContractRegistration.registerHostLogger(runtime, logger);
```

#### Guest Usage

```typescript
import { HostLoggerCaller } from "./generated/host_callers.ts";

const logger = HostLoggerCaller.fromHost(runtimeAbi);
if (logger) {
  logger.log("Hello");
}
```

---

### C++

#### Host Registration

```cpp
#include "generated/contracts.hpp"
#include "generated/registration.hpp"

class MyLogger : public polyplug::host::HostLogger {
public:
    void log(uint32_t level, polyplug::StringView message) override {
        std::cout << "[LOG] " << message.to_string() << std::endl;
    }
};

MyLogger logger;
polyplug::host::HostContractRegistration::register_host_logger(
    runtime, 
    logger
);
```

#### Guest Usage

```cpp
#include "generated/host_callers.hpp"

auto logger = HostLoggerCaller::from_host(runtime_abi);
if (logger) {
    logger->log(StringView("Hello"));
}
```

---

### C#

#### Host Registration

```csharp
using Polyplug.Host;
using Generated.Contracts;

public class MyLogger : IHostLogger
{
    public void Log(uint level, StringView message)
    {
        Console.WriteLine($"[LOG] {message}");
    }
}

var logger = new MyLogger();
HostContractRegistration.RegisterHostLogger(runtime, logger);
```

#### Guest Usage

```csharp
using Generated.HostCallers;

var logger = HostLoggerCaller.FromHost(runtimeAbi);
if (logger != null) {
    logger.Log(1, new StringView("Hello"));
}
```

---

## Memory Ownership Rules

### General Principle

| Buffer | Allocated By | Freed By | Lifetime |
|--------|--------------|----------|----------|
| `args` | Caller (plugin) | Caller | Duration of call |
| `out` | Callee (host) | Callee | Duration of call |

### StringView Parameters

`StringView` parameters point to caller-owned memory:

```rust
// SAFETY: message.ptr is valid for message.len bytes
// The host must not store the pointer beyond the call duration
fn log(&self, message: StringView) {
    let msg = unsafe { message.as_str() };
    // Use msg...
    // DO NOT store message.ptr
}
```

### Interface Lifetime

The `HostContractInterface` must be `'static`:

```rust
// CORRECT: 'static interface
static INTERFACE: HostContractInterface = HostContractInterface { ... };

// FORBIDDEN: Stack-allocated interface
fn register() {
    let interface = HostContractInterface { ... };  // Will dangle
    runtime.register(interface);
}
```

---

## Thread Safety

### Send/Sync Summary

| Type | Send | Sync | Justification |
|------|------|------|---------------|
| `DispatchType` | Yes | Yes | Simple C enum (Copy) |
| `HostContractInterfaceHeader` | Yes | Yes | Plain data (Copy) |
| `NativeHostContractDispatch` | Yes | Yes | Pointer to static data |
| `VmHostContractDispatch` | Yes | Yes | Function pointer + raw pointer |
| `HostContractDispatch` | Yes | Yes | Union of Send+Sync types |
| `HostContractInterface` | Yes | Yes | Composite of Send+Sync types |

### VM-Specific Threading

**Python**:
- GIL acquired per call via `Python::with_gil()`
- Multiple runtimes share the same GIL
- `Py<T>` objects are Send but require GIL to access

**Lua**:
- State access serialized by Mutex
- Each state is thread-isolated
- No concurrent access per state

**JavaScript**:
- Context access serialized by Mutex
- QuickJS contexts are not thread-safe
- No async/await in host contract functions

---

## Version Negotiation Protocol

### Compatibility Rules

| Host Version | Plugin Requests | Compatible? | Action |
|--------------|-----------------|-------------|--------|
| 1.0 | >= 1.0 | Yes | Return interface |
| 1.1 | >= 1.0 | Yes | Return interface |
| 1.0 | >= 1.1 | No | Return NULL |
| 2.0 | >= 1.0 | No | Return NULL (major mismatch) |

### Implementation

```rust
pub(crate) unsafe extern "C" fn host_get_host_contract(
    rt_ctx: *mut c_void,
    contract_id: u64,
    min_minor_version: u32,
) -> *const HostContractInterface {
    let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
    let runtime: &Runtime = unsafe { &*ctx.runtime };
    
    match runtime.host_contracts.read() {
        Ok(contracts) => {
            match contracts.get(&contract_id) {
                Some(interface) => {
                    if interface.header.contract_minor >= min_minor_version {
                        interface as *const HostContractInterface
                    } else {
                        core::ptr::null()
                    }
                }
                None => core::ptr::null(),
            }
        }
        Err(_) => core::ptr::null(),
    }
}
```

---

## Contract ID Calculation

Host contract IDs use FNV-1a hashing with a distinct prefix:

```rust
use polyplug_abi::fnv1a_64;

// Host contract ID
const HOSTLOGGER_CONTRACT_ID: u64 = fnv1a_64(b"host_contract:host.logger@1");

// Plugin contract ID (for comparison)
const WORKER_CONTRACT_ID: u64 = fnv1a_64(b"plugin_contract:example.worker@1");
```

**Hash Input Format**:
- Host contracts: `"host_contract:{name}@{major}"`
- Plugin contracts: `"plugin_contract:{name}@{major}"`

This ensures unique IDs and prevents collisions.

---

## See Also

- `HOST_CONTRACTS.md` - Tutorial and usage guide
- `ABI_ARCHITECTURE.md` - Core ABI design
- `.sisyphus/designs/host-contract-interface.md` - Interface design document
- `.sisyphus/designs/host-runtime-bridge.md` - VM bridge architecture
- `examples/host_contracts/logger/` - Complete working example
