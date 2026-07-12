# Polyplug ABI Architecture

Polyplug uses a dual-ABI system: both the **host** (runtime) and the **guest**
(plugins) export C functions across the FFI boundary.

For ABI terms (`HostApi`, `GuestContractInterface`, host contract, guest contract),
see the [glossary](glossary.md).

## Plugin ABI (Guest Exports)

Plugins are dynamic libraries (`.so`, `.dll`, `.dylib`) that export two functions:

### `polyplug_abi_version`
```c
uint32_t polyplug_abi_version(void);
```
**Called by:** Host during plugin loading
**Returns:** ABI version (currently `1`)
**Purpose:** Version sentinel to ensure compatibility

### `polyplug_init`
```c
AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx);
```
**Called by:** Host while a bundle load is preparing
**Parameters:**
- `host`: The `HostApi` function table; the plugin registers by calling `host->register_guest_contract(host, &descriptor, &interface)`
- `ctx`: Context containing bundle_id and bundle_path
**Purpose:** Plugin constructor - prepares contracts for the runtime

### BundleInitContext
```c
typedef struct {
    uint64_t   bundle_id;    // Bundle ID for dependency enforcement during init
    StringView bundle_path;  // Canonical bundle directory, or empty for in-memory sources
} BundleInitContext;
```
24 bytes total: `bundle_id` (8) + `bundle_path` (16). The context and its
`StringView` are borrowed for the synchronous `polyplug_init` call only. A guest
that needs the path after init copies it into loader-owned state.

## Host ABI (libpolyplug Exports)

The runtime exports lifecycle functions plus three scoped in-process staging
functions. Normal runtime operations — allocation, load/reload, discovery,
resolution, contract registration, and error handling — remain function-pointer
fields on the `HostApi` returned by `polyplug_runtime_create`.

### Runtime Lifecycle
```c
// Create a new runtime instance. Pass NULL for config to use defaults.
// Returns a HostApi* that exposes all runtime operations.
const HostApi* polyplug_runtime_create(const void* config);

// Destroy a runtime instance. Must be called exactly once per handle returned
// by polyplug_runtime_create. Calling it more than once, or concurrently with
// itself on the same handle, is undefined behavior — the handle is freed, same
// as C free(); the HostApi pointer is dangling afterwards and must not be used.
void polyplug_runtime_destroy(const HostApi* host);
```

### In-Process Registration Staging

```c
void polyplug_begin_in_process_bundle(
    const HostApi* host,
    const uint8_t* manifest_bytes,
    size_t manifest_len,
    uint32_t language,
    uint64_t* out_bundle_id,
    AbiError* out_error);
void polyplug_commit_in_process_bundle(
    const HostApi* host,
    uint64_t bundle_id,
    AbiError* out_error);
void polyplug_abort_in_process_bundle(const HostApi* host, uint64_t bundle_id);
```

`begin` parses and validates the canonical bundle manifest and opens an internal
transaction on the calling thread. Generated adapters register every provider
through the existing `HostApi.register_guest_contract` field using only
`PluginDescriptor` and `GuestContractInterface`; language state stays in
`GuestContractInterface.adapter_context`. `commit` consumes the transaction,
validates the complete provider/function/dependency set, and publishes one
registry snapshot. Registration failures before commit call `abort`; a commit
result already owns cleanup and must not be aborted again.

`config` points at a `RuntimeConfig` (`#[repr(C)]`, **72 bytes, align 8**):
`compatibility: Compatibility` (u32, offset 0), `hot_reload_enabled: bool`
(offset 4), `on_reload` callback (offset 8), `on_reload_user_data` (offset 16),
`log` callback (offset 24), `log_user_data` (offset 32), `log_max_level`
(u32, offset 40), `signature_policy: SignaturePolicy` (u32, offset 44), and
`trusted_keys: Array<Ed25519PublicKey>` (24 bytes, offset 48 — the key-pinning
allowlist; empty = TOFU). The `log` callback —
`fn(user_data, level: u32, scope: StringView, message: StringView)` — receives
every runtime diagnostic at or below `log_max_level` (`LogLevel { Error = 1,
Warn = 2, Info = 3, Debug = 4, Trace = 5 }`); when null, Error/Warn messages go
to stderr and `log_max_level` is ignored. The callback may run on any thread,
must not re-enter the runtime, and the `StringView`s are only valid for the
duration of the call. The by-value `StringView` parameters are deliberate (hot
path, no copies); LuaJIT FFI callbacks cannot receive structs by value, so the
Lua host SDK installs the `polyplug_lua_log_trampoline` exported by the
polyplug_lua loader cdylib as `log` and carries a scalar-callback
`PolyplugLuaLogBridge` in `log_user_data` (see
`crates/polyplug_lua/src/ffi.rs`). The `on_reload` callback —
`fn(user_data, phase: *const ReloadPhase)` — receives a **const pointer** to a
`ReloadPhase` whose `ReloadPhaseType` is one of `Preparing = 0`, `Reloaded = 1`,
`Failed = 2`, or `Unloading = 3` (fired before a bundle is invalidated on unload).
The pointer is always non-null; the pointee (and the `StringView`s inside it) is
valid only for the duration of the call — copy to retain. `reason` is the null
view unless `phase_type == Failed`.

