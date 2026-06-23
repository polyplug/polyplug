# JavaScript — Host (app)

A polyplug JS host runs on **Deno, Node.js, or Bun**. The SDK detects the
runtime at startup and switches to the appropriate FFI backend; the rest of
your code is identical across all three.

See [JS — overview](js.md) for install instructions and package names.

---

## Step 1 — Install the host SDK and loaders

```bash
# npm / Node / Bun
npm install @polyplug/host \
            @polyplug/loaders-native \
            @polyplug/loaders-js

# Deno
deno add jsr:@polyplug/host jsr:@polyplug/loaders-native jsr:@polyplug/loaders-js
```

Add loader packages for every guest language you want to support
(`@polyplug/loaders-lua`, `@polyplug/loaders-python`, `@polyplug/loaders-dotnet`).

---

## Step 2 — Design the contract (`api.toml`)

The `api.toml` is the contract between the host and its plugins. It declares
which contracts plugins must implement (`[[plugin_contract]]`) and which services
the host provides back to plugins (`[[host_contract]]`). See `examples/api.toml`
for a complete example.

---

## Step 3 — Generate host callers

```bash
polyplugc generate --api api.toml --lang js-quickjs --out host/generated/
```

This produces typed TypeScript callers, contract-ID constants, and
host-contract registration helpers into `host/generated/host/`:

```
host/generated/host/
├── callers.ts           # typed wrappers: PipelineDecoderContract, etc.
├── contracts.ts         # contract interface types
├── interface_factories.ts  # createHost*Vtable helpers
└── types.ts             # shared enum/struct types
```

The generated callers cache the resolved interface pointer and check the
runtime's revision counter before every dispatch, so they stay correct
across hot-reloads without any extra bookkeeping in your application code.

---

## Step 4 — Open the runtime and register loaders

```ts
import { openPolyplug, runtimeNew } from "@polyplug/host";
import { registerNativeLoader } from "@polyplug/loaders-native";
import { registerJsLoader }     from "@polyplug/loaders-js";
import { registerLuaLoader }    from "@polyplug/loaders-lua";
import { registerPythonLoader } from "@polyplug/loaders-python";
import { registerDotnetLoader } from "@polyplug/loaders-dotnet";

// Point at the compiled libpolyplug shared library.
const lib = openPolyplug("/path/to/libpolyplug.so");
const rt  = runtimeNew(lib);

// Register one loader per guest language you support.
// Wrap each in try/catch: a loader whose backing cdylib is absent throws,
// so the host continues to work for the remaining languages.
for (const [name, register] of [
  ["native",     () => registerNativeLoader(rt)],
  ["js-quickjs", () => registerJsLoader(rt)],
  ["lua",        () => registerLuaLoader(rt)],
  ["python",     () => registerPythonLoader(rt)],
  ["dotnet",     () => registerDotnetLoader(rt)],
]) {
  try { register(); }
  catch (e) { console.error(`loader ${name} unavailable: ${e.message}`); }
}
```

---

## Step 5 — Register host contracts (optional)

If your `api.toml` declares `[[host_contract]]` entries (services the app
provides back to plugins), register them before loading any bundles. Import
the generated factory and provide your implementation:

```ts
import { createHostLoggerVtable } from "./host/generated/host/interface_factories.ts";

class ConsoleLogger {
  Log(message: string) { console.log(`[plugin] ${message}`); }
}

const loggerInterface = createHostLoggerVtable(rt, () => new ConsoleLogger());
rt.registerHostContract(loggerInterface.interfacePtr);
// Keep loggerInterface alive for the runtime's lifetime.
```

---

## Step 6 — Load bundles

```ts
// Load a single bundle directory.
rt.loadBundle("/path/to/my_plugin/");

// Or scan a directory and load every bundle found.
for (const entry of Deno.readDirSync(pluginPath)) {
  if (!entry.isDirectory) continue;
  const manifestPath = `${pluginPath}/${entry.name}/manifest.toml`;
  try { rt.loadBundle(`${pluginPath}/${entry.name}/`); }
  catch (e) { console.error(`failed to load ${entry.name}: ${e.message}`); }
}
```

---

## Step 7 — Resolve a contract and call it

Import the generated contract class, call `.create(rt)` to resolve and
instantiate, call the method, then `.destroy()` to release the instance:

```ts
import {
  PipelineDecoderContract,
} from "./host/generated/host/callers.ts";

const decoder = PipelineDecoderContract.create(rt);
if (decoder) {
  const result = decoder.decode("name,value,42");
  console.log(result);   // e.g. "DECODED:name|value|42"
  decoder.destroy();
}
```

The generated caller automatically re-resolves the interface when the revision
counter changes (hot-reload), so you can call `.create()` again after a reload
to get a fresh instance without restarting the host.

---

## Hot-reload

Native, Lua, and QuickJS bundles support hot-reload. Call
`rt.reloadBundle(bundleId)` (or configure a file-watcher to do so) to swap
the live implementation. The generated callers detect the revision change and
re-resolve transparently on the next call.

---

## Running the example host

```bash
# Requires Deno and a built libpolyplug.so + assembled plugin bundles.
POLYPLUG_LIB=target/release/deps/libpolyplug.so \
POLYPLUG_PLUGIN_PATH=examples/plugins \
deno run --allow-read --allow-ffi --allow-env \
  examples/hosts/js/host.js
```

The full source is at `examples/hosts/js/host.js`. The generated callers it
imports live under `examples/hosts/js/generated/`.
