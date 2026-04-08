# Phase 12: SDK Instance Model Completion - Research

**Researched:** 2026-04-08
**Domain:** SDK architecture, codegen patterns, instance model
**Confidence:** HIGH

## Summary

Phase 12 addresses three remaining SDK requirements from the v1.1 milestone: SDK-01 (Rust host SDK polyplug_abi imports), SDK-05 (JS SDK TypeScript interfaces), and SDK-07 (instance-based wrappers via codegen for all SDKs).

The research reveals that the Rust host SDK already re-exports from `polyplug` (which re-exports from `polyplug_abi`) but has minimal surface area. The JS SDK has auto-generated TypeScript interfaces in `abi/polyplug_abi.ts` but these are separate from the runtime host library. The codegen already produces instance-based wrappers for Rust hosts, but other language SDKs need similar patterns.

**Primary recommendation:** Focus on (1) verifying Rust host SDK imports are complete, (2) ensuring JS SDK TypeScript types are exported from the main module, and (3) extending instance wrapper codegen to all languages.

## User Constraints (from CONTEXT.md)

No CONTEXT.md exists for this phase. Research is unconstrained.

## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| SDK-01 | Update Rust host SDK to use `polyplug_abi` types | Rust host SDK re-exports from `polyplug` which re-exports from `polyplug_abi` - verification needed |
| SDK-05 | Update JS SDK - use types from `polyplug_abi` | JS SDK has `abi/polyplug_abi.ts` with TypeScript interfaces - export integration needed |
| SDK-07 | Add instance-based wrappers to all SDKs (codegen) | Rust codegen has instance wrappers; other languages have stubs - extension needed |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| polyplug_abi | workspace | ABI types | Canonical source of truth for FFI types |
| polyplugc | workspace | Code generator | Generates host/guest bindings for all languages |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| polyplug_codegen | workspace | Codegen library | Shared code generation primitives |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| polyplug_abi types | Duplicate definitions in each SDK | Duplicate definitions cause maintenance burden and potential ABI mismatches |
| Generated instance wrappers | Manual wrapper code | Manual code is error-prone and inconsistent across languages |

**Installation:**
N/A - workspace crates

**Version verification:** Workspace-managed versions.

## Architecture Patterns

### Current SDK Structure

```
sdks/
├── rust/
│   ├── host/src/          # Re-exports from polyplug crate
│   └── guest/src/         # Re-exports from polyplug_abi
├── python/
│   ├── host/polyplug/     # Runtime wrapper
│   ├── polyplug_abi/      # ABI types (ctypes structs)
│   └── guest/             # Guest library
├── csharp/
│   ├── abi/               # AbiConstants class
│   ├── host/              # Runtime class
│   └── guest/             # Guest library
├── lua/
│   ├── abi/               # FFI cdef types
│   ├── host/              # Runtime wrapper
│   └── guest/             # Guest library
├── js/
│   ├── abi/               # TypeScript interfaces
│   ├── host/              # Deno FFI runtime wrapper
│   └── guest/             # Guest library
└── cpp/
    ├── abi/               # Header-only ABI types
    ├── host/              # Runtime wrapper
    └── guest/             # Guest library
```

### Pattern 1: Instance Wrapper (Rust Codegen)

**What:** RAII wrapper that holds `interface`, `instance`, and `host` pointers; calls `create_instance` on construction, `destroy_instance` on drop.

**When to use:** Host-side callers that need to invoke methods on guest contracts.

**Example:**
```rust
// Source: crates/polyplugc/src/generators/rust.rs:1290-1370
/// RAII wrapper that manages instance lifecycle:
/// - `new()`: calls `create_instance` on the resolved interface
/// - `drop()`: calls `destroy_instance` to clean up
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
}

impl Drop for XxxContract {
    fn drop(&mut self) {
        if !self.instance.data.is_null() {
            unsafe { ((*self.interface).destroy_instance)(self.host, self.instance); }
        }
    }
}
```

### Pattern 2: polyplug_abi Type Re-export

**What:** SDK modules re-export types from `polyplug_abi` rather than defining duplicates.

**When to use:** All SDK host/guest libraries that need ABI types.

**Example:**
```rust
// sdks/rust/guest/src/lib.rs
pub use polyplug_abi::GuestContractInterface;
pub use polyplug_abi::GuestContractInstance;
pub use polyplug_abi::HostInterface;
```

### Anti-Patterns to Avoid

