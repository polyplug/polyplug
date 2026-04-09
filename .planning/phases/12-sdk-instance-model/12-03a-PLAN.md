---
phase: 12-sdk-instance-model
plan: 03a
type: execute
wave: 2
depends_on: [12-01, 12-02]
files_modified:
  - crates/polyplugc/src/generators/cpp.rs
  - crates/polyplugc/src/generators/python.rs
autonomous: true
requirements: [SDK-07]
user_setup: []

must_haves:
  truths:
    - "C++ codegen generates RAII instance wrapper class with constructor/destructor"
    - "Python codegen generates instance wrapper class with __init__/__del__"
    - "Both wrappers call create_instance on creation, destroy_instance on cleanup"
  artifacts:
    - path: "crates/polyplugc/src/generators/cpp.rs"
      provides: "C++ code generator"
      contains: "class.*Contract"
      min_lines: 500
    - path: "crates/polyplugc/src/generators/python.rs"
      provides: "Python code generator"
      contains: "__del__"
  key_links:
    - from: "polyplugc C++/Python generators"
      to: "Rust generator pattern"
      via: "copy pattern from rust.rs"
      pattern: "struct.*instance.*GuestContractInstance"
---

<objective>
Add instance wrapper codegen to C++ and Python generators (Part 1 of SDK-07).

Purpose: Enable host applications in C++ and Python to use RAII instance wrappers for safe lifecycle management.
Output: Updated C++ and Python generators with instance wrapper generation matching Rust pattern.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md

<interfaces>
<!-- Rust instance wrapper pattern to replicate in C++ and Python -->

From crates/polyplugc/src/generators/rust.rs (lines 1290-1370):
```rust
pub struct XxxContract {
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
    host: *const HostInterface,
}

impl XxxContract {
    pub fn new(handle: GuestContractHandle, host: *const HostInterface) -> Option<Self> {
        let interface = /* resolve handle */;
        let instance = unsafe { ((*interface).create_instance)(host, ptr::null()) };
        if instance.data.is_null() { return None; }
        Some(Self { interface, instance, host })
    }
    
    pub fn is_valid(&self) -> bool { !self.instance.data.is_null() }
    
    pub fn reset(&mut self) {
        // Destroy old, create new
    }
}

impl Drop for XxxContract {
    fn drop(&mut self) {
        if !self.instance.data.is_null() {
            unsafe { ((*self.interface).destroy_instance)(self.host, self.instance); }
        }
    }
}
```

Key components to implement:
1. Wrapper struct/class holding interface, instance, host pointers
2. Constructor that resolves handle and calls create_instance
3. Destructor/dispose that calls destroy_instance
4. is_valid() check method
5. Method callers that pass instance as first argument
</interfaces>
</context>

<tasks>

<task type="auto">
  <name>Task 1: Add instance wrapper generation to C++ generator</name>
  <files>crates/polyplugc/src/generators/cpp.rs</files>
  <read_first>
    - crates/polyplugc/src/generators/cpp.rs (current C++ generator - has stubs)
    - crates/polyplugc/src/generators/rust.rs:1290-1400 (Rust wrapper pattern to copy)
    - crates/polyplug_abi/src/guest/guest_contract_instance.rs (GuestContractInstance struct)
  </read_first>
  <action>
    Add `generate_host_instance_wrapper_cpp` function following Rust pattern:
    
    1. Generate RAII wrapper class:
       ```cpp
       class XxxContract {
       private:
           const GuestContractInterface* interface_;
           GuestContractInstance instance_;
           const HostInterface* host_;
       
       public:
           // Constructor: resolve handle + create_instance
           static std::optional<XxxContract> create(GuestContractHandle handle, const HostInterface* host) {
               const GuestContractInterface* iface = polyplug_runtime_resolve_contract(host, handle);
               if (!iface) return std::nullopt;
               GuestContractInstance inst = iface->create_instance(host, nullptr);
               if (inst.data == nullptr) return std::nullopt;
               return XxxContract(iface, inst, host);
           }
           
           // Destructor: calls destroy_instance
           ~XxxContract() {
               if (instance_.data != nullptr) {
                   interface_->destroy_instance(host_, instance_);
               }
           }
           
           bool is_valid() const { return instance_.data != nullptr; }
           
           // Method callers...
       };
       ```
    
    2. Call this function from `generate_host_side_cpp` after interface generation
    3. Update method caller generation to pass `instance_` as first argument to dispatch calls
    4. Ensure proper nullptr checks and RAII semantics
  </action>
  <verify>
    <automated>grep -c "class.*Contract" crates/polyplugc/src/generators/cpp.rs | grep -v "^0$"</automated>
  </verify>
  <acceptance_criteria>
    - crates/polyplugc/src/generators/cpp.rs contains function `generate_host_instance_wrapper_cpp` or similar
    - crates/polyplugc/src/generators/cpp.rs generates class with `GuestContractInstance instance_` member
    - crates/polyplugc/src/generators/cpp.rs generates destructor calling `destroy_instance`
    - crates/polyplugc/src/generators/cpp.rs generates constructor calling `create_instance`
    - cargo test -p polyplugc passes after changes
  </acceptance_criteria>
  <done>C++ generator produces RAII instance wrapper classes.</done>
