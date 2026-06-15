# Host Contracts Tutorial

## Terminology Note

This document uses the following terminology (current as of v1.1):
- **HostApi**: The runtime's ABI function table provided to guests during `polyplug_init`
- **HostContractInterface**: A contract the host implements for guests to call

## Overview

Host contracts enable **bidirectional communication** between the host application and plugins. While plugin contracts define functions that the host can call on plugins, host contracts define functions that plugins can call back to the host.

This creates a two-way communication channel:

```
Host Application                    Plugin Bundle
       │                                  │
       │  register_host_contract()        │
       │─────────────────────────────────>│
       │                                  │
       │  call plugin function            │
       │─────────────────────────────────>│
       │                                  │
       │         plugin calls host        │
       │<─────────────────────────────────│
       │                                  │
       │  return result                   │
       │<─────────────────────────────────│
```

## Terminology Clarification

- **HostApi**: The runtime's ABI provided to guests (host's functions like `alloc`, `find_guest_contract`, `register_guest_contract`, etc.). This is passed to plugins during `polyplug_init`.
- **HostContractInterface**: A contract the host implements for guests to call (e.g., logging, metrics). This is registered via `register_host_contract`.
- **GuestContractInterface**: A contract plugins implement for the host to call.

The naming separation clarifies the Host/Guest relationship: the host provides the HostApi, while both host and guest can provide contract interfaces.

## How Host Contracts Differ from Plugin Contracts

| Aspect | Plugin Contracts | Host Contracts |
|--------|-----------------|----------------|
| **Direction** | Host calls plugin | Plugin calls host |
| **Implementation** | Plugin implements | Host implements |
| **Registration** | Via `polyplug_init` | Via `register_host_contract` |
| **Discovery** | Host finds plugin | Plugin queries host |
| **Use Case** | Plugin functionality | Host services (logging, metrics, config) |

## Singleton vs Per-Instance Host Contracts

Each `[[host_contract]]` carries a `singleton` flag (defaults to `false` — i.e.
per-instance). The application developer chooses per contract:

```toml
[[host_contract]]
name = "host.logger"
version = "1.0.0"
singleton = true     # one shared instance for all plugins (default is false)
```

What the flag controls — the **runtime's** instance caching:

- `singleton = true` — the runtime creates the instance once (lazily, on the first
  `get_host_contract`) and hands the same `HostContractInstance` to every plugin caller
  (cached in `singleton_instances`).
- `singleton = false` — the runtime calls the provider's `create_instance` once per
  `get_host_contract` caller, so each caller receives its own instance and
  `destroy_instance` reclaims it.

Whether distinct instances actually hold **independent state** also depends on the
provider's `create_instance`:

