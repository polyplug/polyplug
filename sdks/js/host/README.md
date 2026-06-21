# @polyplug/host

polyplug host runtime for JavaScript/TypeScript: load plugin bundles at runtime
and call guest contracts through the frozen C ABI.

> **Runtime requirement: Deno.** The host loads the native runtime through
> Deno's FFI (`Deno.dlopen`) and reads `Deno.build` / `Deno.env`. It installs and
> imports under Node.js but throws at runtime there. A Node FFI backend is
> planned. Until then, use Deno.

Depends on [`@polyplug/abi`](https://www.npmjs.com/package/@polyplug/abi).
