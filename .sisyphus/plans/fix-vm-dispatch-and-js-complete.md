# Fix VM Dispatch Bug and Complete JS Refactor

## TL;DR

> **Problem 1 - VM Dispatch Bug:**
> - Generated host callers hardcode `DispatchType::Native`
> - JS plugins use `DispatchType::VirtualMachine`
> - Result: SEGFAULT when calling JS from Rust host
>
> **Problem 2 - JS Handwritten Code:**
> - All 5 JS plugins have handwritten vtables with hardcoded values
> - Generated code exists but NOT used
> - Build script copies handwritten files
>
> **Solution:**
> 1. **Fix VM Dispatch** - Propagate `is_native`, generate correct dispatch type
> 2. **Fix All Code Generators** - Rust, C#, C++, Python
> 3. **Refactor JS** - Use generated code only, no handwritten vtables
> 4. **Update Build Script** - Use rolldown for JS
>
> **Estimated Effort:** Very Large (12-16 hours)
> **Critical Path:** Fix codegen → Fix JS → Update build → Test

---

## Problem 1: VM Dispatch Bug

### Root Cause Analysis

**The Bug:**
```rust
// rust.rs line 610 - HARDCODED NATIVE
out.push_str("    dispatch_type: DispatchType::Native,\n");

// Generated host caller - ALWAYS uses native
let fn_ptr: *const () = *vtable.dispatch.native.functions.add(fn_id);
```

**But JS plugins need:**
```rust
// VM dispatch
vtable.dispatch.vm.call(loader_data, fn_id, args, out)
```

