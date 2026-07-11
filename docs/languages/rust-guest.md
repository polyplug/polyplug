# Rust — Guest (plugin)

Write a polyplug plugin in Rust: generate the ABI glue, build a `cdylib`, and
assemble a bundle any polyplug host can load. New to polyplug? Start with the
[Quick Start](../QUICKSTART.md).

See also: [Rust overview](rust.md) · [Rust — Host (app)](rust-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and create a `cdylib` crate with the guest SDK dependencies:

```bash
cargo install polyplugc
```

```toml
# Cargo.toml
[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_abi   = "0.1"
polyplug_guest = "0.1"
polyplug_utils = "0.1"
```

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the library file per
platform, and which contracts this bundle implements. The `api` field points at
the shared `api.toml` contract (see `examples/api.toml`).

```toml
# bundle.toml
[bundle]
name = "my_plugin"
version = "1.0.0"
api = "../api.toml"   # path to api.toml, relative to this file
loader = "native"

[bundle.file]
linux.x86_64   = "libmy_plugin.so"
macos.aarch64  = "libmy_plugin.dylib"
macos.x86_64   = "libmy_plugin.dylib"
windows.x86_64 = "my_plugin.dll"

[[plugin]]
name = "my_plugin"
implements = ["pipeline.Decoder@1.0"]
```

`implements` names each contract as `<namespace>.<Name>@<major_version>`. Add one
`[[plugin]]` section per plugin in the bundle. To declare a runtime dependency on
another contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang rust --out generated
```

This writes the contract trait(s), instance machinery, `polyplug_init`, generated
types, and a `manifest.toml` under `generated/`. Re-run whenever
`bundle.toml` or `api.toml` changes; never edit generated files. For the emitted
symbol names, see [Generated names](../generated-names.md).

## 4. Implement the plugin

Wire in the generated module, implement the generated trait, and export the
factory and ABI version. Full source: `examples/guests/rust/decoder`.

```rust
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineDecoderGuestContract;

struct Plugin {
    host: HostContext,
}

impl PipelineDecoderGuestContract for Plugin {
    fn decode(&self, input: StringView) -> Result<StringView, GuestError> {
        // SAFETY: `input` is a valid StringView live for this call's duration,
        // per the ABI contract for dispatch arguments.
        let s: &str = unsafe { to_str(&input) }?;
        self.host.alloc_string(&format!("DECODED:{}", s.replace(',', "|")))
    }
}

#[unsafe(no_mangle)]
pub fn polyplug_create_my_plugin(host: HostContext) -> Box<dyn PipelineDecoderGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}
```

- Store the per-instance `HostContext` in your struct and allocate every return
  value through it (`host.alloc_string(...)`) — it is the
  [instance payload](../glossary.md).
- `to_str` views a `StringView` as `&str`, valid only for the call.
- `polyplug_abi_version` and `polyplug_create_<plugin>` are the two mandatory
  exports; the generated `init.rs` provides `polyplug_init`. Trait and factory
  names come from [Generated names](../generated-names.md).

## In-process Rust guest

An in-process guest is an ordinary Rust implementation object created by the
host and registered at runtime. Generate its typed adapters through the public
code-generation library; the CLI continues to generate the path-loaded guest ABI:

```rust
use polyplug_codegen::{
    GenerateConfig, Lang, RustGuestMode, Side, generate_rust_guest, write_output,
};

let output = generate_rust_guest(
    GenerateConfig {
        api_toml: "api.toml".into(),
        lang: Lang::Rust,
        side: Side::Guest,
        out_dir: "generated".into(),
    },
    RustGuestMode::InProcess {
        bundle_name: "my_plugin".to_owned(),
    },
)?;
write_output(&output, std::path::Path::new("generated"))?;
```

The generated module exposes `interfaces::InProcessFactories` and
`init::in_process_bundle`. Supply ordinary factory functions and register the
returned value by ownership:

```rust
fn create_my_plugin(host: HostContext) -> Box<dyn PipelineDecoderGuestContract> {
    Box::new(Plugin { host })
}

let bundle = generated::init::in_process_bundle(
    generated::interfaces::InProcessFactories {
        my_plugin: create_my_plugin,
    },
);
runtime.register_in_process_bundle(bundle)?;
```

`Runtime` owns the factory resident until logical unload. Each runtime has a
separate resident, so the same generated module can use different factories in
different runtimes. Use ordinary `unload_bundle` and re-registration operations
for lifecycle management.

## 5. Build

```bash
cargo build --release
```

The library lands in `target/release/` (e.g. `libmy_plugin.so` on Linux).

## 6. Assemble the bundle

Copy the built library next to the generated `manifest.toml`:

```
dist/my_plugin/
├── manifest.toml       # from generated/manifest.toml
└── libmy_plugin.so     # from target/release/
```

## 7. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_plugin
```

This checks the manifest is consistent, the declared file is present for the
current platform, and the bundle conforms to the ABI rules.

## 8. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_plugin --key keys/signing.key
polyplugc verify --bundle-dir dist/my_plugin
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/rust/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/rust/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/rust/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/rust/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/rust/validator/` | `pipeline.Validator` |
