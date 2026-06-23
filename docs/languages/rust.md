# Rust — polyplug

Rust is a first-class host **and** guest in polyplug. As a host it links the
runtime crate directly — no FFI hop — giving registry lookups and dispatch
overhead in the low single-digit nanoseconds (~2.4 ns measured). As a guest it
compiles to a native `cdylib` and shares the same ABI types, so the host and
plugin speak the same language at the type level.

## Install

**CLI** — generates host callers and guest glue from a `.toml` contract:

```bash
cargo install polyplugc
```

**Host runtime** — add to your app's `Cargo.toml`:

```toml
[dependencies]
polyplug        = "0.1"   # core runtime (Runtime, builder, scanner)
polyplug_abi    = "0.1"   # ABI types (StringView, GuestContractHandle, …)
polyplug_native = "0.1"   # loader for native (.so / .dylib / .dll) bundles
# add whichever language loaders your host needs:
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

- **[Rust — Host (app)](rust-host.md)** — embed the runtime, load plugins of
  any language, call contracts.
- **[Rust — Guest (plugin)](rust-guest.md)** — write a Rust plugin, generate
  glue, build a `cdylib`, assemble and validate the bundle.

## Examples

Working, tested code lives in the repository:

- Host: `examples/hosts/rust/` (`src/main.rs`) — the primary reference host;
  registers all six loaders and runs the full five-stage pipeline.
- Guests: `examples/guests/rust/` — five `cdylib` plugins implementing the
  pipeline contracts (`decoder`, `transformer`, `encoder`, `reporter`,
  `validator`).

Generated host callers for the examples are at
`examples/hosts/rust/generated/`; generated guest glue for each plugin is at
`examples/guests/rust/<plugin>/generated/`.