- **Duplicate type definitions:** Each SDK should not define its own `RuntimeConfig`, `StringView`, etc. - import from polyplug_abi
- **Inconsistent naming:** Types should match `polyplug_abi` naming exactly (not `RuntimeConfigC`, `PluginInterface` vs `GuestContractInterface`)

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Instance wrapper | Manual RAII class | polyplugc codegen | Ensures consistent create/destroy lifecycle across all languages |
| ABI type definitions | Custom ctypes/FFI structs | polyplug_abi exports | Guarantees ABI layout matches Rust |

**Key insight:** The codegen already handles instance wrappers for Rust. The gap is extending this pattern to other languages and ensuring TypeScript types are exported from the JS SDK main module.

## Common Pitfalls

### Pitfall 1: Missing Type Re-exports

**What goes wrong:** SDK defines its own type with same name but different layout than `polyplug_abi`.

**Why it happens:** Copy-paste during initial SDK implementation, not updating when polyplug_abi changes.

**How to avoid:** Always re-export from polyplug_abi; verify with size/layout tests.

**Warning signs:** FFI calls crash with memory corruption; tests show size mismatches.

### Pitfall 2: Instance Wrapper Without Drop

**What goes wrong:** Wrapper creates instance but doesn't call `destroy_instance` on cleanup.

**Why it happens:** Language doesn't have RAII destructors (JS, Python), developer forgets to call cleanup.

**How to avoid:** Provide explicit cleanup method; document hot-reload safety contract.

**Warning signs:** Memory leaks; hot-reload fails with "instances remaining" warning.

### Pitfall 3: TypeScript Types Not Exported

**What goes wrong:** TypeScript interfaces exist in `abi/polyplug_abi.ts` but aren't accessible from `import { ... } from "@polyplug/runtime"`.

**Why it happens:** Module structure doesn't re-export from main entry point.

**How to avoid:** Ensure `mod.ts` exports all relevant types from `abi/polyplug_abi.ts`.

**Warning signs:** TypeScript errors about missing types; users import from internal paths.

## Code Examples

### polyplug_abi Type Inventory

```rust
// crates/polyplug_abi/src/lib.rs - exports available for SDK use
pub use runtime::{Compatibility, RuntimeConfig, ReloadPhaseData, ReloadPhaseType};
pub use types::{AbiError, AbiErrorCode, StringView, Version, Buffer, Array, DependencyInfo};
pub use dispatch::{DispatchType, DispatchMechanisms, NativeDispatch, VmDispatch, VmLoaderData};
pub use guest::{GuestContractInterface, GuestContractInstance};
pub use host::{HostContractInterface, HostContractInstance, HostInterface, RuntimeInterface};
pub use plugin::{PluginHandle, PluginDescriptor, PluginContext};
pub use polyplug_utils::{GuestContractId, HostContractId};
```

### Current JS SDK Module Exports

```typescript
// sdks/js/mod.ts
export * from "./abi/polyplug_abi.ts";  // Exports all TypeScript interfaces
export { Runtime, openPolyplug, runtimeNew, onReload, setConfig, ... } from "./host/mod.js";
export { ReloadPhase } from "./host/polyplug/reload_phase.js";
```

The JS SDK already exports TypeScript types from `abi/polyplug_abi.ts`. The types include:
- `StringView`, `Buffer`, `AbiError`, `PluginHandle`, `HostContext`
- `NativeDispatch`, `VmDispatch`, `PluginInterface` (should be `GuestContractInterface`)
- `HostVTable` (should be `RuntimeAbi` or `HostInterface`)
- `PluginContext`, `RuntimeConfig`

### Instance Wrapper C++ Codegen

```cpp
// crates/polyplugc/src/generators/cpp.rs:338-459
// Generates create_instance and destroy_instance stubs
static GuestContractInstance Xxx_create_instance_stub(const HostInterface* host, const void* args) noexcept {
    return GuestContractInstance{ .data = nullptr, .contract_id = 0 };
}
static void Xxx_destroy_instance_stub(const HostInterface* host, GuestContractInstance instance) noexcept {
    // No-op for stateless plugins
}
```

