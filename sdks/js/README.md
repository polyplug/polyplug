# polyplug JavaScript SDK

Build polyplug hosts and plugins in JavaScript/TypeScript. The host side loads
the native runtime through FFI; guest plugins run in an embedded QuickJS VM.
Strings are native UTF-8 — no transcoding.

> **Host runtime: Deno.** The host uses `Deno.dlopen` for FFI. Generated host
> caller bindings target Deno as well.

## Install

```bash
# Host application (+ a loader package per guest language you support)
npm install @polyplug/host @polyplug/loaders-native
# @polyplug/loaders-{js,lua,python,dotnet} as needed
# Deno: deno add jsr:@polyplug/host jsr:@polyplug/loaders-native

# Plugin author (rolldown bundles the entry script)
npm install @polyplug/guest
npm install --save-dev rolldown
```

Install the CLI to generate bindings:

```bash
npm install -g @polyplug/cli          # or, on Deno: deno install -gA -n polyplugc npm:@polyplug/cli
```

## Generate bindings

```bash
polyplugc generate --bundle bundle.toml --lang js-quickjs --out ./generated
```

## Shared generated declarations

The command remains unified by default. For a shared JavaScript package, emit
or import DomainTypes as `@app/domain` and GuestContracts as
`@app/guest-contracts`; see the [canonical split-output guide][codegen].


## Host application

```typescript
import { openPolyplug, runtimeNew } from "@polyplug/host";

const runtime = runtimeNew(openPolyplug("./libpolyplug.so"));
runtime.loadBundle("./plugins/my_plugin");

const decoder = PipelineDecoderContract.create(runtime);
if (decoder) {
    const result = decoder.decode(input);
}
```

## Internal plugins

The default command emits external plugin bindings. Generate the internal
profile explicitly when the application supplies JavaScript/TypeScript
implementations:

```bash
polyplugc generate --bundle bundle.toml --internal --lang js-quickjs --out ./generated
```

The generated `internal.ts` exports `InternalProviders`, `Registration`, and
`register(runtime, providers)`. Each `InternalProviders` property is a factory
returning a typed implementation:

```typescript
import { InternalProviders, register } from "./generated/internal/<bundle>-<bundle-id-hex>/internal.ts";

const registration = register(runtime, new InternalProviders({
    platform_plugin_platform_plugin: () => new PlatformPlugin(),
}));
const bundleId = registration.bundleId;
```

The registrar consumes provider factories per attempt, validates the exact
manifest provider/function/dependency set, and atomically publishes it.
`Registration` exposes named generated host caller bindings from the committed
handles; use them exactly like callers found after external plugin loading.
Before `runtime.unloadBundle(bundleId)`, the application must quiesce every
caller and destroy all guest instances for the bundle. A successful unload
invalidates those callers and releases the generated provider roots; callers
must not be used afterward.

## Plugin author

Implement the contract functions, register them with the generated
`set<Contract>Impl`, and bundle everything (e.g. with rolldown) into one entry
script. The host bridge is threaded in — never `globalThis`:

```javascript
import { setDecoderImpl } from './generated/guest/contracts';
import { polyplug_init } from './generated/guest/init';
import { toStr, allocStringArena } from '@polyplug/guest';

function decode(input) {
    const result = allocStringArena(`DECODED:${toStr(input)}`);
    return {
        ptr_lo: Number(result.ptr & 0xFFFFFFFFn),
        ptr_hi: Number((result.ptr >> 32n) & 0xFFFFFFFFn),
        len: result.len,
    };
}

setDecoderImpl(decode);
export { polyplug_init };
```

## Learn more

- [JavaScript — Host guide][host] — embed the runtime, hot-reload, signing
- [JavaScript — Guest guide][guest] — generate → implement → bundle
- [JavaScript overview][overview] · [polyplug docs][docs] · [examples][examples]

[overview]: https://github.com/polyplug/polyplug/blob/main/docs/languages/js.md
[host]: https://github.com/polyplug/polyplug/blob/main/docs/languages/js-host.md
[guest]: https://github.com/polyplug/polyplug/blob/main/docs/languages/js-guest.md
[docs]: https://github.com/polyplug/polyplug/tree/main/docs
[examples]: https://github.com/polyplug/polyplug/tree/main/examples
[codegen]: https://github.com/polyplug/polyplug/blob/main/docs/CODE_GENERATION.md
