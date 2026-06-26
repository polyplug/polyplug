# C# / .NET — Host (app)

Embed the polyplug runtime in a C# application, load plugins written in any
supported language, and call their contracts through generated typed callers.

See also: [C# overview](csharp.md) · [C# — Guest (plugin)](csharp-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and add the host SDK plus a loader package per guest language:

```bash
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc

dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native    # always needed for native bundles
dotnet add package Polyplug.Loaders.Python    # + Python guests
dotnet add package Polyplug.Loaders.Lua       # + Lua guests
dotnet add package Polyplug.Loaders.Js        # + JS (QuickJS) guests
dotnet add package Polyplug.Loaders.Dotnet    # + .NET / C# guests
```

The generated marshalling needs `AllowUnsafeBlocks` in your `.csproj`:

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
  </PropertyGroup>
</Project>
```

A C# host can load guests written in any supported language — register the
matching loader when you build the runtime.

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang csharp --out host/generated
```

Writes the typed callers into `Polyplug.Generated`. Never edit them — see
[Generated names](../generated-names.md).

## 3. Build the runtime

Use `RuntimeBuilder` to `Build()` the runtime, then register one loader per guest
language you want to load.

```csharp
using Polyplug.Host;
using Polyplug.Loaders.Native;
using Polyplug.Loaders.Python;
using Polyplug.Loaders.Lua;
using Polyplug.Loaders.Js;
using Polyplug.Loaders.Dotnet;

var rt = new RuntimeBuilder().Build();

rt.RegisterNativeLoader();
rt.RegisterPythonLoader();
rt.RegisterLuaLoader();
rt.RegisterJsLoader();
rt.RegisterDotnetLoader();
```

Register loaders before loading any non-native bundle. (`.PluginDir(path)`
auto-loads at build time, before loaders exist — native only.) The full
multi-loader host is `examples/hosts/csharp/Program.cs`.

### Hot-reload callback (optional)

Pass `.OnReload(...)` to observe reload phases. Hot-reload applies to native,
Lua, and JS bundles — Python and .NET do not reload. See
[Hot Reload](../HOT_RELOAD_DESIGN.md) and
[Reload limitations](../RELOAD_LIMITATIONS.md).

```csharp
var rt = new RuntimeBuilder()
    .OnReload(phase =>
    {
        if (phase.IsPreparing())
            Console.Error.WriteLine($"[reload] preparing {phase.BundleName}");
        else if (phase.IsReloaded())
            Console.Error.WriteLine($"[reload] reloaded {phase.BundleName}");
        else if (phase.IsFailed())
            Console.Error.WriteLine($"[reload] failed {phase.BundleName}: {phase.Reason}");
    })
    .Build();
```

### Signature policy (optional)

```csharp
var rt = new RuntimeBuilder()
    .SignaturePolicy(SignaturePolicy.Required)
    .Build();
```

`Required` rejects unsigned or tampered bundles; `.TrustedKeys(...)` pins
accepted signers. See the [Trust Model](../TRUST_MODEL.md).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), build it with the generated factory and register it before loading
bundles:

```csharp
using System.Runtime.InteropServices;
using Polyplug.Abi;
using Polyplug.Generated;

HostContractInterface loggerIface =
    InterfaceFactories.CreateHostLoggerInterface(new ConsoleLogger());
nint loggerIfacePtr = Marshal.AllocHGlobal(Marshal.SizeOf<HostContractInterface>());
Marshal.StructureToPtr(loggerIface, loggerIfacePtr, false);
rt.RegisterHostContract(loggerIfacePtr);

class ConsoleLogger : IHostLogger
{
    public void Log(string message) => Console.WriteLine($"[plugin] {message}");
    public void LogWithLevel(ref LogLevel level, string message) =>
        Console.WriteLine($"[plugin][{level}] {message}");
}
```

## 5. Load bundles

A bundle is a directory containing a `manifest.toml`. Load each one explicitly;
`LoadBundle` dispatches to the loader matching the bundle's `loader` field.

```csharp
foreach (var bundleDir in Directory.GetDirectories(pluginPath)
             .Where(d => File.Exists(Path.Combine(d, "manifest.toml"))))
{
    rt.LoadBundle(bundleDir);
}
```

## 6. Call a contract

Create a generated caller from the runtime, marshal inputs with
`PinnedStringView`, and read `StringView` results with `StringViewHelper`.

```csharp
using Polyplug.Guest;   // PinnedStringView, StringViewHelper

if (PipelineDecoderContractCaller.Create(rt) is { } decoder)
{
    using (decoder)
    using (var input = new PinnedStringView("name,value,42"))
    {
        StringView result = decoder.Decode(input.View);
        Console.WriteLine(StringViewHelper.ToString(result));   // DECODED:name|value|42
    }
}
```

Each `*ContractCaller` is `IDisposable`; `Create(rt)` returns `null` when no
provider for that contract is loaded.

.NET guests do not hot-reload — see [Reload limitations](../RELOAD_LIMITATIONS.md).

## Full reference

`examples/hosts/csharp/Program.cs` registers all five loaders, a host contract,
scans a directory, loads every bundle, and runs a five-stage pipeline end to
end. Generated callers live at `examples/hosts/csharp/generated/`.
