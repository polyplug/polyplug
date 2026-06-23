# polyplug C# SDK

Complete C# support for polyplug plugin runtime.

## Structure

```
sdks/csharp/
├── abi/           # ABI type definitions (auto-generated from Rust)
├── host/          # Host runtime library for C# applications
├── guest/         # Guest library for C# plugin authors
├── loaders/       # Loader implementations (.NET runtime adapter)
├── Directory.Build.props
└── Polyplug.slnx
```

## Installation

### As Host Application

```bash
dotnet add package Polyplug
```

### As Plugin Author

```bash
dotnet add package Polyplug.Guest
```

## Quick Start

### Host Application

```csharp
using Polyplug;

// PluginDir scans the directory at Build() and loads every bundle found
// (a subdirectory containing manifest.toml), mirroring the Rust builder.
var runtime = new RuntimeBuilder()
    .PluginDir("./plugins")
    .Build();

// ...or load a single bundle explicitly (e.g. after registering loaders):
// runtime.LoadBundle("./plugins/my_plugin");

// Use generated host callers to interact with plugins
var decoder = PipelineDecoder.Create(runtime);
if (decoder.HasValue)
{
    var result = decoder.Value.Decode(input);
}
```

### Plugin Author

The generated glue handles `polyplug_init` and contract registration. You
implement the generated contract interface and register a **factory** at module
load — the factory receives the `HostApi` pointer for every host-created
instance, so the host handle lives in the instance, never in a static:

```csharp
using System.Runtime.CompilerServices;
using Polyplug.Guest;
using Polyplug.Abi;

public sealed class DecoderImpl : IPipelineDecoderGuestContract
{
    // Host handle for this runtime, captured at instance creation.
    private readonly IntPtr _host;

    public DecoderImpl(IntPtr host)
    {
        _host = host;
    }

    public StringView Decode(StringView input)
    {
        string s = StringViewHelper.ToString(input);
        return PolyplugHost.AllocString(_host, $"DECODED:{s}");
    }
}

public static class Registration
{
    [ModuleInitializer]
    public static void Register()
    {
        // The factory receives the HostApi pointer per created instance.
        DecoderInterfaces.SetDecoderFactory(host => new DecoderImpl(host));
    }
}
```

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate C# bindings from api.toml
polyplugc generate --api api.toml --lang csharp --out ./generated

# Generate C# bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang csharp --out ./src/generated
```

## Bundle layout

After building, assemble the bundle directory yourself:

```
dist/my-plugin/
├── manifest.toml          # emitted by `generate` (carries the precomputed bundle_id)
└── MyPlugin.dll           # the assembly you compiled (loader = "dotnet")
```

Validate the assembled directory before shipping:

```bash
polyplugc validate --bundle-dir dist/my-plugin/
```

## Components

### ABI (`abi/`)

Auto-generated from Rust ABI definitions. Contains:
- `StringView` — UTF-8 string view (transcoded from UTF-16 at boundary)
- `Buffer` — Byte buffer with host allocator
- `AbiError` — Error code and message
- `GuestContractHandle` — Opaque plugin reference
- `GuestContractInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

C# wrappers over the polyplug C ABI:
- `Runtime` — Main runtime class
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- P/Invoke declarations for all ABI functions

### Guest Library (`guest/`)

Helpers for C# plugins (the entry point and registration are generated):
- `PolyplugHost.AllocString(IntPtr hostPtr, string)` / `PolyplugHost.Log(...)` —
  host services with the `HostApi` pointer passed explicitly (no statics)
- `PinnedStringView` — pinned UTF-8 view for boundary crossings
- `GuestException` — Exception boundary; plugin crashes don't take down host
- Authors implement the generated `I<Contract>GuestContract` and register a
  factory via the generated `Set<Contract>Factory` (e.g. in a `[ModuleInitializer]`)

### Loaders (`loaders/`)

.NET runtime adapter:
- `RegisterDotnetLoader()` — Register .NET loader with runtime
- Supports standard .NET (CLR) and NativeAOT
- Automatic framework version detection

## Hot-Reload

To enable hot-reload, pass the `OnReload` callback through the builder
(per-instance — the built Runtime owns the callback storage, no statics):

```csharp
using Polyplug.Host;

var runtime = new RuntimeBuilder()
    .OnReload(phase => {
        switch (phase.Type) {
            case ReloadPhaseType.Preparing:
                // Destroy instances for this bundle
                instances.Remove(phase.BundleId);
                break;
            case ReloadPhaseType.Reloaded:
                Console.WriteLine($"Reloaded: {phase.BundleName}");
                break;
            case ReloadPhaseType.Failed:
                Console.WriteLine($"Failed: {phase.Reason}");
                break;
        }
    })
    .Build();
```

**Key points:**
- Registering an `OnReload` callback enables hot-reload for that runtime
- The callback is supplied at build time and owned by the Runtime instance
- Host must track and destroy instances on `Preparing` notification
- Hot-reload applies to native, Lua, and JS (QuickJS) bundles; the .NET loader's
  `reload()` returns `HotReloadDisabled` (use collectible-ALC `unload` instead)
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

## Bundle Signing & Key Pinning

Set a signature policy to require/verify each bundle's `bundle.sig`, and
optionally pin an allowlist of trusted Ed25519 verifying keys:

```csharp
using Polyplug.Host;
using Polyplug.Abi;

// 32-byte Ed25519 verifying keys (public keys only — signing keys stay offline).
byte[] key1 = LoadVerifyingKey("author1.pub"); // 32 bytes
byte[] key2 = LoadVerifyingKey("author2.pub"); // 32 bytes

var runtime = new RuntimeBuilder()
    .SignaturePolicy(SignaturePolicy.Required)
    .TrustedKeys([key1, key2])
    .Build();
```

**Key points:**

- Each key passed to `TrustedKeys` must be exactly 32 bytes; otherwise the
  builder throws `ArgumentException`.
- **Empty allowlist (default)** = Trust-On-First-Use: a bundle's embedded
  verifying key is trusted as long as its signature is internally consistent.
- **Non-empty allowlist** + a policy other than `SignaturePolicy.Off` = key
  pinning: after Ed25519 verification, the runtime additionally requires the
  bundle's embedded key to be in the allowlist; a re-signed bundle with an
  attacker key is rejected.
- The runtime **copies** the trusted keys during `Build()`
  (`polyplug_runtime_create`); the unmanaged key buffer is only needed for that
  call and is freed as soon as create returns.
- See [Trust Model](../../docs/TRUST_MODEL.md) for the full signing/pinning design.

## Performance Notes

- **Hot path**: Single indirect call via `calli` IL instruction
- **String transcoding**: UTF-16 ↔ UTF-8 at boundary only
- **Memory**: All cross-boundary data uses host allocator
- **Unsafe code**: Confined to generated `Init.cs` only

## Requirements

- .NET 10.0 or later
- For NativeAOT: Publish with `-p:PublishAot=true`

## Runtime Isolation Note

The .NET loader uses a **process-wide CLR runtime**. This means:
- Multiple `Runtime` instances in the same process share the same CLR runtime
- .NET assemblies from different runtimes share the same loader cache
- **For full isolation between .NET runtimes, use separate processes**

Other loaders (Lua, JavaScript, Native) provide per-runtime isolation.

## See Also

- `../cpp/` — C++ SDK
- `../python/` — Python SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
