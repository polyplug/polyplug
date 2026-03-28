# Host Contracts Example — Logger

Demonstrates bidirectional communication between host and plugin using host contracts.

## Overview

This example shows how:
1. The **host** implements a `host.logger` contract that provides a `log` function
2. The **plugin** implements an `example.worker` contract that provides a `do_work` function
3. When the host calls `do_work`, the plugin can call back to the host's `log` function to report progress

## Architecture

```
Host                          Plugin
  │                             │
  │  register_host_contract()   │
  │─────────────────────────────│
  │                             │
  │  do_work("hello world")     │
  │─────────────────────────────│
  │                             │
  │         log("Processing...")│
  │─────────────────────────────│
  │                             │
  │         log("Step 1...")    │
  │─────────────────────────────│
  │                             │
  │         log("Step 2...")    │
  │─────────────────────────────│
  │                             │
  │         log("Step 3...")    │
  │─────────────────────────────│
  │                             │
  │  "WORKED: HELLO WORLD"      │
  │─────────────────────────────│
```

## API Definition

```toml
# Plugin contract: The plugin implements a worker
[[plugin_contract]]
name = "example.worker"
version = "1.0.0"

[[plugin_contract.functions]]
name = "do_work"
params = [{ name = "input", type = "StringView" }]
return = "StringView"

# Host contract: The host implements a logger
[[host_contract]]
name = "host.logger"
version = "1.0.0"

[[host_contract.functions]]
name = "log"
params = [{ name = "message", type = "StringView" }]
returns = "void"
```

## File Structure

```
examples/host_contracts/logger/
├── api.toml
├── README.md
├── host/
│   ├── rust/
│   │   ├── Cargo.toml
│   │   └── src/main.rs
│   ├── python/
│   │   ├── requirements.txt
│   │   └── host.py
│   ├── csharp/
│   │   ├── HostLogger.csproj
│   │   └── Program.cs
│   ├── lua/
│   │   └── host.lua
│   ├── js/
│   │   └── host.js
│   └── cpp/
│   │   ├── CMakeLists.txt
│   │   └── main.cpp
└── guest/
    ├── rust/
    │   ├── Cargo.toml
    │   ├── bundle.toml
    │   └── src/lib.rs
    ├── python/
    │   ├── bundle.toml
    │   └── plugin.py
    ├── csharp/
    │   ├── bundle.toml
    │   └── Plugin.cs
    ├── lua/
    │   ├── bundle.toml
    │   └── plugin.lua
    ├── js/
    │   ├── bundle.toml
    │   └── plugin.js
    └── cpp/
        ├── bundle.toml
        └── plugin.cpp
```

## Build and Run

### 1. Generate Code

```bash
# Generate host-side code
polyplugc generate --api api.toml --lang rust --out host/rust/generated
polyplugc generate --api api.toml --lang python --out host/python/generated
polyplugc generate --api api.toml --lang csharp --out host/csharp/generated
polyplugc generate --api api.toml --lang lua --out host/lua/generated
polyplugc generate --api api.toml --lang js_deno --out host/js/generated
polyplugc generate --api api.toml --lang cpp --out host/cpp/generated

# Generate guest-side code
polyplugc generate --bundle guest/rust/bundle.toml --lang rust --out guest/rust/generated
polyplugc generate --bundle guest/python/bundle.toml --lang python --out guest/python/generated
polyplugc generate --bundle guest/csharp/bundle.toml --lang csharp --out guest/csharp/generated
polyplugc generate --bundle guest/lua/bundle.toml --lang lua --out guest/lua/generated
polyplugc generate --bundle guest/js/bundle.toml --lang js_deno --out guest/js/generated
polyplugc generate --bundle guest/cpp/bundle.toml --lang cpp --out guest/cpp/generated
```

### 2. Build Plugin

```bash
# Rust plugin
cd guest/rust && cargo build --release

# Copy to plugins directory
mkdir -p plugins/rust_worker
cp guest/rust/generated/manifest.toml plugins/rust_worker/
cp guest/rust/target/release/libworker.so plugins/rust_worker/
```

### 3. Run Host

```bash
# Rust host
cd host/rust && cargo run --release

# Python host
cd host/python && python host.py

# C# host
cd host/csharp && dotnet run

# Lua host
cd host/lua && lua host.lua

# JavaScript host
cd host/js && deno run --allow-read --allow-ffi --allow-env host.js

# C++ host
cd host/cpp && cmake -B build && cmake --build build && ./build/logger_host
```

## Expected Output

```
loading plugins from: examples/host_contracts/logger/plugins

  loaded: rust_worker

discovered 1 bundles

=== Logger Host (Rust) ===

Input: "hello world"

[PLUGIN LOG] Processing input: hello world
[PLUGIN LOG] Step 1: Analyzing input
[PLUGIN LOG] Step 2: Transforming data
[PLUGIN LOG] Step 3: Generating output
[host] do_work("hello world") = "WORKED: HELLO WORLD"

done.
```

## Key Concepts

### Host Contract Registration

The host must register its implementation before loading plugins:

```rust
let logger_impl = Box::new(ConsoleLogger);
let logger_vtable = create_logger_vtable(logger_impl);
runtime.register_host_contract(HOSTLOGGER_CONTRACT_ID, logger_vtable)?;
```

### Guest Access to Host Contract

The plugin accesses the host contract through the generated caller:

```rust
let logger = HostLoggerCaller::from_host(get_host_vtable(), 1);
if let Some(logger) = logger {
    if logger.is_valid() {
        logger.log("Processing...".to_string());
    }
}
```

### Contract ID Calculation

Host contract IDs use a distinct prefix to avoid collisions with plugin contract IDs:

- Plugin contract: `plugin_contract:name@major` → FNV-1a hash
- Host contract: `host_contract:name@major` → FNV-1a hash

This ensures `host.logger` and a hypothetical `plugin.logger` have different IDs.

## See Also

- `api.toml` — API definition
- `../examples/README.md` — Main examples overview
- `../../docs/host_contracts.md` — Host contracts documentation