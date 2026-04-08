---
phase: 12-sdk-instance-model
plan: 03b
type: execute
wave: 3
depends_on: [12-03a]
files_modified:
  - crates/polyplugc/src/generators/lua.rs
  - crates/polyplugc/src/generators/csharp.rs
  - crates/polyplugc/src/generators/js_quickjs.rs
autonomous: false
requirements: [SDK-07]
user_setup: []

must_haves:
  truths:
    - "Lua codegen generates instance closure wrapper with create/destroy methods"
    - "C# codegen generates instance wrapper class with constructor/dispose"
    - "JS QuickJS codegen generates instance wrapper class"
    - "All wrappers call create_instance on creation, destroy_instance on cleanup"
  artifacts:
    - path: "crates/polyplugc/src/generators/lua.rs"
      provides: "Lua code generator"
      contains: "__gc"
    - path: "crates/polyplugc/src/generators/csharp.rs"
      provides: "C# code generator"
      contains: "IDisposable"
    - path: "crates/polyplugc/src/generators/js_quickjs.rs"
      provides: "JS QuickJS code generator"
      contains: "class.*Contract"
  key_links:
    - from: "polyplugc Lua/C#/JS generators"
      to: "Rust generator pattern"
      via: "copy pattern from rust.rs"
      pattern: "struct.*instance.*GuestContractInstance"
---

<objective>
Add instance wrapper codegen to Lua, C#, and JS QuickJS generators (Part 2 of SDK-07).

Purpose: Enable host applications in Lua, C#, and JavaScript to use RAII instance wrappers for safe lifecycle management.
Output: Updated generators with instance wrapper generation matching Rust pattern, plus verification checkpoint.
</objective>

<execution_context>
@$HOME/.claude/get-shit-done/workflows/execute-plan.md
@$HOME/.claude/get-shit-done/templates/summary.md
</execution_context>

<context>
@.planning/PROJECT.md
@.planning/ROADMAP.md
@.planning/STATE.md
@.planning/phases/12-sdk-instance-model/12-03a-SUMMARY.md

<interfaces>
<!-- Rust instance wrapper pattern to replicate in Lua, C#, and JS -->

