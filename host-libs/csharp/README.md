# polyplug C# Host Library

.NET bindings for the polyplug plugin runtime. Provides type-safe plugin resolution, hot-reload support, and loader registration for multiple language runtimes.

## Prerequisites

- **.NET 10.0** or later
- **libpolyplug.so** (Linux), **libpolyplug.dylib** (macOS), or **polyplug.dll** (Windows) — the core polyplug shared library

## Quick Start

```csharp
using Polyplug;
using Polyplug.Loaders;

// Configure hot-reload behavior BEFORE creating runtime
Runtime.SetConfig(new RuntimeConfig {
    HotReloadMaxRetries = 5,
    HotReloadRetryIntervalMs = 2000,
    HotReloadAbortOnMaxRetries = false
});

// Register reload callback BEFORE creating runtime
Runtime.OnReload(phase => {
    if (phase.IsPreparing()) {
        Console.WriteLine($"Preparing reload: bundle {phase.BundleId}, retry {phase.RetryCount}");
    } else if (phase.IsReloaded()) {
        Console.WriteLine($"Reloaded: bundle {phase.BundleId}");
    } else if (phase.IsFailed()) {
        Console.WriteLine($"Reload failed: {phase.Reason}");
    }
});

// Create runtime (applies pending config/callback)
var rt = Runtime.Builder()
    .PluginDir("/path/to/plugins")
    .Build();

// Register loaders
NativeLoader.Register(rt);
PythonLoader.Register(rt, "3.11");

// Load a plugin bundle
rt.LoadBundle("/path/to/my_plugin_bundle");

// Find a plugin by contract ID
const ulong CONTRACT_ID = 0xCC4232FAB0410D2BUL;
var handle = rt.FindByContract(CONTRACT_ID, 1);

if (!handle.IsValid) {
    // Plugin not found — handle error
    return;
}

// Resolve to a PluginGuard
var guard = rt.GetGuard(handle);
if (!guard.IsValid) {
    // Resolution failed
    return;
}

// Get vtable and call plugin functions
var vtable = guard.VTable;
```

## Runtime API

### Builder Pattern

The `Runtime` class uses a fluent builder for construction:

```csharp
var rt = Runtime.Builder()
    .PluginDir("/path/to/plugins")      // Optional: set plugin search directory
    .Compatibility(0)                   // Optional: compatibility mode flags
    .Build();                           // Throws on failure
```

**Methods:**

- `PluginDir(string path)` — Add a directory to the plugin search path (call multiple times for multiple directories)
- `Compatibility(uint mode)` — Set compatibility mode flags (default: 0)
- `Build()` — Construct the Runtime instance (throws on failure)

### Core Runtime Methods

```csharp
public sealed class Runtime {
    // Configure runtime behavior. Must be called BEFORE Runtime instantiation.
    public static void SetConfig(RuntimeConfig config);
    
    // Register hot-reload callback. Must be called BEFORE Runtime instantiation.
    public static void OnReload(Action<ReloadPhase> callback);
    
    // Find a plugin by contract ID and minimum version
    public PluginHandle FindByContract(ulong contractId, uint minVersion);
    
    // Resolve a handle to a PluginGuard
    public PluginGuard GetGuard(PluginHandle handle);
    
    // Load a plugin bundle from disk
    public void LoadBundle(string path);
    
    // Reload a plugin bundle (hot-reload)
    public void ReloadBundle(string path);
}
```

### RuntimeConfig

```csharp
public class RuntimeConfig {
    public uint HotReloadMaxRetries { get; set; } = 3;
    public uint HotReloadRetryIntervalMs { get; set; } = 1000;
    public bool HotReloadAbortOnMaxRetries { get; set; } = true;
}
```

### ReloadPhase

```csharp
public enum ReloadPhaseType { Preparing, Reloaded, Failed }

public sealed class ReloadPhase {
    public ReloadPhaseType Type { get; }
    public ulong BundleId { get; }
    public string BundleName { get; }
    public uint RetryCount { get; }      // Only for Preparing
    public string Reason { get; }        // Only for Failed
    
    // Helper methods
    public bool IsPreparing();
    public bool IsReloaded();
    public bool IsFailed();
}
```

**Phase Types:**

- `Preparing` — Fired BEFORE vtable swap. Host should destroy all instances for the bundle. `RetryCount` indicates retry attempt (0 = first attempt).
- `Reloaded` — Fired AFTER vtable swap. Host can create new instances.
- `Failed` — Fired when reload is aborted after max retries. Old vtable is kept. `Reason` contains error description.

## PluginGuard API

The `PluginGuard` struct provides RAII management of resolved plugins.

### Key Features

- **Vtable caching** — The vtable pointer is cached at construction time
- **RAII cleanup** — Plugin is automatically released when the guard is disposed
- **Readonly struct** — Guards are immutable value types
- **Null safety** — Failed resolution creates a null guard (no exceptions)

### Usage Pattern

