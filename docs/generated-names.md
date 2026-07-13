# Generated names

`polyplugc generate` derives every symbol it emits from three inputs: the
contract (`namespace.Type@major` in `api.toml`), the contract's methods, and the
`[[plugin]] name` in `bundle.toml`. This page is the one place those name
mappings live; the per-language guides link here instead of repeating them.

For terms (contract, guest contract, host contract, descriptor, factory), see
the [glossary](glossary.md).

## Worked example

Every table below resolves the same contract so you can read the pattern against
a concrete name:

| Input | Value |
|---|---|
| Contract | `pipeline.Decoder@1.0` |
| Namespace | `pipeline` |
| Type | `Decoder` |
| Method | `decode` |
| `[[plugin]] name` | `decoder` |
| Host contract called back into | `host.logger` |

Three derivations recur across languages:

- **`{Ns}{Type}`** — the namespace and type each PascalCased and concatenated:
  `pipeline.Decoder` → `PipelineDecoder`.
- **`{NS_TYPE}`** — the contract name uppercased, `.` and `-` replaced with `_`:
  `pipeline.Decoder` → `PIPELINE_DECODER`.
- **`{PLUGIN}`** — the `[[plugin]] name` uppercased, dots replaced with `_`:
  `decoder` → `DECODER`.

### Contract-ID constant

The runtime identifies a contract by a 64-bit id. `generate` names that id
constant after the **contract** — `{NS_TYPE}_CONTRACT_ID`,
`PIPELINE_DECODER_CONTRACT_ID` — everywhere it appears, so one value has one
name:

- **Host / types files (every language)** — the constant a **host** author uses
  to resolve a contract (e.g. `find_guest_contract`).
- **Guest interfaces file (Rust, C++, C# only)** — the same
  `{NS_TYPE}_CONTRACT_ID` name, referenced internally by the generated interface;
  the per-implementation symbols beside it (interface, function table, instance
  thunks) are `{PLUGIN}`-named because they belong to the plugin, but the id
  belongs to the contract.

Python, Lua, and JS guests emit **no** id constant — they wire the id through the
generated class, factory setter, or descriptor instead.

## Explicit internal plugin profile

`polyplugc generate --bundle bundle.toml --internal --lang <language> --out <dir>`
emits both generated guest provider bindings and generated host caller bindings
under a bundle-identity namespace. The registrar consumes provider input, returns
the committed `BundleId`, and exposes named callers constructed from exact
committed handles:

| Language | Generated registration surface |
|---|---|
| Rust | `guest::domain::{InternalProviderFactory, InternalProviders}` + `guest::init::register` |
| C++ | `internal_plugin::register_internal_plugin` → `InternalPluginRegistration` |
| C# | `InternalPlugin.Register` + `RegistrationInput` → `Registration` |
| Python | `internal.py`: `InternalPluginProviders` + `register` → `InternalPluginRegistration` |
| Lua | `guest/internal.lua`: `providers` + `register` |
| JavaScript | `internal.ts`: `InternalProviders` + `register` → `Registration` |

These are generated API symbols. The product terms are **internal plugin**,
**external plugin**, **generated guest provider bindings**, and **generated host
caller bindings**.

## Rust (`--lang rust`)

| Piece | Pattern | Example |
|---|---|---|
| Guest trait (you implement) | `{Ns}{Type}GuestContract` | `PipelineDecoderGuestContract` |
| Trait method | api method name (snake_case) | `decode` |
| Factory export (you write) | `polyplug_create_{plugin}` | `polyplug_create_decoder` |
| Guest contract-ID constant | `{NS_TYPE}_CONTRACT_ID` | `PIPELINE_DECODER_CONTRACT_ID` |
| Host caller (from `--api`) | `{Ns}{Type}Contract` | `PipelineDecoderContract` |
| Guest→host caller | `Host{Name}Caller` | `HostLoggerCaller` |

## C++ (`--lang cpp`)

| Piece | Pattern | Example |
|---|---|---|
| Guest contract base (you subclass) | `{Ns}{Type}GuestContract` | `PipelineDecoderGuestContract` |
| Method (override) | api method name | `decode` |
| Factory export (you write) | `polyplug_create_{plugin}`, in namespace `polyplug_plugin` | `polyplug_create_decoder` |
| Host caller (from `--api`) | `{Ns}{Type}Contract` | `PipelineDecoderContract` |
| Guest→host caller (`host_contracts.hpp`) | `Host{Name}Contract` | `HostLoggerContract` |

## C# (`--lang csharp`)

Emitted into the `Polyplug.Generated` namespace.

| Piece | Pattern | Example |
|---|---|---|
| Guest interface (you implement) | `I{Ns}{Type}GuestContract` | `IPipelineDecoderGuestContract` |
| Method | api method name, PascalCase | `Decode` |
| Factory hook | `{Type}Interfaces.Set{Type}Factory(...)` | `DecoderInterfaces.SetDecoderFactory` |
| Host caller (from `--api`) | `{Ns}{Type}ContractCaller` | `PipelineDecoderContractCaller` |
| Guest→host caller (`HostContracts.cs`) | `Host{Name}Contract` | `HostLoggerContract` |

## Python (`--lang python`)

| Piece | Pattern | Example |
|---|---|---|
| Guest base class (you subclass) | `{PLUGIN}{Ns}{Type}Plugin` | `DECODERPipelineDecoderPlugin` |
| Method (override) | api method name (snake_case) | `decode` |
| Factory setter | `set_{plugin}_factory(...)` | `set_decoder_factory` |
| Host caller (from `--api`) | `{Ns}{Type}ContractCaller` | `PipelineDecoderContractCaller` |
| Guest→host caller (`host_contracts.py`) | `Host{Name}Contract` | `HostLoggerContract` |

## Lua (`--lang lua`)

A Lua guest returns an instance table from its factory — there is no generated
contract type. The `contracts.lua` module exposes the factory setter.

| Piece | Pattern | Example |
|---|---|---|
| Factory setter (on the `contracts` module) | `set_{plugin}_factory(fn)` | `set_decoder_factory` |
| Instance method | api method name | `decode` |
| Host caller (from `--api`) | `{Ns}{Type}Contract` | `PipelineDecoderContract` |
| Guest→host caller (`host_contracts.lua`) | `Host{Name}Contract` | `HostLoggerContract` |

## JavaScript / QuickJS (`--lang js-quickjs`)

A QuickJS guest returns an object whose methods are positional (`fn0`, `fn1`, …
in the contract's declared method order).

| Piece | Pattern | Example |
|---|---|---|
| Interface const (`contracts.ts`) | `{PLUGIN}_INTERFACE` | `DECODER_INTERFACE` |
| Descriptor const (`contracts.ts`) | `{PLUGIN}_DESCRIPTOR` | `DECODER_DESCRIPTOR` |
| Factory setter | `set{Type}Factory(fn)` | `setDecoderFactory` |
| Instance method | positional `fn{N}` | `fn0` (for `decode`) |
| Host caller (from `--api`) | `{Ns}{Type}Contract` | `PipelineDecoderContract` |
| Guest→host caller (`host_contracts.ts`) | `Host{Name}Contract` | `HostLoggerContract` |
