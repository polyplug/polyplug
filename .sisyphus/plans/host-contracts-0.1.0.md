# Host Contracts Implementation Plan for 0.1.0

## CRITICAL RULES - ENFORCED

### Rule 1: All Tasks Required - No Priority Levels
**THERE IS NO LOWER PRIORITY TASKS. THERE IS NO OPTIONAL TASKS.**
All tasks must be completed in the exact order specified. No task may be skipped, deferred, or marked as optional.

### Rule 2: Plan Execution As-Is
**PLAN MEANT TO BE EXECUTED AS-IS. DO NOT EDIT TASK DESCRIPTIONS.**
Only mark tasks as completed (`[x]`) when fully completed. Never modify task descriptions, requirements, or acceptance criteria during execution.

### Rule 3: Gap Discovery Protocol
**IF EXECUTOR FINDS A GAP, STOP AND ASK USER.**
Never take decisions independently. When encountering undefined behavior, missing specification, or unclear requirements, STOP execution and ask the user for clarification.

### Rule 4: Extensions Are Being Removed
**EXTENSIONS ARE COMPLETELY REMOVED AND REPLACED BY HOST CONTRACTS.**
- The old `Extension` system (`get_extension`) is **deleted**, not deprecated
- Host Contracts are the **replacement**, not coexistence
- No backwards compatibility for Extensions
- All extension-related code removed from codebase

---

## TL;DR

Implement **Host Contracts** - a reverse of Plugin Contracts where plugins call host-provided functions bidirectionally with full type safety.

**Key Changes:**
- Break ABI (pre-1.0, no backwards compatibility constraint)
- Rename existing `[[contract]]` to `[[plugin_contract]]` in `api.toml`
- Add new `[[host_contract]]` section to `api.toml`
- Add `HostRuntime` enum for VM host support
- Implement Host Runtime Bridge for VM-based hosts (Python/Lua/JS)
- Full code generation for all 6 languages (both host-side and guest-side)
- Update all examples with Host Contracts

**Estimated Effort:** 5-6 weeks
**Critical Path:** Wave 0 (Design) → Wave 1 (ABI) → Wave 5 (Bridge) → Wave 6 (Examples)

---

## Architecture Overview

### Symmetric Contract System

```
Bidirectional Communication:

┌─────────────────┐         Host Contracts         ┌─────────────────┐
│   Host          │◄────────────────────────────────│   Plugin        │
│ (Rust/Python/   │         (Plugin calls Host)      │ (Any Language)  │
│  Lua/JS)        │                                │                 │
└─────────────────┘                                └─────────────────┘
         │                                                  │
         │         Plugin Contracts                         │
         │─────────────────────────────────────────────────►│
         │         (Host calls Plugin)                      │
         ▼                                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        Rust Runtime Core                            │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────────────┐   │
│  │   Registry   │  │  Host Bridge │  │  Plugin Loaders         │   │
│  │              │  │  (VM support)│  │  (Native/VM)            │   │
│  └──────────────┘  └──────────────┘  └─────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

### Host Runtime Types

```rust
pub enum HostRuntime {
    Rust = 0,       // Native Rust host
    Python = 1,     // Python host via ctypes/cffi
    Lua = 2,        // Lua host via FFI
    JavaScript = 3, // JavaScript host (Deno/Node)
}
```

### Host Contract Flow

```
Native Plugin calling Host Contract:
1. Plugin calls HostLoggerContract::log() via generated code
2. Generated code calls host.get_host_contract(HOST_LOGGER_ID)
3. Runtime returns vtable pointer
4. If Host is Native: Direct function call
5. If Host is VM: Runtime Bridge translates to VM call

