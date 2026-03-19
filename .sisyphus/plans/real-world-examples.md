# Real-World Examples Plan

## Overview

Update all examples to be "real-world" examples that:
1. Use `polyplugc` to generate code
2. Link to host-libs and guest-libs properly
3. Use `RuntimeConfig` for hot-reload configuration
4. Clean up plugin instances when hot-reload happens
5. Hide vtable and PluginGuard from app users (factory method pattern)

## Current State

### Generated Code
- `examples/hosts/rust/src/generated/host/host_callers.rs` - exposes `handle` and `runtime` directly
- Uses `runtime.resolve_plugin(self.handle)` on every call (good for hot-reload)
- No factory method pattern (`create()` method)

### Examples
- All 6 host examples exist (Rust, C++, Python, C#, Lua, JS)
- All 6 guest examples exist (Rust, C++, Python, C#, Lua, JS)
- Hot-reload callback registered but no instance cleanup

## Required Changes

### Phase 1: Update Codegen (if needed)

- [ ] Verify codegen generates factory method pattern (`create()`)
- [ ] Verify codegen hides vtable/guard from public API
- [ ] Verify codegen generates `is_valid()` and `reset()` methods

### Phase 2: Update Rust Host Example

- [ ] Update `examples/hosts/rust/src/main.rs`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase
  - Use factory method pattern for creating callers

### Phase 3: Update C++ Host Example

- [ ] Update `examples/hosts/cpp/main.cpp`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase

### Phase 4: Update Python Host Example

- [ ] Update `examples/hosts/python/host.py`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase

### Phase 5: Update C# Host Example

- [ ] Update `examples/hosts/csharp/Program.cs`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase

### Phase 6: Update Lua Host Example

- [ ] Update `examples/hosts/lua/host.lua`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase

### Phase 7: Update JS Host Example

- [ ] Update `examples/hosts/js/host.js`:
  - Use `RuntimeConfig` for hot-reload settings
  - Track plugin instances per bundle
  - Clean up instances in `Preparing` phase
  - Re-create instances in `Reloaded` phase

### Phase 8: Update Guest Examples

- [ ] Verify all guest examples work with updated host examples
- [ ] Ensure guest examples use guest-libs properly

### Phase 9: Update Build Scripts

- [ ] Update `examples/build.sh` to use polyplugc
- [ ] Update `examples/build_all.sh` to build all examples

### Phase 10: Documentation

- [ ] Update `examples/README.md` with new patterns
- [ ] Add hot-reload cleanup example to documentation

## Key Patterns

### Instance Tracking Pattern

```rust
// Track instances per bundle
let instances: HashMap<u64, Vec<Box<dyn Any>>> = HashMap::new();

// In reload callback
match phase {
    ReloadPhase::Preparing { bundle_id, .. } => {
        // Clean up all instances for this bundle
        instances.remove(&bundle_id);
    }
    ReloadPhase::Reloaded { bundle_id, .. } => {
        // Re-create instances for this bundle
        // instances.insert(bundle_id, create_new_instances());
    }
    _ => {}
}
```

### Factory Method Pattern

```rust
// Instead of:
let decoder = PipelineDecoderContract::new(handle, runtime);

// Use:
let decoder = PipelineDecoderContract::create(runtime, min_version)?;
if decoder.is_valid() {
    let result = decoder.decode(input)?;
}
```

## Estimated Effort

- Phase 1: 1-2 hours (if codegen needs updates)
- Phase 2-7: 3-4 hours (6 examples)
- Phase 8: 1 hour
- Phase 9: 1 hour
- Phase 10: 1 hour

**Total: 7-10 hours**