</task>

<task type="auto">
  <name>Task 2: Add instance wrapper generation to Python generator</name>
  <files>crates/polyplugc/src/generators/python.rs</files>
  <read_first>
    - crates/polyplugc/src/generators/python.rs (current Python generator - has stubs)
    - crates/polyplugc/src/generators/rust.rs:1290-1400 (Rust wrapper pattern)
  </read_first>
  <action>
    Add `generate_host_instance_wrapper_python` function:
    
    1. Generate wrapper class:
       ```python
       class XxxContract:
           def __init__(self, handle: GuestContractHandle, host: ctypes.c_void_p):
               self._interface = polyplug_runtime_resolve_contract(host, handle)
               if not self._interface:
                   raise ValueError("Contract not found")
               self._instance = self._interface.contents.create_instance(host, None)
               if self._instance.data is None:
                   raise ValueError("create_instance failed")
               self._host = host
           
           def __del__(self):
               if self._instance.data is not None:
                   self._interface.contents.destroy_instance(self._host, self._instance)
           
           def is_valid(self) -> bool:
               return self._instance.data is not None
           
           # Method callers...
       ```
    
    2. Call from `generate_host_side_python` after interface generation
    3. Update method callers to pass `self._instance` as first argument
    4. Add proper ctypes pointer handling for interface.contents access
  </action>
  <verify>
    <automated>grep -c "def __del__" crates/polyplugc/src/generators/python.rs | grep -v "^0$"</automated>
  </verify>
  <acceptance_criteria>
    - crates/polyplugc/src/generators/python.rs generates class with `__init__` calling `create_instance`
    - crates/polyplugc/src/generators/python.rs generates `__del__` calling `destroy_instance`
    - crates/polyplugc/src/generators/python.rs generates `_instance` member
    - cargo test -p polyplugc passes
  </acceptance_criteria>
  <done>Python generator produces instance wrapper classes with __init__/__del__.</done>
</task>

</tasks>

<threat_model>
## Trust Boundaries

| Boundary | Description |
|----------|-------------|
| Generated code - FFI boundary | Instance wrappers call create/destroy_instance through FFI |

## STRIDE Threat Register

| Threat ID | Category | Component | Disposition | Mitigation Plan |
|-----------|----------|-----------|-------------|-----------------|
| T-12-03a-01 | Tampering | Instance wrapper lifecycle | mitigate | Generated code checks instance.data != null before use, calls destroy on cleanup |
| T-12-03a-02 | Elevation | Instance handle reuse | mitigate | destroy() sets instance.data = null to prevent reuse after cleanup |
| T-12-03a-03 | DoS | Memory leak from missing cleanup | mitigate | RAII patterns (C++) or __del__ (Python) ensure cleanup on scope exit |

Key security property: Generated wrappers must null-check instance.data before every operation and nullify after destroy.
</threat_model>

<verification>
- Both generators have instance wrapper generation functions
- cargo test -p polyplugc passes
- Generated code contains create_instance/destroy_instance lifecycle
</verification>

<success_criteria>
- C++ generator produces RAII class with destructor
- Python generator produces class with __del__
- All tests pass
</success_criteria>

<output>
After completion, create `.planning/phases/12-sdk-instance-model/12-03a-SUMMARY.md`
</output>