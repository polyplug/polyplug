# CLAUDE.md — polyplug

This file defines the **mandatory, non-negotiable rules** for every agent, contributor, and AI assistant working on this codebase. No exceptions. No shortcuts. No "just this once."

If you are unsure whether something violates a rule — it probably does. Ask first.

---

## Project Overview

polyplug is a universal, blazing-fast, cross-language plugin runtime platform written in Rust. A host application loads plugin bundles at runtime; each bundle exports one or more guest contracts that the host discovers and calls through a frozen C ABI. Plugins can be written in any language (Rust, C++, C#, Python, Lua, JavaScript) — the `polyplugc` CLI generates the language-specific glue code from a `.toml` contract definition.

**Trust model**: See `docs/TRUST_MODEL.md` for bundle identity, declared dependencies, and ABI freeze details.

**Do NOT timeout tasks.** Wait for them to complete; the system will notify you. Do NOT poll.

---

## Architecture

### Crates

| Crate | Purpose |
|---|---|
| `polyplug` | Core runtime: `Runtime`, `RuntimeStore`, loader, reload, FFI entry points |
| `polyplug_abi` | Frozen ABI types: `HostApi`, `BundleInitContext`, `GuestContractInterface`, `AbiError`, etc. |
| `polyplug_utils` | Shared hash utilities (`fnv1a_64`, `bundle_id`, `contract_id`) |
| `polyplug_common` | Shared schema TYPES + their pure behaviour (`manifest.toml`: `ManifestData`, `ManifestError`, `parse_from_str`/`validate`). NO I/O, logging, or orchestration. Lets `polyplugc` accept what the runtime accepts without depending on the runtime |
| `polyplug_native` | Loader for native (`cdylib`) bundles — supports hot-reload (as do the Lua and JS loaders; Python and .NET do not) |
| `polyplug_python` | Loader for Python bundles |
| `polyplug_lua` | Loader for Lua bundles |
| `polyplug_js` | Loader for JavaScript (QuickJS) bundles |
| `polyplug_dotnet` | Loader for .NET/C# bundles |
| `polyplug_guest` | Guest-side Rust helper (links into plugin dylibs); lives at `sdks/rust/guest`, not under `crates/` |
| `polyplug_codegen` | ABI-SDK code-generation library; its `languages/` emitters are driven by `polyplug_abi`'s build script. `polyplugc` consumes only its shared `data`/`error`/config types |
| `polyplugc` | CLI tool: parses contract `.toml`, generates host/guest bindings |
| `sdk_validator` | Validates SDK correctness against the ABI |

### FFI Surface (2 exports only)

The host-side Rust library (`libpolyplug`) exposes exactly **two** `#[no_mangle]` C symbols:

```c
void* polyplug_runtime_create(const void* config);   // returns HostApi pointer
void  polyplug_runtime_destroy(void* host);
```

All other operations go through **`HostApi` struct fields** (function pointers). `HostApi` is `184 bytes` (1 opaque runtime pointer + 21 function pointer fields + 1 trailing `reserved` data pointer: `unload_bundle` at offset 136, `log` at offset 144, `create_guest_instance` at offset 152, `destroy_guest_instance` at offset 160, `revision_counter` at offset 168, and `reserved` at offset 176), `align = 8`. `create_guest_instance` / `destroy_guest_instance` are host-mediated so the runtime can pin the epoch across construction/destruction and attribute each live instance to its contract. `revision_counter` returns a pointer to the runtime's registry revision counter (a `u64` bumped on every load/reload/unload); generated host/peer callers cache the resolved interface and poll this counter with one acquire load before each dispatch, re-resolving when it changes so a cached interface pointer never dangles after a reload/unload. The `reserved` pointer carries no meaning — producers set it to null, consumers must not read it; it exists only to keep the frozen struct size expandable later. Cross-boundary allocation flows through the `alloc` / `free` fields on `HostApi` — there are no separate `polyplug_host_alloc` / `polyplug_host_free` exports.

### `polyplug_init` — the plugin entry point

Every plugin bundle must export this 2-argument function:

```c
// All generators must produce this signature (language-specific syntax):
AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
```

- `host` — the function table; plugins call `host->register_guest_contract(host, &descriptor, &interface)` to register
- `ctx` — 24-byte struct: `{ bundle_id: u64, bundle_path: StringView }`

The 3-argument form `fn(rt_ctx, host, ctx)` is **gone** — do not use it.

### Code Generators (`polyplug_codegen`)

There are **two separate codegen pipelines** — they share no language emitters by design:

- **ABI-SDK emitters** live in `crates/polyplug_codegen/src/languages/`:
  `cpp.rs`, `csharp.rs`, `python.rs`, `lua.rs`, `js.rs` (no Rust emitter — the Rust ABI
  sources are the truth themselves). They are driven at
  build time by `crates/polyplug_abi/build/generate.rs`, which emits the `sdks/*/abi`
  files from the extracted ABI types.
- **Contract-plugin generators** live in `crates/polyplugc/src/generators/`:
  `rust.rs`, `cpp.rs`, `csharp.rs`, `python.rs`, `lua.rs`, `js_quickjs.rs`. They are driven
  by the `polyplugc` CLI to generate per-contract host/guest bindings.

`polyplugc` does **not** reuse `polyplug_codegen`'s `languages/` emitters — it depends on
`polyplug_codegen` only for shared `data` / `error` / config types (e.g. `GenerateConfig`,
`PolyplugcError`, `ResolvedBundleFile`).

There is **no `js_deno.rs`** — JS generation targets QuickJS only.

### SDKs (`sdks/`)

Each language SDK has `abi/`, `host/`, `guest/`, and `loaders/` subdirectories:

```
sdks/
├── rust/        abi/, guest/   (the host side IS the `polyplug` crate)
├── cpp/         abi/, host/, guest/, loaders/
├── csharp/      abi/, host/, guest/, loaders/, abi.tests/, guest.tests/, host.tests/
├── python/      abi/, host/, guest/, loaders/, polyplug_abi/
├── lua/         abi/, host/, guest/, loaders/
└── js/          abi/, host/, guest/, loaders/
```

### Hot-Reload

Native (`cdylib`), Lua, and JS (QuickJS) bundles support hot-reload — their `reload()` re-reads the on-disk source and swaps the live interface (gated on `hot_reload_enabled`). Python and .NET loaders return `RuntimeError::HotReloadDisabled` from `reload()` unconditionally. Registry reads are lock-free: readers pin a crossbeam-epoch guard and serve from an immutable published `ReadView`. Reload and unload republish that view and reclaim the superseded interface `Arc` and the underlying dylib/VM through epoch-deferred reclamation — the memory is freed once no reader is still pinned in the prior epoch. A reader pinned before the swap therefore keeps both the old interface and its still-mapped library alive until it unpins; a raw interface pointer cached and used after the owning bundle is unloaded is documented undefined behaviour (the host must quiesce first), whereas the host-mediated instance lifecycle calls (`create_guest_instance` / `destroy_guest_instance`) pin the epoch across their operation.

### Runtime Isolation Known Limitations

**Python**: CPython initializes once per process — multiple `Runtime` instances share the interpreter.
**CLR/.NET**: Same constraint; CLR initializes once per process.

---

## Non-Negotiable Rules

### 1. Module Structure

**Use `filename.rs` for single-file modules. Use `filename/mod.rs` ONLY when the module has (or immediately needs) submodules inside the same folder.**

```
// CORRECT — single-file module (no children)
src/
├── error.rs
├── reload.rs
└── runtime_store.rs

// CORRECT — multi-file module (has submodules)
src/
└── loader/
    ├── mod.rs
    ├── manifest.rs
    └── scanner.rs

// FORBIDDEN — folder wrapper for a single file with no children
src/
├── registry/
│   └── mod.rs   ← FORBIDDEN when registry has no submodules
```

**Rule:** if a `folder/mod.rs` has zero sibling `.rs` files and zero subdirectories inside the folder, collapse it to `folder.rs` and delete the empty directory.

**FORBIDDEN module pattern — never use this:**

```rust
// FORBIDDEN — NEVER DO THIS
pub mod loader {
    include!("loader.rs");
}
```

### 2. Import Placement

**`use` statements are ONLY allowed at the top of a file. Using `use` inside functions, structs, or impl blocks is FORBIDDEN.**

```rust
// CORRECT — use at file top only
use std::collections::HashMap;
use crate::runtime_store::RuntimeStore;

pub fn do_something() {
    // no use statements here
}

// FORBIDDEN — use inside function
pub fn do_something() {
    use std::collections::HashMap; // FORBIDDEN
}

// FORBIDDEN — use inside impl
impl MyStruct {
    use crate::runtime_store::RuntimeStore; // FORBIDDEN
}
```

---

### 3. Explicit Types

**ALWAYS add explicit type annotations. Do NOT rely on compiler type inference except in the two allowed cases below.**

```rust
// FORBIDDEN — type is unclear
let data = calculate_something();
let result = process(input);
let config = load_config();
let items = collect_all();

// CORRECT — type is explicit
let data: ContractIR = calculate_something();
let result: AbiError = process(input);
let config: RuntimeConfig = load_config();
let items: Vec<PluginDescriptor> = collect_all();
```

**The ONLY exceptions — obviously clear cases:**

```rust
// CORRECT — struct construction, type is the struct name itself
let descriptor = PluginDescriptor { name: "decoder", version: "1.0" };

// CORRECT — numeric casting, type is the cast target
let len = raw_len as u32;
```

**Everything else requires an explicit type annotation. When in doubt, annotate.**

---

### 4. Error Handling

**NEVER use `.unwrap()` in production code. Ever. No exceptions.**

**`.expect()` is ONLY allowed in test code (`#[cfg(test)]` blocks or `tests/` directory). Even there, prefer proper error handling.**

```rust
// FORBIDDEN in production code
let plugin = registry.get(id).unwrap();
let file = File::open(path).unwrap();
let value = map.get(&key).unwrap();

// FORBIDDEN in production code
let plugin = registry.get(id).expect("plugin must exist"); // FORBIDDEN outside tests

// CORRECT — propagate with ?
let plugin: GuestContractHandle = registry.get(id)?;

// CORRECT — handle explicitly
let plugin: GuestContractHandle = match registry.get(id) {
    Some(p) => p,
    None => return Err(RuntimeError::PluginNotFound { id }),
};
```

**ALWAYS use proper Error types. Never use string errors in production code.**

```rust
// FORBIDDEN
return Err("plugin not found".to_string());
return Err(anyhow::anyhow!("something went wrong"));  // only if anyhow is approved

// CORRECT — define and use proper error types
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("plugin not found: contract_id={contract_id}")]
    PluginNotFound { contract_id: u64 },

    #[error("version mismatch: required={required}, found={found}")]
    VersionMismatch { required: Version, found: Version },

    #[error("dependency cycle detected involving: {plugin_name}")]
    DependencyCycle { plugin_name: String },

    #[error("bundle load failed: {path}")]
    BundleLoadFailed { path: String, #[source] source: std::io::Error },
}
```

**Every module that can fail must define or re-export its error type explicitly.**

---

### 5. No Implicit Behaviour

**Never rely on implicit behaviour. Be explicit in all things.**

- Always specify visibility (`pub`, `pub(crate)`, `pub(super)`, or private — never leave it ambiguous)
- Always specify lifetimes when they are not trivially inferred by the borrow checker
- Always use fully qualified paths when there is any ambiguity
- Always specify integer literal types when the type is not declared on the binding

```rust
// FORBIDDEN — ambiguous integer literal
let id = 42;

// CORRECT
let id: u64 = 42;

// FORBIDDEN — ambiguous visibility
fn helper() { }

// CORRECT
pub(crate) fn helper() { }
```

---

### 6. Safety

**All `unsafe` blocks must have a `// SAFETY:` comment explaining exactly why the unsafe operation is sound.**

```rust
// FORBIDDEN — unsafe without justification
unsafe {
    std::ptr::write(out_ptr, result);
}

// CORRECT
// SAFETY: out_ptr is guaranteed non-null and properly aligned by the ABI contract.
// The caller (host runtime) allocates the buffer before calling this function.
// The buffer is sized to hold exactly one T as enforced by codegen.
unsafe {
    std::ptr::write(out_ptr, result);
}
```

**Generated code may use unsafe for performance. All unsafe in generated code must be justified in the generator source that produces it, not in the generated output itself.**

---

### 7. ABI Stability

**The core ABI freezes at v1.0. There is no public release yet, so the project is currently pre-1.0.**

- **Pre-1.0 (current state):** ABI-visible changes (struct fields, field order, function
  signatures) ARE permitted, but only after explicit discussion with and approval from the
  owner. No unilateral ABI changes — ever.
- **At and after v1.0:** the ABI is frozen. The rules below apply with no exceptions:
  - Never add fields to `#[repr(C)]` structs that are part of the frozen ABI
  - Never change the order of fields in ABI structs
  - Never change function signatures in the core ABI
  - New functionality is added through application-defined host/guest contracts, not by growing the ABI; the single trailing `reserved` pointer is the only sanctioned post-freeze expansion slot

If you believe an ABI change is necessary, stop and raise it as a discussion with the owner. Do not proceed unilaterally.

---

### 8. Memory Rules

**All memory crossing plugin boundaries must use the host allocator (`alloc` / `free` fields on `HostApi`).**

- A plugin must never free memory it did not allocate
- Generated code must never introduce copies of cross-boundary data that are not in the host allocator
- GC language bindings must never place cross-boundary data on the managed heap

---

### 9. String Rules

**All strings at the ABI boundary are UTF-8 `StringView`. No exceptions.**

- C# generated code must transcode UTF-16 → UTF-8 at the boundary
- Python generated code must encode to UTF-8 bytes before crossing
- Never pass a null-terminated C string across the ABI — always use `StringView` (ptr + len)

---

### 10. Code Generation Rules

**Generated code is held to the same rules as hand-written code with one exception: `unsafe` is permitted freely in generated code when justified in the generator source.**

- Generated files must have a header comment marking them as generated
- Generated files must never be edited by hand
- If a generated file needs to change, fix the generator

```rust
// THIS FILE IS AUTO-GENERATED BY polyplugc
// DO NOT EDIT BY HAND
// Re-generate with: polyplugc generate --bundle bundle.toml --lang rust --out src/generated
```

**All code generators MUST produce identical ABI mechanisms. No exceptions.**

Every generator (rust, cpp, csharp, python, lua, js) must generate code that:

1. **Uses the same `polyplug_init` signature:**
   ```c
   // All generators must produce this signature (language-specific syntax):
   AbiError polyplug_init(const HostApi* host, const BundleInitContext* ctx)
   ```

2. **Uses the same registration mechanism:**
   ```c
   // All generators must call register_guest_contract via the HostApi self-passing pattern:
   host->register_guest_contract(host, &descriptor, &interface)
   ```

3. **Never uses global state or thread-locals in generated code — including VM globals.** Generated guest code may not deposit state OR host capabilities in any global / module-level / VM namespace. For the VM languages (python, lua, js) this is the **global-free VM-dispatch mechanism**, identical across all three: `polyplug_init` RETURNS `(registrations, abi_error)` (nothing is deposited into `_G` / a module attribute / `globalThis` / VM userdata); the `HostApi` pointer is threaded into each `factory(host, …)` and stored as an instance field; the per-call arena allocator is a dispatch argument (`fn(instance, args_ptr, out_ptr, arena_ptr, arena_alloc)`); and JS additionally threads a `bridge` object (memory accessors + host-contract caller + `arenaAlloc`) as an explicit argument instead of `globalThis.polyplug`. See Rule 12 for the full contract.

   For VM guests, items 1–2 describe the C-ABI surface presented by the **Rust loader trampoline**, not the guest text itself: the trampoline exports the `AbiError polyplug_init(host, ctx)` symbol and calls `host->register_guest_contract` after consuming the guest's returned `registrations`. The guest reaches the identical ABI path through the trampoline — it never writes the C signature or calls `register_guest_contract` directly.

4. **Never hand-types an ABI function-pointer signature.** A generator must obtain ABI signatures from the auto-regenerated mirror — by compiling against it (cpp/csharp/rust) or deriving from the field at runtime (python `type(field)`, js typed-Rust install) — never by writing the signature as a literal string. A hand-typed `polyplug_*_host_*` signature in a generator is a bug: when the ABI changes, the mirror updates but the literal goes stale, and the drift is **invisible to `cargo build`/`clippy`/`test`** because the emitted text is never compiled by the Rust toolchain. It surfaces only as calling-convention UB at runtime.

   **The sole unavoidable exception is LuaJIT's `ffi.cdef`,** which structurally requires literal C text (it cannot compile-check or derive from a field). That single surface is the one place the drift can hide, so it is guarded by a toolchain-free conformance floor — `lua_host_trampoline_cdefs_are_out_param_abi` in `crates/polyplugc/src/generators/lua.rs` — which regenerates the cdefs and asserts they match the real trampolines in `crates/polyplug_lua/src/ffi.rs`. Any new lua cdef for an ABI symbol must be added to that test. Runtime execution of the lua host path (the `just verify-abi` ceiling) is the second layer.

**Why this matters:** Different registration mechanisms (e.g., divergent `HostApi` field layouts or calling conventions) break the ABI contract and cause runtime failures. All plugins, regardless of language, must interact with the host through the exact same ABI path.

**Verification:** When adding or modifying a generator, compare its output with `rust.rs` to ensure ABI parity. The bug class above (generated ABI-signature drift) is caught in two layers that both run in CI **and** locally: a toolchain-free `cargo test` floor (structural — no luajit/g++/dotnet needed), and a `just verify-abi` execution ceiling (load+dispatch, strict in CI, graceful-skip locally). See `docs/ABI_ARCHITECTURE.md` § "ABI conformance testing".

---

### 11. Dependency Version Management

**All dependency versions must be declared in the workspace `Cargo.toml`. Crates must never specify a version inline — use `{ workspace = true }` instead.**

The workspace `Cargo.toml` owns the version. Each crate `Cargo.toml` owns the features. Never mix them up.

```toml
# CORRECT — workspace Cargo.toml (owns versions, optional base features)
[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
tokio = "1"
thiserror = "1"

# CORRECT — crate Cargo.toml (inherits version, may add crate-specific features)
[dependencies]
serde = { workspace = true }
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
thiserror = { workspace = true }

# FORBIDDEN — version declared in crate Cargo.toml
[dependencies]
serde = { version = "1", features = ["derive"] }   # FORBIDDEN — version belongs in workspace
tokio = "1"                                         # FORBIDDEN — version belongs in workspace
```

**Rules:**

- The workspace `Cargo.toml` is the single source of truth for dependency versions
- Crates use `{ workspace = true }` and may extend with `features = [...]` as needed
- Never declare a version number in a crate-level `Cargo.toml`
- Never add `version = ...` alongside `workspace = true` — that is redundant and forbidden
- Optional dependencies must also use `{ workspace = true, optional = true }` — version still lives in workspace

```toml
# FORBIDDEN — version alongside workspace = true
serde = { workspace = true, version = "1" }  # FORBIDDEN

# CORRECT — optional dep still uses workspace for version
serde = { workspace = true, optional = true }
```

---

### 12. Runtime Isolation

**No thread-locals or globals for Runtime. The Runtime must be fully encapsulated and MUST NOT rely on any static, global, or thread-local data.**

This is CRITICAL and MUST NEVER be broken. The design goal is that multiple polyplug runtimes can coexist in the same process, each with its own isolated state.

```rust
// FORBIDDEN — global or thread-local state for runtime
static GLOBAL_REGISTRY: OnceLock<Registry> = OnceLock::new();
thread_local! {
    static CURRENT_RUNTIME: RefCell<Option<*mut Runtime>> = RefCell::new(None);
}

// CORRECT — all state owned by the Runtime instance
pub struct Runtime {
    store: RuntimeStore,
    config: RuntimeConfig,
    // ... all state is instance-owned, no globals
}
```

**Why this matters:**
- Multiple runtimes in the same process must be fully isolated
- Each runtime owns its own `RuntimeStore`, loaded bundles, and configuration
- No shared state between runtime instances
- Enables use cases like: testing with parallel isolated runtimes, embedding multiple plugin systems, sandboxing

**Verification:** When reviewing code, grep for:
- `static` declarations that hold runtime state
- `thread_local!` macros
- `OnceLock`, `LazyLock`, `Lazy` for runtime data
- Any pattern that shares state across Runtime instances

**Known Limitations (External Runtime Constraints)**

**Python Loader**: The CPython interpreter can only be initialized **once per process**. The `polyplug_python` loader uses `static PYTHON_INIT: OnceLock<()>` to ensure single initialization. This means:
- Multiple `Runtime` instances in the same process share the same Python interpreter
- Python plugins from different runtimes can see each other's modules/state
- For full isolation with Python, use separate processes

**CLR / .NET Loader**: The .NET CLR can only be initialized **once per process**. The `polyplug_dotnet` loader uses `static CLR_CONTEXT: OnceCell<...>` to ensure single initialization. This means:
- Multiple `Runtime` instances in the same process share the same CLR runtime
- .NET assemblies from different runtimes share the same loader cache
- For full isolation with .NET, use separate processes

**Lua, JavaScript (QuickJS), and Native loaders**: Fully compliant with runtime isolation. Each bundle gets its own isolated VM.

**Static-free SDKs — ALL languages, host AND guest.** No SDK file (hand-written or generated, any language) may hold runtime or plugin state in module-level / class-static / process-global storage. The host pointer, plugin implementation objects, and per-bundle state always flow through instances and context parameters.

**Global-free VM guests (python, lua, js).** A VM guest may NOT carry runtime/plugin state OR host capabilities in any VM global, module-level, or interpreter namespace — not even a loader-injected one. The earlier "per-VM globals injected by a loader are instance state and are allowed" carve-out is REMOVED. Everything threads explicitly, with one identical mechanism across all three VM languages (Rule 10):

- **Registration is returned, never deposited.** `polyplug_init(host, ctx)` RETURNS `(registrations, abi_error)` — two VM-native values (Lua multiple returns / Python tuple / JS `[registrations, abiError]`). The loader honors `abi_error` first, then consumes `registrations`. Nothing is written to `_G`, a module attribute, `globalThis`, or VM userdata.
- **Host pointer is an argument.** The loader threads the `HostApi` pointer into each `factory(host, …)`; the impl stores it as an INSTANCE field. Helpers take it explicitly (lua/python cast it and call `HostApi` fields directly via FFI/ctypes).
- **Arena allocator is a per-call dispatch argument.** Dispatch signature is `fn(instance, args_ptr, out_ptr, arena_ptr, arena_alloc)` (js additionally threads a `bridge`); no shared arena cell, so concurrent / same-VM reentrant dispatch are correct by construction.
- **JS host capabilities ride a threaded `bridge`.** QuickJS cannot deref raw pointers, so the loader passes a `bridge` object (memory accessors + host-contract caller + `arenaAlloc`) as an explicit argument to `polyplug_init`, each `factory(bridge, hostLo, hostHi)`, and every dispatch call — NEVER `globalThis.polyplug`.

Instance fields on the impl (host pointer, bridge, user state), the chunk/module RETURN value, and per-call arguments are NOT globals and remain allowed. Interpreter-level once-per-process constraints (CPython, CLR bootstrap) are external limitations, not a license for SDK statics.

---

### 13. No Re-exports That Obscure Module Boundaries

**NEVER use `pub use` to re-export items from another crate. Re-exports should only be used for:**
- Re-exporting from the same crate (e.g., `pub use crate::module::Type`)
- Creating a convenient facade for a module's own types

**The entire workspace must be cross-crate-re-export free: no local crate (including every crate under `sdks/`) may `pub use` items from another local crate — no facade exceptions, renamed re-exports (`pub use foo::bar as baz`) included.**

**FORBIDDEN — re-exporting from another crate (even public APIs):**
```rust
// FORBIDDEN — polyplug_codegen re-exporting polyplug_abi's types
pub use polyplug_abi::ir::SomeType;

// FORBIDDEN — re-exporting from a dependency's public API to supply to another crate
pub use polyplug_utils::{bundle_id, contract_id, fnv1a_64};

// FORBIDDEN — re-exporting from a dependency's private module
pub use some_dep::internal_module::Type;
```

**CORRECT — consumers import directly from the source crate:**
```rust
// CORRECT — polyplugc imports directly from polyplug_utils
use polyplug_utils::{bundle_id, contract_id, fnv1a_64};

// CORRECT — re-exporting from same crate
pub use crate::ir::Version;
```

**Why this matters:**
- Re-exports create confusion about where types/functions are actually defined
- They create tight coupling between crates through the re-exporting crate
- They make refactoring harder — changing the source requires updating all re-exports
- They obscure the actual module boundaries and dependencies
- **Most importantly:** If crate A re-exports from crate B, and crate C uses crate A, crate C gets crate B's types through crate A. This creates a dependency chain that makes it unclear where things come from.

**Rule:** If crate C needs something from crate B, it must depend on crate B directly and import from crate B. Never use crate A as a "pass-through" for crate B's exports.

---

### 14. No Backward Compatibility Code

**NEVER add backward compatibility code, deprecated aliases, or migration shims.**

This codebase does NOT maintain backward compatibility. Breaking changes are intentional and expected.

**FORBIDDEN:**
```rust
// FORBIDDEN — deprecated constants for "backward compatibility"
#[deprecated(since = "0.2.0", note = "Use AbiErrorCode::Ok instead")]
pub const ABI_OK: u32 = AbiErrorCode::Ok as u32;

// FORBIDDEN — type aliases for "migration"
pub type OldTypeName = NewTypeName;

// FORBIDDEN — compatibility wrappers
pub fn old_function_name() { new_function_name() }
```

**CORRECT:**
```rust
// CORRECT — just use the new type directly
pub enum AbiErrorCode { Ok = 0, ... }

// CORRECT — consumers update their code
use polyplug_abi::AbiErrorCode;
let code = AbiErrorCode::Ok;
```

**Why this matters:**
- Backward compatibility code creates technical debt
- It obscures the actual API
- It encourages not updating code
- This codebase explicitly does NOT guarantee backward compatibility

**When making breaking changes:**
1. Remove the old code completely
2. Update all usages in the same PR
3. Do NOT leave deprecated shims

---

### 15. Deprecated Re-exports Are FORBIDDEN

**NEVER create deprecated re-exports or "convenience" re-exports from other crates.**

**FORBIDDEN:**
```rust
// FORBIDDEN — deprecated re-exports
//! Deprecated - use `polyplug_abi::runtime::Compatibility` directly.
pub use polyplug_abi::runtime::Compatibility;

// FORBIDDEN — "convenience" re-exports from dependencies
pub use polyplug_abi::SomeType;  // Let users import directly
```

**CORRECT:**
```rust
// CORRECT — users import directly from the source crate
use polyplug_abi::runtime::Compatibility;
```

**Why this matters:**
- Re-exports create confusion about where types actually live
- They create tight coupling between crates
- They make refactoring harder
- They encourage not updating imports

---

### 16. Type Aliases Are FORBIDDEN

**NEVER create type aliases. No exceptions.**

Type aliases obscure the actual type, create confusion, and make refactoring harder. Always use the real type directly.

**FORBIDDEN:**
```rust
// FORBIDDEN — type aliases for "convenience"
pub type Handle = GuestContractHandle;
pub type Result<T> = std::result::Result<T, MyError>;
pub type PluginError = crate::error::Error;

// FORBIDDEN — type aliases for "migration" or "backward compatibility"
pub type OldName = NewName;

// FORBIDDEN — deprecated aliases
#[deprecated(note = "Use NewName")]
pub type OldName = NewName;

// FORBIDDEN — even simple type aliases
pub type MyResult<T> = Result<T, Error>;
```

**CORRECT:**
```rust
// CORRECT — use the actual type directly
fn do_something() -> std::result::Result<(), MyError> { }

// CORRECT — import the type directly if needed
use crate::error::Error;
fn other_thing() -> Result<(), Error> { }
```

**The ONE exception — FFI function-pointer signatures.** A `type` alias whose
right-hand side is an `unsafe extern "C"` / `extern "system"` **function-pointer
signature** is PERMITTED. Such an alias is the single definition point for a
calling convention that crosses the FFI boundary; inlining it at its (often many)
use sites would duplicate an ABI function-pointer signature across the codebase —
exactly the drift hazard **Rule 10** (§10 item 4, "Never hand-types an ABI
function-pointer signature") forbids, where one missed copy after an ABI change is
silent calling-convention UB. The alias keeps the signature defined once.

```rust
// CORRECT — FFI fn-pointer signature alias (single source of truth for the call)
pub(crate) type InitFn =
    unsafe extern "system" fn(*const HostApi, *const BundleInitContext) -> AbiError;
```

This exception is **narrow**: it covers ONLY `extern` function-pointer `type`
aliases. It does NOT relax the ban on data-type aliases (`type Handle = ...`),
`Result<T>` aliases, convenience/migration aliases, or any other form above.

**Why this matters:**
- Type aliases hide the actual type, making code harder to understand
- They create confusion about which name is "real"
- They make global refactoring painful — must update all aliases
- IDEs and tools show the alias, not the underlying type
- This codebase does NOT maintain backward compatibility — use the canonical name
- The FFI fn-pointer exception is the inverse case: there the alias PREVENTS
  drift, so it is the maintainable choice (see Rule 10 §10 item 4)

---

### 17. ABI_* Constants Are FORBIDDEN

**NEVER use `ABI_OK`, `ABI_ERROR_*`, or any `ABI_` prefixed constants. Use `AbiErrorCode` enum instead.**

The `AbiErrorCode` enum is the canonical way to represent ABI error codes across all languages. Using raw constants creates inconsistency and makes the codebase harder to maintain.

**FORBIDDEN:**
```rust
// FORBIDDEN — use AbiErrorCode enum
if err.code == ABI_OK { }
if err.code == ABI_ERROR_GENERIC { }
return ABI_ERROR_INVALID_POINTER;

// FORBIDDEN — in all SDKs (Python, JS, Lua, C#, C++)
if err.code == ABI_OK:  # Python
if (err.code == ABI_OK) { ... }  // JavaScript
if err.code == ABI_OK then  -- Lua
if (err.Code == AbiConstants.ABI_OK)  // C#
```

**CORRECT:**
```rust
// CORRECT — use AbiErrorCode enum
use polyplug_abi::AbiErrorCode;
if err.code == AbiErrorCode::Ok as u32 { }
if err.code == AbiErrorCode::Generic as u32 { }
return AbiErrorCode::InvalidPointer as u32;

// Python
if err.code == AbiErrorCode.Ok:

// JavaScript
if (err.code === AbiErrorCode.Ok) { ... }

// Lua
if err.code == AbiErrorCode.Ok then

// C#
if (err.Code == (uint)AbiErrorCode.Ok)

// C++
if (err.code == AbiErrorCode_Ok)
```

**Why this matters:**
- Consistent error handling across all languages
- Type-safe enum instead of magic numbers
- Easier to extend with new error codes
- Generated code uses the canonical form
- No confusion between constants and enums

---

### 18. Test Failures Must Be Fixed, Never Skipped

**NEVER skip, ignore, or mark tests as `#[ignore]` to avoid fixing failures.**

- If a test fails, find and fix the root cause
- If a test is flaky, fix the race condition or timing issue
- If a test crashes (SIGSEGV, panic, etc.), debug and fix the underlying bug
- `#[ignore]` is ONLY acceptable for tests that require unavailable external resources (e.g., specific hardware, paid services)

**FORBIDDEN:**
```rust
#[test]
#[ignore = "this test is flaky"]  // FORBIDDEN — fix the flakiness instead
fn test_something() { }

#[test]
#[ignore = "causes SIGSEGV"]  // FORBIDDEN — debug and fix the crash
fn test_something_else() { }
```

**In CI workflows:**
- Never use `--skip` to avoid failing tests
- Never use `--ignore` to exclude failing tests
- If a test fails in CI but passes locally, investigate the environmental difference

**Why this matters:**
- Skipped tests hide bugs that will bite users in production
- A test failure is a bug report — treat it as such
- Technical debt compounds: every skipped test makes the codebase less trustworthy

---

### 19. SDK Helper Surface Is Derived From `checks/sdk_validator.yaml`

**`checks/sdk_validator.yaml` is the single source of truth for built-in-type helper methods. Helpers live ONLY in the validator-target files — duplicate or stale helper implementations anywhere else are FORBIDDEN.**

- The golden method set in `checks/sdk_validator.yaml` defines what every language must implement; the `targets:` section defines the ONE file per language where those helpers live (the `sdks/*/abi` mirrors and `sdks/rust/guest`).
- Never hand-write a second copy of a helper (or a "Helper" class duplicating one) in guest/host/loader SDK files — consumers use the validated implementation.
- Adding a new helper concept = add it to `checks/sdk_validator.yaml` AND implement it in ALL validated targets in the same change; the validator must stay green (`cargo run -p sdk-validator -- --config checks/sdk_validator.yaml --fail-on-missing`).
- A helper found outside the validated files is stale scaffolding: delete it and retarget any consumers.

---

### 20. No Inline Fully-Qualified Paths

**Import symbols at the top of the module and use the SHORT name at every use-site. Inline fully-qualified multi-segment paths are FORBIDDEN.** This is the use-site companion to Rule 2 (`use` at file top only): the path is named once in a `use`, never spelled out inline.

```rust
// FORBIDDEN — inline fully-qualified paths
let p: std::path::PathBuf = std::path::PathBuf::new();
return Err(crate::error::RuntimeError::PluginNotFound { contract_id });
let v: polyplug_abi::types::Version = polyplug_abi::types::Version::default();

// CORRECT — import at top, short name at use-site
use std::path::PathBuf;
use crate::error::RuntimeError;
use polyplug_abi::types::Version;

let p: PathBuf = PathBuf::new();
return Err(RuntimeError::PluginNotFound { contract_id });
let v: Version = Version::default();
```

Applies to ALL roots — same-crate (`crate::`), cross-crate (`polyplug_abi::`, `libloading::`, …), and stdlib (`std::`, `core::`, `alloc::`) — and ALL referents: types, generics, trait bounds, signatures, match patterns, free functions, enum variants, and consts.

**The four exceptions (the guard does not flag these):**

1. **`core::str::*` / `std::str::*`** — the bare `str::Item` form is an ambiguous primitive-type projection (and `str::from_utf8` trips an msrv lint); keep these fully-qualified.
2. **Module-qualified forms whose leading segment is an imported module**, not a crate root — `ptr::null_mut()`, `mem::size_of()`, `fs::read(…)`, `de::Error` (after `use core::ptr;` / `use std::fs;` / `use serde::de;`). These are idiomatic and do not cross a crate boundary inline.
3. **`crate::` paths inside `macro_rules!` bodies** — macros expand at the call site and may need the `crate::` root for hygiene.
4. **FFI function-pointer `type` aliases** (the Rule 16 exception) — defined once in a `use`-topped file and imported by callers.

**Enforcement:** `just verify-no-fq-paths` runs the committed ast-grep rule `checks/no_inline_fq_paths.yaml` (CI: the **SDK Consistency** job). See `docs/WORKFLOW.md` § "Import hygiene".

---

### 21. Crate Layering — Codegen Never Depends On The Runtime

**The code-generation crates MUST NEVER depend on the `polyplug` runtime crate. This is non-negotiable and must be preserved by every future change.**

The layer graph is strict and one-directional:

```
polyplug_utils (leaf: ids/hashing only)
    ↑
polyplug_abi (frozen C ABI types)
    ↑
polyplug_common (shared schema TYPES + their pure behaviour)
    ↑
{ polyplug runtime, polyplugc, polyplug_codegen }
```

1. **`polyplug_utils` never depends on the runtime** — it is the leaf (ids/hashing). Keep it dependency-light.
2. **`polyplug_codegen` and `polyplugc` MUST NEVER depend on `polyplug` (the runtime) — for any reason.** A codegen tool that pulls in the runtime drags the runtime `cdylib` into every consumer's build graph, which collides (`libpolyplug.so`, cargo #6313/E0460) and defeats using the CLI from a plugin's `build.rs`. If the CLI needs a runtime type, that type belongs in `polyplug_common`, not in a runtime dependency. Verify with `cargo tree -p polyplugc -e normal` — the `polyplug` runtime crate must not appear.
3. **`polyplug_common` holds shared TYPES and their intrinsic, pure, self-contained behaviour only** — deserialization, self-validation, pure resolution. **NO I/O, NO logging, NO runtime orchestration.** Filesystem reads, logger-aware diagnostics, and dependency-resolution-with-side-effects stay in the runtime as thin wrappers over the pure `polyplug_common` operations.
4. **`polyplugc` is a bin-only CLI — it exports nothing.** There is no `polyplugc` library API; consumers invoke the compiled binary (`polyplugc generate …`). Do not add a `[lib]` or `pub` surface to it.

**Why this matters:** the migration proof (a real host embedding polyplug) requires that a plugin's `build.rs` can run `polyplugc` without the runtime entering its build graph. Breaking any rule above reintroduces the `cdylib` collision and makes the CLI unusable as a build-time tool.

---

### 22. Universal Host/Guest Feature Parity

**A generally applicable polyplug feature MUST ship for every maintained host
and guest language in the same change. Rust-only delivery is forbidden.**

The maintained language set is Rust, C++, C#, Python, Lua, and
JavaScript/QuickJS.

- Public runtime concepts, contracts, lifecycle semantics, validation, errors,
  and capability claims are language-neutral.
- Language-specific mechanics belong behind that language's generator, SDK, or
  loader. They must not create a parallel public core model.
- One canonical bundle, contract, registration, dependency, lifecycle, and
  discovery path serves every source and language.
- Equivalent feature semantics are mandatory; the physical mechanism may differ
  by loader. For example, a VM loader may evaluate source while a native loader
  delegates a materialized artifact to the operating-system loader.
- A feature is not shipped while any maintained language silently ignores it,
  stubs it, rejects it without an owner-approved platform limitation, or lacks
  enforcement tests.
- Every feature change must include a six-language host/guest parity review and
  the applicable matrix tests. A limitation requires explicit owner approval and
  current-state documentation.
- Cross-compilation proves that target artifacts compile and link. It never
  substitutes for native execution tests on the target operating system.

### 23. Loader Ownership and Canonical Sources

**Executable-source mechanics are loader responsibilities.**

- `Path`, source text, artifact bytes, and linked native entrypoints must enter
  one runtime load/registration transaction.
- Loaders acquire, validate, own, and reclaim their language-specific resident
  resource: native module, linked table, VM, interpreter module, or managed
  assembly context.
- Core runtime types describe canonical bundle semantics. Do not add
  language-specific duplicate bundle/contract types.
- A loader must stage every contract and resource before publication. Failure
  publishes nothing and releases staged ownership exactly once.
- Multi-contract registration is atomic for every language: readers observe
  either the complete bundle or no bundle.
- Linked native code may be logically unloaded but cannot be physically unmapped
  from its host executable. Path/byte native artifacts remain owned by the
  operating-system loader.

### 24. Delegated Audits Are Leads, Not Truth

**Never implement a reviewer or sub-agent finding without independent
verification against current source and a reproducer or enforcing test.**

- Preserve audit leads and their validation status in the active Plane work
  item and `.audit/` ledger until closure. Audit ledgers NEVER live in `docs/`;
  `docs/` is the mdBook product documentation source.
- For each accepted finding, record exact source evidence, triggering behavior,
  affected languages, the root fix, and the regression test.
- Record rejected findings and why they are false, so another agent does not
  repeat the same mistaken cleanup.
- Dead-looking code requires the `use vs remove` judgment: determine whether it
  is incomplete required wiring or genuine waste before deletion.

---

## Project Structure

```
polyplug/
├── CLAUDE.md                        this file (stays at root)
├── README.md                        stays at root (GitHub convention)
├── CHANGELOG.md                      stays at root (GitHub convention)
├── .audit/                          temporary working audit ledgers; never mdBook content
├── docs/                            ALL other documentation (.md) lives here —
│                                    incl. TRUST_MODEL.md, ABI_ARCHITECTURE.md
├── crates/
│   ├── polyplug/                    core runtime
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── error.rs
│   │       ├── ffi.rs               2 #[no_mangle] exports: _create / _destroy
│   │       ├── host_bridge.rs
│   │       ├── reload.rs
│   │       ├── runtime.rs
│   │       ├── runtime_builder.rs
│   │       ├── runtime_store.rs
│   │       ├── compatibility/       has submodules: bundle_node, contract_capability, …
│   │       │   └── mod.rs + *.rs
│   │       └── loader/              has submodules: manifest, scanner, bundle_loader
│   │           └── mod.rs + *.rs
│   ├── polyplug_abi/                frozen ABI types
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ffi.rs
│   │       ├── host/                host_api.rs, host_contract_interface.rs, …
│   │       ├── guest/               guest_contract_interface.rs, guest_contract_instance.rs
│   │       ├── plugin/              guest_contract_handle.rs, plugin_context.rs, plugin_descriptor.rs
│   │       ├── dispatch/            dispatch_type.rs, native_dispatch.rs, vm_dispatch.rs, …
│   │       ├── runtime/             compatibility.rs, reload_phase.rs, runtime_config.rs
│   │       └── types/               abi_error.rs, error_code.rs, array.rs, string_view.rs, version.rs, …
│   ├── polyplug_utils/              fnv1a_64, bundle_id, contract_id
│   ├── polyplug_common/             shared schema TYPES + pure behaviour (manifest.rs)
│   │   └── src/  lib.rs, manifest.rs
│   ├── polyplug_native/             native cdylib loader (supports hot-reload)
│   │   └── src/  config.rs, ffi.rs, lib.rs, loader.rs
│   ├── polyplug_python/             Python loader
│   │   └── src/  config.rs, context.rs, ffi.rs, isolation.rs, lib.rs, loader.rs
│   ├── polyplug_lua/                Lua loader
│   │   └── src/  bridge.rs, config.rs, ffi.rs, lib.rs, loader.rs
│   ├── polyplug_js/                 JavaScript (QuickJS) loader
│   │   └── src/  bridge.rs, config.rs, ffi.rs, lib.rs, loader.rs
│   ├── polyplug_dotnet/             .NET/C# loader
│   │   └── src/  config.rs, context.rs, ffi.rs, lib.rs, version.rs
│   ├── polyplug_codegen/            codegen library
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── context.rs, data.rs, error.rs, generator.rs, reserved.rs
│   │       └── languages/
│   │           ├── mod.rs
│   │           ├── cpp.rs, csharp.rs, python.rs, lua.rs, js.rs
│   ├── polyplugc/                   CLI binary
│   │   └── src/
│   │       ├── main.rs
│   │       ├── ir.rs, lib.rs, parser.rs, validate.rs
│   │       └── generators/
│   │           ├── mod.rs
│   │           ├── rust.rs, cpp.rs, csharp.rs, python.rs, lua.rs, js_quickjs.rs
│   └── sdk_validator/               validates SDKs against the ABI
├── sdks/
│   ├── rust/    abi/, guest/   (host side = the polyplug crate itself)
│   ├── cpp/     abi/, host/, guest/, loaders/
│   ├── csharp/  abi/, host/, guest/, loaders/, abi.tests/, guest.tests/, host.tests/
│   ├── python/  abi/, host/, guest/, loaders/, polyplug_abi/
│   ├── lua/     abi/, host/, guest/, loaders/
│   └── js/      abi/, host/, guest/, loaders/
└── examples/
    ├── api.toml
    ├── guests/
    ├── hosts/
    └── plugins/
```

---

## Quick Reference — Forbidden vs Correct

| Forbidden | Correct |
|---|---|
| `folder/mod.rs` with no submodules | `folder.rs` (flat file) |
| `use` inside function/impl | `use` at file top only |
| `include!()` module pattern | proper `mod` declarations |
| `let x = foo()` (inferred) | `let x: MyType = foo()` |
| `.unwrap()` anywhere | `?` operator or explicit match |
| `.expect()` in production | proper error types + `?` |
| `return Err("string")` | `return Err(MyError::Variant)` |
| `unsafe { ... }` no comment | `// SAFETY: ...` before every unsafe block |
| unilateral ABI struct changes | pre-1.0: only with owner approval; at/after 1.0: new functionality via host/guest contracts (ABI frozen; only the `reserved` slot may be repurposed) |
| editing generated files | fix the generator, re-run polyplugc |
| `fn polyplug_init(rt_ctx, host, ctx)` (3 args) | `fn polyplug_init(host, ctx)` (2 args — canonical) |
| different ABI mechanisms per generator | identical `polyplug_init` + `register_guest_contract` across all generators |
| global state / thread-locals in generated code | all context flows through `host` and `ctx` parameters |
| global state / thread-locals for Runtime | all state owned by Runtime instance |
| dependency version in crate `Cargo.toml` | version in workspace `Cargo.toml`, `{ workspace = true }` in crate |
| `version = ...` alongside `workspace = true` | omit version in crate entirely — workspace owns it |
| type aliases (`pub type OldName = NewName`) | use canonical names everywhere |
| `ABI_OK` / `ABI_ERROR_*` constants | `AbiErrorCode::Ok` / `AbiErrorCode::*` enum |
| `pub use other_crate::Type` | consumers import from source crate directly |
| SDK static / module-global holding runtime or plugin state (any language, host or guest) | state flows through instances and context parameters |
| duplicate "helper" implementations outside the `checks/sdk_validator.yaml` target files | helpers live only in validated files; golden set in `checks/sdk_validator.yaml` |
| documentation `.md` at repo root (except CLAUDE.md, README.md, CHANGELOG.md) | all docs live in `docs/` |
| inline fully-qualified path (`std::path::PathBuf`, `crate::error::Foo`) at a use-site | `use` it at module top, write the short name (exceptions: `core::str::*`, module-qualified `ptr::`/`fs::`, macro `crate::`) |
| generally applicable Rust-only feature | equivalent feature semantics for all six host and guest languages |
| language-specific public bundle/contract model | one canonical core model with mechanics behind loaders/generators |
| per-contract publication during bundle initialization | stage the complete bundle, then commit atomically |
| cross-compilation used as runtime proof | cross-compile/link plus native target execution |
| blindly accepting delegated findings | independently verify source, trigger, root fix, and regression test |

---

## Enforcement

Every pull request must pass:

1. `cargo clippy -- -D warnings` — zero warnings tolerated
2. `cargo fmt --check` — formatting must be clean
3. `cargo test` — all tests must pass
4. Manual review against this CLAUDE.md checklist

A reviewer finding any violation of this document must reject the PR immediately, regardless of how minor the violation appears. Consistency is non-negotiable.
