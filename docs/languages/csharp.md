# C# / .NET — polyplug

C# / .NET works as both a host and a guest. As a host it embeds the runtime
through the `Polyplug.Host` SDK and loads plugins of any language. As a guest it
compiles to a `.NET` class library that any polyplug host can load. For measured
overhead, see [Performance](../PERFORMANCE.md).

## Install

**CLI** — generates host callers and guest glue from an `api.toml` contract:

```bash
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc
```

**Host SDK** — add to your app, plus a loader package per guest language:

```bash
dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native    # native cdylib bundles
dotnet add package Polyplug.Loaders.Python    # Python guests
dotnet add package Polyplug.Loaders.Lua       # Lua guests
dotnet add package Polyplug.Loaders.Js        # JS (QuickJS) guests
dotnet add package Polyplug.Loaders.Dotnet    # .NET / C# guests
```

**Guest SDK** — add to your plugin's class-library project:

```bash
dotnet add package Polyplug.Guest
```

## Guides

- **[C# — Host (app)](csharp-host.md)** — embed the runtime, register loaders,
  load plugins of any language, call contracts.
- **[C# — Guest (plugin)](csharp-guest.md)** — write a C# plugin, generate glue,
  build a `.dll`, assemble and validate the bundle.

New to polyplug? Start with the [Quick Start](../QUICKSTART.md).

## Examples

- Host: `examples/hosts/csharp/` (`Program.cs`) — registers all five loaders and
  runs the full five-stage pipeline.
- Guests: `examples/guests/csharp/` — five class-library plugins (`decoder`,
  `transformer`, `encoder`, `reporter`, `validator`).

Generated code lives under `examples/hosts/csharp/generated/` (host callers) and
`examples/guests/csharp/<plugin>/generated/` (guest glue).