**Impact:**
- Calling JS plugins from Rust host = **SEGFAULT**
- All VM-based plugins (JS, Python, Lua) affected
- Only Native plugins (Rust, C++, C#) work

### Files to Fix

| File | Lines | Issue |
|------|-------|-------|
| `crates/polyplug_codegen/src/generators/rust.rs` | 610, 691 | Hardcoded `DispatchType::Native` |
| `crates/polyplug_codegen/src/generators/rust.rs` | 1181-1185 | Host caller uses native only |
| `crates/polyplug_codegen/src/generators/csharp.rs` | 374, 511 | Same bug |
| `crates/polyplug_codegen/src/generators/cpp.rs` | 304, 358 | Same bug |
| `crates/polyplug_codegen/src/generators/python.rs` | N/A | No DispatchType handling |
| `crates/polyplug_codegen/src/parser.rs` | 595 | Has `is_native` but not propagated |

---

## Problem 2: JS Handwritten Code

### Current State

**All 5 JS plugins have handwritten vtables:**

```javascript
// decoder.js - HANDWRITTEN (WRONG)
globalThis.DECODER_VTABLE = {
    contractLo: 0xB0C3DC1E,  // ← HARDCODED!
    contractHi: 0x12F3C106,  // ← HARDCODED!
    contractName: "pipeline.Decoder@1", // ← HARDCODED!
    functions: [decode]
};
```

**Generated code exists but ignored:**
- `generated/guest/init.ts` - Proper polyplug_init
- `generated/guest/contracts.ts` - Proper vtable
- `generated/guest/vtable.ts` - Empty (needs fix)

**Build script copies wrong files:**
```bash
# build_all.sh line 76 - WRONG
cp "$dir/$plugin.js" "$bundle_dir/"  # Copies handwritten!
```

---

## Execution Strategy

### Phase 1: Fix VM Dispatch (Critical)
```
Task 1: Propagate is_native from parser to IR
Task 2: Fix rust.rs to generate correct dispatch_type
Task 3: Fix rust.rs host callers for VM dispatch
Task 4: Fix csharp.rs dispatch
Task 5: Fix cpp.rs dispatch
Task 6: Fix python.rs dispatch
Task 7: Test VM dispatch with JS plugin
```

### Phase 2: Fix JS Generated Code
```
Task 8: Fix generated/vtable.ts (currently empty)
Task 9: Create generated/index.ts for bundling
Task 10: Verify generated contracts.ts has correct exports
```

### Phase 3: Refactor JS Handwritten Code
```
Task 11: Rewrite decoder.js (remove vtable, keep only logic)
Task 12: Rewrite encoder.js
Task 13: Rewrite transformer.js
Task 14: Rewrite reporter.js
Task 15: Rewrite validator.js
```

### Phase 4: Update Build System
```
Task 16: Update build_all.sh to use rolldown for JS
Task 17: Update build to use generated code
Task 18: Test JS build pipeline
```

### Phase 5: Verification
```
Task 19: Build all examples
Task 20: Run pipeline (Rust host calling JS plugins)
Task 21: Verify no segfaults
Task F1: Final verification
```

---

## TODOs

- [x] 1. Propagate is_native from Parser to IR

  **File:** `crates/polyplug_codegen/src/ir.rs`
  
  **Add field to Contract struct:**
  ```rust
  pub struct Contract {
      // ... existing fields ...
      pub is_native: bool,  // NEW
  }
  ```

  **Commit:** `feat(codegen): add is_native to Contract IR`

---

- [x] 2. Fix rust.rs - Generate Correct dispatch_type

  **File:** `crates/polyplug_codegen/src/generators/rust.rs`
  
  **Current (line 610):**
  ```rust
  out.push_str("    dispatch_type: DispatchType::Native,\n");
  ```
  
  **New:**
  ```rust
  let dispatch_type = if contract.is_native {
      "DispatchType::Native"
  } else {
      "DispatchType::VirtualMachine"
  };
  out.push_str(&format!("    dispatch_type: {},\n", dispatch_type));
  ```

  **Also fix line 691** (guest vtable generation)

  **Commit:** `fix(codegen): generate correct dispatch_type in rust`

---

- [x] 3. Fix rust.rs Host Callers for VM Dispatch

  **File:** `crates/polyplug_codegen/src/generators/rust.rs` lines 1181-1185
  
  **Current:**
  ```rust
  let fn_ptr: *const () = *vtable.dispatch.native.functions.add(fn_id);
  let dispatch_fn: unsafe extern "C" fn(...) = core::mem::transmute(fn_ptr);
  dispatch_fn(args_ptr, out_ptr)
  ```
  
  **New:**
  ```rust
  let err: AbiError = unsafe {
      if vtable.dispatch_type == DispatchType::Native {
          let fn_ptr: *const () = *vtable.dispatch.native.functions.add(fn_id);
          let dispatch_fn: unsafe extern "C" fn(...) = core::mem::transmute(fn_ptr);
          dispatch_fn(args_ptr, out_ptr)
      } else {
          vtable.dispatch.vm.call(vtable.dispatch.vm.loader_data, fn_id, args_ptr, out_ptr)
      }
  };
  ```

  **Commit:** `fix(codegen): add VM dispatch support to rust host callers`

---

- [x] 4-6. Fix Other Language Generators

  **csharp.rs lines 374, 511:**
  ```csharp
  // Fix to check is_native
  dispatch_type: contract.is_native ? DispatchType.Native : DispatchType.VirtualMachine,
  ```

  **cpp.rs lines 304, 358:**
  ```cpp
  // Fix to check is_native
  .dispatch_type = contract.is_native ? DispatchType::Native : DispatchType::VirtualMachine,
  ```

  **python.rs:**
  ```python
  # Add dispatch_type handling
  dispatch_type = "DispatchType.Native" if contract.is_native else "DispatchType.VirtualMachine"
  ```

  **Commit:** `fix(codegen): add VM dispatch to csharp, cpp, python`

---

- [x] 7. Test VM Dispatch

  **Test:** Call JS plugin from Rust host
  
  **Assert:** No segfault, returns correct result

  **Commit:** `test(vm): add VM dispatch integration test`

---

- [x] 8. Fix generated/vtable.ts (Currently Empty)

  **File:** `examples/guests/js/decoder/generated/guest/vtable.ts`
  
  **Current:** Empty file
  
  **Should contain:**
  ```typescript
  import { DECODER_VTABLE } from './contracts';
  export { DECODER_VTABLE };
  ```

  **Fix js_quickjs.rs generator** to populate vtable.ts

  **Commit:** `fix(codegen): populate vtable.ts in js generator`

---

- [x] 9. Create generated/index.ts

  **File:** `examples/guests/js/decoder/generated/index.ts`
  
  **Content:**
  ```typescript
  import { setDecoderImpl } from './guest/vtable';
  import { PipelineDecoderContract } from './guest/contracts';
  export { setDecoderImpl, PipelineDecoderContract };
  ```

  **Fix js_quickjs.rs** to generate index.ts

  **Commit:** `feat(codegen): generate index.ts for js`

---

- [x] 10. Verify contracts.ts Exports

  **Check:** `examples/guests/js/decoder/generated/guest/contracts.ts`
  
  **Should export:**
  - `DECODER_VTABLE`
  - `setDecoderImpl`
  - `PipelineDecoderContract`

  **Commit:** (if fixes needed)

---

- [x] 11-15. Rewrite All 5 JS Plugin Files

  **Pattern (like C# Decoder.cs):**
  ```javascript
  // decoder.js - ONLY PLUGIN LOGIC
  export function decode(input) {
      // Use SDK helpers
      const s = toStr(input);
      const decoded = s.replace(/,/g, '|');
      return allocString(`DECODED:${decoded}`);
  }
  ```
  
  **Remove:**
  - Handwritten vtables
  - Hardcoded contract IDs
  - Handwritten polyplug_init
  - Byte-by-byte loops

  **Commit per file:**
  - `refactor(js): rewrite decoder to use generated code`
  - etc.

---

- [x] 16. Update build_all.sh

  **File:** `examples/build_all.sh` lines 75-77
  
  **Current:**
  ```bash
  js-quickjs)
      cp "$dir/$plugin.js" "$bundle_dir/"
      ;;
  ```
  
  **New:**
  ```bash
  js-quickjs)
      # Bundle generated TypeScript with rolldown
      cd "$dir/generated"
      rolldown index.ts --format iife --platform neutral --file "$bundle_dir/$plugin.js"
      ;;
  ```

  **Commit:** `fix(build): use rolldown for js instead of cp`

---

- [x] 17-18. Test Build Pipeline

  **Commands:**
  ```bash
  cd examples
  ./build_all.sh
  ls plugins/js_*  # Should show 5 JS plugins
  ```

  **Commit:** NO

---

- [x] 19-21. Final Verification

  **Build:** All examples compile
  **Run:** `POLYPLUG_PLUGIN_PATH=./plugins ./hosts/rust/target/release/pipeline_host`
  **Assert:** 
  - No segfaults
  - JS plugins work
  - All 5 plugins in output

  **Commit:** NO

---

## Final Verification Checklist

```bash
# 1. VM dispatch works
cargo test --package polyplug -- vm_dispatch

# 2. No hardcoded values in JS
grep -r "contractLo\|contractHi" examples/guests/js/*.js
# Expected: No matches

# 3. JS uses generated code
grep "from.*generated" examples/guests/js/decoder/decoder.js
# Expected: import found

# 4. Build succeeds
cd examples && ./build_all.sh

# 5. Pipeline runs without segfault
POLYPLUG_PLUGIN_PATH=./plugins ./hosts/rust/target/release/pipeline_host
# Expected: Completes successfully

# 6. All 5 JS plugins work
grep "\[decoder\]\|\[encoder\]\|\[transformer\]\|\[reporter\]\|\[validator\]" pipeline_output.txt
# Expected: All 5 found
```

---

## Commit Summary

1. `feat(codegen): add is_native to Contract IR`
2. `fix(codegen): generate correct dispatch_type in rust`
3. `fix(codegen): add VM dispatch support to rust host callers`
4. `fix(codegen): add VM dispatch to csharp`
5. `fix(codegen): add VM dispatch to cpp`
6. `fix(codegen): add VM dispatch to python`
7. `test(vm): add VM dispatch integration test`
8. `fix(codegen): populate vtable.ts in js generator`
9. `feat(codegen): generate index.ts for js`
10. `refactor(js): rewrite decoder to use generated code`
11. `refactor(js): rewrite encoder to use generated code`
12. `refactor(js): rewrite transformer to use generated code`
13. `refactor(js): rewrite reporter to use generated code`
14. `refactor(js): rewrite validator to use generated code`
15. `fix(build): use rolldown for js instead of cp`

---

## Success Criteria

- [x] No segfault when calling JS from Rust
- [x] All 5 JS plugins use generated code
- [x] No handwritten vtables
- [x] No hardcoded contract IDs
- [x] Build succeeds
- [x] Pipeline runs
- [x] All plugins work

**Final State:** VM dispatch fixed, JS fully generated, everything works! ✅