Note: C++ codegen has stubs but not full RAII wrapper generation like Rust.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `PluginInterface` naming | `GuestContractInterface` | Phase 1 | Clearer host/guest distinction |
| `HostVTable` naming | `HostInterface` (guest) / `RuntimeInterface` (host) | Phase 11 | Self-passing pattern |
| `RuntimeConfigC` suffix | `RuntimeConfig` | Phase 10 | Matches polyplug_abi exactly |
| Manual instance management | Generated RAII wrappers | Phase 3 | Safer lifecycle management |

**Deprecated/outdated:**
- `PluginInterface`: Use `GuestContractInterface`
- `HostVTable`: Use `HostInterface` (for guest parameter) or `RuntimeInterface` (for host implementation)
- `RuntimeConfigC`: Use `RuntimeConfig`

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | JS SDK TypeScript types are sufficient for SDK-05 | SDK-05 | Types may need updating to match current polyplug_abi naming |
| A2 | Instance wrapper generation for non-Rust languages is the main gap for SDK-07 | SDK-07 | Other gaps may exist in codegen coverage |

**If this table is empty:** All claims in this research were verified or cited.

## Open Questions (RESOLVED)

1. **JS SDK TypeScript type naming** — RESOLVED: Update TypeScript types to match polyplug_abi naming (GuestContractInterface, HostInterface, RuntimeInterface)

2. **Instance wrapper generation scope** — RESOLVED: Generate instance wrappers for all languages with RAII patterns where applicable (Python __del__, C# IDisposable, Lua __gc, JS class with destroy method, C++ destructor)

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust 1.85+ | Codegen, SDKs | ✓ | 1.85 | — |
| Deno 1.38+ | JS SDK | ✓ | — | — |
| Python 3.10+ | Python SDK | ✓ | — | — |
| .NET 10.0 | C# SDK | ✓ | — | — |
| LuaJIT | Lua SDK | ✓ | — | — |

**Missing dependencies with no fallback:**
- None identified

**Missing dependencies with fallback:**
- N/A

## Validation Architecture

### Test Framework

| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` + cargo test |
| Config file | Cargo.toml workspace |
| Quick run command | `cargo test --workspace --lib` |
| Full suite command | `cargo test --workspace` |

### Phase Requirements → Test Map

| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| SDK-01 | Rust host SDK imports from polyplug_abi | unit | `cargo test -p polyplug_sdk_host` | ✅ |
| SDK-05 | JS SDK exports TypeScript types | compile | `deno check sdks/js/mod.ts` | ✅ |
| SDK-07 | Codegen produces instance wrappers | unit | `cargo test -p polyplugc` | ✅ |

### Sampling Rate

- **Per task commit:** `cargo test --workspace --lib`
- **Per wave merge:** `cargo test --workspace`
- **Phase gate:** Full suite green before `/gsd-verify-work`

### Wave 0 Gaps

- [ ] TypeScript type verification - `deno check` for JS SDK
- [ ] Instance wrapper codegen tests for all languages

*(If no gaps: "None - existing test infrastructure covers all phase requirements")*

## Security Domain

> `security_enforcement` not explicitly set in config.json - section included.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|------------------|
| V2 Authentication | no | N/A |
| V3 Session Management | no | N/A |
| V4 Access Control | no | N/A |
| V5 Input Validation | yes | FFI boundary validates pointers |
| V6 Cryptography | no | N/A |

### Known Threat Patterns for SDK FFI

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Null pointer dereference | Tampering | Null checks at FFI boundary |
| Buffer overflow | Tampering | Size validation on arrays/buffers |
| Type confusion | Tampering | `#[repr(C)]` guarantees layout |

## Sources

### Primary (HIGH confidence)

- `crates/polyplug_abi/src/lib.rs` - Type exports
- `crates/polyplugc/src/generators/rust.rs:1290-1370` - Instance wrapper codegen pattern
- `sdks/js/mod.ts` - JS SDK module structure
- `sdks/rust/guest/src/lib.rs` - Rust guest SDK type re-exports

### Secondary (MEDIUM confidence)

- `.planning/phases/05-sdk-updates/05-VERIFICATION.md` - Phase 5 verification with gaps
- `.planning/phases/10-sdk-cleanup-completion/10-VERIFICATION.md` - Phase 10 verification (SDK-02, SDK-03, SDK-04, SDK-06 complete)

### Tertiary (LOW confidence)

- None

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - workspace crates, verified by reading source
- Architecture: HIGH - documented in code and verification files
- Pitfalls: HIGH - based on codebase analysis and previous phase gaps

**Research date:** 2026-04-08
**Valid until:** 30 days (stable architecture)