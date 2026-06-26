# C# / .NET — Guest (plugin)

Write a polyplug plugin in C#: generate the ABI glue, build a `.NET` class
library, and assemble a bundle any polyplug host can load. New to polyplug? Start
with the [Quick Start](../QUICKSTART.md).

See also: [C# overview](csharp.md) · [C# — Host (app)](csharp-host.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and add the guest SDK to a class-library project:

```bash
dotnet tool install -g Polyplug.Cli      # or: cargo install polyplugc
dotnet add package Polyplug.Guest
```

The generated marshalling needs `AllowUnsafeBlocks`, and `AssemblyName` must
match the `file` in `bundle.toml`:

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

## 2. Write the bundle manifest

`bundle.toml` declares the bundle name, target loader, the assembly file, and
which contracts this bundle implements. The `api` field points at the shared
`api.toml` contract (see `examples/api.toml`).

```toml
# bundle.toml
[bundle]
name = "csharp_decoder"
version = "1.0.0"
api = "../api.toml"   # path to api.toml, relative to this file
loader = "dotnet"
file = "decoder.dll"

[[plugin]]
name = "decoder"
implements = ["pipeline.Decoder@1.0"]
```

`implements` names each contract as `<namespace>.<Name>@<major_version>`. Add one
`[[plugin]]` section per plugin in the bundle. To declare a runtime dependency on
another contract, add a `[[dependency]]` section:

```toml
[[dependency]]
kind        = "contract"
contract    = "pipeline.Validator"
min_version = "1.0"
```

## 3. Generate the guest glue

```bash
polyplugc generate --bundle bundle.toml --lang csharp --out generated
```

This writes the contract interface(s), the factory registration hook, the
`polyplug_init` trampoline, bundle constants, and a `manifest.toml`
under `generated/`, all in the `Polyplug.Generated` namespace. Re-run whenever
`bundle.toml` or `api.toml` changes; never edit generated files. For the emitted
symbol names, see [Generated names](../generated-names.md).

## 4. Implement the plugin

Implement the generated interface, then register the factory from a
`[ModuleInitializer]`. Full source: `examples/guests/csharp/decoder`.

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

- The factory receives the `HostApi` pointer for each instance; store it as an
  instance field (see [instance payload](../glossary.md)).
- Read `StringView` inputs with `StringViewHelper.ToString`; return owned strings
  with `PolyplugHost.AllocString(_host, ...)`.
- The captured `_host` pointer also lets you call host contracts:
  `PolyplugHost.Log(_host, level, scope, message)` invokes the host logger.

Interface and factory names come from [Generated names](../generated-names.md).

## 5. Build

```bash
dotnet build -c Release      # → decoder.dll
```

## 6. Assemble the bundle

Copy the built assembly, the generated `manifest.toml`, and **every dependency
assembly** the plugin needs into one bundle directory:

```
dist/csharp_decoder/
├── manifest.toml          # from generated/manifest.toml
├── decoder.dll            # from bin/Release/net10.0/
└── <dependency assemblies, if any>
```

## 7. Validate the bundle

```bash
polyplugc validate --bundle-dir dist/csharp_decoder
```

This checks the manifest is consistent, the declared assembly is present, and the
bundle conforms to the ABI rules.

## 8. Sign the bundle (optional)

If the target host enforces a signature policy, sign the bundle:

```bash
polyplugc keygen --out keys/           # generate keypair once; keep signing.key secret
polyplugc sign --bundle-dir dist/csharp_decoder --key keys/signing.key
polyplugc verify --bundle-dir dist/csharp_decoder
```

`sign` validates the bundle, then writes a detached `bundle.sig`. See the
[Trust Model](../TRUST_MODEL.md).

## Full reference

Reference plugins:

| Plugin | Path | Contract |
|---|---|---|
| decoder | `examples/guests/csharp/decoder/` | `pipeline.Decoder` |
| transformer | `examples/guests/csharp/transformer/` | `data.Transformer` (declares a dependency) |
| encoder | `examples/guests/csharp/encoder/` | `pipeline.Encoder` |
| reporter | `examples/guests/csharp/reporter/` | `data.Reporter` (calls a host contract) |
| validator | `examples/guests/csharp/validator/` | `pipeline.Validator` |
