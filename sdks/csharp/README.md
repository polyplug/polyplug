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

## Host application

```csharp
using Polyplug;

var runtime = new RuntimeBuilder().PluginDir("./plugins").Build();

var decoder = PipelineDecoder.Create(runtime);
if (decoder.HasValue)
{
    var result = decoder.Value.Decode(input);
}
```

## In-process host registration

Generated guest bindings expose a typed `InProcessBundleFactory`. Pass ordinary
C# factories; the generated adapter creates the complete
`InProcessBundleRegistration` and retains its tables, delegates, factories, and
implementation objects in the runtime-owned resident:

```csharp
var bundle = transformer.InProcessBundleFactory.CreateInProcessBundle(
    host => new TransformerImpl(host));
ulong bundleId = runtime.RegisterInProcessBundle(bundle);
```

Registration is one synchronous native transaction. The runtime keeps the
resident rooted only after that transaction succeeds. Successful registration
transfers the bundle exactly once; the same bundle object cannot be registered
with another runtime. A rejected registration leaves it reusable.
`UnloadBundle(bundleId)` releases the resident only after logical unload
succeeds; a failed unload leaves the bundle and its managed state available to
existing callers.

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
