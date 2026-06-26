# JavaScript — Host (app)

Embed the polyplug runtime in a Deno, Node.js, or Bun application, load plugins
written in any supported language, and call their contracts through generated
typed callers. Your code is identical on Deno, Node.js, and Bun.

See also: [JS overview](js.md) · [JS — Guest (plugin)](js-guest.md) ·
[glossary](../glossary.md)

## 1. Install

Install the CLI and the host SDK plus a loader per guest language:

```bash
# npm / Bun
npm install -g @polyplug/cli
npm install @polyplug/host \
            @polyplug/loaders-native \
            @polyplug/loaders-js \
            @polyplug/loaders-lua \
            @polyplug/loaders-python \
            @polyplug/loaders-dotnet

# Deno (jsr mirrors — same packages)
deno install -gA -n polyplugc npm:@polyplug/cli   # CLI is npm-only, not JSR
deno add jsr:@polyplug/host jsr:@polyplug/loaders-native jsr:@polyplug/loaders-js
```

A JS host can load guests written in any supported language — register the
matching loader when you build the runtime.

## 2. Generate host callers

Author or obtain the shared `api.toml` contract (see `examples/api.toml`), then
generate the typed callers. Re-run whenever the contract changes.

```bash
polyplugc generate --api api.toml --lang js-quickjs --out host/generated
```

This writes `host/generated/host/` with the typed caller classes
(`callers.ts`), contract interface types (`contracts.ts`), host-contract
interface factories (`interface_factories.ts`), and shared types (`types.ts`).
Never edit these files. For the emitted symbol names, see
[Generated names](../generated-names.md).

## 3. Build the runtime

Point `openPolyplug` at the compiled `libpolyplug` shared library, create the
runtime, and register one loader per guest language:

```ts
import {
  openPolyplug, runtimeNew, COMPATIBILITY_STRICT,
} from "@polyplug/host";
import { registerNativeLoader } from "@polyplug/loaders-native";
import { registerJsLoader }     from "@polyplug/loaders-js";
import { registerLuaLoader }    from "@polyplug/loaders-lua";
import { registerPythonLoader } from "@polyplug/loaders-python";
import { registerDotnetLoader } from "@polyplug/loaders-dotnet";

const lib = openPolyplug("/path/to/libpolyplug.so");
const rt  = runtimeNew(lib, {
  config: { compatibility: COMPATIBILITY_STRICT, hotReloadEnabled: true },
});

// A loader whose backing cdylib is absent throws; wrap each so the host keeps
// working for the remaining languages.
const loaders = [
  { name: "native",     register: () => registerNativeLoader(rt) },
  { name: "js-quickjs", register: () => registerJsLoader(rt) },
  { name: "lua",        register: () => registerLuaLoader(rt) },
  { name: "python",     register: () => registerPythonLoader(rt) },
  { name: "dotnet",     register: () => registerDotnetLoader(rt) },
];
for (const loader of loaders) {
  try { loader.register(); }
  catch (e) { console.error(`loader ${loader.name} unavailable: ${e.message}`); }
}
```

The full multi-loader host is `examples/hosts/js/host.js`.

### Hot-reload callback (optional)

Pass an `onReload` callback to observe reload phases. Hot-reload applies to
native, Lua, and JS bundles — see [Hot Reload](../HOT_RELOAD_DESIGN.md).

```ts
import { runtimeNew, ReloadPhase } from "@polyplug/host";

const rt = runtimeNew(lib, {
  config: { hotReloadEnabled: true },
  onReload: (phase: ReloadPhase) => {
    if (phase.isPreparing()) console.error("[reload] preparing");
    if (phase.isReloaded())  console.error("[reload] reloaded");
    if (phase.isFailed())    console.error(`[reload] failed: ${phase.reason}`);
  },
});
```

### Signature policy (optional)

```ts
import { runtimeNew, SignaturePolicy } from "@polyplug/host";

const rt = runtimeNew(lib, {
  config: { signaturePolicy: SignaturePolicy.Required },
});
```

`Required` rejects unsigned or tampered bundles. See the
[Trust Model](../TRUST_MODEL.md).

## 4. Register a host contract (optional)

If your `api.toml` defines a host contract (a service the host provides to
plugins), register it before loading bundles. Import the generated factory and
provide your implementation:

```ts
import { createHostLoggerVtable } from "./host/generated/host/interface_factories.ts";

class ConsoleLogger {
  Log(message: string) { console.log(`[plugin] ${message}`); }
  LogWithLevel(level: number, message: string) {
    console.log(`[plugin][${level}] ${message}`);
  }
}

const loggerInterface = createHostLoggerVtable(rt, () => new ConsoleLogger());
rt.registerHostContract(loggerInterface.interfacePtr);
// Keep loggerInterface alive for the runtime's lifetime.
```

## 5. Load bundles

```ts
// Load a single bundle directory.
rt.loadBundle("/path/to/my_plugin/");

// Or scan a directory and load every bundle found.
for (const entry of Deno.readDirSync(pluginPath)) {
  if (!entry.isDirectory) continue;
  try { rt.loadBundle(`${pluginPath}/${entry.name}/`); }
  catch (e) { console.error(`failed to load ${entry.name}: ${e.message}`); }
}
```

`loadBundle` reads the directory's `manifest.toml` and dispatches to the loader
matching the bundle's `loader` field.

## 6. Call a contract

Call `.create(rt)` to resolve and instantiate the contract, call the method,
then `.destroy()` to release the instance:

```ts
import { PipelineDecoderContract } from "./host/generated/host/callers.ts";

const decoder = PipelineDecoderContract.create(rt);
if (decoder) {
  const result = decoder.decode("name,value,42");
  console.log(result);   // DECODED:name|value|42
  decoder.destroy();
}
```

A hot-reloaded plugin is picked up automatically — see
[Hot Reload](../HOT_RELOAD_DESIGN.md).

## Full reference

`examples/hosts/js/host.js` registers all five loaders, a host contract, scans a
directory, loads every bundle, and runs a five-stage pipeline end to end.
Generated callers live at `examples/hosts/js/generated/`.
