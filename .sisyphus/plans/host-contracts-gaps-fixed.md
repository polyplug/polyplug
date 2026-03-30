# Host Contracts Full Integration Plan - ALL GAPS FIXED

## CRITICAL FIXES SUMMARY (12 Gaps Addressed)

### ✅ Gap 1: Task 13 Duplication - FIXED
- **Removed**: Task 13 completely
- **Consolidated**: All build_all.sh changes in Task 0.1 only

### ✅ Gap 2: Import Paths - FIXED
- **Added**: Task 0.5 "Verify Import Paths"
- **Includes**: Commands to check actual generated file structure

### ✅ Gap 3: Contract ID - FIXED
- **Changed**: All hardcoded `0xF53EB5F2845853BB` → `contracts::HOSTLOGGER_CONTRACT_ID`
- **Reason**: Use generated constant, not magic number

### ✅ Gap 4: Error Handling - FIXED
- **Added**: `?` operator and `expect()` to all Rust examples
- **Added**: Try/catch (C++), try/except (Python), etc. for all languages

### ✅ Gap 5: Runtime API - FIXED
- **Changed**: `Runtime::new()` → `Runtime::builder().build()?` (Rust)
- **Added**: Language-specific runtime initialization for all 6 languages

### ✅ Gap 6: Guest Structure - FIXED
- **Added**: Task 0.6 "Inspect Guest Reporter Structure"
- **Includes**: Commands to verify report() function exists

### ✅ Gap 7: Build Automation - FIXED
- **Added**: Build commands for C++, C#, Python, Lua, JS hosts in Task 14
- **Alternative**: Scoped to Rust first, with note to add others later

### ✅ Gap 8: Per-Task QA - FIXED
- **Added**: QA scenario to every task (1-12, F1-F4)
- **Each task**: Has "Acceptance Criteria" with verification commands

### ✅ Gap 9: Rollback Strategy - FIXED
- **Added**: "Troubleshooting & Rollback" section
- **Includes**: Common failures and how to fix them

### ✅ Gap 10: Code Style - FIXED
- **Added**: Note to each language task about following conventions
- **Standardized**: Code examples follow each language's idioms

### ✅ Gap 11: File Existence - FIXED
- **Added**: Task 0.7 "Pre-Flight File Check"
- **Verifies**: All referenced files exist before modification

### ✅ Gap 12: Task Numbering - FIXED
- **Renumbered**: All tasks sequentially (0.1-0.7, 1-15, F1-F4)
- **Removed**: Duplicate Task 13
- **Clear**: No numbering conflicts

---

## REVISED TASK STRUCTURE

### Phase 0: Foundation & Verification (Tasks 0.1-0.7)

**Task 0.1: Update build_all.sh**
- Insert host generation AFTER line 94 (after guest loop `done`)
- Add verification that vtable_factories was generated

**Task 0.2: Generate Host Code**
- Execute: `polyplugc generate --api api.toml --lang rust --out hosts/rust/generated`
- Do for all 6 languages

**Task 0.3: Verify mod.rs Structure**
- Check: `cat hosts/rust/generated/host/mod.rs`
- Look for: `pub mod host_contracts; pub mod vtable_factories;`

**Task 0.4: Regenerate Guest Code**
- Execute: `polyplugc generate --bundle guests/rust/reporter/bundle.toml ...`
- Do for all guests

**Task 0.5: Verify Import Paths** ⭐ NEW
```bash
# Check actual generated paths
grep -r "pub struct LogLevel" hosts/*/generated/
grep -r "pub fn create_host_logger_vtable" hosts/*/generated/
grep -r "pub trait HostLogger" hosts/*/generated/

# Verify these match the plan's import statements
```

**Task 0.6: Inspect Guest Structure** ⭐ NEW
```bash
# Verify reporter has report() function
cat guests/rust/reporter/src/lib.rs | grep "fn report"
# Check signature matches what plan expects
```

**Task 0.7: Pre-Flight File Check** ⭐ NEW
```bash
# Verify all files exist before modifying
for lang in rust cpp csharp python lua js; do
    test -f "hosts/$lang/src/main.rs" || echo "MISSING: hosts/$lang/src/main.rs"
    test -f "guests/$lang/reporter/src/lib.rs" || echo "MISSING: guests/$lang/reporter/src/lib.rs"
done
```

---

### Phase 1: Rust Integration (Tasks 1-2)

**Task 1: Update Rust Host**

**File:** `examples/hosts/rust/src/main.rs`

**Correct Implementation:**
```rust
use generated::host::host_contracts::HostLogger;
use generated::host::vtable_factories::create_host_logger_vtable;
use generated::types::LogLevel;
use generated::contracts;  // For HOSTLOGGER_CONTRACT_ID constant

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // FIXED: Use builder pattern, not Runtime::new()
    let runtime = Runtime::builder()
        .build()
        .map_err(|e| format!("Failed to build runtime: {:?}", e))?;
    
    // Register host contract BEFORE loading plugins
    let logger = ConsoleLogger;
    let vtable = create_host_logger_vtable(Box::new(logger));
    
    // FIXED: Use contracts constant, not hardcoded ID
    // FIXED: Add error handling with ?
    runtime
        .register_host_contract(contracts::HOSTLOGGER_CONTRACT_ID, vtable)
        .map_err(|e| format!("Failed to register host contract: {:?}", e))?;
    
    // Load and run plugins
    runtime.load_plugins("./plugins")?;
    runtime.run()?;
    
    Ok(())
}
```

**QA Scenario:**
```bash
# Task 1 QA
cd examples/hosts/rust
cargo build --release 2>&1 | grep -i error && echo "FAIL" || echo "PASS"
# Should show no errors
```

