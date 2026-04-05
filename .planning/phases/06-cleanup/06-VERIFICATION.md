---
phase: 06-cleanup
verified: 2026-04-05T12:00:00Z
status: gaps_found
score: 2/4 requirements verified
gaps:
  - truth: "No 'vtable' naming remains in codebase (search: vtable, VTable, VTABLE)"
    status: failed
    reason: "Generator templates still produce vtable-named functions and files"
    breakdown:
      rust_generator:
        - file: "crates/polyplugc/src/generators/rust.rs:1888"
          issue: "Function name uses create_{}_vtable instead of create_{}_interface"
        - file: "crates/polyplugc/src/generators/rust.rs:1892"
          issue: "VM factory function name uses create_{}_vtable_vm"
        - file: "crates/polyplugc/src/generators/rust.rs:2006-2008"
          issue: "NativeDispatch missing function_count field in host interface factory"
        - file: "crates/polyplugc/src/generators/rust.rs:2082"
          issue: "HostContractId should be used instead of raw u64 for contract_id"
      missing_exports:
        - file: "crates/polyplug_abi/src/lib.rs:28"
          issue: "VmDispatch not exported, but generator imports it from polyplug_abi"
      missing_imports_in_generated_code:
        - file: "Generated interface_factories.rs"
          issue: "Missing: AbiErrorCode, abi_error_ok, string_view_from_static"
  - truth: "All tests pass with new instance model and naming"
    status: failed
    reason: "Generated code has type mismatches and missing fields"
    breakdown:
      type_mismatches:
        - issue: "HostContractId vs u64"
          files: ["Generated host interface factories", "Test files"]
        - issue: "AbiErrorCode vs u32"
          files: ["Generated host_callers.rs", "Test files"]
        - issue: "NativeDispatch missing function_count"
          file: "crates/polyplugc/src/generators/rust.rs:2006-2008"
      missing_exports:
        - issue: "VmDispatch not exported from polyplug_abi"
          impact: "Host interface factories fail to compile"
---
# Phase 6: Cleanup Verification Report

**Phase Goal:** Remove all vtable/legacy naming and update to Guest/Host terminology consistently
**Verified:** 2026-04-05T12:00:00Z
**Status:** gaps_found
**Re-verification:** Yes — after gap closure plans 06-05 through 06-09

## Goal Achievement

### Observable Truths (Success Criteria from ROADMAP.md)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | No "vtable" naming remains in codebase | FAILED | Generator function names still use `_vtable` suffix |
| 2 | No *C suffix types in FFI | VERIFIED | RuntimeConfigC intentional, others renamed |
| 3 | Documentation uses Guest/Host terminology | VERIFIED | All docs updated with terminology notes |
| 4 | All tests pass with new naming | FAILED | Build fails with type mismatches, missing fields |

**Score:** 2/4 truths verified

## Root Cause Analysis

### Issue 1: Generator Function Names Still Use "vtable"

**Location:** `crates/polyplugc/src/generators/rust.rs:1888`

```rust
let factory_name: String = format!(
    "create_{}_vtable",  // <-- Should be "create_{}_interface"
    contract.name.replace('.', "_").to_lowercase()
);
```

**Impact:** Generated host interface factories have function names like `create_host_logger_vtable` instead of `create_host_logger_interface`.

**Fix:** Rename to `create_{}_interface` and `create_{}_interface_vm`.

---

### Issue 2: Missing `function_count` in NativeDispatch

**Location:** `crates/polyplugc/src/generators/rust.rs:2006-2008`

```rust
out.push_str("            native: NativeDispatch {\n");
out.push_str("                functions: FUNCTIONS.as_ptr() as *const *const (),\n");
out.push_str("            },\n");  // <-- Missing function_count field!
```

**Compare to working code at line 741:**
```rust
out.push_str("        native: NativeDispatch {\n");
out.push_str(&format!("            function_count: {fn_count}_u32,\n"));
out.push_str(&format!("            functions: {upper}_FNS.as_ptr() as *const *const (),\n"));
out.push_str("        },\n");
```

