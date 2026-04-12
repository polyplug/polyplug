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

var runtime = Runtime.Builder()
    .PluginDir("./plugins")
    .Build();

// Load a plugin bundle
runtime.LoadBundle("./plugins/my_plugin");

// Use generated host callers to interact with plugins
var decoder = PipelineDecoder.Create(runtime);
if (decoder.HasValue)
{
    var result = decoder.Value.Decode(input);
}
```

### Plugin Author

```csharp
using Polyplug.Guest;

[PolyplugPlugin]
public static class MyPlugin
{
    public static void Init(HostInterface host, BundleInitContext ctx)
    {
        // Register your contract implementations
        registrar.Register<IPipelineDecoder>(new DecoderImpl());
    }
}

public class DecoderImpl : IPipelineDecoder
{
    public string Decode(string input)
    {
        return $"DECODED:{input}";
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

Bootstrap layer for C# plugins:
- `[PolyplugPlugin]` attribute — Marks plugin entry point
- `HostInterface` — Contract registration
- `BundleInitContext` — Bundle metadata
- Exception boundary — Plugin crashes don't take down host

### Loaders (`loaders/`)

.NET runtime adapter:
- `RegisterDotnetLoader()` — Register .NET loader with runtime
- Supports standard .NET (CLR) and NativeAOT
- Automatic framework version detection

## Hot-Reload

To enable hot-reload, set `HotReloadEnabled = true` and register an `OnReload` callback:

```csharp
using Polyplug;

// Enable hot-reload
var config = new RuntimeConfig { HotReloadEnabled = true };
Runtime.SetConfig(config);

// Register callback before creating runtime
Runtime.OnReload(phase => {
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
});

var runtime = Runtime.Builder().Build();
```

**Key points:**
- `HotReloadEnabled` defaults to `false` — must be explicitly enabled
- Callback must be registered **before** creating the runtime
- Host must track and destroy instances on `Preparing` notification
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

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
