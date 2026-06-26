# polyplug JavaScript SDK

Build polyplug hosts and plugins in JavaScript/TypeScript. The host side loads
the native runtime through FFI; guest plugins run in an embedded QuickJS VM.
Strings are native UTF-8 — no transcoding.

> **Runtime: Deno.** The host loader uses `Deno.dlopen` and `Deno.build` /
> `Deno.env`. The package installs under Node.js but the host loader throws
> there. A Node FFI backend is planned — until then, run hosts on Deno.

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
polyplugc generate --bundle bundle.toml --lang js --out ./generated
```

## Host application

```typescript
import { Runtime } from "@polyplug/host";

const runtime = Runtime.builder().pluginDir("./plugins").build();

const decoder = PipelineDecoder.create(runtime);
if (decoder) {
    const result = decoder.decode(input);
}
```

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
