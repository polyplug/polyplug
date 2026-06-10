# Quick Start — Write Your First Plugin in 10 Minutes

This guide walks through the full plugin-development flow end-to-end: define a
contract, generate the glue code, implement the plugin, build it, validate it.
The host-side embedding walkthrough follows. Everything here is based on a Rust
guest and a Rust host — the simplest, most self-contained path.

---

## Prerequisites

- Rust toolchain (stable, edition 2024)
- `polyplugc` built from the workspace:
  ```
  cargo build --release -p polyplugc
  ```
  The binary is at `target/release/polyplugc`. Add it to your `PATH` or invoke
  it with its full path throughout this guide.

---

## Step 1 — Define the contract

The contract file (`api.toml`) is the shared specification between app
developers and plugin developers. It declares what plugins must implement
(`[[plugin_contract]]`) and, optionally, what services the host offers back
(`[[host_contract]]`).

Create a directory for the project and write `api.toml`:

```toml
# api.toml
[[plugin_contract]]
name = "greeter.Hello"
version = "1.0.0"

[[plugin_contract.functions]]
name = "greet"
params = [{ name = "name", type = "StringView" }]
return = "StringView"
```

The `name` field uses a `namespace.Type` convention. Supported parameter and
return types include `StringView`, `Buffer`, `bool`, `i32`, `u32`, `i64`,
`u64`, `f32`, `f64`, and `void`. Enums defined in the same file can also be
referenced as types.

---

## Step 2 — Write `bundle.toml`

The `bundle.toml` is the plugin developer's manifest. It declares what this
bundle is, which language it targets, what file the loader will find at runtime,
and which contracts it implements.

```toml
# bundle.toml
[bundle]
name = "my_greeter"
version = "1.0.0"
api = "../api.toml"        # path to the api.toml, relative to this file
runtime = "native"

[bundle.file]
linux.x86_64   = "libmy_greeter.so"
macos.aarch64  = "libmy_greeter.dylib"
macos.x86_64   = "libmy_greeter.dylib"
windows.x86_64 = "my_greeter.dll"

[[plugin]]
name = "my_greeter"
version = "1.0.0"
implements = ["greeter.Hello@1.0"]
```

`implements` references the contract as `name@major_version`. The `runtime`
field must be one of: `native`, `lua`, `python`, `js-quickjs`, `dotnet`.

---

## Step 3 — Generate guest glue code

```
polyplugc generate --bundle bundle.toml --lang rust --out generated
```

This writes six files into `generated/`:

```
generated/
├── manifest.toml          ship-ready manifest (never edit by hand)
└── guest/
    ├── mod.rs
    ├── contracts.rs       the trait(s) you implement
    ├── interfaces.rs      set_*_impl() registration helpers
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
trait on any struct, then register it with `set_my_greeter_impl`:

```rust
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, alloc_string, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::GreeterHelloGuestContract;
use generated::interfaces::set_my_greeter_impl;

struct Plugin;

impl GreeterHelloGuestContract for Plugin {
    fn greet(&self, name: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `name` is a valid StringView live for the duration of this call.
        let s: &str = unsafe { to_str(&name) };
        alloc_string(&format!("Hello, {}!", s))
    }
}

