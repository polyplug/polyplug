# polyplug-guest — Rust Guest SDK

`polyplug_guest` gives Rust plugin authors the helpers — `HostContext`,
`GuestError`, `to_str`, the host allocator — needed to implement a
[polyplug](https://github.com/polyplug/polyplug) contract as a native `cdylib`
that any polyplug host can load.

You are writing a plugin, not a host. For the host side, embed the `polyplug`
crate instead — see the [Rust host guide][host].

## Install

Your plugin must be a `cdylib` and depend on the ABI types plus the guest
helpers (never on the host-side `polyplug` crate):

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_abi   = "0.1"   # ABI types shared with the host
polyplug_guest = "0.1"   # guest helpers: HostContext, GuestError, to_str, …
polyplug_utils = "0.1"   # bundle_id / contract_id hash utilities
```

Install the CLI to generate the bindings:

```bash
cargo install polyplugc
```

## Generate bindings

`polyplugc` emits the trait, vtable, and `polyplug_init` from your contract:

```bash
polyplugc generate --bundle bundle.toml --lang rust --out ./generated
```

## Shared generated declarations

The command remains unified by default. For an application-internal Rust
plugin, generate DomainTypes and GuestContracts once in a `common` crate and
ImportOnly from the binding crate; see the [canonical split-output guide][codegen].

## Internal plugins

The default command emits external plugin guest bindings. Generate the explicit
internal profile when the application supplies Rust implementations:

```bash
polyplugc generate --bundle bundle.toml --internal --lang rust --out ./generated
```

The generated profile exports `guest::domain::InternalProviders` and
`guest::init::register`. Factories may capture application configuration and
produce a fresh domain implementation per guest instance:

```rust
use generated::guest::{
    domain::{InternalProviderFactory, InternalProviders},
    init::register,
};

let registration = register(
    runtime.clone(),
    InternalProviders {
        platform_plugin_platform_plugin: InternalProviderFactory::new(|| Box::new(PlatformPlugin::new())),
    },
)?;
let bundle_id = registration.bundle_id;
```

`register` consumes provider input on the attempt, stages generated guest
provider bindings, validates the exact manifest provider/function/dependency
set, and atomically commits. Its named generated host caller bindings use the
committed handles, exactly as for external plugin discovery. Unload only after
callers and instances drain; successful unload invalidates callers and releases
the generated provider state.

## Implement

Implement the generated `<Contract>GuestContract` trait and export the author
factory. The `HostContext` is captured per instance — there is no process-wide
host storage:

```rust
use polyplug_abi::StringView;
use polyplug_guest::{HostContext, to_str};
use generated::contracts::PipelineDecoderGuestContract;

struct Decoder { host: HostContext }

impl PipelineDecoderGuestContract for Decoder {
    fn decode(&self, input: StringView) -> StringView {
        let s = to_str(input).unwrap_or_default();
        self.host.alloc_string(&format!("DECODED:{s}")).unwrap_or(StringView::null())
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn polyplug_create_my_plugin(host: HostContext) -> Box<dyn PipelineDecoderGuestContract> {
    Box::new(Decoder { host })
}
```

Build a release `cdylib` and drop it next to the generated `manifest.toml` to
form a bundle.

## Learn more

- [Rust — Guest guide][guest] — the full generate → implement → build → bundle walkthrough
- [Rust — Host guide][host] — embed the runtime in a Rust app
- [Rust overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/rust.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/rust-guest.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/rust-host.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
[codegen]: https://github.com/polyplug/polyplug/blob/main/docs/CODE_GENERATION.md