**Impact:** Generated host interface factories fail to compile with "missing field `function_count`".

**Fix:** Add `function_count` field generation to match the working pattern.

---

### Issue 3: VmDispatch Not Exported

**Location:** `crates/polyplug_abi/src/lib.rs:28`

**Current:**
```rust
pub use dispatch::{DispatchType, DispatchMechanisms, NativeDispatch};
```

**Needed:**
```rust
pub use dispatch::{DispatchType, DispatchMechanisms, NativeDispatch, VmDispatch};
```

**Impact:** Generated code that imports `use polyplug_abi::VmDispatch;` fails to compile.

---

### Issue 4: Missing Imports in Generated Code

**Location:** `crates/polyplugc/src/generators/rust.rs:1864-1872`

**Current imports:**
```rust
out.push_str("use polyplug_abi::HostContractInterface;\n");
out.push_str("use polyplug_abi::HostContractInstance;\n");
out.push_str("use polyplug_abi::DispatchMechanisms;\n");
out.push_str("use polyplug_abi::NativeDispatch;\n");
out.push_str("use polyplug_abi::VmDispatch;\n");
out.push_str("use polyplug_abi::DispatchType;\n");
out.push_str("use polyplug_abi::StringView;\n");
out.push_str("use polyplug_abi::AbiError;\n");
out.push_str("use polyplug_abi::Version;\n");
```

**Missing:**
```rust
out.push_str("use polyplug_abi::AbiErrorCode;\n");
out.push_str("use polyplug_abi::abi_error_ok;\n");
out.push_str("use polyplug_abi::string_view_from_static;\n");
```

---

### Issue 5: HostContractId Type Mismatch

**Location:** `crates/polyplugc/src/generators/rust.rs:2082`

**Current:**
```rust
out.push_str(&format!("        contract_id: 0x{contract_id:016X}_u64,\n"));
```

**Needed:**
```rust
out.push_str(&format!("        contract_id: HostContractId::from(0x{contract_id:016X}_u64),\n"));
```

**Impact:** Type mismatch between `HostContractId` and `u64`.

---

### Issue 6: AbiErrorCode vs u32 in Error Construction

**Location:** `crates/polyplugc/src/generators/rust.rs:2169`

**Current:**
```rust
out.push_str("                code: AbiErrorCode::Panic as u32,\n");
```

**Needed:**
```rust
out.push_str("                code: AbiErrorCode::Panic,\n");
```

**Impact:** `AbiError.code` is `AbiErrorCode`, not `u32`.

---

## Required Fixes Summary

| # | File | Line | Issue | Fix |
|---|------|------|-------|-----|
| 1 | `rust.rs` | 1888 | Function name `create_{}_vtable` | Rename to `create_{}_interface` |
| 2 | `rust.rs` | 1892 | VM function name `create_{}_vtable_vm` | Rename to `create_{}_interface_vm` |
| 3 | `rust.rs` | 2006 | Missing `function_count` field | Add `function_count: {fn_count}_u32,` |
| 4 | `polyplug_abi/src/lib.rs` | 28 | VmDispatch not exported | Add to exports |
| 5 | `rust.rs` | 1864-1872 | Missing imports | Add AbiErrorCode, abi_error_ok, string_view_from_static |
| 6 | `rust.rs` | 2082 | Raw u64 for contract_id | Wrap in HostContractId::from() |
| 7 | `rust.rs` | 2169 | `as u32` cast | Remove cast, AbiError.code is AbiErrorCode |

---

## Requirements Coverage

| Requirement | Description | Status | Evidence |
|-------------|-------------|--------|----------|
| CLN-01 | Remove all "vtable" naming | FAILED | Generator functions still use `_vtable` suffix |
| CLN-02 | Remove *C suffix types | VERIFIED | RuntimeConfigC intentional, others renamed |
| CLN-03 | Update documentation | VERIFIED | All docs updated |
| CLN-04 | Tests pass | FAILED | Build fails with type errors |

---

_Verified: 2026-04-05T12:00:00Z_
_Verifier: Claude (gsd-verifier)_