### Cross-Boundary Allocator (via HostApi fields)
```c
// Allocate memory that crosses the plugin/host boundary.
// Returns NULL for size == 0 or invalid alignment.
uint8_t* host->alloc(const HostApi* host, size_t size, size_t align);

// Free memory previously allocated by host->alloc.
// Must pass the SAME size and align used for the allocation.
void host->free(const HostApi* host, uint8_t* ptr, size_t size, size_t align);
```

### All Other Operations (via HostApi fields)

`polyplug_runtime_create` returns a pointer to `HostApi`, a `184`-byte
`#[repr(C)]` struct: one opaque runtime pointer, 21 function-pointer fields, and
a trailing reserved pointer. `unload_bundle` is at offset 136, `log` at offset
144, `create_guest_instance` at offset 152, `destroy_guest_instance` at offset
160, `registry_revision` at offset 168, and `reserved` at offset 176 (producers
set it to null; consumers must not read it). Host applications
and plugins call these fields using the self-passing pattern, e.g.
`host->load_bundle(host, path, path_len)`.

`registry_revision(host)` returns the current runtime-wide registry revision as
`u64`. The runtime performs its acquire atomic load before returning that value,
so callers in every supported host language receive the same synchronization
without observing Rust atomic storage through a foreign pointer.
The fields cover bundle lifecycle (`load_bundle`, `reload_bundle`, `unload_bundle`),
contract discovery (`find_guest_contract`, `find_all_guest_contracts`,
`resolve_guest_contract`), instance lifecycle (`create_guest_instance`,
`destroy_guest_instance`),
registration (`register_guest_contract`, `register_host_contract`,
`register_loader`), and error handling (`get_last_error`, `get_error_len`),
among others.

## Execution Flow

```
Host Application
    │
    ▼
polyplug_runtime_create() ──► HostApi* (Runtime Instance)
    │
    ▼
host->load_bundle(host, path, len)
    │
    ├── dlopen(plugin.so)
    ├── dlsym(polyplug_abi_version) ──► Check version
    ├── dlsym(polyplug_init)
    │
    ▼
Call: polyplug_init(host, ctx)
    │
    ├── Plugin builds interfaces
    ├── Plugin calls host->register_guest_contract(host, &descriptor, &interface)
    └── Interfaces stored in RuntimeStore
    │
    ▼
host->find_guest_contract(host, contract_id, ver) ──► Get handle
    │
    ▼
host->resolve_guest_contract(host, handle) ──► Get interface
    │
    ▼
Call plugin functions via interface
    │
    ▼
polyplug_runtime_destroy(host)
```

## ABI Stability

The core ABI freezes at v1.0 per §7 of CLAUDE.md. The project is currently pre-1.0
(no public release yet), so ABI-visible changes are still permitted with explicit
owner approval. At and after v1.0:
- `HostApi` layout cannot change
- `BundleInitContext` layout cannot change (no field additions or removals)
- `polyplug_init` signature is fixed (2 params)
- All additions go through the host/guest contract model

## Forward Compatibility

New functionality should use host contract interfaces resolved via
`HostApi.get_host_contract`. The trailing `reserved: *const c_void` pointer
(offset 176) is the only sanctioned post-freeze expansion slot; producers set
it to null, consumers must not read it.

## ABI Conformance Testing

Every ABI function pointer has one canonical signature defined in `polyplug_abi`.
Each language generator must reproduce that signature in its generated/loader
code. **A signature that drifts from the canonical type is silent
calling-convention UB** — and the most dangerous case is drift in *generated
non-Rust text*, because the Rust toolchain never compiles it, so
`cargo build`/`clippy`/`test` cannot see the mismatch. (This bug class actually
occurred: a stale LuaJIT `ffi.cdef` declared a by-value `AbiError` return after
the ABI moved to the out-param convention.)

Who catches drift, per language:

| Language | How its generated ABI signatures are obtained | What checks drift |
|---|---|---|
| rust | typed fn assigned to a typed field | `rustc` at build |
| cpp | C++ fn vs the regenerated mirror header | `g++` when the plugin/host compiles |
| csharp | delegate / `UnmanagedCallersOnly` vs the mirror | C# compiler at build |
| python | `ctypes.cast(..., type(field))` — derived from the mirror | by construction (cannot drift) |
| js | loader installs a **typed Rust** fn into the field | `rustc` |
| **lua** | **`ffi.cdef` literal C text** (LuaJIT requires it) | **nothing at compile time** |