```csharp
// Resolve plugin and get cached vtable
var guard = rt.GetGuard(handle);

// Check if resolution succeeded
if (!guard.IsValid) {
    // Handle null guard (resolution failed)
    return;
}

// Get cached vtable
var vtable = guard.VTable;

// Cast to your contract-specific type
var contract = new MathContractVTable(vtable);

// Call plugin functions
var result = contract.Add(1, 2);
```

### PluginGuard Methods

```csharp
public readonly struct PluginGuard {
    public bool IsValid { get; }
    public IntPtr VTable { get; }
    
    // Static method to create a null guard
    public static PluginGuard Reset();
}
```

## Loader Package Structure

Polyplug supports loading plugins from multiple language runtimes. **Each loader is a separate assembly** under the `Loaders/` directory:

```
host-libs/csharp/
├── Polyplug.sln
├── Polyplug/
│   ├── Polyplug.csproj
│   ├── Runtime.cs
│   ├── RuntimeConfig.cs
│   ├── ReloadPhase.cs
│   ├── PluginGuard.cs
│   └── NativeMethods.cs
└── Loaders/
    ├── Native/
    │   ├── Polyplug.Loaders.Native.csproj
    │   └── NativeLoader.cs
    ├── Python/
    │   ├── Polyplug.Loaders.Python.csproj
    │   └── PythonLoader.cs
    ├── Lua/
    │   ├── Polyplug.Loaders.Lua.csproj
    │   └── LuaLoader.cs
    ├── Js/
    │   ├── Polyplug.Loaders.Js.csproj
    │   └── JsLoader.cs
    └── JsDeno/
        ├── Polyplug.Loaders.JsDeno.csproj
        └── JsDenoLoader.cs
```

### Loader Registration Pattern

Each loader provides a `Register` method:

```csharp
using Polyplug.Loaders;

// Register native C/C++ loader
NativeLoader.Register(rt);

// Register Python loader with minimum version requirement
PythonLoader.Register(rt, "3.11");

// Register LuaJIT loader
LuaLoader.Register(rt);

// Register QuickJS loader
JsLoader.Register(rt);

// Register Deno loader
JsDenoLoader.Register(rt);
```

### Loader-Specific Configuration

Some loaders accept configuration parameters:

**Python loader:**
```csharp
// Require Python 3.11 or later
PythonLoader.Register(rt, "3.11");
```

**Native, Lua, JS, Deno loaders:**
```csharp
// No configuration required
NativeLoader.Register(rt);
LuaLoader.Register(rt);
JsLoader.Register(rt);
JsDenoLoader.Register(rt);
```

## Hot-Reload Support

The polyplug runtime supports hot-reloading of plugin bundles with automatic notification.

### Configuring Hot-Reload

```csharp
var config = new RuntimeConfig {
    HotReloadMaxRetries = 5,
    HotReloadRetryIntervalMs = 2000,
    HotReloadAbortOnMaxRetries = false
};
Runtime.SetConfig(config);
```

### Registering Reload Callback

The callback must be registered BEFORE creating the Runtime:

```csharp
Runtime.OnReload(phase => {
    switch (phase.Type) {
        case ReloadPhaseType.Preparing:
            if (phase.RetryCount == 0) {
                Console.WriteLine($"First attempt - cleanup instances for bundle {phase.BundleId}");
            } else {
                Console.WriteLine($"Retry {phase.RetryCount} - missed cleanup!");
            }
            break;
            
        case ReloadPhaseType.Reloaded:
            Console.WriteLine($"Bundle {phase.BundleId} reloaded successfully");
            break;
            
        case ReloadPhaseType.Failed:
            Console.WriteLine($"Reload failed: {phase.Reason}");
            break;
    }
});

// Now create runtime (applies pending config/callback)
var rt = Runtime.Builder().Build();
```

### Instance Tracking Pattern

Use generated contract callers with factory methods for safe hot-reload:

```csharp
using Generated;

// Factory method returns nullable - null if plugin not found
var decoder = PipelineDecoderCaller.Create(rt, minVersion: 1);
if (decoder == null) {
    // Plugin not available
    return;
}

// Check if instance is still valid
if (decoder.IsValid) {
    var result = decoder.Decode(input);
}

// Explicitly release instance (optional - Dispose pattern)
decoder.Dispose();
```

### Retry Behavior

If instances are not destroyed before vtable swap:

1. Runtime fires `Preparing` with `RetryCount=0`
2. Waits 1 second (or configured interval)
3. If Arc count > 1, fires `Preparing` with `RetryCount=1`
4. Repeats until `HotReloadMaxRetries` is reached
5. If `HotReloadAbortOnMaxRetries=true`, fires `Failed` and keeps old vtable

## Error Handling

### Exception-Based Errors

Runtime construction and bundle loading throw on failure:

```csharp
try {
    var rt = Runtime.Builder().Build();
    rt.LoadBundle("/path/to/bundle");
} catch (Exception e) {
    // Handle error: e.Message contains the error message
    Console.WriteLine($"Runtime error: {e.Message}");
}
```

### Null Guard Pattern

Plugin resolution does **not** throw exceptions. Failed resolution returns a null guard:

```csharp
// Check for null guard using IsValid property
var guard = rt.GetGuard(handle);
if (!guard.IsValid) {
    // Resolution failed — handle gracefully
    return;
}
```

### Handle Validation

The `FindByContract` method returns a `PluginHandle` struct with an `IsValid` property:

```csharp
var handle = rt.FindByContract(CONTRACT_ID, 1);
if (!handle.IsValid) {
    // Plugin not found — handle error
    return;
}
```

## Memory Management

### RAII Guarantees

All polyplug C# types use RAII for automatic cleanup:

- **Runtime** — Destroyed automatically when disposed
- **PluginGuard** — Releases the plugin when disposed
- **Contract Callers** — Implement `IDisposable` for explicit cleanup

```csharp
void UsePlugin() {
    using var rt = Runtime.Builder().Build();
    rt.LoadBundle("/path/to/bundle");

    var handle = rt.FindByContract(CONTRACT_ID, 1);
    var guard = rt.GetGuard(handle);

    // Use plugin...

    // Automatic cleanup:
    // 1. guard releases the plugin when disposed
    // 2. rt destroys the runtime when disposed
    // No manual cleanup needed!
}
```

## Complete Example

```csharp
using Polyplug;
using Polyplug.Loaders;
using Generated;

// Define your contract wrapper
public class MathContractVTable {
    private readonly IntPtr _vtable;
    
    public MathContractVTable(IntPtr vtable) {
        _vtable = vtable;
    }
    
    public int Add(int a, int b) {
        // Call through vtable...
    }
}

class Program {
    static void Main() {
        try {
            // 1. Configure hot-reload
            Runtime.SetConfig(new RuntimeConfig {
                HotReloadMaxRetries = 5,
                HotReloadRetryIntervalMs = 2000
            });
            
            // 2. Register reload callback
            Runtime.OnReload(phase => {
                if (phase.IsPreparing()) {
                    Console.WriteLine($"Preparing: bundle {phase.BundleId}");
                } else if (phase.IsReloaded()) {
                    Console.WriteLine($"Reloaded: bundle {phase.BundleId}");
                } else if (phase.IsFailed()) {
                    Console.WriteLine($"Failed: {phase.Reason}");
                }
            });
            
            // 3. Create runtime with plugin directory
            var rt = Runtime.Builder()
                .PluginDir("/usr/local/lib/polyplug/plugins")
                .Build();

            // 4. Register loaders for different language runtimes
            NativeLoader.Register(rt);
            PythonLoader.Register(rt, "3.11");

            // 5. Load plugin bundles
            rt.LoadBundle("/path/to/math_plugin");
            rt.LoadBundle("/path/to/utils_plugin");

            // 6. Find and resolve the math plugin
            const ulong MATH_CONTRACT = 0xCC4232FAB0410D2BUL;
            var handle = rt.FindByContract(MATH_CONTRACT, 1);

            if (!handle.IsValid) {
                Console.WriteLine("Math plugin not found");
                return;
            }

            // 7. Resolve with cached vtable
            var guard = rt.GetGuard(handle);
            if (!guard.IsValid) {
                Console.WriteLine("Failed to resolve math plugin");
                return;
            }

            // 8. Cast vtable to contract type
            var math = new MathContractVTable(guard.VTable);

            // 9. Call plugin functions
            var sum = math.Add(10, 20);
            var product = math.Multiply(3, 7);

            Console.WriteLine($"10 + 20 = {sum}");
            Console.WriteLine($"3 * 7 = {product}");

            // 10. Automatic cleanup via using statement
        } catch (Exception e) {
            Console.WriteLine($"Error: {e.Message}");
        }
    }
}
```

## Build Integration

### .NET Project Example

```xml
<Project Sdk="Microsoft.NET.Sdk">

  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>

  <ItemGroup>
    <ProjectReference Include="..\..\host-libs\csharp\Polyplug\Polyplug.csproj" />
    <ProjectReference Include="..\..\host-libs\csharp\Loaders\Native\Polyplug.Loaders.Native.csproj" />
    <ProjectReference Include="..\..\host-libs\csharp\Loaders\Python\Polyplug.Loaders.Python.csproj" />
  </ItemGroup>

</Project>
```

### Runtime Requirements

Minimum .NET version:

- **.NET 10.0** or later

## ABI Stability

The polyplug ABI is **frozen at version 1**. All structures and function signatures are stable and will not change between minor versions.

## Further Reading

- `../../docs/TRUST_MODEL.md` — Bundle identity, declared dependencies, and ABI freeze details
- `../lua/README.md` — LuaJIT FFI binding documentation
- `../../crates/polyplug/` — Rust runtime core implementation