static INIT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
fn init() {
    let _ = INIT.get_or_init(|| {
        let _ = set_my_greeter_impl(Box::new(Plugin));
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}

#[unsafe(no_mangle)]
unsafe extern "C" fn polyplug_user_init() {
    init();
}
```

Key points:

- `alloc_string` allocates the return value through the host allocator so the
  host can safely free it after the call. All strings returned across the ABI
  must go through this helper.
- `to_str` re-borrows a `StringView` as a `&str` without copying. The resulting
  reference is valid only for the duration of the call.
- `polyplug_abi_version` and `polyplug_user_init` are the two mandatory exports.
  The generated `init.rs` provides the actual ABI entry point
  (`polyplug_init`) that the loader calls; `polyplug_user_init` is the hook
  where your registration code runs.

---

## Step 6 — Build

```
cargo build --release
```

The output on Linux is `target/release/libmy_greeter.so`. On macOS it is
`target/release/libmy_greeter.dylib`; on Windows, `target/release/my_greeter.dll`.

---

## Step 7 — Assemble the bundle

A bundle is a directory containing `manifest.toml` plus the artifact named by
its `[file]` entry:

```
dist/my_greeter/
├── manifest.toml       (from generated/manifest.toml)
└── libmy_greeter.so    (from target/release/)
```

```
mkdir -p dist/my_greeter
cp generated/manifest.toml dist/my_greeter/
cp target/release/libmy_greeter.so dist/my_greeter/
```

The `manifest.toml` was generated in step 3. Never hand-edit it; it contains a
precomputed `id` field (`fnv1a_64(name)`) that the runtime verifies.

---

## Step 8 — Validate the bundle

```
polyplugc validate --bundle-dir dist/my_greeter
```

Expected output:

```
OK: dist/my_greeter
```

This runs the same manifest checks the loader applies at runtime: the `id` is
consistent with the `name`, the artifact named in `[file]` exists for the
current platform, the version parses correctly, and the artifact extension
matches the declared runtime. Catching mistakes here is cheaper than chasing a
load-time error inside the host.

---

## Step 9 — Load the plugin from a host

On the host side, generate typed callers from the same `api.toml`:

```
polyplugc generate --api api.toml --lang rust --out host/generated
```

This writes `host/generated/host/host_callers.rs`, `types.rs`, and `mod.rs`.
The caller struct is named after the contract (`GreeterHelloContract` for
`greeter.Hello`); the contract-ID constant lives in `types.rs`
(`GREETER_HELLO_CONTRACT_ID`).

A minimal host that loads and calls the plugin:

```rust
use polyplug_abi::runtime::RuntimeConfig;
use polyplug::loader::scanner;
use polyplug::runtime::Runtime;
use polyplug_abi::{Compatibility, GuestContractHandle, UnloadMode};
use polyplug_native::{NativeConfig, NativeLoader};
use std::path::PathBuf;

#[path = "host/generated/mod.rs"]
mod generated;

use generated::host::host_callers::GreeterHelloContract;
use generated::host::types::GREETER_HELLO_CONTRACT_ID;

fn main() {
    let config = RuntimeConfig {
        compatibility: Compatibility::Strict,
        unload_mode: UnloadMode::Retire,
        hot_reload_enabled: false,
        on_reload: None,
        on_reload_user_data: core::ptr::null_mut(),
    };

    let runtime: &'static Runtime = Box::leak(Box::new(
        Runtime::builder()
            .loader(NativeLoader::new(NativeConfig {}))
            .config(config)
            .build()
            .expect("runtime build"),
    ));

    let plugins_dir = PathBuf::from("dist");
    let scan = scanner::scan_dirs(std::slice::from_ref(&plugins_dir));
    for (path, _manifest) in &scan.found {
        runtime.load_bundle(path).expect("load bundle");
    }

    let handle: GuestContractHandle = runtime
        .find_guest_contract(GREETER_HELLO_CONTRACT_ID, 0)
        .expect("contract not found");

    let mut caller = GreeterHelloContract::new(handle, runtime.as_context_ptr())
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

## What the generated names look like

The naming convention `polyplugc` applies for a contract `namespace.Type`:

| Generated item | Pattern | Example |
|---|---|---|
| Guest trait | `NamespaceTypeGuestContract` | `GreeterHelloGuestContract` |
| impl setter | `set_<bundle_name>_impl()` | `set_my_greeter_impl()` |
| Host caller struct | `NamespaceTypeContract` | `GreeterHelloContract` |
| Contract-ID constant | `NAMESPACE_TYPE_CONTRACT_ID` | `GREETER_HELLO_CONTRACT_ID` |

---

## Available types

| `api.toml` type | Rust guest type | Notes |
|---|---|---|
| `StringView` | `StringView` | UTF-8 ptr+len; use `to_str()` to borrow, `alloc_string()` to return |
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