LuaJIT is the only surface no compiler can check, so the defense is split into
two layers that **both run in CI and locally**:

1. **Floor — toolchain-free `cargo test`.** A structural test
   (`lua_host_trampoline_cdefs_are_out_param_abi`) regenerates the lua host
   factory and asserts the trampoline cdefs equal the real signatures in
   `crates/polyplug_lua/src/ffi.rs` (void return + trailing out-pointer), and
   forbids the by-value forms. It needs nothing but `cargo` — no luajit, no
   toolchain versions, identical result on every machine and in CI. This is the
   layer that makes the drift class unmergeable everywhere.

2. **Ceiling — `just verify-abi` execution.** Loads a bundle and dispatches
   through every installed ABI fn-pointer (including a guest calling *back into*
   a VM-language host contract, which fires `polyplug_lua_host_vm_dispatch`) and
   asserts the returned value/`AbiError`. CI invokes it with `--require-all`
   (a missing toolchain is a hard failure, so coverage is guaranteed); locally
   the same recipe runs whatever is installed and loudly skips the rest.
   `just setup-toolchains` installs current stable for full local runs — there
   is no pinned environment (no nix/devcontainer); the floor is version-
   independent and the ceiling targets latest. As of 2026-06: .NET SDK 10.x,
   Python 3.14.x, Deno 2.8.x, GCC 15.x, QuickJS-ng 0.15.x, LuaJIT `v2.1` tip.

### Canonical ABI struct shapes

Beyond function-pointer signatures, the validator also pins the canonical ABI
**structs** — `StringView`, `AbiError`, `Version`, `Array`, `Buffer`,
`ArenaOverflowBlock`, `CallArena`, `GuestContractInstance`, and
`BundleInitContext` — across every language mirror. For each it enforces the
golden field-name set *and* the golden declaration order (the proxy for ABI
layout), keyed off `checks/sdk_validator.yaml`'s `structs:` / `struct_targets:`
sections. Each mirror's native field spelling (C# PascalCase, etc.) is
normalized back to snake before comparison, so the check is convention-agnostic.
As with enums, the rust ABI source is listed as a target for each struct, so
the yaml golden set cannot silently disagree with the real types. Pinning the
`AbiError` shape (`{ code, message }`) is the validator's enforcement of the
out-param ABI's type foundation: a mirror cannot quietly change `AbiError`'s
layout. The out-param *convention* on the function pointers themselves remains
runtime-proven by the lua-cdef floor and the `just verify-abi` ceiling above.

### Built-in-type marshaling — behavioral proof

The validator pins struct *shapes* statically; the built-in-type *marshaling*
(the generated code that reads and writes those types across the boundary) is
proven byte-correct at runtime by `crates/polyplugc/tests/generate_e2e_array.rs`.
Each test generates guest glue for a contract, then compiles and runs a driver
under the real language toolchain (rust/cpp/lua/js/python) and asserts the bytes
that land in memory:

- **Kitchen sink** — `Array<Kitchen>` where `Kitchen` carries one field of every
  primitive width/signedness/float (`bool, u8, i16, i32, u64, f64, f32`) plus two
  embedded `StringView`s. Every language marshals the *same* boundary values —
  `u8::MAX`, `i16`/`i32` MIN & MAX, `u64::MAX`, a unicode string, an **empty**
  string — so every distinct field-writer branch and the arena realign/stride
  math is exercised, not just the `u32 + StringView` shape. A companion `empty()`
  return proves the `len == 0` path (no element alloc, no string loop).
- **Buffer regression guard** — a contract with `Buffer` (an *owning*, non-`Copy`
  type) as a struct field and as a standalone param must generate rust glue that
  compiles: it locks in that such structs/arg-packs do **not** derive `Copy`,
  `use polyplug_abi::Buffer`, and unpack the POD pack by value. (`Buffer` is not
  a valid `Array<T>` element precisely because `alloc_array` requires `T: Copy`.)

The golden helper set in `checks/sdk_validator.yaml` covers `StringView`
(`to_str`/`starts_with`/…) and the contract-ID hashers; `Buffer`/`Array` are
pinned as struct shapes only (no ergonomic helper method exists in any language,
so there is none to validate), while their marshaling correctness is covered by
the e2e matrix above.

Import hygiene across the workspace (no inline fully-qualified paths at use-sites)
is enforced by a separate guard — `just verify-no-fq-paths` / `checks/no_inline_fq_paths.yaml`
— also in the SDK Consistency CI job. See `docs/WORKFLOW.md` § "Import hygiene".
