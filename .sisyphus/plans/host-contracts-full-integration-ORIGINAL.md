# Host Contracts Full Integration Plan

## Goal
Complete the migration by integrating host.logger into ALL main examples, then delete the standalone host_contracts/ directory.

## Current State
- ✅ Code generators support host contracts
- ✅ Runtime supports host contracts
- ✅ host_contracts/logger example works standalone
- ❌ Main examples don't use host contracts
- ❌ host_contracts/ directory still exists

## Target State
- ✅ All 6 hosts (Rust, C++, C#, Python, Lua, JS) implement and register host.logger
- ✅ All 6 guest reporters call host.logger
- ✅ Build scripts compile everything
- ✅ Verify scripts confirm PLUGIN LOG output
- ✅ examples/host_contracts/ DELETED

---

## Execution Strategy

### Phase 0: Critical Infrastructure Setup (PRE-BLOCKERS)

**MOMUS IDENTIFIED CRITICAL ISSUES:**
1. ❌ build_all.sh doesn't generate HOST code
2. ❌ examples/hosts/*/generated/ lacks vtable_factories and host_contracts
3. ❌ api.toml has 2 functions but infrastructure not ready

**Task 0.1: ACTUALLY UPDATE build_all.sh**

**File:** `examples/build_all.sh`

**CURRENT STATE (broken):**
```bash
# Line 45 - only generates GUEST code
"$POLYPLUGC" generate --bundle "$dir/bundle.toml" --lang "$lang" --out "$dir/generated"
```

**REQUIRED ADDITION (after line 94, AFTER guest loop ends):**

**CURRENT build_all.sh structure:**
```bash
# Lines 27-94: Guest generation loop
for lang in $LANGS; do
    for plugin in $PLUGINS; do
        # ... guest generation ...
    done
done  # <-- LINE 94: Guest loop ends here

echo ""              # Line 95
echo "[4/4] Building hosts..."  # Line 96
# Line 97+: Host building
```

**INSERT AFTER LINE 94 (after `done`, before empty line):**
```bash
done  # <-- Line 94: INSERT AFTER THIS LINE

# [2.5/4] Generate HOST code with host contracts
# CRITICAL: This is REQUIRED for host.logger integration
# INSERT THIS BLOCK:
echo "[2.5/4] Generating host code with host contracts..."
for lang in rust cpp csharp python lua js; do
    host_dir="hosts/$lang"
    if [ -d "$host_dir" ]; then
        echo "  generating: $lang host"
        "$POLYPLUGC" generate --api api.toml --lang "$lang" --out "$host_dir/generated"
        if [ $? -ne 0 ]; then
            echo "ERROR: Failed to generate host code for $lang"
            exit 1
        fi
    fi
done
echo ""

# Then continue with existing:
echo "[4/4] Building hosts..."
```

**CRITICAL**: This goes AFTER the guest loop (line 94), NOT after line 45 (which is inside the loop).

**Acceptance Criteria:**
- [ ] build_all.sh includes host generation step
- [ ] build_all.sh verifies vtable_factories.* exists after generation
- [ ] build_all.sh exits with error if generation fails

---

**Task 0.2: GENERATE HOST CODE (Run this FIRST)**

**Execute BEFORE any host modifications:**
```bash
cd /mnt/data/Projects/Utils/polyplug/examples

# Generate host code for ALL languages
for lang in rust cpp csharp python lua js; do
    echo "Generating host code for $lang..."
    polyplugc generate --api api.toml --lang $lang --out hosts/$lang/generated
done

# Verify it worked
echo "Verifying generated files..."
for lang in rust cpp csharp python lua js; do
    if ls hosts/$lang/generated/host/vtable_factories.* 1> /dev/null 2>&1; then
        echo "  ✓ $lang: vtable_factories exists"
    else
        echo "  ✗ $lang: vtable_factories MISSING"
        exit 1
    fi
    
    if ls hosts/$lang/generated/host/host_contracts.* 1> /dev/null 2>&1; then
        echo "  ✓ $lang: host_contracts exists"
    else
        echo "  ✗ $lang: host_contracts MISSING"
        exit 1
    fi
done
```

**Expected Output:**
```
Generating host code for rust...
Generating host code for cpp...
Generating host code for csharp...
Generating host code for python...
Generating host code for lua...
Generating host code for js...
Verifying generated files...
  ✓ rust: vtable_factories exists
  ✓ rust: host_contracts exists
  ✓ cpp: vtable_factories exists
  ✓ cpp: host_contracts exists
  ...
```

**Acceptance Criteria:**
- [ ] Command executes without errors
- [ ] All 6 languages have vtable_factories.* generated
- [ ] All 6 languages have host_contracts.* generated
- [ ] Files contain create_host_logger_vtable function
- [ ] Files contain HostLogger trait/interface

---

**Task 0.3: VERIFY Code Generator Creates Correct mod.rs**

**MOMUS CRITICAL NOTE**: Do NOT edit generated files - they have "DO NOT EDIT" headers!

**Instead**: The code generator (polyplugc) MUST create these modules automatically.

**Verify after Task 0.2:**
```bash
# Check if mod.rs was generated with correct modules
cat examples/hosts/rust/generated/host/mod.rs
```

**Expected content:**
```rust
pub mod host_contracts;
pub mod host_callers;
pub mod types;
pub mod vtable_factories;
```

**If modules are missing**, the issue is in the **code generator** (rust.rs), not the generated file.

**ACCEPTANCE**: Plan assumes polyplugc generates correct mod.rs. If not, that's a pre-existing bug in the generator, not this plan's responsibility.

---

**Task 0.4: REGENERATE GUEST CODE (if needed)**

**Execute AFTER Task 0.2:**
```bash
cd /mnt/data/Projects/Utils/polyplug/examples

for lang in rust cpp csharp python lua js; do
    for plugin in decoder encoder transformer reporter validator; do
        bundle_path="guests/$lang/$plugin/bundle.toml"
        if [ -f "$bundle_path" ]; then
            echo "Generating guest: $lang/$plugin..."
            polyplugc generate --bundle "$bundle_path" --lang $lang --out guests/$lang/$plugin/generated
        fi
    done
done
```

---

### Task 0 Summary - EXECUTION ORDER:

**DO THESE IN EXACT ORDER:**

1. **Task 0.1**: Edit build_all.sh (add host generation AFTER line 94)
2. **Task 0.2**: Generate host code manually (to verify it works)
3. **Task 0.3**: Verify mod.rs has correct modules (if not, generator bug)
4. **Task 0.4**: Regenerate guest code
5. **THEN** proceed to Tasks 1-12

**Task 13** (Update build_all.sh) is now DONE via Task 0.1 - don't duplicate.

---

### Phase 1: Foundation (Tasks 1-2)

### Phase 2: Rust Integration (Tasks 1-2)

**Task 1: Update Rust Host**
- File: `examples/hosts/rust/src/main.rs`

**Current imports (INCORRECT in original plan):**
```rust
// WRONG - these modules don't exist yet
use generated::host::host_contracts::HostLogger;
use generated::host::vtable_factories::create_host_logger_vtable;
```

**Correct imports after Task 0:**
```rust
mod generated;
use generated::host::host_contracts::HostLogger;
use generated::host::vtable_factories::create_host_logger_vtable;
use generated::types::LogLevel;  // ADD for log_with_level
```

**Full Implementation:**
```rust
struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[PLUGIN LOG] {}", message);
    }
    
    fn log_with_level(&self, level: LogLevel, message: &str) {
        let level_str = match level {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        };
        println!("[PLUGIN LOG] [{}] {}", level_str, message);
    }
}

fn main() {
    let runtime = Runtime::new();
    
    // CRITICAL: Register BEFORE loading plugins
    let logger = ConsoleLogger;
    let vtable = create_host_logger_vtable(Box::new(logger));
    runtime.register_host_contract(0xF53EB5F2845853BB, vtable);
    
    runtime.load_plugins("./plugins");
    runtime.run();
}
```

**Task 2: Update Rust Guest Reporter**
- File: `examples/guests/rust/reporter/src/lib.rs`

**Correct import path (FIXED):**
```rust
// CORRECT path - host_contract_callers is in guest/generated/guest/
use generated::host_contract_callers::HostLoggerCaller;
use generated::types::LogLevel;
```

**Implementation with BOTH functions:**
```rust
fn report(&self, input: &str) -> String {
    let logger = unsafe {
        HostLoggerCaller::from_host(polyplug_guest::get_host_vtable(), 1)
    };
    
    if let Some(logger) = logger {
        if logger.is_valid() {
            // Use log()
            logger.log(format!("Starting report for: {}", input));
            
            // Use log_with_level()
            logger.log_with_level(LogLevel::Info, "Step 1: Parsing input");
            logger.log_with_level(LogLevel::Debug, format!("Input length: {}", input.len()));
            logger.log_with_level(LogLevel::Warn, "Step 2: Processing data");
            logger.log_with_level(LogLevel::Info, "Step 3: Generating output");
            logger.log_with_level(LogLevel::Error, "Step 4: Finalizing report");
        }
    }
    
    format!("Report: {}", input)
}
```

### Phase 3: C++ Integration (Tasks 3-4)

**Task 3: Update C++ Host**
- File: `examples/hosts/cpp/main.cpp`

**Language-Specific Pattern (NOT "Same as Rust"):**
```cpp
#include "generated/host/host_contracts.hpp"
#include "generated/host/vtable_factories.hpp"
#include "generated/types.hpp"

class ConsoleLogger : public polyplug_host::HostLogger {
public:
    void log(polyplug_host::StringView message) override {
        std::cout << "[PLUGIN LOG] " << message.to_string() << std::endl;
    }
    
    void log_with_level(polyplug_host::LogLevel level, polyplug_host::StringView message) override {
        const char* level_str = "";
        switch(level) {
            case polyplug_host::LogLevel::Debug: level_str = "DEBUG"; break;
            case polyplug_host::LogLevel::Info: level_str = "INFO"; break;
            case polyplug_host::LogLevel::Warn: level_str = "WARN"; break;
            case polyplug_host::LogLevel::Error: level_str = "ERROR"; break;
        }
        std::cout << "[PLUGIN LOG] [" << level_str << "] " << message.to_string() << std::endl;
    }
};

int main() {
    auto runtime = polyplug::Runtime::create();
    
    // Register BEFORE loading plugins
    auto logger = std::make_unique<ConsoleLogger>();
    auto vtable = polyplug_host::create_host_logger_vtable(std::move(logger));
    runtime->register_host_contract(polyplug_host::HOSTLOGGER_CONTRACT_ID, vtable);
    
    runtime->load_plugins("./plugins");
    runtime->run();
}
```

**Task 4: Update C++ Guest Reporter**
- File: `examples/guests/cpp/reporter/reporter.cpp`

```cpp
#include "generated/guest/host_contract_callers.hpp"
#include "generated/types.hpp"

std::string report(const std::string& input) {
    auto logger = polyplug_guest::HostLoggerCaller::from_host(
        polyplug_guest::get_host_vtable(), 1
    );
    
    if (logger && logger->is_valid()) {
        logger->log("Starting report for: " + input);
        logger->log_with_level(polyplug_guest::LogLevel::Info, "Step 1");
        logger->log_with_level(polyplug_guest::LogLevel::Debug, "Debug info");
        logger->log_with_level(polyplug_guest::LogLevel::Warn, "Warning");
        logger->log_with_level(polyplug_guest::LogLevel::Error, "Error");
    }
    
    return "Report: " + input;
}
```

### Phase 4: C# Integration (Tasks 5-6)

**Task 5: Update C# Host**
- File: `examples/hosts/csharp/Program.cs`

```csharp
using Polyplug.Generated;

public class ConsoleLogger : IHostLogger {
    public void log(string message) {
        Console.WriteLine($"[PLUGIN LOG] {message}");
    }
    
    public void log_with_level(LogLevel level, string message) {
        string levelStr = level switch {
            LogLevel.Debug => "DEBUG",
            LogLevel.Info => "INFO",
            LogLevel.Warn => "WARN",
            LogLevel.Error => "ERROR",
            _ => "UNKNOWN"
        };
        Console.WriteLine($"[PLUGIN LOG] [{levelStr}] {message}");
    }
}

class Program {
    static void Main() {
        var runtime = Runtime.Create();
        
        // Register BEFORE loading plugins
        var logger = new ConsoleLogger();
        var vtable = VTableFactories.CreateHostLoggerVTable(logger);
        runtime.RegisterHostContract(Contracts.HOSTLOGGER_CONTRACT_ID, vtable);
        
        runtime.LoadPlugins("./plugins");
        runtime.Run();
    }
}
```

**Task 6: Update C# Guest Reporter**
- File: `examples/guests/csharp/reporter/Reporter.cs`

```csharp
using Polyplug.Generated;

public string Report(string input) {
    var logger = HostLoggerCaller.FromHost(PolyplugGuest.GetHostVTable(), 1);
    
    if (logger?.IsValid == true) {
        logger.Log($"Starting report for: {input}");
        logger.LogWithLevel(LogLevel.Info, "Step 1");
        logger.LogWithLevel(LogLevel.Debug, "Debug");
        logger.LogWithLevel(LogLevel.Warn, "Warning");
        logger.LogWithLevel(LogLevel.Error, "Error");
    }
    
    return $"Report: {input}";
}
```

### Phase 5: Python Integration (Tasks 7-8)

**Task 7: Update Python Host**
- File: `examples/hosts/python/host.py`

```python
from generated.host.host_contracts import HostLogger, LogLevel
from generated.host.vtable_factories import create_host_logger_vtable

class ConsoleLogger(HostLogger):
    def log(self, message):
        print(f"[PLUGIN LOG] {message}")
    
    def log_with_level(self, level, message):
        level_str = {
            LogLevel.Debug: "DEBUG",
            LogLevel.Info: "INFO",
            LogLevel.Warn: "WARN",
            LogLevel.Error: "ERROR"
        }.get(level, "UNKNOWN")
        print(f"[PLUGIN LOG] [{level_str}] {message}")

# In main:
logger = ConsoleLogger()
vtable = create_host_logger_vtable(logger)
rt.register_host_contract(contracts.HOSTLOGGER_CONTRACT_ID, vtable)
```

**Task 8: Update Python Guest Reporter**
- File: `examples/guests/python/reporter/reporter.py`

```python
from generated.guest.host_contract_callers import HostLoggerCaller
from generated.types import LogLevel

def report(input_data):
    logger = HostLoggerCaller.from_host(polyplug_guest.get_host_vtable(), 1)
    
    if logger and logger.is_valid():
        logger.log(f"Starting report for: {input_data}")
        logger.log_with_level(LogLevel.Info, "Step 1")
        logger.log_with_level(LogLevel.Debug, "Debug")
        logger.log_with_level(LogLevel.Warn, "Warning")
        logger.log_with_level(LogLevel.Error, "Error")
    
    return f"Report: {input_data}"
```

### Phase 6: Lua Integration (Tasks 9-10)

**Task 9: Update Lua Host**
- File: `examples/hosts/lua/host.lua`

```lua
local host_contracts = require("generated.host.host_contracts")
local vtable_factories = require("generated.host.vtable_factories")
local types = require("generated.types")

local ConsoleLogger = {}
ConsoleLogger.__index = ConsoleLogger

function ConsoleLogger:new()
    return setmetatable({}, self)
end

function ConsoleLogger:log(message)
    print("[PLUGIN LOG] " .. message)
end

function ConsoleLogger:log_with_level(level, message)
    local level_str = ""
    if level == types.LogLevel.Debug then level_str = "DEBUG"
    elseif level == types.LogLevel.Info then level_str = "INFO"
    elseif level == types.LogLevel.Warn then level_str = "WARN"
    elseif level == types.LogLevel.Error then level_str = "ERROR" end
    print("[PLUGIN LOG] [" .. level_str .. "] " .. message)
end

-- In main:
local logger = ConsoleLogger:new()
local vtable = vtable_factories.create_host_logger_vtable(logger)
rt:register_host_contract(contracts.HOSTLOGGER_CONTRACT_ID, vtable)
```

**Task 10: Update Lua Guest Reporter**
- File: `examples/guests/lua/reporter/reporter.lua`

```lua
local host_contract_callers = require("generated.guest.host_contract_callers")
local types = require("generated.types")

function report(input)
    local logger = host_contract_callers.HostLoggerCaller:from_host(polyplug_guest.get_host_vtable(), 1)
    
    if logger and logger:is_valid() then
        logger:log("Starting report for: " .. input)
        logger:log_with_level(types.LogLevel.Info, "Step 1")
        logger:log_with_level(types.LogLevel.Debug, "Debug")
        logger:log_with_level(types.LogLevel.Warn, "Warning")
        logger:log_with_level(types.LogLevel.Error, "Error")
    end
    
    return "Report: " .. input
end
```

### Phase 7: JavaScript Integration (Tasks 11-12)

**Task 11: Update JS Host**
- File: `examples/hosts/js/host.js`

```javascript
import { HostLogger, LogLevel } from "./generated/host/host_contracts.ts";
import { createHostLoggerVtable } from "./generated/host/vtable_factories.ts";

class ConsoleLogger extends HostLogger {
    log(message) {
        console.log(`[PLUGIN LOG] ${message}`);
    }
    
    log_with_level(level, message) {
        const levelStr = {
            [LogLevel.Debug]: "DEBUG",
            [LogLevel.Info]: "INFO",
            [LogLevel.Warn]: "WARN",
            [LogLevel.Error]: "ERROR"
        }[level] || "UNKNOWN";
        console.log(`[PLUGIN LOG] [${levelStr}] ${message}`);
    }
}

// In main:
const logger = new ConsoleLogger();
const vtable = createHostLoggerVtable(logger);
rt.registerHostContract(Contracts.HOSTLOGGER_CONTRACT_ID, vtable);
```

**Task 12: Update JS Guest Reporter**
- File: `examples/guests/js/reporter/reporter.js`

```javascript
import { HostLoggerCaller } from "./generated/guest/host_contract_callers.ts";
import { LogLevel } from "./generated/types.ts";

function report(input) {
    const logger = HostLoggerCaller.fromHost(polyplugGuest.getHostVtable(), 1);
    
    if (logger?.isValid()) {
        logger.log(`Starting report for: ${input}`);
        logger.logWithLevel(LogLevel.Info, "Step 1");
        logger.logWithLevel(LogLevel.Debug, "Debug");
        logger.logWithLevel(LogLevel.Warn, "Warning");
        logger.logWithLevel(LogLevel.Error, "Error");
    }
    
    return `Report: ${input}`;
}
```

### Phase 8: Build Scripts (Task 13)
**Task 13: Update build_all.sh**

**CRITICAL FIX**: Add host code generation (currently missing!)

**File:** `examples/build_all.sh`

**Add after guest generation (around line 45):**
```bash
# [2.5/4] Generate HOST code with host contracts
# CRITICAL: This was missing! Needed for host.logger integration
echo "[2.5/4] Generating host code with host contracts..."
for lang in rust cpp csharp python lua js; do
    host_dir="hosts/$lang"
    if [ -d "$host_dir" ]; then
        echo "  generating: $lang host"
        "$POLYPLUGC" generate --api api.toml --lang "$lang" --out "$host_dir/generated"
        if [ $? -ne 0 ]; then
            echo "ERROR: Failed to generate host code for $lang"
            exit 1
        fi
    fi
done
echo ""
```

**Full build sequence:**
1. Clean
2. Build polyplugc
3. Generate GUEST code (existing)
4. **Generate HOST code** (NEW - critical fix)
5. Build guests
6. Build hosts
7. Copy to plugins

---

### Phase 9: Create verify_hosts.sh (Task 14)
**Task 14: Create verify_hosts.sh**

**File:** `examples/verify_hosts.sh` (NEW FILE)

```bash
#!/bin/bash
# verify_hosts.sh - Verify host contract integration

set -e

echo "=== Verifying Host Contract Integration ==="
echo ""

# Check that host_contracts directory still exists (should be deleted after migration)
if [ -d "host_contracts" ]; then
    echo "⚠ WARNING: host_contracts/ directory still exists"
    echo "   Run 'rm -rf host_contracts/' after migration is complete"
    echo ""
fi

# Verify generated files exist
echo "Checking generated files..."
for lang in rust cpp csharp python lua js; do
    if [ ! -f "hosts/$lang/generated/host/vtable_factories.rs" ] && \
       [ ! -f "hosts/$lang/generated/host/vtable_factories.hpp" ] && \
       [ ! -f "hosts/$lang/generated/host/vtable_factories.cs" ] && \
       [ ! -f "hosts/$lang/generated/host/vtable_factories.py" ] && \
       [ ! -f "hosts/$lang/generated/host/vtable_factories.lua" ] && \
       [ ! -f "hosts/$lang/generated/host/vtable_factories.ts" ]; then
        echo "  ✗ hosts/$lang/generated/host/vtable_factories.* NOT FOUND"
        exit 1
    else
        echo "  ✓ hosts/$lang/generated/host/vtable_factories.* exists"
    fi
done
echo ""

# Build all
echo "Building all examples..."
./build_all.sh
echo ""

# Test Rust host with PLUGIN LOG verification
echo "Testing Rust host with host contract integration..."
if [ -f "hosts/rust/target/release/pipeline_host" ]; then
    output=$(./hosts/rust/target/release/pipeline_host 2>&1 || true)
    if echo "$output" | grep -q "\[PLUGIN LOG\]"; then
        echo "  ✓ PLUGIN LOG messages found"
        echo "$output" | grep "\[PLUGIN LOG\]" | head -5
    else
        echo "  ✗ PLUGIN LOG messages NOT FOUND"
        echo "  Output was:"
        echo "$output" | head -20
        exit 1
    fi
else
    echo "  ✗ Rust host binary not found"
    exit 1
fi
echo ""

# Verify [PLUGIN LOG] appears multiple times
count=$(echo "$output" | grep -c "\[PLUGIN LOG\]" || echo "0")
if [ "$count" -ge 4 ]; then
    echo "  ✓ Found $count PLUGIN LOG messages (expected at least 4)"
else
    echo "  ✗ Only found $count PLUGIN LOG messages (expected at least 4)"
    exit 1
fi
echo ""

# Verify different log levels appear
if echo "$output" | grep -q "\[DEBUG\]"; then
    echo "  ✓ DEBUG level found"
fi
if echo "$output" | grep -q "\[INFO\]"; then
    echo "  ✓ INFO level found"
fi
if echo "$output" | grep -q "\[WARN\]"; then
    echo "  ✓ WARN level found"
fi
if echo "$output" | grep -q "\[ERROR\]"; then
    echo "  ✓ ERROR level found"
fi
echo ""

echo "=== All Verifications Passed ==="
echo ""
echo "Next step: Delete host_contracts/ directory"
echo "  rm -rf host_contracts/"
```

**Make executable:**
```bash
chmod +x examples/verify_hosts.sh
```

---

### Phase 10: Cleanup (Task 15)
**Task 15: Delete host_contracts directory**
- Remove `examples/host_contracts/`
- Remove from .gitignore if present
- Commit: `chore: delete migrated host_contracts example`

---

### Phase 11: Final Verification (Tasks F1-F4)
**Task F1: Build verification**
- Run `./build_all.sh`
- Confirm no errors
- Confirm all 6 languages build

**Task F2: Runtime verification**
- Run `./verify_hosts.sh`
- Confirm `[PLUGIN LOG]` messages appear
- Confirm all 4 log levels appear

**Task F3: Cross-language verification**
- Test Rust host + C++ guest
- Test C++ host + Rust guest
- Test Python host + any guest
- Confirm all combinations produce PLUGIN LOG

**Task F4: Final review**
- Delete host_contracts/
- Confirm no references in codebase
- Confirm integration tests pass
- Confirm no references remain

---

## Detailed Tasks

### Task 0: Regenerate Code

**What to do:**
```bash
cd examples

# Regenerate all hosts
for lang in rust cpp csharp python lua js; do
    polyplugc generate --api api.toml --lang $lang --out hosts/$lang/generated
done

# Regenerate all guests
for lang in rust cpp csharp python lua js; do
    polyplugc generate --bundle guests/$lang/*/bundle.toml --lang $lang --out guests/$lang/*/generated
done
```

**Acceptance:**
- [ ] `hosts/*/generated/host/vtable_factories.*` exists
- [ ] `guests/*/reporter/generated/guest/host_contract_callers.*` exists

---

### Task 1: Update Rust Host

**File:** `examples/hosts/rust/src/main.rs`

**Current code:**
```rust
// Just loads plugins, no host contract
let runtime = Runtime::new();
runtime.load_plugins("./plugins");
```

**New code:**
```rust
use generated::host::host_contracts::HostLogger;
use generated::host::vtable_factories::create_host_logger_vtable;

struct ConsoleLogger;

impl HostLogger for ConsoleLogger {
    fn log(&self, message: &str) {
        println!("[PLUGIN LOG] {}", message);
    }
}

fn main() {
    let runtime = Runtime::new();
    
    // Register host contract BEFORE loading plugins
    let logger = ConsoleLogger;
    let vtable = create_host_logger_vtable(Box::new(logger));
    runtime.register_host_contract(0xF53EB5F2845853BB, vtable);
    
    // Now load plugins
    runtime.load_plugins("./plugins");
    
    // Run pipeline
    runtime.run();
}
```

**Acceptance:**
- [ ] Host implements HostLogger trait
- [ ] Host registers contract before loading plugins
- [ ] Output shows `[PLUGIN LOG]` prefix

---

### Task 2: Update Rust Guest Reporter

**File:** `examples/guests/rust/reporter/src/lib.rs`

**Current code:**
```rust
fn report(&self, input: &str) -> String {
    format!("Report: {}", input)
}
```

**New code:**
```rust
use generated::host_contract_callers::HostLoggerCaller;

fn report(&self, input: &str) -> String {
    // Get host logger
    let logger = unsafe {
        HostLoggerCaller::from_host(polyplug_guest::get_host_vtable(), 1)
    };
    
    // Log processing steps
    if let Some(logger) = logger {
        if logger.is_valid() {
            logger.log(format!("Starting report for: {}", input));
            logger.log("Step 1: Parsing input".to_string());
            logger.log("Step 2: Generating report".to_string());
            logger.log("Step 3: Formatting output".to_string());
        }
    }
    
    format!("Report: {}", input)
}
```

**Acceptance:**
- [ ] Reporter calls HostLoggerCaller::from_host()
- [ ] Reporter logs processing steps
- [ ] Output shows `[PLUGIN LOG]` messages from guest

---

### Task 3: Update C++ Host

**File:** `examples/hosts/cpp/main.cpp`

**Pattern:** Same as Rust - implement HostLogger, register before loading plugins

**Acceptance:**
- [ ] C++ host implements host.logger
- [ ] C++ host registers contract

---

### Task 4: Update C++ Guest Reporter

**File:** `examples/guests/cpp/reporter/reporter.cpp`

**Pattern:** Use generated HostLoggerCaller to log messages

**Acceptance:**
- [ ] C++ reporter calls host logger

---

### Tasks 5-12: Repeat for C#, Python, Lua, JavaScript

Same pattern for each language:
1. Host: Implement logger interface/class
2. Host: Register with runtime
3. Guest: Use generated caller
4. Guest: Log processing steps

---

### Task 13: Delete host_contracts

**Command:**
```bash
rm -rf examples/host_contracts/
```

**Acceptance:**
- [ ] Directory deleted
- [ ] Git shows deletion

---

### Task 14: Update Build Scripts

**File:** `examples/build_all.sh`

**Remove:**
- Any references to `host_contracts/`
- Special handling for host_contracts builds

**Keep:**
- Normal host builds (they now include host.logger)
- Normal guest builds

**File:** `examples/verify_hosts.sh`

**Add:**
```bash
# Verify PLUGIN LOG output
echo "Verifying host contract integration..."
output=$(./hosts/rust/target/release/pipeline_host 2>&1)
if echo "$output" | grep -q "\[PLUGIN LOG\]"; then
    echo "✓ PLUGIN LOG found"
else
    echo "✗ PLUGIN LOG not found"
    exit 1
fi
```

**Acceptance:**
- [ ] build_all.sh runs without errors
- [ ] verify_hosts.sh checks for PLUGIN LOG

---

## Executor Action Items

**DO THESE STEPS IN ORDER:**

### Step 1: Edit build_all.sh (MANUAL EDIT REQUIRED)
```bash
# Edit examples/build_all.sh
# Add host generation code AFTER guest generation (around line 50)
# Use the code from Task 0.1 in this plan
```

### Step 2: Generate Host Code (MUST DO FIRST)
```bash
cd examples
for lang in rust cpp csharp python lua js; do
    polyplugc generate --api api.toml --lang $lang --out hosts/$lang/generated
done
```

### Step 3: Verify Generated Files Exist
```bash
ls hosts/rust/generated/host/vtable_factories.rs
ls hosts/cpp/generated/host/vtable_factories.hpp
# ... check all 6 languages
```

### Step 4: Fix mod.rs Files
Add to `hosts/*/generated/host/mod.rs`:
```rust
pub mod host_contracts;
pub mod vtable_factories;
```

### Step 5: Implement Host Contract Integration
Follow Tasks 1-12 in this plan.

### Step 6: Build and Test
```bash
./examples/build_all.sh
./examples/verify_hosts.sh
```

### Step 7: Delete host_contracts/
```bash
rm -rf examples/host_contracts/
```

---

## Success Criteria

### Verification Commands:
```bash
# 1. Build everything
./examples/build_all.sh

# 2. Verify output
./examples/verify_hosts.sh

# 3. Check PLUGIN LOG appears
./examples/hosts/rust/target/release/pipeline_host 2>&1 | grep "\[PLUGIN LOG\]"

# 4. Check host_contracts deleted
ls examples/host_contracts  # Should fail

# 5. Check all languages
for lang in rust cpp csharp python lua js; do
    echo "Checking $lang..."
    grep -q "host.logger" examples/hosts/$lang/src/* || echo "FAIL: $lang host"
    grep -q "HostLoggerCaller" examples/guests/$lang/reporter/src/* || echo "FAIL: $lang guest"
done
```

### Expected Output:
```
[PLUGIN LOG] Starting report for: TRANSFORMED:NAME|value|42
[PLUGIN LOG] Step 1: Parsing input
[PLUGIN LOG] Step 2: Generating report
[PLUGIN LOG] Step 3: Formatting output
Report: NAME has value 'value' with count 42
```

---

## Estimated Effort

- Task 0 (Regenerate): 0.5 hours
- Tasks 1-2 (Rust): 2 hours
- Tasks 3-4 (C++): 2 hours
- Tasks 5-6 (C#): 2 hours
- Tasks 7-8 (Python): 2 hours
- Tasks 9-10 (Lua): 2 hours
- Tasks 11-12 (JavaScript): 2 hours
- Task 13 (Delete): 0.5 hours
- Task 14 (Scripts): 1 hour
- Verification: 2 hours

**Total: ~16 hours**

---

## Critical Path

Task 0 → Tasks 1-2 → Tasks 3-4 → Tasks 5-6 → Tasks 7-8 → Tasks 9-10 → Tasks 11-12 → Task 13 → Task 14 → F1-F4

**Can Parallelize:** Tasks 1-12 by language (6 parallel tracks)

---

## Commit Strategy

**Phase 1:** `feat(hosts/rust): integrate host.logger contract`
**Phase 2:** `feat(guests/rust): use host.logger in reporter`
**Phase 3-4:** Same pattern for other languages
**Phase 5:** `chore: delete examples/host_contracts directory`
**Phase 6:** `feat(build): update scripts for host contract integration`

---

## Notes

1. **Order matters**: Host must register contract BEFORE loading plugins
2. **Contract ID**: Use 0xF53EB5F2845853BB (from api.toml)
3. **Thread safety**: HostVTable is static OnceLock - safe for guests to call
4. **Null check**: Guests should check if logger.is_valid() before calling
5. **Fallback**: If host doesn't register logger, guests should handle None gracefully