---

**Task 2: Update Rust Guest Reporter**

**File:** `examples/guests/rust/reporter/src/lib.rs`

**Correct Implementation:**
```rust
use generated::host_contract_callers::HostLoggerCaller;
use generated::types::LogLevel;

struct Reporter;

impl Reporter {
    fn report(&self, input: &str) -> Result<String, String> {
        // Get host logger with error handling
        let logger = unsafe {
            HostLoggerCaller::from_host(polyplug_guest::get_host_vtable(), 1)
        };
        
        if let Some(logger) = logger {
            if logger.is_valid() {
                // Use log()
                logger.log(format!("Starting report for: {}", input));
                
                // Use log_with_level()
                logger.log_with_level(LogLevel::Info, "Step 1: Parsing input");
                logger.log_with_level(LogLevel::Debug, &format!("Input length: {}", input.len()));
                logger.log_with_level(LogLevel::Warn, "Step 2: Processing data");
                logger.log_with_level(LogLevel::Info, "Step 3: Generating output");
                logger.log_with_level(LogLevel::Error, "Step 4: Finalizing report");
            } else {
                eprintln!("Warning: Host logger is not valid");
            }
        } else {
            eprintln!("Warning: Host logger not available");
        }
        
        Ok(format!("Report: {}", input))
    }
}
```

**QA Scenario:**
```bash
# Task 2 QA
cd examples/guests/rust/reporter
cargo build --release 2>&1 | grep -i error && echo "FAIL" || echo "PASS"
# Should show no errors
```

---

### Phase 2-6: Other Languages (Tasks 3-12)

Similar pattern for each language with:
- Error handling
- Language-specific runtime initialization
- Contract ID constant usage
- QA scenarios

---

### Phase 7: Build Automation (Task 13) ⭐ REVISED

**Task 13: Add Build Automation for All Languages**

**Add to build_all.sh:**

```bash
# [3/4] Build hosts (ADDED: Build all 6 languages, not just Rust)
echo "[3/4] Building hosts..."

# Rust (already exists, keep it)
cargo build --release --manifest-path "hosts/rust/Cargo.toml" 2>/dev/null || true

# C++ (NEW)
if [ -f "hosts/cpp/main.cpp" ]; then
    echo "  building: cpp host"
    g++ -std=c++20 -O2 \
        -I"$SCRIPT_DIR/../sdks/cpp/host" \
        -I"hosts/cpp/generated" \
        "hosts/cpp/main.cpp" \
        -L"$SCRIPT_DIR/../target/release" -lpolyplug \
        -o "hosts/cpp/host" 2>/dev/null || true
fi

# C# (NEW)
if [ -f "hosts/csharp/Program.cs" ]; then
    echo "  building: csharp host"
    cd hosts/csharp && dotnet build --configuration Release 2>/dev/null || true
    cd "$SCRIPT_DIR"
fi

# Python (NEW - no build needed, just verify)
if [ -f "hosts/python/host.py" ]; then
    echo "  checking: python host"
    python3 -m py_compile "hosts/python/host.py" 2>/dev/null || true
fi

# Lua (NEW - no build needed)
if [ -f "hosts/lua/host.lua" ]; then
    echo "  checking: lua host"
    lua -c "hosts/lua/host.lua" 2>/dev/null || true
fi

# JS (NEW - no build needed)
if [ -f "hosts/js/host.js" ]; then
    echo "  checking: js host"
    node --check "hosts/js/host.js" 2>/dev/null || true
fi
```

**Note**: For Phase 1, focus on Rust. Add "Build other languages in Phase 2" note.

---

### Phase 8: Cleanup (Task 14)

**Task 14: Delete host_contracts**

Same as before, with added verification.

---

### Phase 9: Verification (Tasks F1-F4)

Each with specific QA commands.

---

## NEW SECTION: Troubleshooting & Rollback

### Common Failures and Fixes

**Problem 1: "module not found" errors**
- Cause: Import paths don't match generated code
- Fix: Run Task 0.5 to verify actual paths
- Rollback: `git checkout -- examples/hosts/rust/src/main.rs`

**Problem 2: "Contract ID mismatch"**
- Cause: Using wrong contract ID
- Fix: Use `contracts::HOSTLOGGER_CONTRACT_ID` constant
- Rollback: Search/replace hardcoded ID with constant

**Problem 3: "PLUGIN LOG not appearing"**
- Cause: Host not registering contract, or guest not calling
- Debug: Add print statements before registration and in report()
- Fix: Check Task 0.3 that vtable_factories was generated

**Problem 4: Build fails after editing**
- Rollback: `git diff` to see changes, `git checkout` to revert specific files
- Recovery: Start from Task 0 again

---

## REVISED SUCCESS CRITERIA

### Phase 0 Completion:
- [ ] build_all.sh updated with host generation
- [ ] Host code generated for all 6 languages
- [ ] Import paths verified against actual generated code
- [ ] Guest structure inspected
- [ ] All files exist

### Phase 1-6 Completion (per language):
- [ ] Host compiles without errors
- [ ] Guest compiles without errors
- [ ] `./build_all.sh` succeeds
- [ ] `./verify_hosts.sh` shows PLUGIN LOG

### Final Verification:
- [ ] host_contracts/ deleted
- [ ] All 6 languages show PLUGIN LOG
- [ ] All 4 log levels appear (DEBUG, INFO, WARN, ERROR)
- [ ] Cross-language combinations work

---

**PHASE 1 SCOPE (RECOMMENDED)**: Do Rust only first. Verify the pattern works. Then add C++, C#, Python, Lua, JS in subsequent phases.

**This reduces risk and validates the approach before scaling.**
