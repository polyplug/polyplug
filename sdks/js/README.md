# polyplug JavaScript SDK

Complete JavaScript/TypeScript support for polyplug plugin runtime.

## Structure

```
sdks/js/
├── abi/           # ABI type definitions (auto-generated from Rust)
├── host/          # Host runtime library for JS/TS applications
├── guest/         # Guest library for JS/TS plugin authors
└── loaders/       # Loader implementations (QuickJS runtime adapter)
```

## Installation

### Via JSR (Deno)

```bash
deno add @polyplug/core
```

### Via npm

```bash
npm install @polyplug/core
```

## Quick Start

### Host Application (Deno)

```typescript
import { Runtime } from "@polyplug/core";

const runtime = Runtime.builder()
    .pluginDir("./plugins")
    .build();

// Load a plugin bundle
runtime.loadBundle("./plugins/my_plugin");

// Use generated host callers to interact with plugins
const decoder = PipelineDecoder.create(runtime);
if (decoder) {
    const result = decoder.decode(input);
}
```

### Plugin Author

```typescript
import { plugin } from "@polyplug/guest";

plugin((host, ctx) => {
    host.registerContract(host, descriptor, contractInterface);
});

class DecoderImpl implements PipelineDecoder {
    decode(input: string): string {
        return `DECODED:${input}`;
    }
}
```

## Code Generation

Use `polyplugc` to generate type-safe bindings:

```bash
# Generate TypeScript bindings from api.toml
polyplugc generate --api api.toml --lang js --out ./generated

# Generate TypeScript bindings from bundle.toml
polyplugc generate --bundle bundle.toml --lang js --out ./src/generated
```

## Bundle layout

After bundling, assemble the bundle directory yourself:

```
dist/my-plugin/
├── manifest.toml          # emitted by `generate` (carries the precomputed bundle_id)
└── bundle.js              # the entry script the QuickJS loader evaluates (runtime = "js-quickjs")
```

Validate the assembled directory before shipping:

```bash
polyplugc validate --bundle-dir dist/my-plugin/
```

## Components

### ABI (`abi/`)

Auto-generated from Rust ABI definitions:
- `StringView` — UTF-8 string view
- `Buffer` — Byte buffer with host allocator
- `AbiError` — Error code and message
- `GuestContractHandle` — Opaque plugin reference (lo/hi split for u64)
- `GuestContractInterface` — Plugin vtable with dispatch mechanism

### Host Library (`host/`)

TypeScript wrappers over the polyplug C ABI:
- `Runtime` — Main runtime class
- `RuntimeConfig` — Configuration options
- `ReloadPhase` — Hot-reload notifications
- Deno.dlopen or Node.js FFI bindings

### Guest Library (`guest/`)

Bootstrap layer for JavaScript plugins:
- `plugin()` function — Marks plugin entry point
- `HostApi` — Contract registration
- `BundleInitContext` — Bundle metadata
- Error boundary — Plugin errors don't take down host

### Loaders (`loaders/`)

QuickJS runtime adapter:
- `registerJsLoader()` — Register JS loader with runtime
- Embedded QuickJS via `rquickjs` crate
- Automatic module bundling via Rolldown

## Hot-Reload

To enable hot-reload, pass `config.hotReloadEnabled: true` and an `onReload`
callback per-instance to `runtimeNew` (no module-level state — each runtime
owns its own callback):

```typescript
import { openPolyplug, runtimeNew, ReloadPhase } from "@polyplug/core";

const lib = openPolyplug(libPath);
const runtime = runtimeNew(lib, {
    config: { hotReloadEnabled: true },
    onReload: (phase) => {
        switch (phase.type) {
            case ReloadPhase.TYPE_PREPARING:
                // Destroy instances for this bundle
                instances.delete(phase.bundleId);
                break;
            case ReloadPhase.TYPE_RELOADED:
                console.log(`Reloaded: ${phase.bundleName}`);
                break;
            case ReloadPhase.TYPE_FAILED:
                console.error(`Failed: ${phase.reason}`);
                break;
        }
    },
});
// runtime.destroy() closes the runtime AND the FFI callback it owns.
```

**Key points:**
- `hotReloadEnabled` defaults to `false` — must be explicitly enabled
- The callback is per-runtime (passed at construction); call `runtime.destroy()`
  to release the runtime and its `Deno.UnsafeCallback`
- Host must track and destroy instances on `TYPE_PREPARING` notification
- See [Hot-Reload Design](../../docs/HOT_RELOAD_DESIGN.md) for details

## Runtime Support

### Deno

- Native FFI support via `Deno.dlopen`
- Requires `--allow-ffi` flag
- Full TypeScript support

### Node.js

- Requires `node-ffi-napi` or similar
- ESM and CJS support
- TypeScript via tsc

### Bun

- Native FFI support
- Fast startup times

## Performance Notes

- **Backend**: QuickJS (embedded) or Deno (V8)
- **u64 handling**: Split into lo/hi number pairs (JS lacks 64-bit integers)
- **Strings**: Native UTF-8, no transcoding
- **Memory**: All cross-boundary data in host allocator

## Requirements

- Deno 1.40+ or Node.js 18+
- For bundling: `rolldown` (installed automatically)

## See Also

- `../csharp/` — C# SDK
- `../python/` — Python SDK
- `../../examples/` — Working examples
- `../../docs/` — Design documentation