- **Lua and JavaScript (Deno) host providers** build a fresh implementation from a
  registered factory per `create_instance` and key it by a non-zero instance id, so
  `singleton = false` yields genuinely independent per-instance state. The Deno provider
  uses native dispatch via `Deno.UnsafeCallback` (the SDK's `buildHostContractInterface`);
  see `sdks/lua/host/tests/test_host_contract_per_instance.lua` and
  `sdks/js/host/tests/host_contract_provider_test.ts`.
- **Native host providers (Rust/C++/C#) and the Python provider** carry a single
  implementation pointer through `user_data` (see the note below), so `create_instance`
  returns that same implementation regardless of the flag — they are single-implementation
  by design. Use `singleton = true` for them to make the intent explicit.

## Common Use Cases

Host contracts are ideal for providing shared services to plugins:

- **Logging** - Plugins report events to the host's logging system
- **Metrics** - Plugins record performance metrics
- **Configuration** - Plugins read host configuration
- **Resource Access** - Plugins request host-managed resources
- **Event Emission** - Plugins notify host of important events

## Step-by-Step Tutorial

### Step 1: Define the API

Create an `api.toml` file that defines both plugin and host contracts:

```toml
# api.toml

# Plugin contract: The plugin implements a worker
[[plugin_contract]]
name = "example.worker"
version = "1.0.0"

[[plugin_contract.functions]]
name = "do_work"
params = [{ name = "input", type = "StringView" }]
return = "StringView"

# Host contract: The host implements a logger
# Host contract names MUST start with "host." prefix
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
returns = "void"
```

**Important**: Host contract names must start with the `host.` prefix. This distinguishes them from plugin contracts and ensures unique contract IDs.

> **How the impl is carried.** A host contract factory no longer relies on static or
> thread-local storage. The registrant's implementation pointer is stored in the
> `user_data` field of `HostContractInterface` (offset 40), and `create_instance` /
> `destroy_instance` recover it via `(*this).user_data`. The runtime never reads, writes,
> or frees the pointee — it only stores the pointer. (C# and Python keep an additional
> managed-side reference to the implementation object by documented necessity, so the GC
> does not collect it while the runtime holds the raw `user_data` pointer.)

### Step 2: Generate Code

Generate code for both host and guest sides:

```bash
# Generate host-side code
polyplugc generate --api api.toml --lang rust --out host/generated

# Generate guest-side code  
polyplugc generate --bundle bundle.toml --lang rust --out guest/generated
```

### Step 3: Implement Host Contract (Host Side)

The host application implements the host contract trait:

```rust
// Host application (Rust)
use generated::host::host_contracts::HostLogger;
use generated::host::types::StringView;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[PLUGIN LOG] {}", message);
    }
}
```

### Step 4: Register Host Contract

Before loading plugins, register the host contract implementation:

```rust
use polyplug::runtime::Runtime;
use generated::host::host_contracts::create_logger_interface;

// Create and register host contract
let logger_impl = Box::new(ConsoleLogger);
let logger_interface = create_logger_interface(logger_impl);
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, logger_interface)?;

// Now load plugins - they can call the host's log function
runtime.load_bundle(&plugin_path)?;
```

### Step 5: Call Host Contract from Plugin (Guest Side)

The plugin accesses the host contract through the generated caller:

```rust
// Plugin implementation (Rust)
use generated::contracts::ExampleWorkerPlugin;
use generated::host_contract_callers::HostLoggerCaller;
use polyplug_abi::StringView;
use polyplug_guest::GuestError;

struct WorkerPlugin;

impl ExampleWorkerPlugin for WorkerPlugin {
    fn do_work(&self, input: StringView) -> Result<StringView, GuestError> {
        // Get the host contract caller from the HostApi
        let logger = unsafe {
            HostLoggerCaller::from_host(host, 1)
        };
        
        // Call host contract if available
        if let Some(logger) = logger {
            if logger.is_valid() {
                logger.log("Processing input...")?;
                logger.log("Step 1: Analyzing")?;
                logger.log("Step 2: Transforming")?;
                logger.log("Step 3: Generating output")?;
            }
        }
        
        // Return result
        Ok(result)
    }
}
```

## Examples in All Languages

### Rust (Native Host)

```rust
// 1. Define the trait (auto-generated)
pub trait HostLogger: Send + Sync {
    fn log(&self, message: &str);
}

// 2. Implement the trait
struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[LOG] {}", message);
    }
}

// 3. Register with runtime
let logger_impl = Box::new(ConsoleLogger);
let interface = create_logger_interface(logger_impl);
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, interface)?;
```

### Python (VM Host)

```python
# 1. Define the implementation
from generated.contracts import HostLogger

class ConsoleLogger(HostLogger):
    def log(self, message: str) -> None:
        print(f"[LOG] {message}")

# 2. Register with runtime
from generated.registration import HostContractRegistration

logger = ConsoleLogger()
HostContractRegistration.register_host_logger(runtime, logger)
```

### Lua (VM Host)

```lua
-- 1. Define the implementation
local logger = {
  log = function(self, message)
    print(string.format("[LOG] %s", message))
  end,
}

-- 2. Set metatable
setmetatable(logger, require("generated.contracts").HostLogger)

-- 3. Register with runtime
local registration = require("generated.registration")
registration.register_host_logger(runtime, logger)
```

### JavaScript (VM Host - QuickJS)

```typescript
// 1. Define the implementation
import { HostLogger } from "./generated/contracts.ts";

class ConsoleLogger implements HostLogger {
  log(message: string): void {
    console.log(`[LOG] ${message}`);
  }
}

// 2. Register with runtime
import { HostContractRegistration } from "./generated/registration.ts";

const logger = new ConsoleLogger();
HostContractRegistration.registerHostLogger(runtime, logger);
```

### C++ (Native Host)

```cpp
// 1. Define the implementation
#include "generated/contracts.hpp"

class ConsoleLogger : public polyplug::host::HostLogger {
public:
    void log(uint32_t level, polyplug::StringView message) override {
        std::cout << "[LOG] " << message.to_string() << std::endl;
    }
};

// 2. Register with runtime
ConsoleLogger logger;
polyplug::host::HostContractRegistration::register_host_logger(
    runtime, 
    logger
);
```

### C# (Native Host)

```csharp
// 1. Define the implementation
using Polyplug.Host;

public class ConsoleLogger : IHostLogger
{
    public void Log(uint level, StringView message)
    {
        Console.WriteLine($"[LOG] {message.ToString()}");
    }
}

// 2. Register with runtime
var logger = new ConsoleLogger();
HostContractRegistration.RegisterHostLogger(runtime, logger);
```

## Guest-Side Usage (All Languages)

### Rust

```rust
let logger = unsafe { HostLoggerCaller::from_host(host, 1) };
if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("Hello from plugin!")?;
    }
}
```

### Python

```python
logger = HostLoggerCaller.from_host(host)
if logger:
    logger.log("Hello from plugin!")
```

### Lua

```lua
local logger = M.HostLoggerCaller.from_host(host)
if logger then
  logger:log("Hello from plugin!")
end
```

### JavaScript

```typescript
const logger = HostLoggerCaller.fromHost(host);
if (logger) {
  logger.log("Hello from plugin!");
}
```

### C++

```cpp
auto logger = HostLoggerCaller::from_host(host);
if (logger) {
    logger->log(StringView("Hello from plugin!"));
}
```

### C#

```csharp
var logger = HostLoggerCaller.FromHost(host);
if (logger != null) {
    logger.Log(1, new StringView("Hello from plugin!"));
}
```

## Contract ID Calculation

Host contract IDs use a distinct prefix to avoid collisions with plugin contract IDs:

- **Guest (plugin) contract**: `guest_contract:name@major` → FNV-1a hash
- **Host contract**: `host_contract:name@major` → FNV-1a hash

For example:
- `host.logger@1` → `host_contract:host.logger@1` → FNV-1a hash
- `example.worker@1` → `guest_contract:example.worker@1` → FNV-1a hash

This ensures that `host.logger` and a hypothetical `plugin.logger` have different IDs and never collide.

## Version Negotiation

Host contracts support version negotiation. When a plugin requests a host contract:

1. Plugin specifies minimum minor version it requires
2. Host returns the interface if its minor version >= requested
3. Plugin checks major version matches exactly
4. If incompatible, plugin receives `null` and must handle gracefully

```rust
// Plugin side - request with minimum minor version
let logger = unsafe { HostLoggerCaller::from_host(host, 2) };

// If host implements 1.3 and plugin needs >= 1.2, success
// If host implements 1.1 and plugin needs >= 1.2, returns None
// If host implements 2.0 and plugin needs >= 1.2, returns None (major mismatch)
```

## Error Handling

Host contract calls can fail. Always handle errors gracefully:

```rust
match logger.log("message") {
    Ok(()) => { /* Success */ }
    Err(e) => { 
        // Handle error - host may not implement the contract
        // or the call may have failed
    }
}
```

In languages with exceptions:

```python
try:
    logger.log("message")
except ContractError as e:
    # Handle error
    pass
```

## Best Practices

### 1. Make Host Contracts Optional

Plugins should work even if the host doesn't implement a host contract:

```rust
if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("message")?;
    }
}
// Continue even if logger is not available
```

### 2. Use Descriptive Names

Host contract names should be clear and descriptive:

- ✅ `host.logger` - Clear purpose
- ✅ `host.metrics.recorder` - Specific service
- ❌ `host.func1` - Unclear

### 3. Keep Interfaces Small

Host contracts should have focused, single-responsibility interfaces:

```toml
# Good - focused contract
[[host_contract]]
name = "host.logger"
# Only logging functions

# Good - separate concern
[[host_contract]]
name = "host.metrics"
# Only metrics functions
```

### 4. Document Expected Behavior

Document what the host contract does and any side effects:

```rust
/// Log a message at the specified level.
/// 
/// # Side Effects
/// The message is written to the host's log output.
/// The host may filter by level or discard messages.
/// 
/// # Arguments
/// * `level` - Log level (0=DEBUG, 1=INFO, 2=WARN, 3=ERROR)
/// * `message` - Log message (UTF-8 string)
fn log(&self, level: u32, message: &str);
```

### 5. Handle VM Threading Constraints

For VM-based hosts (Python, Lua, JavaScript):

- Python: GIL is acquired automatically per call
- Lua: State access is serialized by mutex
- JavaScript: Context access is serialized by mutex

Host contract implementations should be thread-safe and avoid long-running operations.
