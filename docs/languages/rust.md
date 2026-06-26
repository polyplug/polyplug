# Rust — polyplug

Rust works as both a host and a guest. As a host it links the runtime crate
directly. As a guest it compiles to a native `cdylib`, and host and plugin share
the same ABI types. For measured overhead, see [Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
cargo install polyplugc
```

**Host runtime** — add to your app's `Cargo.toml`:

```toml
[dependencies]
polyplug        = "0.1"   # core runtime (Runtime, builder, scanner)
polyplug_abi    = "0.1"   # ABI types (StringView, GuestContractHandle, …)
polyplug_native = "0.1"   # loader for native (.so / .dylib / .dll) bundles
# add a loader per guest language you want to support:
polyplug_js     = "0.1"   # JavaScript (QuickJS) bundles
polyplug_lua    = "0.1"   # Lua bundles
polyplug_python = "0.1"   # Python bundles
polyplug_dotnet = "0.1"   # .NET / C# bundles
```

**Guest SDK** — add to your plugin crate's `Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
polyplug_abi   = "0.1"   # ABI types shared with the host
polyplug_guest = "0.1"   # guest helpers: HostContext, GuestError, to_str, …
polyplug_utils = "0.1"   # bundle_id / contract_id hash utilities
```

## Guides

- **[Rust — Host (app)](rust-host.md)** — embed the runtime, load plugins of any
  language, call contracts.
- **[Rust — Guest (plugin)](rust-guest.md)** — write a Rust plugin, generate
  glue, build a `cdylib`, assemble and validate the bundle.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/rust/` (`src/main.rs`) — registers all five loaders and
  runs the full five-stage pipeline.
- Guests: `examples/guests/rust/` — five `cdylib` plugins (`decoder`,
  `transformer`, `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/rust/generated/` (host callers) and
`examples/guests/rust/<plugin>/generated/` (guest glue).
