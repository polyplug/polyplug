# Quick Start — Write Your First Plugin in 10 Minutes

This guide uses a Rust guest and host. For other languages, see the Languages guides.

---

## Prerequisites

### Install the `polyplugc` CLI

`polyplugc` turns a `.toml` contract into typed glue. Install via your ecosystem's
package manager:

```bash
cargo install polyplugc                       # Rust
npm  install -g @polyplug/cli                 # Node  (also: bunx @polyplug/cli,
                                              #        deno install -A npm:@polyplug/cli)
uv   tool install polyplugc                    # Python (also: pipx install polyplugc,
                                              #         pip install polyplugc)
dotnet tool install -g Polyplug.Cli            # .NET
curl -fsSL https://polyplug.github.io/install.sh | bash   # prebuilt binary
```

Or grab a binary straight from the [GitHub Releases](https://github.com/polyplug/polyplug/releases)
page. To build from a checkout of this repo instead: `cargo build --release -p polyplugc`
→ `target/release/polyplugc`.

### For this guide

- A Rust toolchain (stable, edition 2024) — because the example guest and host below are Rust.

---

## Step 1 — Define the contract

The contract file (`api.toml`) is the shared specification between app
developers and plugin developers. Its table role is directional:
`[[guest_contract]]` is implemented by a guest plugin and called by the host,
whereas `[[host_contract]]` is implemented by the host and called by plugins.

`plugin_contract` is not an alias: migrate its table and nested function tables
to `guest_contract` before validation. The precise error is:
“`[[plugin_contract]]` is invalid; use `[[guest_contract]]` instead.”

Create a directory for the project and write `api.toml`:

```toml
# api.toml
[[guest_contract]]
name = "greeter.Hello"
version = "1.0.0"
docs = "Greets a caller by name."

[[guest_contract.functions]]
name = "greet"
docs = "Returns a greeting for the supplied name."
params = [{ name = "name", type = "StringView", docs = "The name to greet." }]
return = { type = "StringView", docs = "The generated greeting." }
```

The `name` field uses a `namespace.Type` convention. Supported parameter and
return types include `StringView`, `Buffer`, `bool`, `i32`, `u32`, `i64`,
`u64`, `f32`, `f64`, and `void`. Enums defined in the same file can also be
referenced as types.

Every contract-binding declaration accepts an optional `docs` string: plugin and
host contracts, functions, parameters, returns, structs, fields, enums, and enum
variants. Documentation is emitted as native API documentation in every generated
binding and does not affect contract IDs, ABI layout, compatibility, or manifests.
Use either the string return form (`return = "StringView"`) or the documented
return table shown above. Documentation line endings normalize to LF; tabs and
ordinary line breaks are supported.

For the complete schema—including structs, enums, host contracts, expanded returns, and per-language `langs` attributes—see the [`api.toml` reference](API_TOML.md).

---

## Step 2 — Write `bundle.toml`

The `bundle.toml` is the plugin developer's manifest. It declares the bundle's
identity, loader, artifact, and contracts.

```toml
# bundle.toml
[bundle]
name = "my_greeter"
version = "1.0.0"
api = "../api.toml"        # path to the api.toml, relative to this file
loader = "native"

[bundle.file]
linux.x86_64   = "libmy_greeter.so"
macos.aarch64  = "libmy_greeter.dylib"
macos.x86_64   = "libmy_greeter.dylib"
windows.x86_64 = "my_greeter.dll"

[[plugin]]
name = "my_greeter"
implements = ["greeter.Hello@1.0"]
```

`implements` references the contract as `name@major_version`. The `loader`
field must be one of: `native`, `lua`, `python`, `js-quickjs`, `dotnet`.

---

## Step 3 — Generate guest glue code

```bash
polyplugc generate --bundle bundle.toml --lang rust --out generated
```

This is the unified default. When the application needs a shared domain crate
or module, keep this command for the normal layout or opt into the exact
split-output flags in [Code generation and split output](CODE_GENERATION.md);
split output is never implied by the quick-start command.

This writes six files into `generated/`:

```text
generated/
├── manifest.toml          generated manifest (never edit by hand)
└── guest/
    ├── mod.rs
    ├── contracts.rs       the trait(s) you implement
    ├── interfaces.rs      instance machinery + author-factory declarations
    ├── host_contract_callers.rs   (host-contract call helpers, if any)
    ├── init.rs            polyplug_init ABI entry point
    └── types.rs           generated enums and structs
```

Re-run this command whenever `bundle.toml` or `api.toml` changes. Never edit
the generated files — regenerate instead.

---

## Step 4 — Set up the Cargo project

Create a `Cargo.toml` for the plugin crate alongside `bundle.toml`:

```toml
[package]
name = "my_greeter"
version = "1.0.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_abi = { path = "path/to/polyplug/crates/polyplug_abi" }
polyplug_guest = { path = "path/to/polyplug/sdks/rust/guest" }
polyplug_utils = { path = "path/to/polyplug/crates/polyplug_utils" }
```

> When shipping a real plugin the `polyplug_abi`, `polyplug_guest`, and
> `polyplug_utils` crates will be available as crates.io dependencies. In a
> checkout of this repository, point `path` at the in-repo crates as above.

---

## Step 5 — Implement the plugin

Create `src/lib.rs`. The generated `contracts.rs` exposes a trait named after
the contract (`GreeterHelloGuestContract` for `greeter.Hello`). Implement that
trait on any struct, then export the factory the generated `create_instance`
calls for every host-created instance (`polyplug_create_<plugin>`):

```rust,ignore
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::GreeterHelloGuestContract;

struct Plugin {
    /// Host handle for this runtime, captured at instance creation.
    host: HostContext,
}

impl GreeterHelloGuestContract for Plugin {
    fn greet(&self, name: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `name` is a valid StringView live for the duration of this call.
        let s: &str = unsafe { to_str(&name) }?;
        self.host.alloc_string(&format!("Hello, {}!", s))
    }
}

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_my_greeter(host: HostContext) -> Box<dyn GreeterHelloGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}
```

Key points:

- `HostContext` is the per-instance handle to the host runtime. Store it in
  your struct and use `host.alloc_string(...)` / `host.log(...)`.
- `host.alloc_string` allocates the return value through the host allocator so
  the host can safely free it after the call. All strings returned across the
  ABI must go through this helper.
- `to_str` re-borrows a `StringView` as a `&str` without copying. The resulting
  reference is valid only for the duration of the call.
- `polyplug_abi_version` and `polyplug_create_<plugin>` are the two mandatory
  exports. The generated `init.rs` provides the actual ABI entry point
  (`polyplug_init`) that the loader calls; the factory is invoked per instance
  by the generated `create_instance`.

---

## Step 6 — Build

```bash
cargo build --release
```

The output on Linux is `target/release/libmy_greeter.so`. On macOS it is
`target/release/libmy_greeter.dylib`; on Windows, `target/release/my_greeter.dll`.

---

## Step 7 — Assemble the bundle

A bundle is a directory containing `manifest.toml` plus the artifact named by
its `[file]` entry:

```text
dist/my_greeter/
├── manifest.toml       (from generated/manifest.toml)
└── libmy_greeter.so    (from target/release/)
```

```bash
mkdir -p dist/my_greeter
cp generated/manifest.toml dist/my_greeter/
cp target/release/libmy_greeter.so dist/my_greeter/
```

The `manifest.toml` was generated in step 3. Never hand-edit it; it contains a
precomputed `id` field (`fnv1a_64(name)`) that the runtime verifies.

---

## Step 8 — Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_greeter
```

Expected output:

```text
OK: dist/my_greeter
```

This runs the same manifest checks the loader applies at runtime: the `id` is
consistent with the `name`, the artifact named in `[file]` exists for the
current platform, the version parses correctly, and the artifact extension
matches the declared runtime.

---

## Step 9 — Load the plugin from a host

On the host side, generate typed callers from the same `api.toml`:

```bash
polyplugc generate --api api.toml --lang rust --out host/generated
```

This writes `host/generated/host/host_callers.rs`, `types.rs`, and `mod.rs`.
The caller struct is named after the contract (`GreeterHelloContract` for
`greeter.Hello`); the contract-ID constant lives in `types.rs`
(`GREETER_HELLO_CONTRACT_ID`).

A minimal host that loads and calls the plugin:

```rust,ignore
use polyplug_abi::runtime::RuntimeConfig;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_abi::{Compatibility, GuestContractHandle};
use polyplug_native::{NativeConfig, NativeLoader};
use std::{path::PathBuf, sync::Arc};

#[path = "host/generated/mod.rs"]
mod generated;

use generated::host::host_callers::GreeterHelloContract;
use generated::host::types::GREETER_HELLO_CONTRACT_ID;

fn main() {
    let config = RuntimeConfig {
        compatibility: Compatibility::Strict,
        hot_reload_enabled: false,
        // on_reload / log callbacks and their user-data + log_max_level default to
        // None / null / Warn — set them only if you want reload/unload phase or
        // diagnostic callbacks.
        ..RuntimeConfig::default()
    };

    let runtime: Arc<Runtime> = Runtime::builder()
        .loader(NativeLoader::new(NativeConfig {}))
        .config(config)
        .build()
        .expect("runtime build");

    let plugins_dir = PathBuf::from("dist");
    let scan = scanner::scan_dirs(std::slice::from_ref(&plugins_dir));
    for (path, _manifest) in &scan.found {
        runtime.load_bundle(path).expect("load bundle");
    }

    let handle: GuestContractHandle = runtime
        .find_guest_contract(GREETER_HELLO_CONTRACT_ID, 0)
        .expect("contract not found");

    let mut caller = GreeterHelloContract::new(handle, Arc::clone(&runtime))
        .expect("caller init");

    let input = polyplug_abi::StringView {
        ptr: b"world".as_ptr(),
        len: 5,
    };
    let result_sv = caller.greet(input).expect("greet failed");

    // SAFETY: result_sv is a valid StringView in host-allocator memory.
    let result = unsafe {
        std::str::from_utf8(std::slice::from_raw_parts(result_sv.ptr, result_sv.len))
    }
    .expect("utf8");
    println!("{}", result);   // Hello, world!
}
```

> The full reference host (with multiple loaders, hot-reload, host contracts,
> and scanner integration) is in `examples/hosts/rust/`. See `EXAMPLES.md` for
> the complete gallery.

---

## Step 10 — (Optional) Sign the bundle

If the host enforces a signature policy, sign the assembled bundle so it loads
under `SignaturePolicy::Required`. Generate a keypair once and keep
`signing.key` secret:

```bash
polyplugc keygen --out keys/
polyplugc sign --bundle-dir dist/my_greeter --key keys/signing.key
polyplugc verify --bundle-dir dist/my_greeter
```

`sign` runs the same checks as `validate --bundle-dir`, then writes
`dist/my_greeter/bundle.sig` — a detached Ed25519 signature over a canonical
digest of every file in the bundle. The signer's **public** key travels inside
`bundle.sig`, so the host needs no key distribution to verify integrity (TOFU —
tamper detection, not author approval).

On the host, opt in to enforcement when building the runtime:

```rust,ignore
use polyplug_abi::runtime::SignaturePolicy;

let runtime = Runtime::builder()
    .loader(NativeLoader::new(NativeConfig {}))
    .config(config)
    .signature_policy(SignaturePolicy::Required)   // reject unsigned/tampered bundles
    .build()
    .expect("runtime build");
```

`Required` rejects an unsigned bundle with `LoaderError::UnsignedBundle` and a
tampered one with `LoaderError::SignatureVerificationFailed`; `WarnOnly` logs the
failure and continues; `Off` (the default) skips the check. Full detail:
[`TRUST_MODEL.md`](TRUST_MODEL.md) § Bundle Signing.

---

## What the generated names look like

The naming convention `polyplugc` applies for a contract `namespace.Type`:

| Generated item | Pattern | Example |
|---|---|---|
| Guest trait | `NamespaceTypeGuestContract` | `GreeterHelloGuestContract` |
| author factory | `polyplug_create_<plugin>()` | `polyplug_create_my_greeter()` |
| Host caller struct | `NamespaceTypeContract` | `GreeterHelloContract` |
| Contract-ID constant | `NAMESPACE_TYPE_CONTRACT_ID` | `GREETER_HELLO_CONTRACT_ID` |

---

## Available types

| `api.toml` type | Rust guest type | Notes |
|---|---|---|
| `StringView` | `StringView` | UTF-8 ptr+len; use `to_str()` to borrow, `HostContext::alloc_string()` to return |
| `Buffer` | `Buffer` | raw ptr+len; use `alloc_buffer()` to return |
| `bool` | `bool` | |
| `i32` / `u32` | `i32` / `u32` | |
| `i64` / `u64` | `i64` / `u64` | |
| `f32` / `f64` | `f32` / `f64` | |
| `void` | `()` (return only) | |
| user enum | generated `#[repr(u32)] enum` | |

---

## Next steps

- See `docs/WORKFLOW.md` for the complete host and guest pipelines with all
  languages.
- See `docs/EXAMPLES.md` for the full reference example gallery.
- Browse `examples/guests/` for working implementations in all six languages.
- Browse `examples/hosts/` for working host applications in all six languages.
- See `docs/TRUST_MODEL.md` § Bundle Signing for the full signing/verification
  model, the canonical digest, and how to layer key-pinning on top.
