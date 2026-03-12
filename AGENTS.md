# AGENTS.md — polyplug

This file defines the **mandatory, non-negotiable rules** for every agent, contributor, and AI assistant working on this codebase. No exceptions. No shortcuts. No "just this once."

If you are unsure whether something violates a rule — it probably does. Ask first.

---

## Project Identity

- **Project name:** `polyplug`
- **CLI tool name:** `polyplugc`
- **Language:** Rust (host runtime, CLI, guest libs)
- **Goal:** The universal, blazing-fast, cross-language plugin runtime platform
- **Trust model**: See `TRUST_MODEL.md` for bundle identity, declared dependencies, and ABI freeze details.

---

## Non-Negotiable Rules

Violating any rule below is grounds for immediate rejection of the change. These are not style suggestions. They are hard requirements.

---

### 1. Module Structure

**Use `filename.rs` for single-file modules. Use `filename/mod.rs` ONLY when the module has (or immediately needs) submodules inside the same folder.**

```
// CORRECT — single-file module (no children)
src/
├── registry.rs
├── graph.rs
└── error.rs

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
├── graph/
│   └── mod.rs   ← FORBIDDEN when graph has no submodules
└── error/
    └── mod.rs   ← FORBIDDEN when error has no submodules
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
use crate::registry::Registry;

pub fn do_something() {
    // no use statements here
}

// FORBIDDEN — use inside function
pub fn do_something() {
    use std::collections::HashMap; // FORBIDDEN
}

// FORBIDDEN — use inside impl
impl MyStruct {
    use crate::registry::Registry; // FORBIDDEN
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
let plugin: PluginHandle = registry.get(id)?;

// CORRECT — handle explicitly
let plugin: PluginHandle = match registry.get(id) {
    Some(p) => p,
    None => return Err(PolyplugError::PluginNotFound { id }),
};
```

**ALWAYS use proper Error types. Never use string errors in production code.**

```rust
// FORBIDDEN
return Err("plugin not found".to_string());
return Err(anyhow::anyhow!("something went wrong"));  // only if anyhow is approved

// CORRECT — define and use proper error types
#[derive(Debug, thiserror::Error)]
pub enum PolyplugError {
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

**The core ABI is frozen. Once released at v1, no ABI-visible struct or function signature may change.**

- Never add fields to `#[repr(C)]` structs that are part of the frozen ABI
- Never change the order of fields in ABI structs
- Never change function signatures in the core ABI
- All new functionality goes through the extension system

If you believe an ABI change is necessary, stop and raise it as a discussion. Do not proceed unilaterally.

---

### 8. Memory Rules

**All memory crossing plugin boundaries must use the host allocator (`host_alloc` / `host_free`).**

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


## Project Structure

```
polyplug/
├── AGENTS.md                        this file
├── crates/
│   ├── polyplug/                    Rust runtime core
│   │   └── src/
│   │       ├── lib.rs               crate root
│   │       ├── abi.rs
│   │       ├── error.rs
│   │       ├── ffi.rs
│   │       ├── graph.rs
│   │       ├── registry.rs
│   │       ├── reload.rs
│   │       ├── runtime.rs
│   │       ├── version.rs
│   │       ├── allocator/           has submodule: tracking
│   │       │   ├── mod.rs
│   │       │   └── tracking.rs
│   │       ├── extensions/          has submodule: trace
│   │       │   ├── mod.rs
│   │       │   └── trace.rs
│   │       └── loader/              has submodules: manifest, scanner
│   │           ├── mod.rs
│   │           ├── manifest.rs
│   │           └── scanner.rs
│   └── polyplugc/                   CLI codegen tool
│       └── src/
│           ├── main.rs              binary entry point
│           ├── error.rs
│           ├── ir.rs
│           ├── pack.rs
│           ├── parser.rs
│           └── generators/          has submodules: rust, cpp, csharp, python, lua, js_*
│               ├── mod.rs
│               ├── rust.rs
│               ├── cpp.rs
│               ├── csharp.rs
│               ├── python.rs
│               ├── lua.rs
│               ├── js_deno.rs
│               └── js_quickjs.rs
├── host-libs/
│   ├── rust/
│   ├── cpp/
│   ├── csharp/
│   ├── python/
│   └── lua/
└── guest-libs/
    ├── rust/
    ├── cpp/
    ├── csharp/
    ├── python/
    └── lua/
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
| modifying ABI structs | new functionality via extensions only |
| editing generated files | fix the generator, re-run polyplugc |
| dependency version in crate `Cargo.toml` | version in workspace `Cargo.toml`, `{ workspace = true }` in crate |
| `version = ...` alongside `workspace = true` | omit version in crate entirely — workspace owns it |

---

## Enforcement

Every pull request must pass:

1. `cargo clippy -- -D warnings` — zero warnings tolerated
2. `cargo fmt --check` — formatting must be clean
3. `cargo test` — all tests must pass
4. Manual review against this AGENTS.md checklist

A reviewer finding any violation of this document must reject the PR immediately, regardless of how minor the violation appears. Consistency is non-negotiable.
