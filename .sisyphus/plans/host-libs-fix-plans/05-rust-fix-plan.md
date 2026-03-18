# Rust Host Lib Fix Plan

## Status: MINOR IMPROVEMENTS NEEDED

## Summary

The Rust host lib is the reference implementation and is already well-optimized. However, there are a few minor improvements that can be made for consistency and ergonomics.

## Issues Found

### 1. Loaders Are Not Applicable

Rust loaders are separate crates in the runtime adapters, not part of the host lib. The host lib itself (`host-libs/rust/`) is just a thin wrapper. This is correct per PRD.

**No change needed.**

### 2. call_plugin_fn Could Be More Type-Safe (MINOR)

**Current State (lib.rs lines 114-154):**
```rust
pub fn call_plugin_fn(
    vtable: *const PluginVTable,
    func_idx: usize,
    input: &str,
) -> Result<String, String> {
    // ...
    let func: extern "C" fn(*const (), *mut ()) -> polyplug_abi::AbiError =
        unsafe { std::mem::transmute(*func_ptr) };
    // ...
}
```

This uses `std::mem::transmute` which is correct but could be more explicit about safety.

**Potential Improvement:**
```rust
pub fn call_plugin_fn(
    vtable: *const PluginVTable,
    func_idx: usize,
    input: &str,
) -> Result<String, String> {
    // SAFETY: vtable is valid and func_idx is bounds-checked
    let vtable = unsafe { &*vtable };
    let funcs: &[*const ()] = unsafe {
        std::slice::from_raw_parts(vtable.functions.cast(), vtable.function_count as usize)
    };
    
    let func_ptr = funcs
        .get(func_idx)
        .ok_or_else(|| format!("function index {} out of bounds", func_idx))?;
    
    // SAFETY: func_ptr points to a valid extern "C" function with the expected signature
    // The plugin contract guarantees this function signature
    let func: extern "C" fn(*const (), *mut ()) -> polyplug_abi::AbiError =
        unsafe { std::mem::transmute(*func_ptr) };
    
    // ...
}
```

The current code is already correct. This is a minor ergonomics improvement.

### 3. Missing Safe Wrapper for PluginGuard (MINOR)

**Current State:**
The Rust host lib doesn't provide a `PluginGuard` wrapper. The `resolve_plugin` function returns a raw `*const PluginVTable`.

**Current:**
```rust
pub unsafe fn resolve_plugin(vtable: &HostVTable, handle: PluginHandle) -> *const PluginVTable {
    unsafe { (vtable.resolve_plugin)(handle) }
}
```

**Potential Improvement:**
Add a `PluginGuard` type for RAII and caching:
```rust
pub struct PluginGuard {
    guard: *mut OpaqueGuard,
    vtable: *const PluginVTable,  // Cached
}

impl PluginGuard {
    /// Returns the cached vtable pointer.
    pub fn vtable(&self) -> *const PluginVTable {
        self.vtable
    }
}

impl Drop for PluginGuard {
    fn drop(&mut self) {
        // SAFETY: guard is valid and will be released
        unsafe {
            polyplug_runtime_plugin_release(self.guard);
        }
    }
}

// In Runtime
impl Runtime {
    pub fn resolve_plugin(&self, handle: PluginHandle) -> Option<PluginGuard> {
        let guard = unsafe { polyplug_runtime_resolve_plugin(self.handle, handle) };
        if guard.is_null() {
            return None;
        }
        // Cache vtable at construction
        let vtable = unsafe { polyplug_runtime_plugin_vtable(guard) };
        Some(PluginGuard { guard, vtable })
    }
}
```

This is optional - the current raw pointer approach is also valid for Rust.

### 4. Codegen Already Optimal

The Rust codegen generates direct vtable dispatch:
```rust
let vtable: &PluginVTable = &*vtable_ptr;
let fn_ptr: *const () = *vtable.functions.add(fn_id);
let dispatch_fn: unsafe extern "C" fn(*const (), *mut ()) -> AbiError = 
    core::mem::transmute(fn_ptr);
dispatch_fn(args_ptr, out_ptr)
```

This is optimal - one pointer load, one indirect call.

**No change needed.**

## Recommended Changes

Since Rust is already the reference implementation and is well-optimized, the changes are minor:

### Optional: Add PluginGuard Wrapper

This would provide:
1. RAII guard management
2. Cached vtable pointer
3. Safe interface

But this is **optional** - the raw pointer approach is idiomatic Rust for FFI.

### Keep Loaders Separate

Rust correctly uses separate adapter crates:
- `polyplug-dotnet`
- `polyplug-python`
- `polyplug-lua`
- `polyplug-js`
- `polyplug-js-deno`

These are **not** part of the host lib. This is correct per PRD.

## Files to Modify (Optional)

1. **host-libs/rust/src/lib.rs**
   - Optionally add `PluginGuard` wrapper

## Decision Required

Should we add `PluginGuard` to Rust host lib?

**Pros:**
- Consistency with C#, Python, JS, Lua
- RAII safety
- Cached vtable

**Cons:**
- Raw pointers are idiomatic Rust for FFI
- Adds abstraction layer
- Not strictly necessary

**Recommendation:** Add `PluginGuard` for consistency and RAII safety.

## Estimated Effort

If adding PluginGuard:
- Implementation: 1 hour
- Testing: 30 minutes

**Total: ~1.5 hours (optional)**

## PRD References

- PRD §8: "polyplug crate (crates.io) — PluginRuntime builder, type-safe ABI wrappers"
- PRD §10: "Plugin developers never write IDs — they write names. Codegen handles the rest."

## Conclusion

The Rust host lib is already optimal. The only recommended change is adding `PluginGuard` for consistency with other languages, but this is optional.