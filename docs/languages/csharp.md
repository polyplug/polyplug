# C# / .NET — polyplug

C# / .NET is a **first-class polyplug language**: a `.dll` can be the **host**
that embeds the runtime, registers loaders, and loads + calls plugins written in
any language — and it can equally be the **guest**, a `.NET` class library that
implements a contract and gets loaded by a host of any language.

Both directions go through the exact same frozen C ABI. The `polyplugc` CLI reads
a `.toml` contract and emits the C# glue (`StringView` marshalling, the
`polyplug_init` trampoline, typed contract callers) so you never hand-write the
ABI surface.

## Install

```bash
# polyplugc CLI — emits the C# glue from a .toml contract
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc

# Host SDK + the loaders you need
dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native    # load native cdylib bundles
dotnet add package Polyplug.Loaders.Python    # + Python guests
dotnet add package Polyplug.Loaders.Lua       # + Lua guests
dotnet add package Polyplug.Loaders.Js        # + JS (QuickJS) guests
dotnet add package Polyplug.Loaders.Dotnet    # + .NET/C# guests

# Guest SDK — for writing a plugin
dotnet add package Polyplug.Guest
```

## Guides

- **[C# — Host (app)](csharp-host.md)** — embed the runtime, register loaders,
  load bundles, resolve and call a contract.
- **[C# — Guest (plugin)](csharp-guest.md)** — implement a contract, build the
  `.dll`, assemble and validate the bundle.

## Examples

- Host: [`examples/hosts/csharp/`](../../examples/hosts/csharp/) — `Program.cs`
  drives all five contracts and registers every loader.
- Guests: [`examples/guests/csharp/`](../../examples/guests/csharp/) — five
  class-library plugins (`decoder`, `transformer`, `encoder`, `reporter`,
  `validator`).
