# polyplug

## What This Is

A high-performance, zero/minimal-overhead cross-language plugin runtime for Rust. Enables host applications to load plugins written in Rust, Python, C#, Lua, JavaScript, or C++ through a unified FFI-based interface with hot-reload support.

## Core Value

**The core runtime is loader-agnostic** — the `polyplug` crate knows about the `BundleLoader` trait and `PluginRegistry`, but NOT about `libloading`, `dlopen`, or any specific loader implementation.

This enables:
- Single API for all supported languages
- Direct function pointer dispatch (zero overhead for native, minimal for VM-based)
- Hot-reload with callback-based instance safety
- Type-safe code generation for all bindings
- Cross-platform support (Linux, macOS, Windows)

## Constraints

| Constraint | Reason |
|------------|--------|
| Architecture | Core crate must have zero loader-specific code or dependencies |
| Safety | Host must destroy all instances before hot-reload completes |
| Compatibility | Breaking changes acceptable — not published yet |
| FFI | All public ABI structs are `#[repr(C)]` |
| Pointers | Raw pointers only at FFI boundary, not in internal Rust code |
| Type Source | No `*C` suffix types — all FFI types defined once in `polyplug_abi` |

## Requirements

### Validated (implemented)

- Instance-based plugin model (host creates/owns instances via factory pattern)
- Callback-based hot-reload (host destroys instances before interface swap)
- Cross-dispatch `call_method` for plugin-plugin communication
- Guest/Host contract separation (renamed from Plugin/Host)
- Singleton and multi-instance host contracts
- All FFI types consolidated in `polyplug_abi`
- Simplified registry (no VTableSlot wrapper, no generation counter)
- All five SDKs updated to use `polyplug_abi` types
- Auto-generated SDK bindings via build script codegen (Phase 19)
- Helper methods embedded inline in codegen for rebuild persistence (Phase 19)

### Out of Scope

- Manifest parsing in core runtime (stays for now, may move later)
- Bundle signing/verification
- Async plugin dispatch
- Lazy bundle loading

## Key Decisions

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-04-03 | Remove "vtable" naming | Confusing terminology; use `GuestContractInterface` |
| 2026-04-03 | Remove VTableSlot wrapper | Unnecessary indirection; registry stores interfaces directly |
| 2026-04-03 | Instance-based model | Host creates/owns instances, not "guards" |
| 2026-04-03 | `create/destroy_instance` in interfaces | Contract-specific factory pattern |
| 2026-04-03 | Instance as first dispatch arg | Consistent for native and VM dispatch |
| 2026-04-03 | Hot-reload via callback | Host destroys instances; no Arc quiescence pattern |
| 2026-04-03 | Rename Plugin Contract → Guest Contract | Clear Host/Guest separation |
| 2026-04-03 | `RuntimeAbi` naming | Clearer than `HostInterface` (host != runtime) |
| 2026-04-03 | All public ABI structs `#[repr(C)]` | Single source of truth, no `*C` types |
| 2026-04-03 | Host contracts: singleton or multi-instance | Flexibility for host-provided services |
| 2026-04-03 | `ContractHandle` without generation | Instances destroyed before hot-reload |
| 2026-04-03 | `PluginContext` init-time only | Two-context model: `rt_ctx` always, `PluginContext` during init |
| 2026-04-03 | `call_method` for cross-dispatch | Plugin-plugin across different dispatch types |
| 2026-04-03 | Opaque instance handles | Type-safe handles, not bare pointers |
| 2026-04-03 | `guest_contract:` hash prefix | Consistent naming with Guest/Host terminology |

---
*Last updated: 2026-04-17 (v1.1 milestone shipped)*