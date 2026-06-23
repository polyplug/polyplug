# C# / .NET — Guest (plugin)

A C# guest is a .NET class library that implements a contract and is loaded by a
polyplug host of any language. `polyplugc` emits the glue (the `polyplug_init`
trampoline, `StringView` marshalling, the factory hook); you implement the
contract interface and build a `.dll`.

This mirrors [`docs/QUICKSTART.md`](../QUICKSTART.md) in C#. See
[`examples/guests/csharp/`](../../examples/guests/csharp/) for five complete
plugins.

## Step 1 — Add the Guest SDK and the CLI

```bash
dotnet add package Polyplug.Guest
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc
```

## Step 2 — Define the contract (`api.toml`)

The contract is shared between host and guest. For a decoder:

```toml
[[plugin_contract]]
name = "pipeline.Decoder"
version = "1.0.0"

[[plugin_contract.functions]]
name = "decode"
params = [{ name = "input", type = "StringView" }]
return = "StringView"
```

## Step 3 — Write `bundle.toml`

Declare `loader = "dotnet"` and point `file` at the built assembly:

```toml
[bundle]
name = "csharp_decoder"
version = "1.0.0"
api = "../api.toml"
loader = "dotnet"
file = "decoder.dll"

[[plugin]]
name = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

## Step 4 — Generate guest glue code

```bash
polyplugc generate --bundle bundle.toml --lang csharp --out generated
```

This emits, in the `Polyplug.Generated` namespace:

- `IPipelineDecoderGuestContract` — the interface you implement,
- `DecoderInterfaces.SetDecoderFactory(...)` — the factory registration hook,
- the `polyplug_init` trampoline (`Init.cs`) and bundle constants.

## Step 5 — Set up the class-library project

```xml
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <Nullable>enable</Nullable>
    <ImplicitUsings>enable</ImplicitUsings>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <AssemblyName>decoder</AssemblyName>
  </PropertyGroup>
</Project>
```

`AssemblyName` must match the `file` in `bundle.toml` (`decoder.dll`).

## Step 6 — Implement the contract

Implement the generated interface. Read `StringView` inputs with
`StringViewHelper`; return owned strings via `PolyplugHost.AllocString` (which
uses the **host allocator** — never the managed heap — for cross-boundary
memory). Register the factory from a `[ModuleInitializer]`; the factory receives
the `HostApi` pointer for each instance and stores it as an instance field.

```csharp
using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

namespace Decoder;

public sealed class DecoderPlugin : IPipelineDecoderGuestContract
{
    private readonly IntPtr _host;

    public DecoderPlugin(IntPtr host) => _host = host;

    public StringView Decode(StringView input)
    {
        string s = StringViewHelper.ToString(input).Replace(',', '|');
        return PolyplugHost.AllocString(_host, $"DECODED:{s}");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        DecoderInterfaces.SetDecoderFactory(host => new DecoderPlugin(host));
    }
}
```

The captured `_host` pointer also lets you call host contracts —
`PolyplugHost.Log(_host, level, scope, message)` invokes the host logger.

## Step 7 — Build

```bash
dotnet build -c Release      # → decoder.dll
```

## Step 8 — Assemble the bundle

Copy `decoder.dll`, the generated `manifest.toml`, and **every dependency
assembly** the plugin needs into one bundle directory beside `manifest.toml` —
the dotnet loader resolves assemblies from the bundle directory.

```
dist/csharp_decoder/
├── manifest.toml
├── decoder.dll
└── <dependency assemblies, if any>
```

## Step 9 — Validate (and optionally sign)

```bash
polyplugc validate --bundle-dir dist/csharp_decoder
```

If the host enforces a signature policy, sign the assembled bundle:

```bash
polyplugc sign   --bundle-dir dist/csharp_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/csharp_decoder
```

`sign` runs the same checks as `validate --bundle-dir`, then writes
`bundle.sig` — a detached Ed25519 signature over a canonical digest of every
file in the bundle.

## Step 10 — Load it from a host

Any polyplug host with the **dotnet loader** registered can now load the bundle
directory and call `pipeline.Decoder`. See [C# — Host (app)](csharp-host.md) for
the C# side.

## Runtime constraints

- **.NET guests do not hot-reload.** The dotnet loader returns
  `HotReloadDisabled` from `reload()`. To pick up a new build, the host must
  unload and load again.
- The CLR initializes once per process; .NET guests in the same process share
  one CLR (and its loader cache).

## See also

- [C# overview](csharp.md) · [C# — Host (app)](csharp-host.md)
- [`docs/QUICKSTART.md`](../QUICKSTART.md) for the canonical flow.
