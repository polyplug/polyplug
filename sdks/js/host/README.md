# @polyplug/host

polyplug host runtime for JavaScript/TypeScript: load plugin bundles at runtime
and call guest contracts through the frozen C ABI.

> **Runtime requirement: Deno.** The host loads the native runtime through
> Deno's FFI (`Deno.dlopen`) and reads `Deno.build` / `Deno.env`. It installs and
> imports under Node.js but throws at runtime there. A Node FFI backend is
> planned. Until then, use Deno.

Depends on [`@polyplug/abi`](https://www.npmjs.com/package/@polyplug/abi).

## Bundle signing & key pinning

`runtimeNew(lib, { config })` configures signature enforcement:

- `config.signaturePolicy` — `SignaturePolicy.Off` (default), `WarnOnly`, or
  `Required`. Controls whether each bundle's `bundle.sig` is verified.
- `config.trustedKeys` — an array of Ed25519 verifying keys, each **32 raw
  bytes** (a `Uint8Array` or `ArrayBuffer`). This is the key-pinning allowlist:

  - **Empty / unset (default)** — Trust-On-First-Use. With a non-`Off` policy
    the runtime verifies each bundle's embedded signature for integrity, but
    does not pin any particular signing key.
  - **Non-empty** — key pinning. After signature verification the runtime
    additionally requires the bundle's embedded verifying key to be in the
    allowlist; a bundle re-signed with an attacker key is rejected.

  Only public (verifying) keys are pinned — the private signing key stays
  offline.

```js
import { openPolyplug, runtimeNew, SignaturePolicy } from "@polyplug/host";

const lib = openPolyplug("/path/to/libpolyplug.so");
const runtime = runtimeNew(lib, {
  config: {
    signaturePolicy: SignaturePolicy.Required,
    trustedKeys: [key1, key2], // each a 32-byte Uint8Array
  },
});
```

The runtime copies `trusted_keys` during `runtimeNew` (`polyplug_runtime_create`),
so the host SDK only holds the packed key buffer across that call and lets it go
once create returns.
