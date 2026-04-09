# Phase 12 Verification: SDK Type Imports

## Overview

This document verifies that Rust SDKs import types from `polyplug_abi` without duplicate definitions, satisfying requirement SDK-01.

---

## SDK-01: Rust SDKs use polyplug_abi types without duplicates

**Status:** VERIFIED

**Evidence:**

### Guest SDK Type Imports

The Rust guest SDK (`sdks/rust/guest/src/lib.rs`) imports all ABI types from `polyplug_abi`:

```
$ grep -c "pub use polyplug_abi::" sdks/rust/guest/src/lib.rs
25
```

**Key type re-exports (25 total):**

| Type | Source | Line |
|------|--------|------|
| `POLYPLUG_ABI_VERSION` | `polyplug_abi::POLYPLUG_ABI_VERSION` | 92 |
| `AbiErrorCode` | `polyplug_abi::AbiErrorCode` | 95 |
| `abi_error_ok` | `polyplug_abi::abi_error_ok` | 121 |
| `string_view_null` | `polyplug_abi::string_view_null` | 124 |
| `string_view_from_static` | `polyplug_abi::string_view_from_static` | 127 |
| `StringView` | `polyplug_abi::types::StringView` | 140 |
| `Buffer` | `polyplug_abi::types::Buffer` | 146 |
| `AbiError` | `polyplug_abi::types::AbiError` | 153 |
| `Version` | `polyplug_abi::types::Version` | 156 |
| `GuestContractInstance` | `polyplug_abi::guest::GuestContractInstance` | 162 |
| `GuestContractHandle` | `polyplug_abi::GuestContractHandle` | 165 |
| `GuestContractInterface` | `polyplug_abi::GuestContractInterface` | 170 |
| `DispatchType` | `polyplug_abi::dispatch::dispatch_type::DispatchType` | 176 |
| `NativeDispatch` | `polyplug_abi::dispatch::native_dispatch::NativeDispatch` | 179 |
| `VmDispatch` | `polyplug_abi::dispatch::vm_dispatch::VmDispatch` | 182 |
| `DispatchMechanisms` | `polyplug_abi::dispatch::dispatch_mechanisms::DispatchMechanisms` | 185 |
| `HostContractInterface` | `polyplug_abi::HostContractInterface` | 203 |
| `HostContractInstance` | `polyplug_abi::HostContractInstance` | 206 |
| `HostInterface` | `polyplug_abi::HostInterface` | 212 |
| `PluginDescriptor` | `polyplug_abi::PluginDescriptor` | 218 |
| `PluginContext` | `polyplug_abi::PluginContext` | 224 |
| `BundleId` | `polyplug_utils::BundleId` | 229 |
| `GuestContractId` | `polyplug_utils::GuestContractId` | 232 |
| `HostContractId` | `polyplug_utils::HostContractId` | 235 |
| `polyplug_host_alloc` | `polyplug_abi::ffi::polyplug_host_alloc` | 239 |
| `polyplug_host_free` | `polyplug_abi::ffi::polyplug_host_free` | 241 |

### No Duplicate Type Definitions

```
$ grep "struct StringView" sdks/rust/guest/src/lib.rs
(no matches)
```

The guest SDK defines only SDK-specific types:
- `HostVtablePtr` - wrapper for host interface pointer (line 72)
- `FnPtr` - wrapper for function pointers in static vtables (line 258)
- `PluginError` - guest-side error type (line 271)

These are **not** ABI types and are correctly defined in the guest SDK.

### Import Chain Verification

```
sdks/rust/guest/src/lib.rs
  └── pub use polyplug_abi::* (25 imports)
        └── crates/polyplug_abi/src/lib.rs (source of truth)
              └── pub use types::{StringView, Buffer, AbiError, ...}
              └── pub use guest::{GuestContractInterface, ...}
              └── pub use host::{HostContractInterface, ...}
              └── pub use plugin::{GuestContractHandle, ...}
```

---

## Host SDK Type Usage

**Status:** VERIFIED

The Rust host SDK (`sdks/rust/host/src/lib.rs`) is intentionally minimal:

```rust
pub mod manifest;
pub mod scanner;
```

No type definitions - the host SDK provides manifest parsing and bundle scanning utilities only. Host applications that need ABI types import directly from `polyplug` crate.

### Core Crate Re-exports

The `polyplug` crate (`crates/polyplug/src/lib.rs`) re-exports from `polyplug_abi`:

```rust
pub use polyplug_abi::runtime::{RuntimeConfig, Compatibility};
```

**Verification:**

```
$ grep "RuntimeConfig" crates/polyplug/src/lib.rs
pub use polyplug_abi::runtime::{RuntimeConfig, Compatibility};
```

**Import chain for host applications:**

```
host application
  └── use polyplug::{Runtime, RuntimeConfig, Compatibility}
        └── crates/polyplug/src/lib.rs
              └── pub use polyplug_abi::runtime::{RuntimeConfig, Compatibility}
                    └── crates/polyplug_abi/src/lib.rs (source of truth)
```

---

## Conclusion

**SDK-01 SATISFIED**

- Rust guest SDK imports 25 types from `polyplug_abi` with no duplicates
- Rust host SDK is minimal (no type definitions needed)
- Core `polyplug` crate re-exports `RuntimeConfig` and `Compatibility` from `polyplug_abi`
- All FFI types have a single source of truth in `polyplug_abi`

---

*Verified: 2026-04-08*
*Phase: 12-sdk-instance-model*
*Plan: 01*