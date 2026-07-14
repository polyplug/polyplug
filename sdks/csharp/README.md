# polyplug C# SDK

Build polyplug hosts and plugins in C#/.NET. The host side wraps the native
runtime through P/Invoke; the guest side compiles to a .NET assembly the .NET
loader runs. Strings transcode UTF-16 ↔ UTF-8 at the boundary. Requires
.NET 10.0+.

## Install

```bash
# Host application (+ a loader package per guest language you support)
dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native    # native cdylib bundles
# Polyplug.Loaders.{Python,Lua,Js,Dotnet} as needed

# Plugin author
dotnet add package Polyplug.Guest
```

Install the CLI to generate bindings:

```bash
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc
```

## Generate bindings

```bash
polyplugc generate --bundle bundle.toml --lang csharp --out ./generated
```

## Shared generated declarations

The command remains unified by default. For a shared .NET declaration assembly,
emit or import DomainTypes as `Common.Domain` and GuestContracts as
`Common.GuestContracts`; see the [canonical split-output guide][codegen].


## Host application

```csharp
using Polyplug.Host;

var runtime = new RuntimeBuilder().PluginDir("./plugins").Build();

var decoder = PipelineDecoderCaller.Create(runtime);
if (decoder.HasValue)
{
    var result = decoder.Value.Decode(input);
}
```

## Internal plugins

The default command emits external plugin bindings. Generate the separate
internal profile for one bundle when the application supplies its providers:

```bash
polyplugc generate --bundle bundle.toml --internal --lang csharp --out ./generated
```

The generated bundle-identity namespace exposes `RegistrationInput`,
`Registration`, and `InternalPlugin.Register`. Pass ordinary C# factories in the
generated input and retain the returned registration:

```csharp
Registration registration = InternalPlugin.Register(runtime, input);
ulong bundleId = registration.BundleId;
```

`Register` consumes `input` on every attempt, stages all generated guest
provider bindings, validates the exact manifest set, and atomically commits.
The `Registration` contains named generated host caller bindings made from the
committed handles. Call them exactly as callers found after external plugin loading.
Before `UnloadBundle(bundleId)`, the application must quiesce every caller and
destroy all guest instances for the bundle. A successful unload invalidates
those callers and releases the managed provider roots; callers must not be used
afterward.

## Plugin author

Implement the generated `I<Contract>GuestContract` and register a factory at
module load. The factory receives the `HostApi` pointer per instance — the host
handle lives in the instance, never in a static:

```csharp
using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

public sealed class DecoderImpl(IntPtr host) : IPipelineDecoderGuestContract
{
    public StringView Decode(StringView input) =>
        PolyplugHost.AllocString(host, $"DECODED:{StringViewHelper.ToString(input)}");
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register() =>
        DecoderInterfaces.SetDecoderFactory(host => new DecoderImpl(host));
}
```

> The .NET CLR initializes once per process: `Runtime` instances in the same
> process share one CLR. Use separate processes for full isolation.

## Learn more

- [C# — Host guide][host] — embed the runtime, hot-reload, bundle signing & key pinning
- [C# — Guest guide][guest] — generate → implement → build → bundle
- [C# overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/csharp.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/csharp-host.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/csharp-guest.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
[codegen]: https://github.com/polyplug/polyplug/blob/main/docs/CODE_GENERATION.md
