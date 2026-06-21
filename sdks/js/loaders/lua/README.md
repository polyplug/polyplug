# @polyplug/loaders-lua

polyplug Lua bundle loader for JavaScript/TypeScript hosts.

> **Runtime requirement: Deno.** This loader opens its native library through
> Deno's FFI (`Deno.dlopen`). It installs and imports under Node.js but throws
> at runtime there. A Node FFI backend is planned. Until then, use Deno.

## Usage

```ts
import { Runtime } from "@polyplug/host";
import { registerLuaLoader } from "@polyplug/loaders-lua";
```

Requires [`@polyplug/host`](https://www.npmjs.com/package/@polyplug/host). The
loader native (`libpolyplug_lua`) must be resolvable by the OS loader or via the
`POLYPLUG_NATIVE_LIB` environment variable.
