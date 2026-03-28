# Migration Guide

This guide covers migration from the old Extension system and `[[contract]]` syntax to the new Host Contracts system with `[[plugin_contract]]` syntax.

## Overview of Changes

The Host Contracts system replaces two previous mechanisms:

1. **Extension System** - Removed entirely, replaced by Host Contracts
2. **`[[contract]]` Syntax** - Renamed to `[[plugin_contract]]` with deprecation warning

## Breaking Changes

### 1. Extension System Removed

The Extension system has been completely removed. All functionality that used extensions must migrate to Host Contracts.

**Before (Extension)**:
```toml
# OLD - Extension system (removed)
[[extension]]
name = "host.logging"
version = "1.0.0"

[[extension.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
```

**After (Host Contract)**:
```toml
# NEW - Host Contracts
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
returns = "void"
```

### 2. Contract Syntax Renamed

The `[[contract]]` table is now `[[plugin_contract]]`. The old syntax still works but emits a deprecation warning.

**Before**:
```toml
[[contract]]
name = "example.worker"
version = "1.0.0"
```

**After**:
```toml
[[plugin_contract]]
name = "example.worker"
version = "1.0.0"
```

### 3. Host Contract Name Prefix

Host contract names must now start with `host.` prefix. This is enforced by the code generator.

**Invalid**:
```toml
[[host_contract]]
name = "logger"  # ERROR: must start with "host."
```

**Valid**:
```toml
[[host_contract]]
name = "host.logger"  # OK
```

### 4. Contract ID Prefix Changed

Contract IDs now use distinct prefixes to avoid collisions:

- **Plugin contracts**: `plugin_contract:{name}@{major}`
- **Host contracts**: `host_contract:{name}@{major}`

This affects:
- Manual contract ID calculations
- Contract ID constants in generated code
- Contract lookup by ID

### 5. Registration API Changed

The host contract registration API has changed from the Extension system.

**Before (Extension)**:
```rust
// OLD - Extension registration
runtime.register_extension(ext_id, ext_vtable)?;
```

**After (Host Contract)**:
```rust
// NEW - Host contract registration
runtime.register_host_contract(contract_id, vtable)?;
```

### 6. Guest-Side API Changed

The guest-side API for accessing host-provided functionality has changed.

**Before (Extension)**:
```rust
// OLD - Extension access
let ext = runtime.get_extension(ext_id)?;
```

**After (Host Contract)**:
```rust
// NEW - Host contract access
let logger = HostLoggerCaller::from_host(host_vtable, min_minor)?;
```

---

## Migration Steps

### Step 1: Update Bundle/API TOML Files

#### For Plugin Contracts

Change `[[contract]]` to `[[plugin_contract]]`:

```toml
# OLD
[[contract]]
name = "example.worker"
version = "1.0.0"

[[contract.functions]]
name = "do_work"
params = [{ name = "input", type = "StringView" }]
return = "StringView"

# NEW
[[plugin_contract]]
name = "example.worker"
version = "1.0.0"

[[plugin_contract.functions]]
name = "do_work"
params = [{ name = "input", type = "StringView" }]
return = "StringView"
```

#### For Host Contracts (formerly Extensions)

Change `[[extension]]` to `[[host_contract]]` and add `host.` prefix:

```toml
# OLD
[[extension]]
name = "logging"
version = "1.0.0"

[[extension.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]

# NEW
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
returns = "void"
```

**Note**: The `returns` field is now required (use `void` for no return value).

### Step 2: Regenerate Code

Re-run the code generator with updated TOML files:

```bash
# Host-side generation
polyplugc generate --api api.toml --lang rust --out host/generated

# Guest-side generation
polyplugc generate --bundle bundle.toml --lang rust --out guest/generated
```

The generated code will have:
- New contract ID constants with `host_contract:` prefix
- Updated trait/interface names
- New registration APIs

### Step 3: Update Host Implementation

#### Rust Example

**Before**:
```rust
// OLD - Extension implementation
use generated::extensions::LoggingExtension;

struct Logger;
impl LoggingExtension for Logger {
    fn log(&self, message: &str) {
        println!("{}", message);
    }
}

let ext = Box::new(Logger);
runtime.register_extension(LOGGING_EXT_ID, ext_vtable)?;
```

**After**:
```rust
// NEW - Host contract implementation
use generated::host::host_contracts::HostLogger;

struct Logger;
impl HostLogger for Logger {
    fn log(&self, message: &str) {
        println!("{}", message);
    }
}

let logger = Box::new(Logger);
let vtable = create_logger_vtable(logger);
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)?;
```

#### Python Example

**Before**:
```python
# OLD
from generated.extensions import LoggingExtension

class Logger(LoggingExtension):
    def log(self, message: str):
        print(message)

ExtensionRegistration.register_logging(runtime, Logger())
```

**After**:
```python
# NEW
from generated.contracts import HostLogger
from generated.registration import HostContractRegistration

class Logger(HostLogger):
    def log(self, message: str) -> None:
        print(message)

HostContractRegistration.register_host_logger(runtime, Logger())
```

### Step 4: Update Guest Implementation

#### Rust Example

**Before**:
```rust
// OLD - Extension access
use generated::extensions::LoggingExtensionCaller;

let logger = LoggingExtensionCaller::from_runtime(runtime, LOGGING_EXT_ID)?;
logger.log("message")?;
```

