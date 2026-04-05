---
status: testing
phase: 03-instance-model
source:
  - .planning/phases/03-instance-model/03-01-SUMMARY.md
  - .planning/phases/03-instance-model/03-02-SUMMARY.md
  - .planning/phases/03-instance-model/03-03-SUMMARY.md
  - .planning/phases/03-instance-model/03-04-SUMMARY.md
  - .planning/phases/03-instance-model/03-05-SUMMARY.md
started: "2026-04-04T14:30:00Z"
updated: "2026-04-04T14:30:00Z"
---

## Current Test

number: 1
name: Workspace Build Verification
expected: |
  Running `cargo check --workspace` completes without compilation errors.
  All crates (polyplug, polyplug_abi, polyplugc, polyplug_utils) compile successfully.
awaiting: user response

## Tests

### 1. Workspace Build Verification
expected: Running `cargo check --workspace` completes without compilation errors. All crates compile successfully.
result: pending

### 2. Guest VTable Instance Lifecycle in Codegen
expected: |
  Generated guest vtable code includes create_instance and destroy_instance stub functions.
  The dispatch wrapper signature includes GuestContractInstance as the first parameter.
  Can verify by generating code or checking generator source includes these patterns.
result: pending

### 3. Host Contract Singleton Field in Codegen
expected: |
  Generated host contract factory code includes a singleton: bool field in the vtable structure.
  Instance lifecycle stubs (create_instance, destroy_instance) are generated for host contracts.
  All 6 language generators (Rust, C#, Python, Lua, C++, JS) include singleton support.
result: pending

### 4. Runtime Singleton Instance Cache
expected: |
  The Runtime struct contains a singleton_instances field for caching singleton host contract instances.
  The host_get_host_contract FFI callback is implemented and handles singleton vs multi-instance contracts differently.
result: pending

### 5. Instance Wrapper RAII Pattern in Host Callers
expected: |
  Generated host caller structs use instance-based RAII pattern with GuestContractInstance field.
  The new() method calls create_instance and the Drop impl calls destroy_instance.
  Dispatch passes the instance as the first argument (native) or second argument (VM).
result: pending

## Summary

total: 5
passed: 0
issues: 0
pending: 5
skipped: 0

## Gaps

[none yet]