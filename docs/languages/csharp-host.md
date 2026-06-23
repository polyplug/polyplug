# C# / .NET — Host (app)

A C# host embeds the polyplug runtime, registers a loader per guest language, loads
bundles, then resolves and calls contracts through generated, typed callers. A C#
host can load guests written in **any** language — it only needs the matching
loader package registered.

See [`examples/hosts/csharp/`](../../examples/hosts/csharp/) for the complete
reference host.

## Step 1 — Add the SDK + loader packages and the CLI

```bash
dotnet add package Polyplug.Host
dotnet add package Polyplug.Loaders.Native     # native cdylib guests
dotnet add package Polyplug.Loaders.Python     # Python guests
dotnet add package Polyplug.Loaders.Lua        # Lua guests
dotnet add package Polyplug.Loaders.Js         # JS (QuickJS) guests
dotnet add package Polyplug.Loaders.Dotnet     # .NET/C# guests

dotnet tool install -g Polyplug.Cli            # or: cargo install polyplugc
```

Your `.csproj` needs `AllowUnsafeBlocks` for the generated marshalling:

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

## Step 2 — Generate host callers

`polyplugc` reads the contract `.toml` and emits typed contract callers plus
the host-contract interface factories.

```bash
polyplugc generate --api api.toml --lang csharp --out host/generated
```

This produces, in the `Polyplug.Generated` namespace:

- one `*ContractCaller` per contract (e.g. `PipelineDecoderContractCaller`),
- `InterfaceFactories` for host contracts you provide to plugins,
- `ContractIds` / `Types` constants.

## Step 3 — Build the runtime and register loaders

Use `RuntimeBuilder` to configure and `Build()` the `Runtime`, then register a
loader per guest language you want to load.

```csharp
using Polyplug.Host;
using Polyplug.Loaders.Native;
using Polyplug.Loaders.Python;
using Polyplug.Loaders.Lua;
using Polyplug.Loaders.Js;
using Polyplug.Loaders.Dotnet;

var rt = new RuntimeBuilder()
    .OnReload(phase =>
    {
        if (phase.IsPreparing())
            Console.Error.WriteLine($"[HOT-RELOAD] {phase.BundleName} (0x{phase.BundleId:X16})");
    })
    .Build();

rt.RegisterNativeLoader();
rt.RegisterPythonLoader();
rt.RegisterLuaLoader();
rt.RegisterJsLoader();
rt.RegisterDotnetLoader();
```

`RuntimeBuilder` also exposes `.PluginDir(path)` (auto-load at build time),
`.SignaturePolicy(...)`, and `.TrustedKeys(...)`. Note that builder-time
auto-loading runs **before** the loaders above are registered, so a host that
loads non-native guests should register loaders first and load bundles
explicitly (Step 5).

## Step 4 — (Optional) Provide a host contract to plugins

If your contract `.toml` declares a `host_contract`, plugins can call back into
the host. Build the interface with the generated factory and register it. The
struct is copied into unmanaged memory the runtime keeps for its whole lifetime.

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

## Step 5 — Load bundles

A bundle is a directory containing a `manifest.toml`. Load each one explicitly:

```csharp
foreach (var bundleDir in Directory.GetDirectories(pluginPath)
             .Where(d => File.Exists(Path.Combine(d, "manifest.toml"))))
{
    rt.LoadBundle(bundleDir);
}
```

## Step 6 — Resolve and call a contract

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
provider for that contract is loaded. The caller caches the resolved interface
and re-resolves automatically across reloads/unloads via the runtime's revision
counter, so a cached caller never dispatches through a dangling interface.

## Runtime constraints

- A C# host can load guests of **any** language — register the matching loader.
- Native, Lua, and JS (QuickJS) bundles support hot-reload; Python and .NET do
  not.
- **CLR once per process:** the .NET CLR initializes once per process, so
  multiple `Runtime` instances in the same process share one CLR — .NET guests
  from different runtimes share the loader cache. For full isolation with .NET
  guests, use separate processes.

## See also

- [C# overview](csharp.md) · [C# — Guest (plugin)](csharp-guest.md)
- [`docs/QUICKSTART.md`](../QUICKSTART.md) for the canonical end-to-end flow.
