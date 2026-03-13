# polyplug C# Host Example

A simple C# host that loads and runs polyplug plugins using the `Polyplug` host library.

## What It Does

This example demonstrates the full polyplug hosting workflow from C#:

1. **Creates a `Runtime`** — the polyplug plugin runtime instance
2. **Loads plugin bundles** — from the `examples/guests/` directories
3. **Finds plugins by contract** — using `FindByBundle` / `FindByContract`
4. **Calls plugin functions** — via the ABI vtable
5. **Runs a multi-language pipeline** — decoder → transformer → encoder → reporter → validator

## Requirements

- [.NET 10 SDK](https://dotnet.microsoft.com/download/dotnet/10.0)
- The `libpolyplug.so` native library (built from the Rust crate in `crates/polyplug/`)
- Guest plugins built (see `examples/build.sh`)

## Building

```bash
# Build the native runtime first
cargo build -p polyplug

# Build all guest plugins
./examples/build.sh

# Build this host
dotnet build examples/hosts/csharp/Host.csproj
```

## Running

```bash
# Run using dotnet
dotnet run --project examples/hosts/csharp/Host.csproj
```

Or point to a custom native library via environment variable:

```bash
POLYPLUG_SO=/path/to/libpolyplug.so dotnet run --project examples/hosts/csharp/Host.csproj
```

## Expected Output

```
=== polyplug C# host example ===
Loading 12 guest plugins...
  [OK]   1/12 rust/decoder
  [OK]   2/12 rust/encoder
  ...
--- Run 1: Rust decoder, C++ transformer, Rust encoder, C# reporter, C++ validator ---
Run output: ALICE,HELLO,3
Run summary: ...
Validation: ok (...)
--- Run 2: Python decoder, Lua transformer, C# encoder, Python reporter, Lua validator ---
...
pipeline complete
```

## API Usage

The example uses the `Polyplug` namespace from `host-libs/csharp/`:

```csharp
using Polyplug;

// Build and initialise the runtime
Runtime runtime = Runtime.Builder().Init();

// Load a plugin bundle (path to bundle directory)
runtime.LoadBundle("/path/to/my_plugin");

// Find a plugin by contract ID
ulong handle = runtime.FindByContract(contractId, minVersion: 0);

// Resolve to a PluginGuard (holds the plugin alive)
using PluginGuard guard = runtime.ResolvePlugin(handle);

// Get the vtable and call a function
IntPtr vtablePtr = guard.GetVTable();
PluginVTable vtable = Marshal.PtrToStructure<PluginVTable>(vtablePtr);
IntPtr fnPtr = Marshal.ReadIntPtr(vtable.FunctionsPtr, 0 * IntPtr.Size);
AbiFn fn = Marshal.GetDelegateForFunctionPointer<AbiFn>(fnPtr);
AbiError err = fn(argsPtr, outPtr);
```

## Key Types

| Type | Description |
|------|-------------|
| `Runtime` | The host runtime. Created via `Runtime.Builder().Init()`. |
| `RuntimeBuilder` | Fluent builder for `Runtime`. |
| `PluginGuard` | Scoped handle to a live plugin. Dispose when done. |
| `PluginVTable` | The plugin's function table (contract ID, version, function pointers). |
| `StringView` | ABI-safe UTF-8 string (pointer + length). |
| `Buffer` | ABI-safe byte buffer (pointer + length + capacity). |
| `AbiError` | ABI error type with a code and optional message. |

## See Also

- `host-libs/csharp/` — the `Polyplug` host library source
- `examples/guests/csharp/` — C# guest plugin examples
- `examples/abi_types.md` — canonical ABI type reference
- `examples/api.toml` — the API definition used to generate bindings