VM Plugin calling Host Contract:
1. Python plugin calls host_logger.log()
2. Python generated code calls through ctypes
3. Runtime receives C ABI call
4. If Host is Native: Direct function call  
5. If Host is VM: Runtime Bridge translates to VM call
```

---

## Execution Waves

### Wave 0: Design (BLOCKS ALL OTHER WAVES)
**Status**: Must complete before Wave 1

These tasks produce design documents that guide all implementation. STOP and ask user if any design decision is unclear.

- [x] **Task D1**: Define HostContractVTable Base Struct
  
  **Specification**:
  Define the C ABI structure for host contract vtables with support for both native and VM dispatch.
  
  **Requirements**:
  ```rust
  #[repr(C)]
  pub struct HostContractVTableHeader {
      pub vtable_version: u32,      // Structure version for ABI evolution
      pub contract_id: u64,         // fnv1a_64("host.logger@1")
      pub contract_major: u32,      // Major version
      pub contract_minor: u32,      // Minor version
      pub function_count: u32,      // Number of functions
      pub dispatch_type: DispatchType,  // Native or VirtualMachine
  }
  
  #[repr(C)]
  pub struct NativeHostContractDispatch {
      pub functions: *const *const (),  // Array of function pointers
  }
  
  #[repr(C)]
  pub struct VmHostContractDispatch {
      pub call: unsafe extern "C" fn(
          bridge_data: *mut c_void,
          fn_id: u32,
          args: *const (),
          out: *mut (),
      ) -> AbiError,
      pub bridge_data: *mut c_void,
  }
  
  #[repr(C)]
  pub union HostContractDispatch {
      pub native: NativeHostContractDispatch,
      pub vm: VmHostContractDispatch,
  }
  
  #[repr(C)]
  pub struct HostContractVTable {
      pub header: HostContractVTableHeader,
      pub dispatch: HostContractDispatch,
  }
  ```
  
  **Design Decisions Required**:
  - Function pointer signatures for host contract functions
  - Memory ownership rules for parameters/returns
  - Error propagation mechanism
  
  **Deliverable**: `.sisyphus/designs/host-contract-vtable.md`
  
  **Acceptance Criteria**:
  - [ ] All struct layouts documented with sizes and alignments
  - [ ] Function signatures defined for example host contracts
  - [ ] Memory ownership rules specified
  - [ ] Version negotiation protocol defined
  
  **QA Verification**:
  ```bash
  # Verify design document exists and is complete
  test -f .sisyphus/designs/host-contract-vtable.md
  grep -q "struct HostContractVTable" .sisyphus/designs/host-contract-vtable.md
  grep -q "Memory Ownership" .sisyphus/designs/host-contract-vtable.md
  ```

- [x] **Task D2**: Define Host Runtime Bridge Architecture
  
  **Specification**:
  Design the bridge system that allows VM-based hosts (Python/Lua/JS) to implement host contracts.
  
  **Key Insight**: When host is VM-based, the Rust runtime must act as a bridge between plugin (any language) and host (VM).
  
  **Architecture**:
  ```
  Plugin (any language) → Runtime Bridge → Host (VM)
  
  For VM Host:
  1. Plugin calls host contract via C ABI
  2. Runtime receives call
  3. Runtime Bridge translates to VM call
  4. Bridge calls into VM (Python ctypes, Lua FFI, JS Deno FFI)
  5. Host implementation executes
  ```
  
  **Bridge Trait Design**:
  ```rust
  pub trait HostRuntimeBridge: Send + Sync {
      fn runtime_type(&self) -> HostRuntime;
      
      /// Call a host contract function
      fn call_host_contract(
          &self,
          contract_id: u64,
          fn_id: u32,
          args: *const (),
          out: *mut (),
      ) -> AbiError;
  }
  
  // For Python host
  pub struct PythonHostBridge {
      // Stores Python callable objects for each host contract
      contracts: HashMap<u64, PyObject>,
  }
  
  impl HostRuntimeBridge for PythonHostBridge {
      fn call_host_contract(...) -> AbiError {
          // Acquire GIL
          // Call Python implementation
          // Handle Python exceptions
          // Return AbiError
      }
  }
  ```
  
  **Deliverable**: `.sisyphus/designs/host-runtime-bridge.md`
  
  **Acceptance Criteria**:
  - [ ] Bridge trait defined
  - [ ] Python bridge design documented
  - [ ] Lua bridge design documented  
  - [ ] JavaScript bridge design documented
  - [ ] GIL/thread safety rules specified
  - [ ] Exception handling strategy defined

- [x] **Task D3**: Design Code Generation for Host Contracts
  
  **Specification**:
  Design what code is generated for both host-side and guest-side.
  
  **Generated for Host** (what host implements):
  - Rust: Trait definition
  - Python: Abstract base class
  - Lua: Metatable with methods
  - JS: Interface/abstract class
  - C++: Pure virtual class
  - C#: Interface
  
  **Generated for Guest** (what plugin uses to call host):
  - All languages: Contract caller with `from_host()` factory
  
  **Example - Rust Host**:
  ```rust
  // Generated in host/contracts.rs
  pub trait HostLogger: Send + Sync {
      fn log(&self, level: u32, message: &str);
      fn logf(&self, level: u32, format: &str, args: &[Value]);
  }
  
  // Registration helper
  impl Runtime {
      pub fn register_host_logger(&self, impl_: Box<dyn HostLogger>) {
          // Creates vtable, registers with runtime
      }
  }
  ```
  
  **Example - Python Host**:
  ```python
  # Generated in host/contracts.py
  from abc import ABC, abstractmethod
  
  class HostLogger(ABC):
      @abstractmethod
      def log(self, level: int, message: str) -> None:
          pass
      
      @abstractmethod
      def logf(self, level: int, format: str, args: list) -> None:
          pass
  
  class Runtime:
      def register_host_logger(self, impl_: HostLogger) -> None:
          # Creates bridge, registers with runtime
          pass
  ```
  
  **Example - Rust Guest**:
  ```rust
  // Generated in guest/host_contracts.rs
  pub struct HostLoggerContract {
      vtable: &'static HostLoggerVTable,
  }
  
  impl HostLoggerContract {
      pub fn from_host(host: &HostVTable) -> Option<Self> {
          let ptr = host.get_host_contract(HOST_LOGGER_ID)?;
          Some(Self { vtable: unsafe { &*(ptr as *const _) } })
      }
      
      pub fn log(&self, level: u32, message: &str) -> Result<(), ContractError> {
          // Calls through vtable
      }
  }
  ```
  
  **Deliverable**: `.sisyphus/designs/host-contract-codegen.md`
  
  **Acceptance Criteria**:
  - [ ] Host-side generated code shown for all 6 languages
  - [ ] Guest-side generated code shown for all 6 languages
  - [ ] Registration API designed for all 6 languages
  - [ ] VM bridge integration specified

- [x] **Task D4**: Design Host Contract ID Scheme
  
  **Specification**:
  Design collision-free contract ID generation for host contracts.
  
  **Problem**: Plugin contracts and host contracts must never collide.
  
  **Solution**: Namespace prefix in hash input
  ```rust
  // Host contract ID
  host_contract_id = fnv1a_64(b"host_contract:logger@1")
  
  // Plugin contract ID  
  plugin_contract_id = fnv1a_64(b"plugin_contract:logger@1")
  ```
  
  **Deliverable**: Documented in design files
  
  **Acceptance Criteria**:
  - [ ] Collision test case designed
  - [ ] No overlap possible between host and plugin contract IDs

**Wave 0 Completion Gate**:
All 4 design documents must be complete and reviewed before proceeding to Wave 1. If any design decision is unclear, STOP and ask user.

---

### Wave 1: ABI Layer (FOUNDATION)
**Status**: Blocked by Wave 0
**Blocks**: Wave 2, Wave 3, Wave 5

- [x] **Task 1**: Rename `[[contract]]` to `[[plugin_contract]]` in all api.toml files
  
  **Specification**:
  Globally rename all occurrences of `[[contract]]` to `[[plugin_contract]]` in:
  - All api.toml files
  - Parser code (RawApiSchema)
  - IR (Intermediate Representation)
  - All code generators
  - Documentation
  
  **Files to Modify**:
  - `examples/api.toml`
  - `crates/polyplug_codegen/src/parser.rs`
  - `crates/polyplug_codegen/src/ir.rs`
  - All generator files in `crates/polyplug_codegen/src/generators/`
  - Documentation files
  
  **Backwards Compatibility**:
  - Phase 1 (this release): Accept both `[[contract]]` and `[[plugin_contract]]`, emit deprecation warning for `[[contract]]`
  - Parser should normalize both to internal `PluginContract` type
  
  **Acceptance Criteria**:
  - [ ] `grep -r "^\[\[contract\]\]" --include="*.toml" .` returns 0 results (except in test fixtures)
  - [ ] `grep -r "^\[\[plugin_contract\]\]" --include="*.toml" .` returns expected results
  - [ ] Parser accepts both syntaxes
  - [ ] Deprecation warning emitted for old syntax
  - [ ] All existing tests pass
  
  **QA Verification**:
  ```bash
  # Test new syntax
  cat > /tmp/test_new.toml << 'EOF'
  [[plugin_contract]]
  name = "test.decoder"
  version = "1.0.0"
  EOF
  cargo run -p polyplugc -- validate --api /tmp/test_new.toml
  # Expected: OK
  
  # Test old syntax with warning
  cat > /tmp/test_old.toml << 'EOF'
  [[contract]]
  name = "test.decoder"
  version = "1.0.0"
  EOF
  cargo run -p polyplugc -- validate --api /tmp/test_old.toml 2>&1
  # Expected: OK with deprecation warning
  ```
  
  **Commit**:
  - Message: `refactor(abi): rename [[contract]] to [[plugin_contract]]`
  - Files: All modified files
  - Pre-commit: `cargo test -p polyplug_codegen --lib`

- [x] **Task 2**: Add `HostRuntime` enum and ABI types
  
  **Specification**:
  Add the HostRuntime enum and HostContractVTable types to polyplug_abi.
  
  **Implementation**:
  In `crates/polyplug_abi/src/lib.rs`:
  ```rust
  /// Host runtime type identifier
  #[repr(u8)]
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum HostRuntime {
      Rust = 0,
      Python = 1,
      Lua = 2,
      JavaScript = 3,
  }
  
  /// Host contract vtable header
  #[repr(C)]
  pub struct HostContractVTableHeader {
      pub vtable_version: u32,
      pub contract_id: u64,
      pub contract_major: u32,
      pub contract_minor: u32,
      pub function_count: u32,
      pub dispatch_type: DispatchType,
  }
  
  /// Native dispatch for host contracts
  #[repr(C)]
  pub struct NativeHostContractDispatch {
      pub functions: *const *const (),
  }
  
  /// VM dispatch for host contracts
  #[repr(C)]
  pub struct VmHostContractDispatch {
      pub call: unsafe extern "C" fn(
          bridge_data: *mut c_void,
          fn_id: u32,
          args: *const (),
          out: *mut (),
      ) -> AbiError,
      pub bridge_data: *mut c_void,
  }
  
  #[repr(C)]
  pub union HostContractDispatch {
      pub native: NativeHostContractDispatch,
      pub vm: VmHostContractDispatch,
  }
  
  #[repr(C)]
  pub struct HostContractVTable {
      pub header: HostContractVTableHeader,
      pub dispatch: HostContractDispatch,
  }
  
  /// Error codes for host contracts
  pub const ABI_HOST_CONTRACT_NOT_FOUND: u32 = 100;
  pub const ABI_HOST_CONTRACT_VERSION_MISMATCH: u32 = 101;
  pub const ABI_HOST_CONTRACT_CALL_FAILED: u32 = 102;
  ```
  
  **Acceptance Criteria**:
  - [ ] All types compile
  - [ ] Sizes and alignments verified via tests
  - [ ] SAFETY impls for Send/Sync
  
  **QA Verification**:
  ```bash
  cargo test -p polyplug_abi host_contract_types
  # Expected: All layout tests pass
  ```
  
  **Commit**:
  - Message: `feat(abi): add HostRuntime enum and HostContractVTable types`
  - Files: `crates/polyplug_abi/src/lib.rs`
  - Pre-commit: `cargo test -p polyplug_abi`

- [x] **Task 3**: Add `get_host_contract` to HostVTable AND Remove Extensions
  
  **Specification**:
  Add the `get_host_contract` function pointer to HostVTable AND completely remove the `get_extension` field.
  
  **Implementation**:
  In `crates/polyplug_abi/src/lib.rs`:
  
  **REMOVE**:
  ```rust
  // DELETE THIS ENTIRELY:
  pub get_extension: unsafe extern "C" fn(rt_ctx: *mut c_void, extension_id: u32) -> *const (),
  ```
  
  **ADD**:
  ```rust
  #[repr(C)]
  pub struct HostVTable {
      // ... existing fields ...
      pub resolve_plugin: unsafe extern "C" fn(...),
      
      // NEW: Get host contract vtable (REPLACES get_extension)
      pub get_host_contract: unsafe extern "C" fn(
          rt_ctx: *mut c_void,
          contract_id: u64,
          min_version: u32,
      ) -> *const HostContractVTable,
  }
  ```
  
  **Files to Modify for Extension Removal**:
  - `crates/polyplug_abi/src/lib.rs` - Remove `get_extension` from HostVTable
  - `crates/polyplug/src/extensions/` - DELETE ENTIRE DIRECTORY
  - `crates/polyplug/src/runtime.rs` - Remove extension registration, remove `host_get_extension` callback
  - `crates/polyplug/src/lib.rs` - Remove extension module export
  - All code generators - Remove EXT_TRACE_ID generation
  - All SDKs - Remove extension-related code
  
  **Breaking Change**: This is a complete ABI break. Extensions are gone.
  
  **Acceptance Criteria**:
  - [ ] `get_extension` field removed from HostVTable
  - [ ] `crates/polyplug/src/extensions/` directory deleted
  - [ ] No `Extension` trait references remaining
  - [ ] No `EXT_TRACE_ID` generation in code generators
  - [ ] HostVTable updated with `get_host_contract`
  - [ ] Size/layout tests updated
  - [ ] All usages updated
  - [ ] `grep -r "get_extension" --include="*.rs" .` returns 0 results
  - [ ] `grep -r "Extension" --include="*.rs" crates/polyplug/src/` returns 0 results (except in comments explaining removal)
  
  **QA Verification**:
  ```bash
  # Verify extension removal
  grep -r "get_extension" --include="*.rs" crates/
  # Expected: No results (or only in CHANGELOG/historical docs)
  
  # Verify new field exists
  grep -q "get_host_contract" crates/polyplug_abi/src/lib.rs
  # Expected: Found
  
  cargo test -p polyplug_abi host_vtable_layout
  # Expected: New size verified
  ```
  
  **Commit**:
  - Message: `feat(abi)!: replace get_extension with get_host_contract, remove Extension system`
  - Files: `crates/polyplug_abi/src/lib.rs`, `crates/polyplug/src/extensions/` (deleted), `runtime.rs`, all generators
  - Pre-commit: `cargo test -p polyplug_abi --lib`

- [x] **Task 4**: Implement host contract ID calculation
  
  **Specification**:
  Implement collision-free host contract ID generation.
  
  **Implementation**:
  In `crates/polyplug_abi/src/lib.rs`:
  ```rust
  /// Calculate host contract ID from name and major version
  pub fn host_contract_id(name: &str, major: u32) -> u64 {
      let input = format!("host_contract:{}@{}", name, major);
      fnv1a_64(input.as_bytes())
  }
  
  /// Calculate plugin contract ID from name and major version
  pub fn plugin_contract_id(name: &str, major: u32) -> u64 {
      let input = format!("plugin_contract:{}@{}", name, major);
      fnv1a_64(input.as_bytes())
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Functions implemented
  - [ ] No collision between host and plugin IDs for same name
  - [ ] Tests verify collision avoidance
  
  **QA Verification**:
  ```bash
  cargo test -p polyplug_abi contract_id_collision
  # Expected: Test passes, no collisions
  ```
  
  **Commit**:
  - Message: `feat(abi): add host_contract_id and plugin_contract_id functions`
  - Files: `crates/polyplug_abi/src/lib.rs`
  - Pre-commit: `cargo test -p polyplug_abi`

---

### Wave 2: Parser and IR
**Status**: Blocked by Wave 1
**Blocks**: Wave 3

- [x] **Task 5**: Update parser to support `[[plugin_contract]]` and `[[host_contract]]`
  
  **Specification**:
  Update TOML parser to recognize both `[[plugin_contract]]` and `[[host_contract]]` sections.
  
  **Files**: `crates/polyplug_codegen/src/parser.rs`
  
  **Implementation**:
  ```rust
  // Raw API schema
  pub struct RawApiSchema {
      pub plugin_contracts: Vec<RawPluginContract>,  // Renamed from contracts
      pub host_contracts: Vec<RawHostContract>,      // NEW
      // ... other fields ...
  }
  
  pub struct RawHostContract {
      pub name: String,
      pub version: String,
      pub functions: Vec<RawFunction>,
  }
  ```
  
  **Validation Rules**:
  - Host contract names must start with "host."
  - No duplicate contract names across both types
  - Version format must be semantic
  
  **Acceptance Criteria**:
  - [ ] Parser accepts `[[plugin_contract]]`
  - [ ] Parser accepts `[[host_contract]]`
  - [ ] Backwards compatibility for `[[contract]]` with warning
  - [ ] Validation rules enforced
  
  **QA Verification**:
  ```bash
  cat > /tmp/test_host_contract.toml << 'EOF'
  [[host_contract]]
  name = "host.logger"
  version = "1.0.0"
  
  [[host_contract.functions]]
  name = "log"
  params = [{ name = "level", type = "u32" }, { name = "message", type = "StringView" }]
  returns = "void"
  EOF
  cargo run -p polyplugc -- validate --api /tmp/test_host_contract.toml
  # Expected: OK
  ```
  
  **Commit**:
  - Message: `feat(parser): add support for [[plugin_contract]] and [[host_contract]]`
  - Files: `parser.rs`
  - Pre-commit: `cargo test -p polyplug_codegen parser`

- [x] **Task 6**: Update Intermediate Representation (IR)
  
  **Specification**:
  Update IR to include HostContract alongside PluginContract.
  
  **Files**: `crates/polyplug_codegen/src/ir.rs`
  
  **Implementation**:
  ```rust
  pub struct ResolvedApiSchema {
      pub plugin_contracts: Vec<ResolvedPluginContract>,  // Renamed
      pub host_contracts: Vec<ResolvedHostContract>,      // NEW
      // ... other fields ...
  }
  
  pub struct ResolvedHostContract {
      pub name: String,
      pub id: u64,  // host_contract_id(name, major)
      pub version: Version,
      pub functions: Vec<ResolvedFunction>,
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] IR types updated
  - [ ] All IR consumers compile
  
  **Commit**:
  - Message: `refactor(ir): add HostContract to IR`
  - Files: `ir.rs`, all files that use IR
  - Pre-commit: `cargo build -p polyplug_codegen`

---

### Wave 3: Rust Code Generation
**Status**: Blocked by Wave 2
**Blocks**: Wave 4, Wave 6

- [x] **Task 7**: Generate Rust host-side contract traits
  
  **Specification**:
  Generate Rust traits that hosts implement for host contracts.
  
  **Files**: `crates/polyplug_codegen/src/generators/rust.rs`
  
  **Generated Code Example**:
  ```rust
  // Generated: host/contracts.rs
  pub trait HostLogger: Send + Sync {
      fn log(&self, level: u32, message: &str);
      fn logf(&self, level: u32, format: &str, args: &[serde_json::Value]);
  }
  
  // Registration helper
  impl Runtime {
      pub fn register_host_logger(&self, impl_: Box<dyn HostLogger>) -> Result<(), RegistrationError> {
          // Creates vtable, registers
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Trait generated for each host contract
  - [ ] Registration helper generated
  - [ ] Generated code compiles
  
  **QA Verification**:
  ```bash
  cat > /tmp/test_api.toml << 'EOF'
  [[host_contract]]
  name = "host.logger"
  version = "1.0.0"
  
  [[host_contract.functions]]
  name = "log"
  params = [{ name = "level", type = "u32" }, { name = "message", type = "StringView" }]
  returns = "void"
  EOF
  cargo run -p polyplugc -- generate --api /tmp/test_api.toml --lang rust --out /tmp/rust_out
  test -f /tmp/rust_out/host/contracts.rs
  grep -q "trait HostLogger" /tmp/rust_out/host/contracts.rs
  ```
  
  **Commit**:
  - Message: `feat(rust): generate host-side contract traits`
  - Files: `rust.rs`
  - Pre-commit: `cargo test -p polyplug_codegen rust_generator`

- [x] **Task 8**: Generate Rust guest-side host contract callers
  
  **Specification**:
  Generate Rust code that plugins use to call host contracts.
  
  **Generated Code Example**:
  ```rust
  // Generated: guest/host_contracts.rs
  pub struct HostLoggerContract {
      vtable: &'static HostLoggerVTable,
  }
  
  impl HostLoggerContract {
      pub fn from_host(host: &HostVTable) -> Option<Self> {
          let ptr = host.get_host_contract(HOST_LOGGER_CONTRACT_ID, 0)?;
          if ptr.is_null() {
              return None;
          }
          Some(Self { vtable: unsafe { &*ptr } })
      }
      
      pub fn log(&self, level: u32, message: &str) -> Result<(), ContractError> {
          // Marshal args, call through vtable
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller struct generated
  - [ ] `from_host()` factory method generated
  - [ ] Methods for each contract function generated
  
  **Commit**:
  - Message: `feat(rust): generate guest-side host contract callers`
  - Files: `rust.rs`
  - Pre-commit: `cargo test -p polyplug_codegen rust_generator`

---

### Wave 4: Runtime Host Contract Support
**Status**: Blocked by Wave 3
**Blocks**: Wave 5, Wave 6

- [x] **Task 9**: Implement host contract registration in Runtime
  
  **Specification**:
  Add host contract storage and registration to Runtime.
  
  **Files**: `crates/polyplug/src/runtime.rs`
  
  **Implementation**:
  ```rust
  pub struct Runtime {
      // ... existing fields ...
      host_contracts: RwLock<HashMap<u64, &'static HostContractVTable>>,
      host_runtime: HostRuntime,
      host_bridge: Option<Box<dyn HostRuntimeBridge>>,  // For VM hosts
  }
  
  impl RuntimeBuilder {
      pub fn host_runtime(mut self, runtime: HostRuntime) -> Self {
          self.host_runtime = runtime;
          self
      }
      
      pub fn host_bridge(mut self, bridge: Box<dyn HostRuntimeBridge>) -> Self {
          self.host_bridge = Some(bridge);
          self
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Runtime stores host contracts
  - [ ] Runtime stores host runtime type
  - [ ] Runtime stores host bridge (for VM hosts)
  - [ ] `get_host_contract` callback implemented
  
  **QA Verification**:
  ```bash
  cargo test -p polyplug runtime_host_contracts
  ```
  
  **Commit**:
  - Message: `feat(runtime): add host contract registration`
  - Files: `runtime.rs`
  - Pre-commit: `cargo test -p polyplug --lib`

- [x] **Task 10**: Implement `get_host_contract` callback
  
  **Specification**:
  Implement the HostVTable callback that plugins call to get host contracts.
  
  **Implementation**:
  ```rust
  pub(crate) unsafe extern "C" fn host_get_host_contract(
      rt_ctx: *mut c_void,
      contract_id: u64,
      min_version: u32,
  ) -> *const HostContractVTable {
      if rt_ctx.is_null() {
          return core::ptr::null();
      }
      
      let ctx: &HostContext = unsafe { &*(rt_ctx as *const HostContext) };
      let runtime: &Runtime = unsafe { &*ctx.runtime };
      
      match runtime.host_contracts.read() {
          Ok(contracts) => {
              match contracts.get(&contract_id) {
                  Some(vtable) => {
                      // Check version compatibility
                      if vtable.header.contract_minor >= min_version {
                          *vtable
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
  
  **Acceptance Criteria**:
  - [ ] Callback implemented
  - [ ] Version checking works
  - [ ] Null returned for missing contracts
  - [ ] Thread-safe
  
  **Commit**:
  - Message: `feat(runtime): implement get_host_contract callback`
  - Files: `runtime.rs`
  - Pre-commit: `cargo test -p polyplug`

---

### Wave 5: Host Runtime Bridge (VM Support)
**Status**: Blocked by Wave 4
**Blocks**: Wave 6, Wave 8

- [x] **Task 11**: Define HostRuntimeBridge trait
  
  **Specification**:
  Define the trait that bridges between runtime and VM-based hosts.
  
  **Files**: `crates/polyplug/src/host_bridge.rs` (new file)
  
  **Implementation**:
  ```rust
  pub trait HostRuntimeBridge: Send + Sync {
      fn runtime_type(&self) -> HostRuntime;
      
      /// Register a host contract implementation
      fn register_host_contract(
          &mut self,
          contract_id: u64,
          implementation: Box<dyn Any>,  // VM-specific implementation
      ) -> Result<(), BridgeError>;
      
      /// Call a host contract function
      fn call_host_contract(
          &self,
          contract_id: u64,
          fn_id: u32,
          args: *const (),
          out: *mut (),
      ) -> AbiError;
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Trait defined
  - [ ] Error types defined
  - [ ] Documentation complete
  
  **Commit**:
  - Message: `feat(bridge): define HostRuntimeBridge trait`
  - Files: `host_bridge.rs` (new)
  - Pre-commit: `cargo build -p polyplug`

- [x] **Task 12**: Implement Python Host Bridge
  
  **Specification**:
  Implement the bridge that allows Python hosts to implement host contracts.
  
  **Files**: `crates/polyplug_python/src/bridge.rs` (new file)
  
  **Implementation**:
  ```rust
  pub struct PythonHostBridge {
      contracts: HashMap<u64, PyObject>,  // Python callable objects
  }
  
  impl HostRuntimeBridge for PythonHostBridge {
      fn call_host_contract(
          &self,
          contract_id: u64,
          fn_id: u32,
          args: *const (),
          out: *mut (),
      ) -> AbiError {
          Python::with_gil(|py| {
              // Get Python implementation
              let impl_ = match self.contracts.get(&contract_id) {
                  Some(obj) => obj,
                  None => return AbiError { code: ABI_HOST_CONTRACT_NOT_FOUND, ... },
              };
              
              // Convert args to Python types
              // Call Python function
              // Handle Python exceptions
              // Convert result back
          })
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Bridge implemented
  - [ ] GIL handled correctly
  - [ ] Python exceptions converted to AbiError
  - [ ] Type conversion works for all primitive types
  
  **QA Verification**:
  ```bash
  cargo test -p polyplug_python host_bridge
  ```
  
  **Commit**:
  - Message: `feat(python): implement Python Host Bridge`
  - Files: `bridge.rs`, integration with `lib.rs`
  - Pre-commit: `cargo test -p polyplug_python`

- [x] **Task 13**: Implement Lua Host Bridge
  
  **Specification**:
  Implement bridge for Lua hosts.
  
  **Files**: `crates/polyplug_lua/src/bridge.rs`
  
  **Similar structure to Task 12**
  
  **Commit**:
  - Message: `feat(lua): implement Lua Host Bridge`

- [x] **Task 14**: Implement JavaScript Host Bridge
  
  **Specification**:
  Implement bridge for JavaScript hosts.
  
  **Files**: `crates/polyplug_js/src/bridge.rs`
  
  **Similar structure to Task 12**
  
  **Commit**:
  - Message: `feat(js): implement JavaScript Host Bridge`

---

### Wave 6: Other Language Code Generators
**Status**: Blocked by Wave 3
**Blocks**: Wave 8

- [x] **Task 15**: Generate C++ host-side contract traits
  
  **Specification**:
  Generate C++ abstract classes that hosts implement for host contracts.
  
  **Files**: `crates/polyplug_codegen/src/generators/cpp.rs`
  
  **Generated Code**:
  ```cpp
  // Generated: host/contracts.hpp
  class HostLogger {
  public:
      virtual void log(uint32_t level, StringView message) = 0;
      virtual void logf(uint32_t level, StringView format, Buffer args) = 0;
      virtual ~HostLogger() = default;
  };
  
  // Registration helper
  class Runtime {
  public:
      void register_host_logger(std::shared_ptr<HostLogger> impl_);
  };
  ```
  
  **Acceptance Criteria**:
  - [ ] Abstract class generated for each host contract
  - [ ] Virtual destructor included
  - [ ] Registration helper generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(cpp): generate host-side contract traits`

- [x] **Task 16**: Generate C++ guest-side host contract callers
  
  **Specification**:
  Generate C++ classes that plugins use to call host contracts.
  
  **Generated Code**:
  ```cpp
  // Generated: guest/host_contracts.hpp
  class HostLoggerContract {
  public:
      static std::optional<HostLoggerContract> from_host(const HostVTable* host);
      void log(uint32_t level, std::string_view message);
      void logf(uint32_t level, std::string_view format, std::vector<Value> args);
  private:
      const HostLoggerVTable* vtable_;
  };
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller class generated
  - [ ] `from_host()` factory method generated
  - [ ] Methods for each contract function generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(cpp): generate guest-side host contract callers`

- [x] **Task 17**: Generate C# host-side contract interfaces
  
  **Specification**:
  Generate C# interfaces that hosts implement for host contracts.
  
  **Files**: `crates/polyplug_codegen/src/generators/csharp.rs`
  
  **Generated Code**:
  ```csharp
  // Generated: host/Contracts.cs
  public interface IHostLogger {
      void Log(uint level, string message);
      void Logf(uint level, string format, object[] args);
  }
  
  // Registration helper
  public partial class Runtime {
      public void RegisterHostLogger(IHostLogger impl_);
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Interface generated for each host contract
  - [ ] Registration helper generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(csharp): generate host-side contract interfaces`

- [ ] **Task 18**: Generate C# guest-side host contract callers
  
  **Specification**:
  Generate C# classes that plugins use to call host contracts.
  
  **Generated Code**:
  ```csharp
  // Generated: guest/HostContracts.cs
  public class HostLoggerContract {
      private readonly HostLoggerVTable _vtable;
      
      public static HostLoggerContract FromHost(HostVTable host) { ... }
      public void Log(uint level, string message);
      public void Logf(uint level, string format, object[] args);
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller class generated
  - [ ] `FromHost()` factory method generated
  - [ ] Methods for each contract function generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(csharp): generate guest-side host contract callers`

- [ ] **Task 19**: Generate Python host-side contract ABCs
  
  **Specification**:
  Generate Python abstract base classes that hosts implement.
  
  **Files**: `crates/polyplug_codegen/src/generators/python.rs`
  
  **Generated Code**:
  ```python
  # Generated: host/contracts.py
  from abc import ABC, abstractmethod
  
  class HostLogger(ABC):
      @abstractmethod
      def log(self, level: int, message: str) -> None:
          pass
      
      @abstractmethod
      def logf(self, level: int, format: str, args: list) -> None:
          pass
  
  class Runtime:
      def register_host_logger(self, impl_: HostLogger) -> None:
          # Creates bridge, registers with runtime
          pass
  ```
  
  **Acceptance Criteria**:
  - [ ] Abstract base class generated for each host contract
  - [ ] Registration helper generated
  - [ ] Generated code runs
  
  **Commit**:
  - Message: `feat(python): generate host-side contract ABCs`

- [ ] **Task 20**: Generate Python guest-side host contract callers
  
  **Specification**:
  Generate Python classes that plugins use to call host contracts.
  
  **Generated Code**:
  ```python
  # Generated: guest/host_contracts.py
  class HostLoggerContract:
      def __init__(self, vtable: ctypes.c_void_p):
          self._vtable = vtable
      
      @classmethod
      def from_host(cls, host) -> Optional[HostLoggerContract]:
          # Call host.get_host_contract()
          pass
      
      def log(self, level: int, message: str) -> None:
          # Call through vtable
          pass
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller class generated
  - [ ] `from_host()` factory method generated
  - [ ] Methods for each contract function generated
  - [ ] Generated code runs
  
  **Commit**:
  - Message: `feat(python): generate guest-side host contract callers`

- [ ] **Task 21**: Generate Lua host-side contract metatables
  
  **Specification**:
  Generate Lua metatables that hosts implement for host contracts.
  
  **Files**: `crates/polyplug_codegen/src/generators/lua.rs`
  
  **Generated Code**:
  ```lua
  -- Generated: host/contracts.lua
  HostLogger = {}
  HostLogger.__index = HostLogger
  
  function HostLogger:new()
      local obj = {}
      setmetatable(obj, self)
      return obj
  end
  
  function HostLogger:log(level, message)
      error("abstract method")
  end
  
  function HostLogger:logf(level, format, args)
      error("abstract method")
  end
  
  -- Registration helper
  function Runtime:register_host_logger(impl_)
      -- Creates bridge, registers with runtime
  end
  ```
  
  **Acceptance Criteria**:
  - [ ] Metatable generated for each host contract
  - [ ] Registration helper generated
  - [ ] Generated code runs
  
  **Commit**:
  - Message: `feat(lua): generate host-side contract metatables`

- [ ] **Task 22**: Generate Lua guest-side host contract callers
  
  **Specification**:
  Generate Lua functions that plugins use to call host contracts.
  
  **Generated Code**:
  ```lua
  -- Generated: guest/host_contracts.lua
  HostLoggerContract = {}
  HostLoggerContract.__index = HostLoggerContract
  
  function HostLoggerContract.from_host(host)
      -- Call host.get_host_contract()
      local vtable = host.get_host_contract(HOST_LOGGER_ID)
      if not vtable then return nil end
      return setmetatable({vtable = vtable}, HostLoggerContract)
  end
  
  function HostLoggerContract:log(level, message)
      -- Call through vtable
  end
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller metatable generated
  - [ ] `from_host()` factory function generated
  - [ ] Methods for each contract function generated
  - [ ] Generated code runs
  
  **Commit**:
  - Message: `feat(lua): generate guest-side host contract callers`

- [ ] **Task 23**: Generate JavaScript host-side contract interfaces
  
  **Specification**:
  Generate JavaScript/TypeScript interfaces that hosts implement.
  
  **Files**: `crates/polyplug_codegen/src/generators/js_quickjs.rs`
  
  **Generated Code**:
  ```typescript
  // Generated: host/contracts.ts
  export interface HostLogger {
      log(level: number, message: string): void;
      logf(level: number, format: string, args: any[]): void;
  }
  
  export class Runtime {
      registerHostLogger(impl_: HostLogger): void {
          // Creates bridge, registers with runtime
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Interface generated for each host contract
  - [ ] Registration helper generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(js): generate host-side contract interfaces`

- [ ] **Task 24**: Generate JavaScript guest-side host contract callers
  
  **Specification**:
  Generate JavaScript classes that plugins use to call host contracts.
  
  **Generated Code**:
  ```typescript
  // Generated: guest/host_contracts.ts
  export class HostLoggerContract {
      private vtable: bigint;
      
      static fromHost(host: HostVTable): HostLoggerContract | null {
          // Call host.get_host_contract()
      }
      
      log(level: number, message: string): void {
          // Call through vtable
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] Guest caller class generated
  - [ ] `fromHost()` factory method generated
  - [ ] Methods for each contract function generated
  - [ ] Generated code compiles
  
  **Commit**:
  - Message: `feat(js): generate guest-side host contract callers`

---

### Wave 7: SDK Updates
**Status**: Blocked by Wave 6
**Blocks**: Wave 8

- [ ] **Task 25**: Update Rust SDK with host contract registration
  
  **Specification**:
  Update `polyplug` crate with host contract registration API.
  
  **Implementation**:
  ```rust
  impl Runtime {
      pub fn register_host_contract<T: HostContract>(&self, impl_: T) {
          // Implementation
      }
  }
  ```
  
  **Commit**:
  - Message: `feat(rust-sdk): add host contract registration API`

- [ ] **Task 26**: Update Python SDK with host contract registration
  
  **Specification**:
  Update Python host SDK with host contract registration API.
  
  **Implementation**:
  ```python
  # Generated: host/contracts.py
  class Runtime:
      def register_host_logger(self, impl_: HostLogger) -> None:
          """Register a HostLogger implementation with the runtime."""
          # Creates PythonHostBridge, registers with runtime
          pass
  
      def get_host_logger(self) -> Optional[HostLogger]:
          """Get registered HostLogger for plugin calls."""
          pass
  ```
  
  **Acceptance Criteria**:
  - [ ] `register_host_logger()` method added to Runtime
  - [ ] Bridge created between Python implementation and C ABI
  - [ ] GIL handling implemented correctly
  - [ ] Generated code compiles and runs
  
  **Files**: `sdks/python/host/polyplug/`
  
  **Commit**:
  - Message: `feat(python-sdk): add host contract registration`

- [ ] **Task 27**: Update C# SDK with host contract registration
  
  **Specification**:
  Update C# host SDK with host contract registration API.
  
  **Implementation**:
  ```csharp
  // Generated: Host/Contracts.cs
  public partial class Runtime {
      public void RegisterHostLogger(IHostLogger impl_) {
          // Creates vtable, registers with runtime
      }
      
      public IHostLogger GetHostLogger() {
          // Returns registered implementation for plugin calls
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] `RegisterHostLogger()` method added to Runtime
  - [ ] Interface vtable created
  - [ ] Generated code compiles
  
  **Files**: `sdks/csharp/host/Polyplug/`
  
  **Commit**:
  - Message: `feat(csharp-sdk): add host contract registration`

- [ ] **Task 28**: Update Lua SDK with host contract registration
  
  **Specification**:
  Update Lua host SDK with host contract registration API.
  
  **Implementation**:
  ```lua
  -- Generated: host/contracts.lua
  function Runtime:register_host_logger(impl_)
      -- Creates LuaHostBridge, registers with runtime
  end
  
  function Runtime:get_host_logger()
      -- Returns registered implementation for plugin calls
  end
  ```
  
  **Acceptance Criteria**:
  - [ ] `register_host_logger()` function added to Runtime
  - [ ] Bridge created between Lua implementation and C ABI
  - [ ] Generated code runs
  
  **Files**: `sdks/lua/host/polyplug/`
  
  **Commit**:
  - Message: `feat(lua-sdk): add host contract registration`

- [ ] **Task 29**: Update JavaScript SDK with host contract registration
  
  **Specification**:
  Update JavaScript host SDK with host contract registration API.
  
  **Implementation**:
  ```typescript
  // Generated: host/contracts.ts
  export class Runtime {
      registerHostLogger(impl_: HostLogger): void {
          // Creates JavaScriptHostBridge, registers with runtime
      }
      
      getHostLogger(): HostLogger | null {
          // Returns registered implementation for plugin calls
      }
  }
  ```
  
  **Acceptance Criteria**:
  - [ ] `registerHostLogger()` method added to Runtime
  - [ ] Bridge created between JS implementation and C ABI
  - [ ] Generated code compiles
  
  **Files**: `sdks/js/host/polyplug/`
  
  **Commit**:
  - Message: `feat(js-sdk): add host contract registration`

- [ ] **Task 30**: Update C++ SDK with host contract registration
  
  **Specification**:
  Update C++ host SDK with host contract registration API.
  
  **Implementation**:
  ```cpp
  // Generated: host/contracts.hpp
  class Runtime {
  public:
      void register_host_logger(std::shared_ptr<HostLogger> impl_);
      std::shared_ptr<HostLogger> get_host_logger();
  };
  ```
  
  **Acceptance Criteria**:
  - [ ] `register_host_logger()` method added to Runtime
  - [ ] Vtable created and registered with runtime
  - [ ] Generated code compiles
  
  **Files**: `sdks/cpp/host/polyplug/`
  
  **Commit**:
  - Message: `feat(cpp-sdk): add host contract registration`

---

### Wave 8: Examples and Documentation
**Status**: Blocked by Wave 7
**Blocks**: Wave 9

- [ ] **Task 31**: Create Host Contracts examples
  
  **Specification**:
  Create examples demonstrating host contracts in all 6 languages.
  
  **Examples**:
  1. `examples/host_contracts/logger/` - Simple logging host contract
  2. `examples/host_contracts/metrics/` - Metrics recording host contract
  3. `examples/host_contracts/bidirectional/` - Host calls plugin, plugin calls host
  
  **Each example includes**:
  - api.toml with both plugin_contract and host_contract
  - Host implementation in each language
  - Plugin implementation in each language
  - Build and run instructions
  
  **Acceptance Criteria**:
  - [ ] Logger example in all 6 languages
  - [ ] Metrics example in all 6 languages
  - [ ] Bidirectional example in all 6 languages
  - [ ] All examples build and run correctly
  
  **QA Verification**:
  ```bash
  just build-examples
  just verify-examples
  # Expected: All examples pass
  ```
  
  **Commit**:
  - Message: `feat(examples): add host contracts examples`
  - Files: `examples/host_contracts/`

- [ ] **Task 32**: Update existing examples to use `[[plugin_contract]]`
  
  **Specification**:
  Update all existing examples to use new syntax.
  
  **Files**: All `examples/*/api.toml` files
  
  **Commit**:
  - Message: `refactor(examples): update to use [[plugin_contract]]`

- [ ] **Task 33**: Write Host Contracts tutorial documentation
  
  **Specification**:
  Write comprehensive documentation for host contracts.
  
  **Documents**:
  - `docs/HOST_CONTRACTS.md` - Full tutorial
  - `docs/HOST_CONTRACTS_API.md` - API reference
  - `MIGRATION.md` - Migration guide from old syntax
  
  **Commit**:
  - Message: `docs: add host contracts documentation`

---

### Wave 9: Testing and Verification
**Status**: Blocked by Wave 8
**Blocks**: Completion

- [ ] **Task 34**: Write unit tests for host contract types
  
  **Specification**:
  Test all host contract types in polyplug_abi.
  
  **Tests**:
  - Layout tests (size, alignment, offsets)
  - Send/Sync tests
  - ID collision tests
  
  **Commit**:
  - Message: `test(abi): add host contract unit tests`

- [ ] **Task 35**: Write parser tests for host contracts
  
  **Specification**:
  Test parser with host contract syntax.
  
  **Tests**:
  - Valid host contract parsing
  - Invalid name rejection (no "host." prefix)
  - Duplicate name rejection
  - Version validation
  
  **Commit**:
  - Message: `test(parser): add host contract parser tests`

- [ ] **Task 36**: Write code generator tests
  
  **Specification**:
  Test code generation for all 6 languages.
  
  **Tests**:
  - Generated code compiles
  - Generated code runs correctly
  - Host-side and guest-side both tested
  
  **Commit**:
  - Message: `test(codegen): add host contract codegen tests`

- [ ] **Task 37**: Write runtime integration tests
  
  **Specification**:
  Test runtime host contract functionality.
  
  **Tests**:
  - Registration
  - Lookup
  - Version checking
  - Thread safety
  - VM bridge integration
  
  **Commit**:
  - Message: `test(runtime): add host contract integration tests`

- [ ] **Task 38**: Write cross-language tests
  
  **Specification**:
  Test cross-language host contract calls.
  
  **Test Matrix**:
  - Rust host + all 6 plugin languages
  - Python host + all 6 plugin languages
  - Lua host + all 6 plugin languages
  - Total: 18 combinations (not 36, host is single language)
  
  **Commit**:
  - Message: `test(integration): add cross-language host contract tests`

- [ ] **Task 39**: Final verification
  
  **Specification**:
  Complete final verification of entire implementation.
  
  **Verification Checklist**:
  - [ ] `cargo test --workspace` passes
  - [ ] `cargo clippy -- -D warnings` clean
  - [ ] `cargo fmt --check` clean
  - [ ] All examples build and run
  - [ ] SDK validator passes
  - [ ] Documentation complete and accurate
  - [ ] No `[[contract]]` syntax remaining (except deprecated support)
  - [ ] **NO EXTENSION SYSTEM REMAINING** - `get_extension` completely removed
  - [ ] **NO EXTENSION CODE** - `crates/polyplug/src/extensions/` deleted
  - [ ] **NO EXT_TRACE_ID** in generated code
  - [ ] **NO EXTENSION REFERENCES** in codebase
  
  **QA Verification**:
  ```bash
  # Full test suite
  cargo test --workspace
  
  # Linting
  cargo clippy -- -D warnings
  cargo fmt --check
  
  # Examples
  just build-examples
  just verify-examples
  
  # SDK validator
  just validate-sdks
  
  # Check for old contract syntax
  grep -r "^\[\[contract\]\]" --include="*.toml" . | grep -v "test.*deprecated"
  # Expected: Only deprecation test fixtures
  
  # Verify EXTENSION SYSTEM COMPLETELY REMOVED
  grep -r "get_extension" --include="*.rs" crates/
  # Expected: No results
  
  grep -r "EXT_TRACE_ID" --include="*.rs" crates/
  # Expected: No results
  
  test -d crates/polyplug/src/extensions/
  # Expected: Directory does not exist
  
  grep -r "mod extensions" crates/polyplug/src/lib.rs
  # Expected: No results
  
  grep -r "Extension" --include="*.rs" crates/polyplug/src/ | grep -v "// Extension system was removed"
  # Expected: Only historical comments
  ```
  
  **DO NOT COMMIT** - This is final verification only

---

### Final Verification Wave
**Status**: Blocked by Task 39

- [ ] **Task F1**: Plan Compliance Audit — `oracle`
  
  **Specification**:
  Verify all tasks completed according to plan.
  
  **Check**:
  - All tasks marked complete
  - No tasks skipped
  - No deviations from plan
  
  **If Gaps Found**: STOP and ask user

- [ ] **Task F2**: Code Quality Review — `oracle`
  
  **Specification**:
  Review code quality across all changes.
  
  **Check**:
  - All clippy warnings resolved
  - No unsafe code without SAFETY comments
  - No TODO/FIXME comments remaining
  - Consistent naming conventions
  
  **If Issues Found**: STOP and ask user

- [ ] **Task F3**: Real Manual QA — `oracle`
  
  **Specification**:
  Manually verify all examples work correctly.
  
  **Verify**:
  - Run each example manually
  - Verify bidirectional communication
  - Check error handling
  
  **If Issues Found**: STOP and ask user

- [ ] **Task F4**: Scope Fidelity Check — `oracle`
  
  **Specification**:
  Verify no scope creep occurred.
  
  **Check**:
  - No features added beyond plan
  - No unnecessary refactoring
  - All changes relate to host contracts
  
  **If Scope Creep Found**: STOP and ask user

---

## Dependency Matrix

| Task | Depends On | Blocks |
|------|-----------|--------|
| D1-D4 | — | Wave 1 |
| 1-4 | D1-D4 | Wave 2 |
| 5-6 | 1-4 | Wave 3 |
| 7-8 | 5-6 | Wave 4 |
| 9-10 | 2, 3, 7, 8 | Wave 5 |
| 11 | 9-10 | Wave 6, 8 |
| 12-14 | 11 | Wave 6, 8 |
| 15-24 | 5-6, 7-8 | Wave 7 |
| 25 | 7-8, 11 | Wave 8 |
| 26-30 | 15-24, 12-14 | Wave 8 |
| 31-33 | 25-30 | Wave 9 |
| 34-39 | 31-33 | F1-F4 |
| F1-F4 | 39 | — |

---

## Commit Strategy

- **One commit per task** (unless task spans multiple files logically)
- **Commit message format**: `type(scope): description`
- **Pre-commit hooks**: Run relevant tests before each commit
- **No merge commits**: Rebase workflow

---

## Success Criteria

### Must Have (All Required)
- [ ] `[[plugin_contract]]` and `[[host_contract]]` work in api.toml
- [ ] All 6 languages support both contract types (host and guest)
- [ ] Host Runtime Bridge works for Python, Lua, JavaScript
- [ ] Examples run successfully with bidirectional communication
- [ ] All tests pass
- [ ] Documentation complete

### Must NOT Have (Prohibited)
- [ ] No `[[contract]]` syntax in any non-test files
- [ ] **NO EXTENSION SYSTEM REMAINING** - `get_extension` completely removed
- [ ] No `Extension` trait or types remaining
- [ ] No extension-related code in generators
- [ ] No async support (out of scope)
- [ ] No scope creep

---

## Execution Rules Reminder

### Rule 1: All Tasks Required
**THERE IS NO LOWER PRIORITY TASKS. THERE IS NO OPTIONAL TASKS.**
Every task from D1 to F4 must be completed in order.

### Rule 2: Plan Execution As-Is
**DO NOT EDIT TASK DESCRIPTIONS.**
Only mark tasks as completed (`[x]`). Never modify requirements.

### Rule 3: Gap Discovery Protocol
**IF EXECUTOR FINDS A GAP, STOP AND ASK USER.**
Never take decisions independently.

### Rule 4: Extensions Are Being Removed - ENFORCED
**EXTENSIONS ARE COMPLETELY REMOVED AND REPLACED BY HOST CONTRACTS.**
- `get_extension` field **REMOVED** from HostVTable
- `crates/polyplug/src/extensions/` directory **DELETED**
- `Extension` trait **REMOVED**
- All extension code in generators **REMOVED**
- `EXT_TRACE_ID` generation **REMOVED**
- Host Contracts are the **REPLACEMENT**, not coexistence
- **NO BACKWARDS COMPATIBILITY** for Extensions

---

## Extension System Removal Notice

**THE EXTENSION SYSTEM IS COMPLETELY REMOVED BY THIS PLAN**

### What Is Being Deleted:
1. `crates/polyplug/src/extensions/` - **ENTIRE DIRECTORY DELETED**
2. `HostVTable.get_extension` field - **REMOVED**
3. `Extension` trait - **REMOVED**
4. `ExtensionEntry` struct - **REMOVED**
5. All `EXT_TRACE_ID` generation in code generators - **REMOVED**
6. Runtime extension registration - **REMOVED**

### What Replaces It:
**Host Contracts** provide the same capability (plugins calling host) with proper type safety, schema declarations, and full code generation.

### Migration:
- Old: `optional = ["trace"]` in bundle.toml + `get_extension(EXT_TRACE_ID)`
- New: `[[host_contract]]` in api.toml + `HostLoggerContract::from_host(host)`

---

*Plan Version: 3.0*
*Last Updated: After user correction - Extensions fully removed, not coexisting*
*Ready for Execution: YES*
