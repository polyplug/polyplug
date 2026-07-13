# @polyplug/loaders-js

polyplug JavaScript (QuickJS) bundle loader for JavaScript/TypeScript hosts.

This loader uses the active JavaScript SDK FFI backend, supporting Deno, Node,
and Bun.

## Usage

```ts
import { Runtime } from "@polyplug/host";
import { bridgeLibrary, registerJsLoader } from "@polyplug/loaders/js";

registerJsLoader(runtime);
const bridge = bridgeLibrary();
```

Requires [`@polyplug/host`](https://www.npmjs.com/package/@polyplug/host). The
loader native (`libpolyplug_js`) must be resolvable by the OS loader or via the
`POLYPLUG_JS_LIB` environment variable. The explicit internal plugin generated
profile obtains the bridge through `bridgeLibrary()`; external plugins are
acquired by this loader from their bundle artifacts.