**After**:
```rust
// NEW - Host contract access
use generated::host_contract_callers::HostLoggerCaller;
use polyplug_guest::ffi::get_host_vtable;

let logger = unsafe {
    HostLoggerCaller::from_host(get_host_vtable(), 0)
};

if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("message")?;
    }
}
```

#### Python Example

**Before**:
```python
# OLD
from generated.extension_callers import LoggingExtensionCaller

logger = LoggingExtensionCaller.from_runtime(runtime, LOGGING_EXT_ID)
logger.log("message")
```

**After**:
```python
# NEW
from generated.host_callers import HostLoggerCaller

logger = HostLoggerCaller.from_host(host_vtable)
if logger:
    logger.log("message")
```

### Step 5: Update Contract ID Constants

Update any hardcoded contract IDs to use the new prefix:

**Before**:
```rust
// OLD
const LOGGING_EXT_ID: u64 = 0x1234567890abcdef;  // fnv1a("extension:logging@1")
```

**After**:
```rust
// NEW - Use generated constant
const HOSTLOGGER_CONTRACT_ID: u64 = generated::host::host_contracts::HOSTLOGGER_CONTRACT_ID;
// Or calculate: fnv1a_64(b"host_contract:host.logger@1")
```

### Step 6: Handle Graceful Degradation

Host contracts are optional. Update plugin code to handle cases where the host doesn't implement a contract:

**Before**:
```rust
// OLD - Extension required
let logger = get_extension(LOGGING_EXT_ID)?;  // Error if not found
logger.log("message")?;
```

**After**:
```rust
// NEW - Host contract optional
let logger = HostLoggerCaller::from_host(host_vtable, 0);
if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("message")?;
    }
}
// Continue even if logger is not available
```

---

## Migration Checklist

- [ ] Update all `[[contract]]` to `[[plugin_contract]]` in bundle.toml files
- [ ] Update all `[[extension]]` to `[[host_contract]]` in api.toml files
- [ ] Add `host.` prefix to all host contract names
- [ ] Add `returns` field to all host contract functions
- [ ] Re-run `polyplugc generate` for all bundles and APIs
- [ ] Update host-side trait implementations
- [ ] Update host-side registration calls
- [ ] Update guest-side contract access code
- [ ] Update contract ID constants
- [ ] Add graceful degradation for optional contracts
- [ ] Test all plugins with host applications
- [ ] Update documentation references

---

## Common Migration Patterns

### Pattern 1: Logging Service

**Old Extension**:
```toml
[[extension]]
name = "logging"
```

**New Host Contract**:
```toml
[[host_contract]]
name = "host.logger"
```

**Migration**:
```rust
// Before
impl LoggingExtension for ConsoleLogger { ... }
runtime.register_extension(LOGGING_EXT_ID, vtable)?;

// After
impl HostLogger for ConsoleLogger { ... }
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, vtable)?;
```

### Pattern 2: Metrics Service

**Old Extension**:
```toml
[[extension]]
name = "metrics"
```

**New Host Contract**:
```toml
[[host_contract]]
name = "host.metrics"
```

**Migration**:
```rust
// Before
impl MetricsExtension for PrometheusMetrics { ... }

// After
impl HostMetrics for PrometheusMetrics { ... }
```

### Pattern 3: Configuration Service

**Old Extension**:
```toml
[[extension]]
name = "config"
```

**New Host Contract**:
```toml
[[host_contract]]
name = "host.config"
```

**Migration**:
```rust
// Before
let config = ConfigExtensionCaller::from_runtime(runtime)?;

// After
let config = HostConfigCaller::from_host(host_vtable, 0);
```

---

## Deprecation Timeline

| Version | Status |
|---------|--------|
| 0.1.0 | `[[contract]]` deprecated, `[[plugin_contract]]` introduced |
| 0.1.0 | Extension system deprecated, Host Contracts introduced |
| 0.2.0 | `[[contract]]` removed (breaking) |
| 0.2.0 | Extension system removed (breaking) |

**Recommendation**: Migrate to Host Contracts immediately. The Extension system will be removed in version 0.2.0.

---

## Troubleshooting

### Error: "contract name must start with 'host.'"

**Cause**: Host contract name missing `host.` prefix.

**Fix**:
```toml
# WRONG
[[host_contract]]
name = "logger"

# CORRECT
[[host_contract]]
name = "host.logger"
```

### Error: "contract ID mismatch"

**Cause**: Using old contract ID calculation without `host_contract:` prefix.

**Fix**: Regenerate code or update ID calculation:
```rust
// OLD
const ID = fnv1a_64(b"extension:logging@1");

// NEW
const ID = fnv1a_64(b"host_contract:host.logger@1");
```

### Error: "get_host_contract not found"

**Cause**: Using old HostVTable without `get_host_contract` field.

**Fix**: Regenerate host-side code with latest `polyplugc`.

### Plugin crashes when calling host contract

**Cause**: Host contract not registered before plugin load.

**Fix**: Register host contracts before loading plugins:
```rust
// CORRECT order
runtime.register_host_contract(ID, vtable)?;  // First
runtime.load_bundle(&path)?;                  // Then
```

---

## See Also

- `HOST_CONTRACTS.md` - Host contracts tutorial
- `HOST_CONTRACTS_API.md` - API reference
- `examples/host_contracts/logger/` - Complete working example
- `.sisyphus/designs/host-contract-*.md` - Design documents