From crates/polyplugc/src/generators/rust.rs (lines 1290-1370):
```rust
pub struct XxxContract {
    interface: *const GuestContractInterface,
    instance: GuestContractInstance,
    host: *const HostInterface,
}

impl XxxContract {
    pub fn new(handle: PluginHandle, host: *const HostInterface) -> Option<Self> {
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
  <name>Task 1: Add instance wrapper generation to Lua generator</name>
  <files>crates/polyplugc/src/generators/lua.rs</files>
  <read_first>
    - crates/polyplugc/src/generators/lua.rs (current Lua generator - has stubs)
    - crates/polyplugc/src/generators/rust.rs:1290-1400 (Rust wrapper pattern)
  </read_first>
  <action>
    Add `generate_host_instance_wrapper_lua` function:
    
    1. Generate closure-based wrapper:
       ```lua
       function XxxContract_new(handle, host)
           local interface = polyplug_runtime_resolve_contract(host, handle)
           if not interface then return nil end
           local instance = interface.create_instance(host, nil)
           if instance.data == nil then return nil end
           
           local wrapper = {
               _interface = interface,
               _instance = instance,
               _host = host,
               
               is_valid = function(self)
                   return self._instance.data ~= nil
               end,
               
               destroy = function(self)
                   if self._instance.data ~= nil then
                       self._interface.destroy_instance(self._host, self._instance)
                       self._instance.data = nil
                   end
               end,
               -- Method callers...
           }
           
           -- Set __gc metamethod for cleanup
           local mt = { __gc = function(self) self:destroy() end }
           setmetatable(wrapper, mt)
           
           return wrapper
       end
       ```
    
    2. Call from `generate_host_side_lua`
    3. Update method callers to pass `self._instance`
    4. Use __gc metamethod for automatic cleanup
  </action>
  <verify>
    <automated>grep -c "__gc" crates/polyplugc/src/generators/lua.rs | grep -v "^0$"</automated>
  </verify>
  <acceptance_criteria>
    - crates/polyplugc/src/generators/lua.rs generates function creating wrapper table
    - crates/polyplugc/src/generators/lua.rs generates `create_instance` call in constructor
    - crates/polyplugc/src/generators/lua.rs generates `destroy_instance` call in cleanup
    - crates/polyplugc/src/generators/lua.rs uses `__gc` metamethod for cleanup
    - cargo test -p polyplugc passes
  </acceptance_criteria>
  <done>Lua generator produces instance wrapper closures with __gc cleanup.</done>
</task>

<task type="auto">
  <name>Task 2: Add instance wrapper generation to C# generator</name>
  <files>crates/polyplugc/src/generators/csharp.rs</files>
  <read_first>
    - crates/polyplugc/src/generators/csharp.rs (current C# generator)
    - crates/polyplugc/src/generators/rust.rs:1290-1400 (Rust wrapper pattern)
  </read_first>
  <action>
    Add `generate_host_instance_wrapper_csharp` function:
    
    1. Generate wrapper class with IDisposable:
       ```csharp
       public class XxxContract : IDisposable {
           private readonly GuestContractInterface* _interface;
           private GuestContractInstance _instance;
           private readonly HostInterface* _host;
           private bool _disposed;
           
           public static XxxContract? Create(PluginHandle handle, HostInterface* host) {
               var iface = NativeMethods.polyplug_runtime_resolve_contract(host, handle);
               if (iface == null) return null;
               var inst = iface->create_instance(host, null);
               if (inst.Data == null) return null;
               return new XxxContract(iface, inst, host);
           }
           
           private XxxContract(GuestContractInterface* iface, GuestContractInstance inst, HostInterface* host) {
               _interface = iface; _instance = inst; _host = host;
           }
           
           public bool IsValid => _instance.Data != null;
           
           public void Dispose() {
               if (!_disposed && _instance.Data != null) {
                   _interface->destroy_instance(_host, _instance);
                   _disposed = true;
               }
           }
           
           // Method callers...
       }
       ```
    
    2. Call from `generate_host_side_csharp`
    3. Update method callers to pass `_instance`
    4. Implement IDisposable pattern for proper cleanup
  </action>
  <verify>
    <automated>grep -c "IDisposable" crates/polyplugc/src/generators/csharp.rs | grep -v "^0$"</automated>
  </verify>
  <acceptance_criteria>
    - crates/polyplugc/src/generators/csharp.rs generates class implementing `IDisposable`
    - crates/polyplugc/src/generators/csharp.rs generates `Dispose()` calling `destroy_instance`
    - crates/polyplugc/src/generators/csharp.rs generates `Create` factory method calling `create_instance`
    - cargo test -p polyplugc passes
  </acceptance_criteria>
  <done>C# generator produces instance wrapper classes with IDisposable.</done>
</task>

<task type="auto">
  <name>Task 3: Add instance wrapper generation to JS QuickJS generator</name>
  <files>crates/polyplugc/src/generators/js_quickjs.rs</files>
  <read_first>
    - crates/polyplugc/src/generators/js_quickjs.rs (current JS QuickJS generator)
    - crates/polyplugc/src/generators/rust.rs:1290-1400 (Rust wrapper pattern)
  </read_first>
  <action>
    Add `generate_host_instance_wrapper_js` function:
    
    1. Generate wrapper class:
       ```javascript
       class XxxContract {
           constructor(handle, host) {
               this._interface = polyplugRuntimeResolveContract(host, handle);
               if (!this._interface) throw new Error("Contract not found");
               this._instance = this._interface.create_instance(host, null);
               if (!this._instance || this._instance.data === null) {
                   throw new Error("create_instance failed");
               }
               this._host = host;
           }
           
           isValid() {
               return this._instance && this._instance.data !== null;
           }
           
           destroy() {
               if (this._instance && this._instance.data !== null) {
                   this._interface.destroy_instance(this._host, this._instance);
                   this._instance.data = null;
               }
           }
           
           // Method callers...
       }
       ```
    
    2. Call from `generate_host_side_js`
    3. Update method callers to pass `this._instance`
    4. Add explicit destroy() method since JS has no deterministic cleanup
  </action>
  <verify>
    <automated>grep -c "class.*Contract" crates/polyplugc/src/generators/js_quickjs.rs | grep -v "^0$"</automated>
  </verify>
  <acceptance_criteria>
    - crates/polyplugc/src/generators/js_quickjs.rs generates class with `constructor` calling `create_instance`
    - crates/polyplugc/src/generators/js_quickjs.rs generates `destroy()` calling `destroy_instance`
    - crates/polyplugc/src/generators/js_quickjs.rs generates `_instance` member
    - cargo test -p polyplugc passes
  </acceptance_criteria>
  <done>JS QuickJS generator produces instance wrapper classes with explicit destroy.</done>
</task>

<task type="checkpoint:human-verify">
  <name>Task 4: Verify all generators produce instance wrappers</name>
  <files>N/A - verification only</files>
  <action>Manual verification of codegen tests and generated output correctness.</action>
  <what-built>Instance wrapper generation added to C++, Python, Lua, C#, and JS QuickJS generators</what-built>
  <how-to-verify>
    1. Run `cargo test -p polyplugc` - all tests should pass
    2. Run `cargo test --workspace --lib` - workspace tests pass
    3. Generate sample code: `cargo run -p polyplugc -- generate --bundle examples/test/bundle.toml --lang cpp --out /tmp/test_cpp`
    4. Verify generated C++ code contains instance wrapper class with create/destroy_instance calls
  </how-to-verify>
  <verify>
    <automated>cargo test -p polyplugc</automated>
  </verify>
  <resume-signal>Type "approved" or describe test failures/generation issues</resume-signal>
  <done>User confirms all tests pass and generated code contains instance wrappers.</done>
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
| T-12-03b-01 | Tampering | Instance wrapper lifecycle | mitigate | Generated code checks instance.data != null before use, calls destroy on cleanup |
| T-12-03b-02 | Elevation | Instance handle reuse | mitigate | destroy() sets instance.data = null to prevent reuse after cleanup |
| T-12-03b-03 | DoS | Memory leak from missing cleanup | mitigate | __gc metamethod (Lua), IDisposable (C#), explicit destroy() (JS) ensure cleanup |

Key security property: Generated wrappers must null-check instance.data before every operation and nullify after destroy.
</threat_model>

<verification>
- All generators have instance wrapper generation functions
- cargo test -p polyplugc passes
- Generated code contains create_instance/destroy_instance lifecycle
</verification>

<success_criteria>
- Lua generator produces closure with __gc metamethod
- C# generator produces IDisposable class
- JS QuickJS generator produces class with destroy()
- All tests pass
- User verifies generated output correctness
</success_criteria>

<output>
After completion, create `.planning/phases/12-sdk-instance-model/12-03b-SUMMARY.md`
</output>