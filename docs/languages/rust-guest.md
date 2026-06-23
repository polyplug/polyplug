# Rust — Guest (plugin)

This guide walks through writing a polyplug plugin in Rust, generating the ABI
glue, building a `cdylib`, and assembling a bundle ready for any polyplug host.

See also: [Rust overview](rust.md) · [Rust — Host (app)](rust-host.md)

---

## 1. Add dependencies

Create a new library crate and configure it as a `cdylib`:

```toml
# Cargo.toml
[package]
name = "my_plugin"
version = "1.0.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_abi   = "0.1"
polyplug_guest = "0.1"
polyplug_utils = "0.1"
```

## 2. Install `polyplugc`

```bash
cargo install polyplugc
```

## 3. Obtain `api.toml`

`api.toml` is the shared contract definition. Your plugin implements one or more
contracts declared there. Obtain it from the API owner or author your own; see
`examples/api.toml` for a reference.

## 4. Write `bundle.toml`

`bundle.toml` is the plugin developer's manifest. It declares the bundle name,
the target loader, the on-disk library file per platform, and which contracts
this bundle implements.

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

`implements` names each contract as `<namespace>.<Name>@<major_version>`. A
bundle can implement multiple contracts — add one `[[plugin]]` section per
plugin. To declare a runtime dependency on another contract, add a
`[[dependency]]` section:

```toml
[[dependency]]
kind     = "contract"
contract = "pipeline.Validator"
min_version = "1.0"
```

## 5. Generate guest glue

```bash
polyplugc generate --bundle bundle.toml --lang rust --out generated
```

This writes:

```
generated/
├── manifest.toml               ship-ready manifest (never edit)
└── guest/
    ├── mod.rs
    ├── contracts.rs            the trait(s) you implement
    ├── interfaces.rs           instance machinery + factory declarations
    ├── host_contract_callers.rs  (host-contract call helpers, if any)
    ├── init.rs                 polyplug_init ABI entry point
    └── types.rs                generated enums and structs
```

Re-run this command whenever `bundle.toml` or `api.toml` changes. Never edit
generated files — fix the contract and regenerate.

## 6. Implement the plugin

Create `src/lib.rs`. Wire in the generated module, implement the trait, and
export the factory and ABI version:

```rust
use polyplug_abi::StringView;
use polyplug_guest::{GuestError, HostContext, to_str};

#[path = "../generated/guest/mod.rs"]
mod generated;

use generated::contracts::PipelineDecoderGuestContract;

struct Plugin {
    /// Host handle captured at instance creation — used for host-allocator
    /// calls and any host-contract calls. Stored per instance; no globals.
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

/// Factory called by the generated `create_instance` for every host-created
/// instance. The implementation travels in `GuestContractInstance.data`.
#[unsafe(no_mangle)]
pub fn polyplug_create_my_plugin(host: HostContext) -> Box<dyn PipelineDecoderGuestContract> {
    Box::new(Plugin { host })
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_abi_version() -> u32 {
    polyplug_abi::POLYPLUG_ABI_VERSION
}
```

Key points:

- The trait name follows the pattern `<Namespace><Name>GuestContract`
  (e.g. `PipelineDecoderGuestContract` for `pipeline.Decoder`).
- The factory name follows `polyplug_create_<plugin_name>` where
  `plugin_name` matches the `name` field in the `[[plugin]]` section.
- `HostContext` is the per-instance host handle. Store it in your struct and use
  `host.alloc_string(...)` to allocate return values through the host allocator.
  Do not store the host pointer in any global or thread-local.
- `to_str` re-borrows a `StringView` as `&str` without copying. The reference
  is valid only for the duration of the call.
- `polyplug_abi_version` and `polyplug_create_<plugin>` are the two mandatory
  exports. The generated `init.rs` provides `polyplug_init`.

### Calling a host contract from a guest

If the `api.toml` defines a host-provided contract (such as a logging service),
the generated `host_contract_callers.rs` provides a typed caller. See
`examples/guests/rust/reporter/src/lib.rs` for the full pattern —
`HostLoggerCaller::from_host` resolves the host contract at call time, and
`logger.log(...)` / `logger.log_with_level(...)` send messages back to the host.

## 7. Build

```bash
cargo build --release
```

The compiled library lands in `target/release/` (e.g. `libmy_plugin.so` on
Linux).

## 8. Assemble the bundle

Copy the built library into a bundle directory alongside the generated
`manifest.toml`:

```
dist/my_plugin/
├── manifest.toml       # from generated/manifest.toml
└── libmy_plugin.so     # from target/release/
```

## 9. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/my_plugin
```

This checks that the manifest is consistent, the declared file is present for
the current platform, and the bundle conforms to the ABI rules.

## 10. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/my_plugin --key keys/signing.key
polyplugc verify --bundle-dir dist/my_plugin
```

`sign` validates the bundle, then writes `dist/my_plugin/bundle.sig` — a
detached Ed25519 signature. The signer's public key travels in `bundle.sig`, so
the host needs no key distribution to verify integrity.

## Generated names reference

For a contract `namespace.Name@major`:

| Item | Generated name |
|---|---|
| Guest trait | `NamespaceNameGuestContract` |
| Contract-ID constant | `NAMESPACE_NAME_CONTRACT_ID` |
| Factory export | `polyplug_create_<plugin_name>` |

## Full reference

The five Rust guest plugins in `examples/guests/rust/` cover the full range:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/rust/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/rust/transformer/` | `data.Transformer` |
| encoder | `examples/guests/rust/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/rust/reporter/` | `data.Reporter` (calls host contract) |
| validator | `examples/guests/rust/validator/` | `pipeline.Validator` |

The reporter plugin is the most instructive: it demonstrates calling a
host-provided contract (`host.logger`) from inside the guest implementation.
The transformer plugin demonstrates declaring a runtime dependency on another
contract (`pipeline.Validator`).
