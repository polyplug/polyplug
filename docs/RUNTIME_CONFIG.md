# Runtime Configuration

## Overview

`RuntimeConfig` provides general runtime configuration options for polyplug. It is extensible for future features.

---

## API

### Rust

```rust
/// General runtime configuration (extensible for future features)
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    // Hot-reload options
    pub hot_reload_max_retries: u32,
    pub hot_reload_retry_interval: Duration,
    pub hot_reload_abort_on_max_retries: bool,
    
    // Future options can be added here:
    // pub log_level: LogLevel,
    // pub allocator: AllocatorConfig,
    // pub plugin_cache_size: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            hot_reload_max_retries: 3,
            hot_reload_retry_interval: Duration::from_secs(1),
            hot_reload_abort_on_max_retries: true,
        }
    }
}

impl RuntimeBuilder {
    /// Configure runtime behavior
    pub fn config(self, config: RuntimeConfig) -> Self;
}
```

### C++

```cpp
// runtime_config.hpp
struct RuntimeConfig {
    uint32_t hot_reload_max_retries = 3;
    uint32_t hot_reload_retry_interval_ms = 1000;
    bool hot_reload_abort_on_max_retries = true;
    
    // Future options can be added here
};

class Runtime {
public:
    void set_config(const RuntimeConfig& config);
    // ...
};
```

### Python

```python
# runtime_config.py
@dataclass
class RuntimeConfig:
    hot_reload_max_retries: int = 3
    hot_reload_retry_interval_seconds: float = 1.0
    hot_reload_abort_on_max_retries: bool = True
    
    # Future options can be added here

class Runtime:
    def set_config(self, config: RuntimeConfig) -> None: ...
```

### C#

```csharp
// RuntimeConfig.cs
public class RuntimeConfig
{
    public uint HotReloadMaxRetries { get; set; } = 3;
    public TimeSpan HotReloadRetryInterval { get; set; } = TimeSpan.FromSeconds(1);
    public bool HotReloadAbortOnMaxRetries { get; set; } = true;
    
    // Future options can be added here
}

public class Runtime
{
    public void SetConfig(RuntimeConfig config);
}
```

### Lua

```lua
-- runtime_config.lua
local RuntimeConfig = {
    hot_reload_max_retries = 3,
    hot_reload_retry_interval = 1.0,  -- seconds
    hot_reload_abort_on_max_retries = true,
    
    -- Future options can be added here
}

function Runtime:set_config(config) end
```

### JavaScript

```javascript
// runtime_config.js
class RuntimeConfig {
    constructor() {
        this.hotReloadMaxRetries = 3;
        this.hotReloadRetryIntervalMs = 1000;
        this.hotReloadAbortOnMaxRetries = true;
        
        // Future options can be added here
    }
}

class Runtime {
    setConfig(config) {}
}
```

---

## Configuration Options

### Hot-Reload Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `hot_reload_max_retries` | `u32` | `3` | Maximum retry attempts before aborting reload |
| `hot_reload_retry_interval` | `Duration` | `1 second` | Time to wait between retry notifications |
| `hot_reload_abort_on_max_retries` | `bool` | `true` | If `true`, abort after max retries; if `false`, retry forever |

---

## Usage Example

```rust
let config = RuntimeConfig {
    hot_reload_max_retries: 5,
    hot_reload_retry_interval: Duration::from_millis(500),
    hot_reload_abort_on_max_retries: true,
};

let rt = Runtime::builder()
    .config(config)
    .build()?;
```

---

## Extensibility

`RuntimeConfig` is designed to be extensible. Future options may include:

- Log level configuration
- Custom allocator settings
- Plugin cache size
- Thread pool configuration
- Memory limits
- Timeout defaults

When adding new options:
1. Add field to `RuntimeConfig` struct
2. Set sensible default in `Default` impl
3. Update host libs (C++, Python, C#, Lua, JS)
4. Update FFI layer
5. Document in this file